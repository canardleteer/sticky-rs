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
//! - **BLE peripheral.** Packs with `wifi` on the default image
//!   (`coex`). Do not combine with `mic`, `radio`, `charge`, or `sd`
//!   (compile_error below).
//! - **DisplayOnly SMP.** The board shows a passkey; the phone types
//!   it. Without `--features remote-debug`, advertise only while
//!   [`embassy_debug::Scene::Pair`] is the current card; walking
//!   away stops it and drops the GATT connection. With remote-debug,
//!   advertise starts on **splash** (cold boot, POWERON, or
//!   `CoreSw`) so a desk host can Connect without walking. UART
//!   prints `pair pin=` on `PassKeyDisplay` and reprints every 5 s
//!   on splash or the pair card until `pair ok`. A paired link is
//!   **held** after leave. After a drop, this image advertises
//!   again immediately (no pair-card gate). Keys still walk pages.
//!   AI Voice is not a confirm.
//! - **RAM bonds this connection.** `HostResources` holds them. Do not
//!   write factory NVS (RF cal and identity live there). A drop
//!   without a live host LTK must **forget** that RAM bond or the
//!   next Connect encrypts with a stale key and never shows
//!   `PassKeyDisplay` (no UART `pair pin=`). A `Reboot` envelope
//!   software-resets the **MCU**, not the host; the next pairing is
//!   a new DisplayOnly PIN.
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
#[cfg(all(feature = "remote-debug", not(feature = "pair")))]
compile_error!("remote-debug needs pair (encrypted GATT after DisplayOnly SMP)");

use crate::{emit, now_ms};

use core::cell::RefCell;
#[cfg(feature = "remote-debug")]
use core::sync::atomic::AtomicU8;
use core::sync::atomic::{AtomicBool, Ordering};

use bt_hci::cmd::le::{LeSetAdvData, LeSetAdvEnable, LeSetAdvParams, LeSetScanResponseData};
use bt_hci::controller::ControllerCmdSync;
use embassy_debug::{Event, PairFailWhy, Scene, PAIR_ADV_NAME, PAIR_FAIL_HOLD_MS};
use embassy_futures::select::{select, Either};
#[cfg(feature = "remote-debug")]
use embassy_futures::select::{select3, Either3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::BT;
use esp_println::println;
use esp_radio::ble::controller::BleConnector;
use trouble_host::prelude::*;

const LOG: &str = "embassy-debug";

/// Live BLE links `HostResources` can hold.
///
/// One desk GATT plus the previous drop while the controller
/// tears the ACL down. `CONNS = 1` raced a rapid host reconnect
/// (advertise started before the slot was free).
const BLE_CONNS: usize = 2;

/// L2CAP channels on that host: SMP + one ATT.
const BLE_CHANNELS: usize = 2;

/// After `accept`, wait for `ConnectionState::Connected` before SMP.
///
/// `set_bondable` / `request_security` error if the slot is not
/// Connected yet (*The Embassy Book*: yield; do not busy-spin).
const SMP_AFTER_ACCEPT_MS: u64 = 80;

/// How many times we retry bondable + Security Request.
const SMP_START_TRIES: u8 = 5;

/// Gap between SMP start tries.
const SMP_START_GAP_MS: u64 = 40;

/// After `Disconnected`, wait before the next advertise so `CONNS`
/// can free. Not a sleep of the MCU.
const AFTER_DROP_MS: u64 = 120;

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

/// Last painted scene persist-byte. Display writes this so remote-debug
/// can reprint `pair pin=` on splash or the pair card only.
#[cfg(feature = "remote-debug")]
static UI_SCENE: AtomicU8 = AtomicU8::new(0);

/// Wake the BLE task when [`set_visible`] changes the gate.
static PAIR_GATE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Allow or stop advertising. The display task calls this on every
/// scene change and before the Ferris off-screen.
///
/// `true` only for [`embassy_debug::Scene::Pair`]. A falling edge
/// cancels an in-flight advertise. Without remote-debug it also
/// drops an accepted connection; with remote-debug the paired
/// GATT stays up until the central disconnects.
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

/// Display task: remember the current card for PIN reprints.
///
/// Remote-debug reads this so a 5 s `pair pin=` reprint stays on
/// splash or the pair card only (*The Embassy Book*: one writer).
pub fn set_scene(scene: Scene) {
    #[cfg(feature = "remote-debug")]
    UI_SCENE.store(scene.persist_byte(), Ordering::Release);
    #[cfg(not(feature = "remote-debug"))]
    let _ = scene;
}

/// Scene the display last painted.
#[cfg(feature = "remote-debug")]
#[must_use]
pub(crate) fn current_scene() -> Option<Scene> {
    Scene::from_persist_byte(UI_SCENE.load(Ordering::Acquire))
}

/// Splash or pair card: remote-debug may reprint `pair pin=` here.
#[cfg(feature = "remote-debug")]
#[must_use]
fn should_reprint_pin() -> bool {
    current_scene().is_some_and(Scene::pair_pin_reprint)
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

/// One connection. GAP + pair token is 32. Remote-debug adds an
/// encrypted write + notify (+ CCCD), so that image uses 64.
#[cfg(not(feature = "remote-debug"))]
#[gatt_server(
    connections_max = 1,
    mutex_type = CriticalSectionRawMutex,
    attribute_table_size = 32
)]
struct Server {
    pair: PairService,
}

/// GAP + pair token + remote-debug RX/TX.
///
/// RX is host→device framed envelopes. TX notifies device→host
/// frames (a snapshot is many ATT payloads). Both
/// `permissions(encrypted)` so a bond is required. UUIDs match
/// [`remote_debug_wire::GATT_SERVICE_UUID`] (local 128-bit, not
/// the pair-card `6b1d0001-…` token).
#[cfg(feature = "remote-debug")]
#[gatt_server(
    connections_max = 1,
    mutex_type = CriticalSectionRawMutex,
    attribute_table_size = 64
)]
struct Server {
    pair: PairService,
    remote: RemoteDebugService,
}

/// Encrypted remote-debug GATT (UUIDs from `remote-debug-peripheral`).
#[cfg(feature = "remote-debug")]
use remote_debug_peripheral::RemoteDebugService;

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

/// Bring up the BLE host; advertise while the pair card is showing
/// (default image) or from splash on (`--features remote-debug`).
///
/// On the unit: default image prints
/// `pair advertise sticky-rs; no NVS; no MAC` when walking to
/// `scene=pair` and stops ADV on leave. Remote-debug prints that
/// line on splash (desk Connect without walking) and keeps ADV up
/// until a central Connects. UART `pair pin=` follows the SMP
/// passkey. In the MCU: controller → trouble-host runner + accept
/// loop. The runner must stay polled or `LeRand` never seeds SMP.
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
    // (not the crate's zero seed). Bonds stay in HostResources RAM
    // for this connection only; [`forget_ram_bond`] on drop.
    let address = Address::random([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
    let mut resources: HostResources<_, DefaultPacketPool, BLE_CONNS, BLE_CHANNELS> =
        HostResources::new();
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
    #[cfg(feature = "remote-debug")]
    {
        debug_assert_eq!(
            remote_debug_wire::GATT_SERVICE_UUID,
            "c81e1000-5c8a-4f0e-9c3a-2e7b1a0d4f11"
        );
        let _ = &server.remote;
    }

    show(PairView::Idle);

    // Desk image: ADV from splash on every boot (POWERON / CoreSw /
    // brownout). Default image stays gated on the pair card.
    #[cfg(feature = "remote-debug")]
    let ungated_adv = true;
    #[cfg(not(feature = "remote-debug"))]
    let ungated_adv = false;

    let pair_loop = async {
        loop {
            if !ungated_adv {
                wait_until_visible(true).await;
            }
            println!("{LOG}: pair advertise {PAIR_ADV_NAME}; no NVS; no MAC");
            show(PairView::Idle);
            match advertise_once(&stack, &mut peripheral, &server, ungated_adv).await {
                Ok(()) => {
                    // Disconnect, or (without remote-debug) the operator
                    // left the pair card. Remote-debug holds GATT after leave.
                    show(PairView::Idle);
                }
                Err(why) => {
                    // UART `pair fail=` even on splash (Ferris does not
                    // steal the glass; the display task ignores PAIR_VIEW
                    // off `Scene::Pair`).
                    show(PairView::Fail(why));
                    if is_visible() {
                        Timer::after(Duration::from_millis(u64::from(PAIR_FAIL_HOLD_MS))).await;
                    }
                    show(PairView::Idle);
                }
            }
            Timer::after(Duration::from_millis(AFTER_DROP_MS)).await;
        }
    };

    let _ = embassy_futures::join::join(runner.run(), pair_loop).await;
}

/// One advertise → accept → SMP session.
///
/// Connectable + scannable undirected, general discoverable, BR/EDR
/// not supported. Empty scan response: the complete local name is
/// already in the adv payload ([`PAIR_ADV_NAME`], 9 bytes).
///
/// After accept: attach GATT, drop a leftover RAM bond for this
/// peer, then Security Request. Do not swallow SMP start errors
/// (`request_security` fails when the link is already encrypted).
async fn advertise_once<C>(
    stack: &Stack<'_, C, DefaultPacketPool>,
    peripheral: &mut Peripheral<'_, C, DefaultPacketPool>,
    server: &Server<'_>,
    ungated: bool,
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

    // Dropping `advertiser` here (operator left the card) stops ADV
    // on the default image. Remote-debug is ungated: splash is not
    // the pair card (`PAIR_VISIBLE` is false); do not treat that as
    // leave or ADV ends before a desk Connect.
    let leave = async {
        if ungated {
            core::future::pending::<()>().await;
        } else {
            wait_until_visible(false).await;
        }
    };
    let conn = match select(advertiser.accept(), leave).await {
        Either::First(Ok(conn)) => conn,
        Either::First(Err(_)) => return Err(PairFailWhy::Advertise),
        Either::Second(()) => return Ok(()),
    };
    // A leftover RAM LTK after the host forgot BlueZ encrypts
    // without PassKeyDisplay. Forget before SMP. Never print the
    // identity (that is a MAC).
    if conn.is_bonded_peer() {
        forget_ram_bond(stack, conn.peer_identity());
    }
    let gatt = conn
        .with_attribute_server(server)
        .map_err(|_| PairFailWhy::Pairing)?;
    // Bondable + request_security sends SMP Security Request so a
    // central Connect (phone Settings or BlueZ Connect) starts
    // DisplayOnly passkey. The PIN is not shown before that. On
    // Linux, do not also call BlueZ Pair(): that races this
    // Security Request (kernel unexpected SMP 0x0B) and cancels.
    start_display_only(gatt.raw()).await?;

    // Before accept, leave still cancelled advertise (select above).
    // After accept: default image drops the link on leave. Remote-debug
    // keeps the paired GATT so a walk off splash / the pair card does
    // not kill the desk session. After a drop, that image advertises
    // again without waiting for the pair card.
    #[cfg(not(feature = "remote-debug"))]
    {
        return match select(
            drive_connection(stack, &gatt, server),
            wait_until_visible(false),
        )
        .await
        {
            Either::First(result) => result,
            Either::Second(()) => {
                forget_ram_bond(stack, gatt.raw().peer_identity());
                Ok(())
            }
        };
    }
    #[cfg(feature = "remote-debug")]
    {
        drive_connection(stack, &gatt, server).await
    }
}

/// Drop a RAM bond. Never format `identity` (that is a MAC).
///
/// Host `disconnect` forgets the BlueZ object. If this image keeps
/// the LTK, the next Connect encrypts without `PassKeyDisplay`.
fn forget_ram_bond<C: Controller, P: PacketPool>(stack: &Stack<'_, C, P>, identity: Identity) {
    let _ = stack.remove_bond_information(identity);
}

/// Bondable + SMP Security Request (DisplayOnly passkey).
///
/// Retries while the ACL is still coming up. Errors if the link is
/// already encrypted (stale LTK) after [`forget_ram_bond`]. Do not
/// call BlueZ `Pair()` on Linux; that races this request.
///
/// # Errors
///
/// [`PairFailWhy::Pairing`] when bondable or Security Request never
/// succeeds. UART prints `pair fail=pairing`.
async fn start_display_only<P: PacketPool>(conn: &Connection<'_, P>) -> Result<(), PairFailWhy> {
    Timer::after(Duration::from_millis(SMP_AFTER_ACCEPT_MS)).await;
    for _ in 0..SMP_START_TRIES {
        if conn.set_bondable(true).is_err() {
            Timer::after(Duration::from_millis(SMP_START_GAP_MS)).await;
            continue;
        }
        if conn.request_security().is_ok() {
            return Ok(());
        }
        Timer::after(Duration::from_millis(SMP_START_GAP_MS)).await;
    }
    Err(PairFailWhy::Pairing)
}

/// GATT + SMP events on one accepted connection.
///
/// A clean disconnect returns to idle advertise (not a fail card).
/// DisplayOnly never needs `PassKeyConfirm` / `PassKeyInput` / OOB;
/// those arms stay empty on purpose.
///
/// With `--features remote-debug`, encrypted writes to RX are
/// reassembled and passed to [`crate::remote_debug::handle_envelope`].
/// A `GetSnapshot` arm/retry streams LAST through TX notifies
/// (*The Embassy Book*: do this on the BLE task, not the display task).
async fn drive_connection<C, P>(
    stack: &Stack<'_, C, P>,
    gatt: &GattConnection<'_, '_, P>,
    #[cfg_attr(not(feature = "remote-debug"), allow(unused_variables))] server: &Server<'_>,
) -> Result<(), PairFailWhy>
where
    C: Controller,
    P: PacketPool,
{
    #[cfg(feature = "remote-debug")]
    let mut rx_asm = remote_debug_wire::FrameAssembler::device_rx();
    #[cfg(feature = "remote-debug")]
    let mut pending_pin: Option<u32> = None;
    let mut seen_pin = false;
    let mut paired = false;
    loop {
        #[cfg(feature = "remote-debug")]
        let event = match select3(
            gatt.next(),
            Timer::after(Duration::from_secs(5)),
            crate::remote_debug::wait_log_ready(),
        )
        .await
        {
            Either3::First(event) => event,
            Either3::Second(()) => {
                if let (Some(pin), false) = (pending_pin, paired) {
                    if should_reprint_pin() {
                        emit(Event::PairPin {
                            t_ms: now_ms(),
                            pin,
                        });
                    }
                }
                continue;
            }
            Either3::Third(()) => {
                notify_pending_logs(gatt, server).await;
                continue;
            }
        };
        #[cfg(not(feature = "remote-debug"))]
        let event = gatt.next().await;
        match event {
            GattConnectionEvent::PassKeyDisplay(key) => {
                let pin = key.value() % 1_000_000;
                seen_pin = true;
                #[cfg(feature = "remote-debug")]
                {
                    pending_pin = Some(pin);
                    paired = false;
                }
                show(PairView::Pin(pin));
            }
            GattConnectionEvent::PairingComplete { .. } => {
                paired = true;
                show(PairView::Ok);
            }
            GattConnectionEvent::PairingFailed(err) => {
                forget_ram_bond(stack, gatt.raw().peer_identity());
                return Err(map_host_error(err));
            }
            GattConnectionEvent::BondLost => {
                forget_ram_bond(stack, gatt.raw().peer_identity());
                return Err(PairFailWhy::BondLost);
            }
            GattConnectionEvent::Encrypted { bond, .. } => {
                // Stored LTK with no PIN this connection: host forgot
                // BlueZ; encrypting that key never shows PassKeyDisplay.
                if bond.is_some() && !paired && !seen_pin {
                    forget_ram_bond(stack, gatt.raw().peer_identity());
                    start_display_only(gatt.raw()).await?;
                }
            }
            GattConnectionEvent::Disconnected { .. } => {
                forget_ram_bond(stack, gatt.raw().peer_identity());
                show(PairView::Idle);
                return Ok(());
            }
            GattConnectionEvent::Gatt { event } => {
                #[cfg(feature = "remote-debug")]
                let remote_write = match &event {
                    GattEvent::Write(write) if write.handle() == server.remote.rx.handle => {
                        let mut chunk = [0u8; 244];
                        let n = write.with_data(|_, data| {
                            let n = data.len().min(chunk.len());
                            chunk[..n].copy_from_slice(&data[..n]);
                            n
                        });
                        Some((chunk, n))
                    }
                    _ => None,
                };
                if let Ok(reply) = event.accept() {
                    reply.send().await;
                }
                #[cfg(feature = "remote-debug")]
                if let Some((chunk, n)) = remote_write {
                    on_remote_rx(gatt, server, &mut rx_asm, &chunk[..n]).await;
                }
            }
            GattConnectionEvent::PassKeyConfirm(_)
            | GattConnectionEvent::PassKeyInput
            | GattConnectionEvent::OobRequest => {}
            _ => {}
        }
    }
}

/// Append one ATT write, handle a complete frame, notify a reply.
///
/// *The Embedded Rust Book*: RX is a small reassembly buffer
/// ([`remote_debug_wire::DEVICE_RX_MAX`]). Snapshot TX streams LAST
/// slices; it does not `Vec` 48 KiB.
#[cfg(feature = "remote-debug")]
async fn on_remote_rx<P: PacketPool>(
    gatt: &GattConnection<'_, '_, P>,
    server: &Server<'_>,
    rx_asm: &mut remote_debug_wire::FrameAssembler,
    chunk: &[u8],
) {
    let frame = match rx_asm.push(chunk) {
        Ok(Some(frame)) => frame,
        Ok(None) => return,
        Err(_) => {
            rx_asm.clear();
            return;
        }
    };
    match crate::remote_debug::handle_envelope(&frame) {
        Ok(crate::remote_debug::EnvelopeOutcome::Snapshot) => {
            remote_debug_peripheral::notify_armed_snapshot(
                gatt,
                &server.remote.tx,
                || crate::remote_debug::with_armed_snapshot_meta(|opt| opt),
                |bw, off, n, dest| {
                    crate::remote_debug::with_armed_frame(|opt| {
                        let Some((_, frame)) = opt else {
                            return 0;
                        };
                        let src = if bw {
                            frame.bw
                        } else {
                            frame.red.unwrap_or(&[])
                        };
                        if off >= src.len() {
                            return 0;
                        }
                        let n = n.min(src.len() - off);
                        dest[..n].copy_from_slice(&src[off..off + n]);
                        n
                    })
                },
            )
            .await;
        }
        Ok(crate::remote_debug::EnvelopeOutcome::Busy { armed }) => {
            let bytes = remote_debug_wire::encode_snapshot_busy(armed);
            remote_debug_peripheral::notify_bytes(gatt, &server.remote.tx, &bytes).await;
        }
        Ok(crate::remote_debug::EnvelopeOutcome::Reboot) => {
            let bytes = remote_debug_wire::encode_reboot_ack();
            remote_debug_peripheral::notify_bytes(gatt, &server.remote.tx, &bytes).await;
            Timer::after(Duration::from_millis(100)).await;
            esp_hal::system::software_reset();
        }
        Ok(crate::remote_debug::EnvelopeOutcome::None) | Err(_) => {}
    }
    notify_pending_logs(gatt, server).await;
}

/// Flush Target / Scene UART copies as GATT `LogLine` (never a PIN).
#[cfg(feature = "remote-debug")]
async fn notify_pending_logs<P: PacketPool>(gatt: &GattConnection<'_, '_, P>, server: &Server<'_>) {
    let mut text = [0u8; embassy_debug::LINE_CAPACITY];
    while let Some((t_ms, n)) = crate::remote_debug::take_log_line(&mut text) {
        let Ok(line) = core::str::from_utf8(&text[..n]) else {
            continue;
        };
        let bytes = remote_debug_wire::encode_log_line(t_ms, line);
        remote_debug_peripheral::notify_bytes(gatt, &server.remote.tx, &bytes).await;
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
