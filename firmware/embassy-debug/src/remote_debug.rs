//! Insecure desk debug: last composed planes and a synthetic tap mux.
//!
//! `--features remote-debug` only. The default image does not compile
//! this module. There is no UART RX parser and no SoftAP / BLE
//! framebuffer protocol in this change — those callers land later and
//! must use [`handle_envelope`].
//!
//! # What this is
//!
//! On the unit: a later injector should hit the same START/STOP rect
//! as a finger. UART `touch` lines gain ` src=phys` / `src=syn` so a
//! desk log can tell them apart. Default-image `p0=` lines stay
//! unchanged when this feature is off.
//!
//! Snapshot capacity is **one frozen slot**. [`publish_compose`] fills
//! LAST only while the slot is empty. A host `GetSnapshot` arms a
//! nonce; later splash / legend / wifi paints stay on DRAW/TX and do
//! not stomp the pull. Matching `SnapshotAck` or `SnapshotClear`
//! releases. Encode from the static planes — do not `Vec` 48 KiB.
//!
//! In the MCU (*The Embassy Book* channels and a critical-section
//! mutex; *The Embedded Rust Book* shared state):
//!
//! - **Snapshot is last compose, not a panel readout.**
//!   [`publish_mono`] / [`publish_gray4`] copy the **DRAW** planes
//!   after compose and **before** the 1-bit path wipes DRAW to send
//!   a cleared second RAM. Do not snapshot `TX` (that is the 180°
//!   transmit copy on 1-bit cards). Gray4 already stores the red
//!   plane in the buffer named `tx` at the caller; that is compose
//!   data, not the mono rotate destination.
//! - **Synthetic taps are framebuffer pixels.** Same 800×480
//!   pre-rotation space as [`embassy_debug::ExpectedFrame`]. Not UART
//!   `p0=` / [`seeed_reterminal_sticky::touch::to_screen`], not a raw
//!   GT911 480×800 sample.
//! - **Source does not change hit-test algebra.**
//!   [`embassy_debug::PanelTouchAlign`] ignores
//!   [`embassy_debug::TouchSource`]. [`crate::touch_task`] tags the
//!   UART line and then calls the same first-contact path as GT911.
//!
//! Extra `.bss`: two [`display::PLANE_BYTES`] copies (96 KiB). Desk-only.
//! Do not enable on a battery sit you care about.
//!
//! # Safety
//!
//! This path is **insecure**. Anyone who can inject a
//! [`embassy_debug::TouchSample`] later (UART, SoftAP, BLE) can tap
//! START. Keep the feature off in the default image.

use core::cell::RefCell;

use embassy_debug::{
    Event, ExpectedFrame, FrameKind, SnapOp, TouchSample, TouchSource, LOG_PREFIX,
};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::channel::Channel;
use esp_println::println;
use remote_debug_wire::v1::envelope::Body;
use remote_debug_wire::{
    decode_envelope, inject_button_gpio, inject_touch_sample, AckOutcome, FrameError, GetOutcome,
    SnapshotSlot,
};
use seeed_reterminal_sticky::display::{self, PageRotation};
use static_cell::ConstStaticCell;

/// How many pending synthetic taps the mux keeps.
///
/// Embassy [`Channel`] depth. [`crate::touch_task`] polls at board
/// [`seeed_reterminal_sticky::touch::STATUS_POLL_MS`]. Four slots
/// cover a short desk burst without a large `.bss` queue.
const SYNTHETIC_CAP: usize = 4;

/// Injected framebuffer taps. [`crate::touch_task`] is the only receiver.
///
/// *The Embassy Book*: a `Channel` is MPMC; `try_send` / `try_receive`
/// do not wait. Overflow drops the tap (same idea as [`crate::emit`]).
static SYNTHETIC: Channel<CriticalSectionRawMutex, TouchSample, SYNTHETIC_CAP> = Channel::new();

/// Injected short-press edges. [`crate::button_task`] is the only receiver.
///
/// Hold-to-standby / sleep stays on the physical pads. Depth matches
/// the tap mux so a desk burst of OK / Page keys does not need a
/// large `.bss` queue.
static BUTTONS: Channel<CriticalSectionRawMutex, SyntheticButton, SYNTHETIC_CAP> = Channel::new();

/// Last composed black/white plane (pre-rotation 800×480, packed).
///
/// Taken once into [`REMOTE`]. Not the panel’s SPI readout.
static LAST_BW: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);

/// Last composed red/gray plane, or unused zeros on a 1-bit card.
static LAST_RED: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);

/// One queued product-key edge (GPIO 4 / 5 / 6).
///
/// Short-press walk only. The physical hold machines stay on the
/// real pads (*The Embedded Rust Book*: do not invent a second
/// scene state machine for injects).
#[derive(Clone, Copy)]
pub(crate) struct SyntheticButton {
    /// Sticky product pad (`4` OK, `5` Page Up, `6` Page Down).
    pub gpio: u8,
    /// `true` on press (same sense as a physical `1 -> 0`).
    pub down: bool,
}

/// Planes plus the hold used to compose them.
///
/// `bw` / `red` are `'static` after [`ConstStaticCell::take`]. The
/// mutex is a critical section (*The Embedded Rust Book*: share by
/// locking, not by cloning 48 KiB onto the stack).
struct LastInner {
    /// Native RAM black/white plane.
    bw: &'static mut [u8; display::PLANE_BYTES],
    /// Native RAM red/gray plane (ignored when [`FrameKind::Mono`]).
    red: &'static mut [u8; display::PLANE_BYTES],
    /// One plane or the gray4 pair.
    kind: FrameKind,
    /// In-plane hold at compose (`View::from_hold`).
    hold: PageRotation,
    /// False before the first successful publish, or if the copy failed
    /// [`ExpectedFrame::is_consistent`].
    ready: bool,
}

/// LAST planes plus the capacity-1 pull slot.
///
/// One mutex so `publish_compose` and `GetSnapshot` cannot race a
/// freeze (*The Embassy Book*: share by locking).
struct RemoteInner {
    /// Last consistent compose, or `None` before the first paint.
    last: Option<LastInner>,
    /// Host pull id, or empty.
    slot: SnapshotSlot,
}

/// Last compose and the frozen pull. Display task writes; Get/Ack/Clear
/// freeze or release.
static REMOTE: Mutex<CriticalSectionRawMutex, RefCell<RemoteInner>> =
    Mutex::new(RefCell::new(RemoteInner {
        last: None,
        slot: SnapshotSlot::new(),
    }));

/// Copy a 1-bit DRAW plane after compose and before `draw.fill(0)`.
///
/// `draw` is pre-rotation panel RAM. Do not pass the `TX` 180° copy.
/// `hold` is the IMU page used to compose (*The Embassy Book*: the
/// display task owns compose; this only publishes).
pub(crate) fn publish_mono(draw: &[u8; display::PLANE_BYTES], hold: PageRotation) {
    publish_compose(draw, None, FrameKind::Mono, hold);
}

/// Copy gray4 DRAW planes (black/white + red) after compose.
///
/// On this image the caller stores the red plane in the buffer named
/// `tx`. That is still compose data, not [`ssd1677_gray4::planes::rotate180_mono`]
/// output. Do not publish after a failed draw.
pub(crate) fn publish_gray4(
    bw: &[u8; display::PLANE_BYTES],
    red: &[u8; display::PLANE_BYTES],
    hold: PageRotation,
) {
    publish_compose(bw, Some(red), FrameKind::Gray4, hold);
}

/// Copy planes into [`REMOTE`] under the critical-section mutex.
///
/// Expectation: `bw.len()` is [`display::PLANE_BYTES`]; `red` is
/// `Some` only for [`FrameKind::Gray4`]. A consistent frame sets
/// [`LastInner::ready`]. An inconsistent copy prints
/// `remote last inconsistent` and stays unpublished so a later
/// reader does not serve a torn pair.
///
/// While the snapshot slot is [`SnapshotSlot::is_armed`], this
/// returns without touching the planes so a later card cannot
/// stomp the pull the host is sending.
fn publish_compose(
    bw: &[u8; display::PLANE_BYTES],
    red: Option<&[u8; display::PLANE_BYTES]>,
    kind: FrameKind,
    hold: PageRotation,
) {
    REMOTE.lock(|cell| {
        let mut remote = cell.borrow_mut();
        if remote.slot.is_armed() {
            return;
        }
        if remote.last.is_none() {
            remote.last = Some(LastInner {
                bw: LAST_BW.take(),
                red: LAST_RED.take(),
                kind,
                hold,
                ready: false,
            });
        }
        let last = remote.last.as_mut().expect("last compose cell");
        last.bw.copy_from_slice(bw);
        match (kind, red) {
            (FrameKind::Gray4, Some(red)) => last.red.copy_from_slice(red),
            (FrameKind::Mono, _) => last.red.fill(0),
            (FrameKind::Gray4, None) => {
                println!("{LOG_PREFIX}: remote last missing red");
                last.ready = false;
                return;
            }
        }
        last.kind = kind;
        last.hold = hold;
        let frame = ExpectedFrame {
            width: display::WIDTH,
            height: display::HEIGHT,
            kind,
            hold: Some(hold),
            bw: last.bw.as_slice(),
            red: match kind {
                FrameKind::Mono => None,
                FrameKind::Gray4 => Some(last.red.as_slice()),
            },
        };
        if frame.is_consistent() {
            last.ready = true;
        } else {
            println!("{LOG_PREFIX}: remote last inconsistent");
            last.ready = false;
        }
    });
}

/// Visit the last compose under the same lock used to publish.
///
/// `None` before the first paint or after an inconsistent copy.
/// The `ExpectedFrame` borrows the LAST planes; do not store it
/// past this call (*The Embassy Book*: the mutex guard ends when
/// `f` returns). A later SoftAP GET uses this instead of touching
/// [`REMOTE`] itself.
///
/// No caller in this change (no framebuffer protocol yet).
#[allow(dead_code)]
pub(crate) fn with_expected_frame<R>(
    f: impl FnOnce(Option<ExpectedFrame<'_, PageRotation>>) -> R,
) -> R {
    REMOTE.lock(|cell| {
        let remote = cell.borrow();
        match remote.last.as_ref() {
            Some(last) if last.ready => f(Some(expected_from_last(last))),
            _ => f(None),
        }
    })
}

/// Visit the armed pull (same planes as LAST, plus the host nonce).
///
/// `None` when the slot is empty. A later transport encodes
/// `Snapshot` from these slices — do not `Vec` the 48 KiB planes
/// (*The Embedded Rust Book*: the implementor owns the buffers).
///
/// No caller in this change (no transport listener yet).
#[allow(dead_code)]
pub(crate) fn with_armed_frame<R>(
    f: impl FnOnce(Option<(u64, ExpectedFrame<'_, PageRotation>)>) -> R,
) -> R {
    REMOTE.lock(|cell| {
        let remote = cell.borrow();
        match (remote.slot.armed_nonce(), remote.last.as_ref()) {
            (Some(nonce), Some(last)) if last.ready => f(Some((nonce, expected_from_last(last)))),
            _ => f(None),
        }
    })
}

/// Borrow LAST as an [`ExpectedFrame`]. Caller holds [`REMOTE`].
fn expected_from_last(last: &LastInner) -> ExpectedFrame<'_, PageRotation> {
    ExpectedFrame {
        width: display::WIDTH,
        height: display::HEIGHT,
        kind: last.kind,
        hold: Some(last.hold),
        bw: last.bw.as_slice(),
        red: match last.kind {
            FrameKind::Mono => None,
            FrameKind::Gray4 => Some(last.red.as_slice()),
        },
    }
}

/// Sticky `PageRotation` discriminant for proto `Snapshot.hold`.
///
/// The schema stays a product token, not a GPIO. Portrait0 = 0,
/// Portrait180 = 1, Landscape0 = 2, Landscape180 = 3.
#[allow(dead_code)]
pub(crate) fn hold_token(rotation: PageRotation) -> u32 {
    match rotation {
        PageRotation::Portrait0 => 0,
        PageRotation::Portrait180 => 1,
        PageRotation::Landscape0 => 2,
        PageRotation::Landscape180 => 3,
    }
}

/// Queue one framebuffer tap for [`crate::touch_task`].
///
/// `sample.x` / `sample.y` are pre-rotation 800×480. Tag
/// [`embassy_debug::TouchSource::Synthetic`]. Returns `false` when
/// [`SYNTHETIC`] is full (the tap is dropped).
///
/// SoftAP / BLE injectors call this later. This change has no UART
/// RX parser and no radio path.
#[allow(dead_code)]
pub(crate) fn inject_touch(sample: TouchSample) -> bool {
    SYNTHETIC.try_send(sample).is_ok()
}

/// Queue one short-press product key for [`crate::button_task`].
///
/// Returns `false` when [`BUTTONS`] is full. UART for the inject is
/// emitted by [`handle_envelope`] so a full channel still logs.
#[allow(dead_code)]
pub(crate) fn inject_button(gpio: u8, down: bool) -> bool {
    BUTTONS.try_send(SyntheticButton { gpio, down }).is_ok()
}

/// Wait for one queued synthetic key (*The Embassy Book* `Channel`).
///
/// [`crate::button_task`] `select`s this with the three physical
/// pads so an inject can wake the same task.
pub(crate) async fn wait_button() -> SyntheticButton {
    BUTTONS.receive().await
}

/// Non-blocking take of one queued synthetic tap.
///
/// [`crate::touch_task`] calls this each poll, before or instead of
/// a GT911 Status read. `None` means the glass path can run.
#[must_use]
pub(crate) fn take_synthetic() -> Option<TouchSample> {
    SYNTHETIC.try_receive().ok()
}

/// Pre-rotation framebuffer → UART `p0=` glass space.
///
/// [`seeed_reterminal_sticky::display::screen_to_framebuffer`] is a
/// 180° involution (`(W-1-x, H-1-y)`). Applying it to a framebuffer
/// point yields the glass point [`seeed_reterminal_sticky::touch::to_screen`]
/// would have printed for the matching GT911 sample. Out of 800×480
/// is `None` (drop the inject).
#[must_use]
pub(crate) fn framebuffer_to_uart_screen(fx: u16, fy: u16) -> Option<(u16, u16)> {
    seeed_reterminal_sticky::display::screen_to_framebuffer(fx, fy)
}

/// Decode one framed [`remote_debug_wire::v1::Envelope`] and apply it.
///
/// Injects go to the tap / key mux. Snapshot Get / Ack / Clear
/// update the frozen slot and emit the `snap` UART line. There is
/// no listener in this image — a later SoftAP or BLE path calls
/// this (*The Embassy Book*: keep protocol out of the display task).
///
/// # Errors
///
/// [`FrameError`] when the bytes are not one version-1 envelope.
#[allow(dead_code)]
pub(crate) fn handle_envelope(bytes: &[u8]) -> Result<(), FrameError> {
    let env = decode_envelope(bytes)?;
    match env.body {
        Some(Body::InjectTouch(msg)) => handle_inject_touch(&msg),
        Some(Body::InjectButton(msg)) => handle_inject_button(&msg),
        Some(Body::GetSnapshot(msg)) => handle_get(msg.nonce),
        Some(Body::SnapshotAck(msg)) => handle_ack(msg.nonce),
        Some(Body::SnapshotClear(_)) => handle_clear(),
        Some(Body::Snapshot(_) | Body::SnapshotBusy(_) | Body::LogLine(_)) | None => {}
    }
    Ok(())
}

/// Map `InjectTouch` and queue it, or emit `touch drop src=syn`.
///
/// Out of 800×480, a failed map, or a full [`SYNTHETIC`] channel
/// are all drops. UART always logs the drop so a desk log is not
/// silent (*The Embedded Rust Book*: ignore is not silence).
fn handle_inject_touch(msg: &remote_debug_wire::v1::InjectTouch) {
    let Ok(sample) = inject_touch_sample(msg) else {
        emit_touch_drop();
        return;
    };
    if framebuffer_to_uart_screen(sample.x, sample.y).is_none() {
        emit_touch_drop();
        return;
    }
    if !inject_touch(sample) {
        emit_touch_drop();
    }
}

/// Map `InjectButton`, print `btn … src=syn`, and queue a short press.
///
/// An unspecified key is dropped without a UART line (nothing to
/// map). A full [`BUTTONS`] channel still printed the edge so the
/// inject is visible; the walk does not run.
fn handle_inject_button(msg: &remote_debug_wire::v1::InjectButton) {
    let Ok(gpio) = inject_button_gpio(&msg.key) else {
        return;
    };
    crate::emit(Event::Button {
        t_ms: crate::now_ms(),
        gpio,
        down: msg.down,
        source: TouchSource::Synthetic,
    });
    let _ = inject_button(gpio, msg.down);
}

/// Arm or refuse `GetSnapshot`. Always logs.
fn handle_get(nonce: u64) {
    let outcome = REMOTE.lock(|cell| {
        let mut remote = cell.borrow_mut();
        let last_ready = remote.last.as_ref().is_some_and(|last| last.ready);
        remote.slot.on_get(nonce, last_ready)
    });
    let (op, n) = match outcome {
        GetOutcome::Armed => (SnapOp::Get, nonce),
        GetOutcome::Retry => (SnapOp::Retry, nonce),
        GetOutcome::Busy { armed } => (SnapOp::Busy { armed }, nonce),
        GetOutcome::Empty => (SnapOp::Empty, nonce),
        GetOutcome::Zero => (SnapOp::GetZero, 0),
    };
    emit_snap(op, n);
}

/// Release on a matching Ack. Mismatch / zero / stale log and stay.
fn handle_ack(nonce: u64) {
    let outcome = REMOTE.lock(|cell| cell.borrow_mut().slot.on_ack(nonce));
    let (op, n) = match outcome {
        AckOutcome::Released => (SnapOp::Ack, nonce),
        AckOutcome::Miss { armed } => (SnapOp::AckMiss { armed }, nonce),
        AckOutcome::Stale => (SnapOp::AckStale, nonce),
        AckOutcome::Zero => (SnapOp::AckZero, 0),
    };
    emit_snap(op, n);
}

/// Operator abort. Always logs `snap clear`.
fn handle_clear() {
    REMOTE.lock(|cell| {
        let _ = cell.borrow_mut().slot.on_clear();
    });
    emit_snap(SnapOp::Clear, 0);
}

/// One `snap` UART line (`format_event` contract).
fn emit_snap(op: SnapOp, nonce: u64) {
    crate::emit(Event::Snap {
        t_ms: crate::now_ms(),
        op,
        nonce,
    });
}

/// One `touch drop src=syn` UART line.
pub(crate) fn emit_touch_drop() {
    crate::emit(Event::TouchDrop {
        t_ms: crate::now_ms(),
    });
}
