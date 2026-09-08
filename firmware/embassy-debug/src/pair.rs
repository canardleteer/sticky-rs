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
//!   it. Advertise only while [`embassy_debug::Scene::Pair`] is the
//!   current card. Walking away stops advertising. Without
//!   `--features remote-debug`, it also drops the GATT connection.
//!   With remote-debug, a paired link is **held** after leave so
//!   encrypted RX/TX can keep serving framed envelopes. Reconnect
//!   after a drop means walk back to the pair card. Keys still walk
//!   pages. AI Voice is not a confirm.
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
#[cfg(all(feature = "remote-debug", not(feature = "pair")))]
compile_error!("remote-debug needs pair (encrypted GATT after DisplayOnly SMP)");

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

/// Encrypted remote-debug GATT (local UUIDs; not SIG).
///
/// `rx` is written in ATT-sized chunks; [`remote_debug_wire::FrameAssembler`]
/// reassembles one u32-LE frame. `tx` notifies chunks the same way.
/// The stored GATT value is a dummy byte; writes and
/// `notify_raw(..., store=false)` carry the ATT payload.
/// *The Embedded Rust Book*: do not hold a 48 KiB plane in the table.
#[cfg(feature = "remote-debug")]
#[gatt_service(uuid = "c81e1000-5c8a-4f0e-9c3a-2e7b1a0d4f11")]
struct RemoteDebugService {
    #[characteristic(
        uuid = "c81e1001-5c8a-4f0e-9c3a-2e7b1a0d4f11",
        write,
        write_without_response,
        permissions(encrypted)
    )]
    rx: u8,
    #[characteristic(
        uuid = "c81e1002-5c8a-4f0e-9c3a-2e7b1a0d4f11",
        notify,
        permissions(encrypted)
    )]
    tx: u8,
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
    #[cfg(feature = "remote-debug")]
    {
        debug_assert_eq!(
            remote_debug_wire::GATT_SERVICE_UUID,
            "c81e1000-5c8a-4f0e-9c3a-2e7b1a0d4f11"
        );
        let _ = &server.remote;
    }

    show(PairView::Idle);

    let pair_loop = async {
        loop {
            wait_until_visible(true).await;
            println!("{LOG}: pair advertise {PAIR_ADV_NAME}; no NVS; no MAC");
            show(PairView::Idle);
            match advertise_once(&mut peripheral, &server).await {
                Ok(()) => {
                    // Disconnect, or (without remote-debug) the operator
                    // left the pair card. Remote-debug holds GATT after leave.
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

    // Before accept, leave still cancelled advertise (select above).
    // After accept: default image drops the link on leave. Remote-debug
    // keeps the paired GATT so a walk off `scene=pair` does not kill
    // the desk session. Advertise stays off until the next pair card.
    #[cfg(not(feature = "remote-debug"))]
    {
        return match select(drive_connection(&gatt, server), wait_until_visible(false)).await {
            Either::First(result) => result,
            Either::Second(()) => Ok(()),
        };
    }
    #[cfg(feature = "remote-debug")]
    {
        drive_connection(&gatt, server).await
    }
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
async fn drive_connection<P: PacketPool>(
    gatt: &GattConnection<'_, '_, P>,
    #[cfg_attr(not(feature = "remote-debug"), allow(unused_variables))] server: &Server<'_>,
) -> Result<(), PairFailWhy> {
    #[cfg(feature = "remote-debug")]
    let mut rx_asm = remote_debug_wire::FrameAssembler::device_rx();
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
            notify_armed_snapshot(gatt, server).await;
        }
        Ok(crate::remote_debug::EnvelopeOutcome::Busy { armed }) => {
            let bytes = remote_debug_wire::encode_snapshot_busy(armed);
            notify_bytes(gatt, &server.remote.tx, &bytes).await;
        }
        Ok(crate::remote_debug::EnvelopeOutcome::None) | Err(_) => {}
    }
}

/// Stream the frozen LAST planes as ATT notify chunks.
///
/// Copies one ATT payload at a time under the snapshot lock, then
/// awaits notify (*The Embassy Book*: do not hold a critical-section
/// mutex across an await).
#[cfg(feature = "remote-debug")]
async fn notify_armed_snapshot<P: PacketPool>(
    gatt: &GattConnection<'_, '_, P>,
    server: &Server<'_>,
) {
    let Some(meta) = crate::remote_debug::with_armed_frame(|opt| {
        opt.map(|(nonce, frame)| {
            (
                nonce,
                frame.width,
                frame.height,
                remote_debug_wire::frame_kind_to_wire(frame.kind),
                frame.hold.map(crate::remote_debug::hold_token),
                frame.bw.len(),
                frame.red.map(<[u8]>::len).unwrap_or(0),
            )
        })
    }) else {
        let bytes = remote_debug_wire::encode_snapshot_busy(0);
        notify_bytes(gatt, &server.remote.tx, &bytes).await;
        return;
    };
    let (nonce, width, height, kind, hold, bw_len, red_len) = meta;
    let mut preamble = [0u8; 64];
    if let Ok(n) = remote_debug_wire::snapshot_preamble_to_slice(
        remote_debug_wire::SnapshotMeta {
            nonce,
            width,
            height,
            kind,
            hold,
        },
        bw_len,
        red_len,
        &mut preamble,
    ) {
        notify_bytes(gatt, &server.remote.tx, &preamble[..n]).await;
    }
    notify_plane_chunks(gatt, server, true, bw_len).await;
    if red_len != 0 {
        let mut hdr = [0u8; 8];
        if let Ok(n) = remote_debug_wire::bytes_field_header_to_slice(7, red_len, &mut hdr) {
            notify_bytes(gatt, &server.remote.tx, &hdr[..n]).await;
        }
        notify_plane_chunks(gatt, server, false, red_len).await;
    }
}

/// Notify `len` bytes of LAST `bw` (`true`) or `red` (`false`).
#[cfg(feature = "remote-debug")]
async fn notify_plane_chunks<P: PacketPool>(
    gatt: &GattConnection<'_, '_, P>,
    server: &Server<'_>,
    bw: bool,
    len: usize,
) {
    let max = notify_payload_max(gatt);
    let mut off = 0;
    while off < len {
        let n = (len - off).min(max);
        let mut chunk = [0u8; 244];
        let copied = crate::remote_debug::with_armed_frame(|opt| {
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
            chunk[..n].copy_from_slice(&src[off..off + n]);
            n
        });
        if copied == 0 {
            return;
        }
        notify_bytes(gatt, &server.remote.tx, &chunk[..copied]).await;
        off += copied;
    }
}

/// ATT notify payload size for this link (opcode + handle eat 3 bytes).
#[cfg(feature = "remote-debug")]
fn notify_payload_max<P: PacketPool>(gatt: &GattConnection<'_, '_, P>) -> usize {
    (gatt.raw().att_mtu() as usize)
        .saturating_sub(3)
        .clamp(20, 244)
}

/// Notify `bytes` in ATT-sized slices. `store` is false: do not write
/// a 48 KiB value into the GATT table.
#[cfg(feature = "remote-debug")]
async fn notify_bytes<P: PacketPool>(
    gatt: &GattConnection<'_, '_, P>,
    tx: &Characteristic<u8>,
    bytes: &[u8],
) {
    let max = notify_payload_max(gatt);
    for part in bytes.chunks(max) {
        let _ = tx.notify_raw(gatt, part, false).await;
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
