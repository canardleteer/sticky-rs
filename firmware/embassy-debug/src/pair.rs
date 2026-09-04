//! BLE peripheral + DisplayOnly passkey (`--features pair` only).
//!
//! Advertise [`embassy_debug::PAIR_ADV_NAME`]. RAM bonds this boot only —
//! no factory NVS. Never print a MAC. The security CSPRNG is seeded from
//! controller `LeRand` when the host runner starts (not the crate zero seed).

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

use bt_hci::cmd::le::{LeSetAdvData, LeSetAdvEnable, LeSetAdvParams, LeSetScanResponseData};
use bt_hci::controller::ControllerCmdSync;
use embassy_debug::{Event, PairFailWhy, PAIR_ADV_NAME, PAIR_FAIL_HOLD_MS};
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
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PairView {
    /// Advertise name + how-to. No PIN yet.
    Idle,
    /// SMP passkey (0..=999999).
    Pin(u32),
    /// Pairing finished.
    Ok,
    /// Pairing did not finish.
    Fail(PairFailWhy),
}

/// Wake the display when [`current_view`] changes.
pub static PAIR_VIEW: Signal<CriticalSectionRawMutex, ()> = Signal::new();

static CURRENT: Mutex<CriticalSectionRawMutex, RefCell<PairView>> =
    Mutex::new(RefCell::new(PairView::Idle));

/// Last pair card contents (idle / PIN / ok / fail).
#[must_use]
pub fn current_view() -> PairView {
    CURRENT.lock(|cell| *cell.borrow())
}

#[gatt_server(
    connections_max = 1,
    mutex_type = CriticalSectionRawMutex,
    attribute_table_size = 32
)]
struct Server {
    pair: PairService,
}

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

/// Bring up BLE advertise and drive the pair card.
#[embassy_executor::task]
pub async fn pair_task(bluetooth: BT<'static>) {
    let Ok(connector) = BleConnector::new(bluetooth, Default::default()) else {
        fail_and_hold(PairFailWhy::BleStart).await;
        return;
    };
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

    println!("{LOG}: pair advertise {PAIR_ADV_NAME}; no NVS; no MAC");
    show(PairView::Idle);

    let pair_loop = async {
        loop {
            if let Err(why) = advertise_once(&mut peripheral, &server).await {
                fail_and_hold(why).await;
                show(PairView::Idle);
            }
        }
    };

    let _ = embassy_futures::join::join(runner.run(), pair_loop).await;
}

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

    let conn = advertiser
        .accept()
        .await
        .map_err(|_| PairFailWhy::Advertise)?;
    let _ = conn.set_bondable(true);
    let _ = conn.request_security();
    let gatt = conn
        .with_attribute_server(server)
        .map_err(|_| PairFailWhy::Pairing)?;

    drive_connection(&gatt).await
}

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

async fn fail_and_hold(why: PairFailWhy) {
    show(PairView::Fail(why));
    Timer::after(Duration::from_millis(u64::from(PAIR_FAIL_HOLD_MS))).await;
}
