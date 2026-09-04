//! BLE peripheral + DisplayOnly passkey (`--features pair` only).
//!
//! # Architecture and pairing contract
//!
//! This task is a walkthrough of a **peripheral** that can pair, not a
//! phone stack. On the unit: Settings → Bluetooth → `sticky-rs`, then
//! a six-digit PIN on the glass only after the phone starts pairing.
//! UART prints `pair pin=`, then `pair ok` or `pair fail=<why>`.
//! Never a MAC.
//!
//! In the MCU:
//!
//! - **BLE only.** `esp-radio` is `ble` without Wi-Fi / `coex`. Do not
//!   combine with `mic`, `radio`, `charge`, or `sd` (compile_error
//!   below).
//! - **DisplayOnly SMP.** The board shows a passkey; the phone types
//!   it. Advertise only while [`embassy_debug::Scene::Pair`] is the
//!   current card. Walking away stops advertising and drops a
//!   connection. Keys still walk pages. AI Voice is not a confirm.
//! - **RAM bonds this boot.** `HostResources` holds them. Do not write
//!   factory NVS (RF cal and identity live there).
//! - **Fixed random address.** Do not read or print the eFuse MAC.
//!   `runner.run()` seeds the security CSPRNG from controller `LeRand`
//!   (not the crate’s zero seed).
//! - **Custom GATT service** with one encrypted-read byte so Settings
//!   pairing has something to bond against. The UUIDs are local, not
//!   Bluetooth SIG assigned.
//!
//! Host-tested tokens live in [`embassy_debug::Event`]
//! (`PairPin` / `PairOk` / `PairFail`). How-to:
//! [README.md](../README.md#pair-test-instructions).

#[cfg(all(feature = "pair", feature = "mic"))]
compile_error!("do not combine pair with mic");
#[cfg(all(feature = "pair", feature = "radio"))]
compile_error!("do not combine pair with radio");
#[cfg(all(feature = "pair", feature = "charge"))]
compile_error!("do not combine pair with charge");
#[cfg(all(feature = "pair", feature = "sd"))]
compile_error!("do not combine pair with sd");

use crate::{emit, now_ms};

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use bt_hci::cmd::le::{LeSetAdvData, LeSetAdvEnable, LeSetAdvParams, LeSetScanResponseData};
use bt_hci::controller::ControllerCmdSync;
use embassy_debug::{Event, PairFailWhy, PAIR_ADV_NAME, PAIR_FAIL_HOLD_MS};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::BT;
use esp_println::println;
use esp_radio::ble::controller::BleConnector;
use trouble_host::prelude::*;

const LOG: &str = "embassy-debug";

/// What the pair card should paint.
///
/// Idle is a how-to, not a fake PIN. [`Self::Pin`] exists only after
/// `PassKeyDisplay`. The display task reads this via [`current_view`]
/// when [`PAIR_VIEW`] wakes it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PairView {
    /// Advertise name + how-to. No PIN yet.
    Idle,
    /// SMP passkey (0..=999999). Same six digits as `pair pin=` on UART.
    Pin(u32),
    /// Pairing finished (`pair ok` / glass `Paired`).
    Ok,
    /// Pairing did not finish (`pair fail=` + [`PairFailWhy::as_str`]).
    Fail(PairFailWhy),
}

/// Wake the display when [`current_view`] changes.
///
/// The display loop only repaints when the current scene is already
/// `Scene::Pair`, so a PIN arriving while the operator is on splash
/// does not steal the glass.
pub static PAIR_VIEW: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Last pair-card contents. Critical-section mutex: the BLE task writes,
/// the display task reads on the same core.
static CURRENT: Mutex<CriticalSectionRawMutex, RefCell<PairView>> =
    Mutex::new(RefCell::new(PairView::Idle));

/// True only while the operator is on [`embassy_debug::Scene::Pair`].
///
/// The display task writes this; the BLE task waits on [`PAIR_GATE`].
static PAIR_VISIBLE: AtomicBool = AtomicBool::new(false);

/// Wake the BLE task when [`set_visible`] changes the gate.
static PAIR_GATE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Allow or stop advertising. The display task calls this on every
/// scene change and before the Ferris off-screen.
///
/// `true` only for [`embassy_debug::Scene::Pair`]. A falling edge
/// cancels an in-flight advertise or drops an accepted connection.
pub fn set_visible(on: bool) {
    let was = PAIR_VISIBLE.swap(on, Ordering::SeqCst);
    if was != on {
        PAIR_GATE.signal(());
    }
}

/// Current pair-card gate. Safe to poll from the BLE task.
#[must_use]
pub fn is_visible() -> bool {
    PAIR_VISIBLE.load(Ordering::Acquire)
}

/// Wait until [`is_visible`] matches `want`.
///
/// [`PAIR_GATE`] is single-waiter. The display task is the only
/// signaler; this function is the only waiter.
async fn wait_until_visible(want: bool) {
    loop {
        if is_visible() == want {
            return;
        }
        PAIR_GATE.wait().await;
    }
}

/// Last pair card contents (idle / PIN / ok / fail).
#[must_use]
pub fn current_view() -> PairView {
    CURRENT.lock(|cell| *cell.borrow())
}

/// One connection. `attribute_table_size` is enough for GAP + this
/// service; raise it if another characteristic is added.
#[gatt_server(
    connections_max = 1,
    mutex_type = CriticalSectionRawMutex,
    attribute_table_size = 32
)]
struct Server {
    pair: PairService,
}

/// Local 128-bit service so Settings pairing has a GATT target.
///
/// These UUIDs are not SIG 16-bit assignments. The `token` read is
/// `permissions(encrypted)` so a bonded link is required after SMP.
#[gatt_service(uuid = "6b1d0001-5c8a-4f0e-9c3a-2e7b1a0d4f11")]
struct PairService {
    #[characteristic(
        uuid = "6b1d0002-5c8a-4f0e-9c3a-2e7b1a0d4f11",
        read,
        value = 1,
        permissions(encrypted)
    )]
    token: u8,
}

/// Bring up the BLE host; advertise only while the pair card is showing.
///
/// On the unit: walking to `scene=pair` prints
/// `pair advertise sticky-rs; no NVS; no MAC` and starts connectable
/// advertise. Leaving that card stops it. In the MCU: controller →
/// trouble-host runner + gated accept loop. The runner must stay
/// polled or `LeRand` never seeds SMP.
#[embassy_executor::task]
pub async fn pair_task(bluetooth: BT<'static>) {
    let Ok(connector) = BleConnector::new(bluetooth, Default::default()) else {
        fail_and_hold(PairFailWhy::BleStart).await;
        return;
    };
    // 10 is the HCI event slot count on the external controller wrapper.
    let ble_controller: ExternalController<_, 10> = ExternalController::new(connector);

    // Fixed random address so we do not read or print the eFuse MAC.
    // `runner.run()` seeds the security CSPRNG from controller LeRand
    // (not the crate's zero seed). Bonds stay in HostResources RAM.
    let address = Address::random([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
    let mut resources: HostResources<_, DefaultPacketPool, 1, 2> = HostResources::new();
    let stack = trouble_host::new(ble_controller, &mut resources)
        .set_random_address(address)
        .set_io_capabilities(IoCapabilities::DisplayOnly)
        .build();
    let mut runner = stack.runner();
    let mut peripheral = stack.peripheral();

    let Ok(server) = Server::new_with_config(GapConfig::default(PAIR_ADV_NAME)) else {
        fail_and_hold(PairFailWhy::BleStart).await;
        return;
    };
    // Keep the derived service in the binary; Settings pairing reads it.
    let _ = &server.pair;

    show(PairView::Idle);

    let pair_loop = async {
        loop {
            wait_until_visible(true).await;
            println!("{LOG}: pair advertise {PAIR_ADV_NAME}; no NVS; no MAC");
            show(PairView::Idle);
            match advertise_once(&mut peripheral, &server).await {
                Ok(()) => {
                    // Disconnect or the operator left the pair card.
                    show(PairView::Idle);
                }
                Err(why) => {
                    if is_visible() {
                        fail_and_hold(why).await;
                    }
                    show(PairView::Idle);
                }
            }
        }
    };

    let _ = embassy_futures::join::join(runner.run(), pair_loop).await;
}

/// One advertise → accept → SMP session.
///
/// Connectable + scannable undirected, general discoverable, BR/EDR
/// not supported. Empty scan response: the complete local name is
/// already in the adv payload ([`PAIR_ADV_NAME`], 9 bytes).
async fn advertise_once<C>(
    peripheral: &mut Peripheral<'_, C, DefaultPacketPool>,
    server: &Server<'_>,
) -> Result<(), PairFailWhy>
where
    C: Controller
        + for<'t> ControllerCmdSync<LeSetAdvData>
        + ControllerCmdSync<LeSetAdvParams>
        + for<'t> ControllerCmdSync<LeSetAdvEnable>
        + for<'t> ControllerCmdSync<LeSetScanResponseData>,
{
    let mut adv_data = [0u8; 31];
    let Ok(adv_len) = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::CompleteLocalName(PAIR_ADV_NAME.as_bytes()),
        ],
        &mut adv_data,
    ) else {
        return Err(PairFailWhy::Advertise);
    };

    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &adv_data[..adv_len],
                scan_data: &[],
            },
        )
        .await
        .map_err(|_| PairFailWhy::Advertise)?;

    // Dropping `advertiser` here (operator left the card) stops ADV.
    let conn = match select(advertiser.accept(), wait_until_visible(false)).await {
        Either::First(Ok(conn)) => conn,
        Either::First(Err(_)) => return Err(PairFailWhy::Advertise),
        Either::Second(()) => return Ok(()),
    };
    // Bondable + request_security sends SMP Security Request so a
    // central Connect (phone Settings or BlueZ Connect) starts
    // DisplayOnly passkey. The PIN is not shown before that. On
    // Linux, do not also call BlueZ Pair(): that races this
    // Security Request (kernel unexpected SMP 0x0B) and cancels.
    let _ = conn.set_bondable(true);
    let _ = conn.request_security();
    let gatt = conn
        .with_attribute_server(server)
        .map_err(|_| PairFailWhy::Pairing)?;

    match select(drive_connection(&gatt), wait_until_visible(false)).await {
        Either::First(result) => result,
        Either::Second(()) => Ok(()),
    }
}

/// GATT + SMP events on one accepted connection.
///
/// A clean disconnect returns to idle advertise (not a fail card).
/// DisplayOnly never needs `PassKeyConfirm` / `PassKeyInput` / OOB;
/// those arms stay empty on purpose.
async fn drive_connection<P: PacketPool>(
    gatt: &GattConnection<'_, '_, P>,
) -> Result<(), PairFailWhy> {
    loop {
        match gatt.next().await {
            GattConnectionEvent::PassKeyDisplay(key) => {
                show(PairView::Pin(key.value() % 1_000_000));
            }
            GattConnectionEvent::PairingComplete { .. } => {
                show(PairView::Ok);
            }
            GattConnectionEvent::PairingFailed(err) => {
                return Err(map_host_error(err));
            }
            GattConnectionEvent::BondLost => {
                return Err(PairFailWhy::BondLost);
            }
            GattConnectionEvent::Disconnected { .. } => {
                show(PairView::Idle);
                return Ok(());
            }
            GattConnectionEvent::Gatt { event } => {
                if let Ok(reply) = event.accept() {
                    reply.send().await;
                }
            }
            GattConnectionEvent::PassKeyConfirm(_)
            | GattConnectionEvent::PassKeyInput
            | GattConnectionEvent::OobRequest => {}
            _ => {}
        }
    }
}

/// Map a trouble-host error to a UART `pair fail=` token.
///
/// `PasskeyEntryFailed` is a user cancel or a wrong code on the phone.
/// Other `Security(_)` reasons collapse to `pairing` so we never print
/// a stack string or a MAC.
fn map_host_error(err: trouble_host::Error) -> PairFailWhy {
    match err {
        trouble_host::Error::Timeout => PairFailWhy::Timeout,
        trouble_host::Error::Security(PairingFailedReason::PasskeyEntryFailed) => {
            PairFailWhy::Cancelled
        }
        trouble_host::Error::Security(_) => PairFailWhy::Pairing,
        _ => PairFailWhy::Unknown,
    }
}

/// Publish a card + matching UART event, then wake the display.
///
/// Idle has no event line (advertise already printed once at start).
/// PIN / ok / fail go through [`emit`] so `log_task` owns the format.
fn show(view: PairView) {
    match view {
        PairView::Idle => {}
        PairView::Pin(pin) => emit(Event::PairPin {
            t_ms: now_ms(),
            pin,
        }),
        PairView::Ok => emit(Event::PairOk { t_ms: now_ms() }),
        PairView::Fail(why) => emit(Event::PairFail {
            t_ms: now_ms(),
            why,
        }),
    }
    CURRENT.lock(|cell| *cell.borrow_mut() = view);
    PAIR_VIEW.signal(());
}

/// Fail card, then sit [`PAIR_FAIL_HOLD_MS`] so the why is readable.
async fn fail_and_hold(why: PairFailWhy) {
    show(PairView::Fail(why));
    Timer::after(Duration::from_millis(u64::from(PAIR_FAIL_HOLD_MS))).await;
}
