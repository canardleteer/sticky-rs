//! Insecure desk debug: last composed planes and a synthetic tap mux.
//!
//! `--features remote-debug` only. The default image does not compile
//! this module. UART stays plaintext (`snap` / `src=syn`). The BLE
//! listener lives in [`crate::pair`]: encrypted RX write →
//! [`handle_envelope`], TX notify of a streamed `Snapshot`.
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
//! LAST lives in a 96 KiB **octal PSRAM carve**, not `.bss`. Pair +
//! Wi-Fi + DRAW/TX already fill internal DRAM; a second pair of
//! planes in `.bss` fails the S3 `dram_seg` / `stack.x` link
//! (`cannot move location counter backwards`). DRAW/TX stay in
//! DRAM so panel SPI DMA does not bounce from PSRAM. Do not add
//! the rest of PSRAM to the global heap (S3 atomics are wrong
//! there; BLE / Wi-Fi `malloc` stays Internal). Desk-only. Do not
//! enable on a battery sit you care about.
//!
//! # Safety
//!
//! This path is **insecure**. Anyone who can inject a
//! [`embassy_debug::TouchSample`] later (UART, SoftAP, BLE) can tap
//! START. Keep the feature off in the default image.

use core::cell::RefCell;

use embassy_debug::{
    Event, ExpectedFrame, FrameKind, Scene, SnapOp, TargetKind, TouchSample, TouchSource,
    LINE_CAPACITY, LOG_PREFIX,
};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use esp_println::println;
use remote_debug_peripheral::{handle_envelope as dispatch_envelope, Device};
use remote_debug_wire::v1::{TargetKind as WireTargetKind, TouchPhase, TouchSpace};
use remote_debug_wire::{
    frame_kind_to_wire, AckOutcome, ClearOutcome, FrameError, GetOutcome, SnapshotMeta,
    SnapshotSlot, StickyLayout,
};
use seeed_reterminal_sticky::display::{self, inject_framebuffer_for_page, PageRotation};

/// How many pending synthetic taps the mux keeps.
///
/// Embassy [`Channel`] depth. [`crate::touch_task`] polls at board
/// [`seeed_reterminal_sticky::touch::STATUS_POLL_MS`]. Eight slots
/// cover a five-point slide stroke plus a short desk burst.
const SYNTHETIC_CAP: usize = 8;

/// Injected framebuffer taps (plus phase). [`crate::touch_task`] is the only receiver.
///
/// *The Embassy Book*: a `Channel` is MPMC; `try_send` / `try_receive`
/// do not wait. Overflow drops the tap (same idea as [`crate::emit`]).
static SYNTHETIC: Channel<CriticalSectionRawMutex, SyntheticTouch, SYNTHETIC_CAP> = Channel::new();

/// UART `format_event` copies for GATT `LogLine` (Target / Scene only).
static LOG_LINES: Channel<CriticalSectionRawMutex, QueuedLog, SYNTHETIC_CAP> = Channel::new();

/// Wake the BLE task when a Target / Scene line is queued.
static LOG_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Injected short-press edges. [`crate::button_task`] is the only receiver.
///
/// Hold-to-standby / sleep stays on the physical pads. Depth matches
/// the tap mux so a desk burst of OK / Page keys does not need a
/// large `.bss` queue.
static BUTTONS: Channel<CriticalSectionRawMutex, SyntheticButton, SYNTHETIC_CAP> = Channel::new();

/// How many bytes [`map_last_planes`] carves from mapped PSRAM.
const LAST_CARVE_BYTES: usize = display::PLANE_BYTES * 2;

/// One queued synthetic tap plus finger phase.
///
/// `DOWN` is a lift→down edge (dots score). `MOVE` feeds slides
/// without `dispatch_first_contact`. `UP` lifts without a first-contact
/// beep (*The Embassy Book*: one mux, one consumer).
#[derive(Clone, Copy)]
pub(crate) struct SyntheticTouch {
    /// Pre-rotation framebuffer sample.
    pub sample: TouchSample,
    /// Wire phase (unset treated as DOWN by the mapper).
    pub phase: TouchPhase,
}

/// One GATT `LogLine` waiting on the BLE task.
#[derive(Clone, Copy)]
struct QueuedLog {
    t_ms: u32,
    len: u8,
    text: [u8; LINE_CAPACITY],
}

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
/// `bw` / `red` are `'static` after [`map_last_planes`] carves them
/// from mapped PSRAM. The mutex is a critical section (*The
/// Embedded Rust Book*: share by locking, not by cloning 48 KiB
/// onto the stack).
struct LastInner {
    /// Native RAM black/white plane.
    bw: &'static mut [u8; display::PLANE_BYTES],
    /// Native RAM red/gray plane (ignored when [`FrameKind::Mono`]).
    red: &'static mut [u8; display::PLANE_BYTES],
    /// One plane or the gray4 pair.
    kind: FrameKind,
    /// In-plane hold at compose (`View::from_hold`).
    hold: PageRotation,
    /// `Scene::persist_byte` at compose.
    scene: u8,
    /// Targets walk id, or `0xff` when not on that card.
    target_step: u8,
    /// Wire mark kind (unspecified off targets).
    target_kind: WireTargetKind,
    /// Expected page X when on targets.
    target_expect_x: u16,
    /// Expected page Y when on targets.
    target_expect_y: u16,
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

/// Map in-package octal PSRAM and install the LAST carve.
///
/// Call once after the latch, before the first compose. Board fact:
/// ESP32-S3R8 **8 MB octal** at 3.3 V (`AP_3v3`). 80 MHz is a proven
/// firmware configuration, not an eFuse field (hardware skill
/// **PSRAM**). `esp_hal::psram::Psram::new` programs MSPI + DCache
/// MMU; we then take the first [`LAST_CARVE_BYTES`] and leave the
/// rest unmapped from the global heap.
///
/// On the unit: UART `remote psram last=` means the carve is live.
/// `remote psram map failed` means [`publish_compose`] will skip
/// (no snapshot) rather than panic. DRAW/TX stay in DRAM.
///
/// In the MCU (*The Embedded Rust Book* `unsafe` at the HAL
/// boundary): `Psram::raw_parts` is the mapped window. The two
/// plane pointers are exclusive; nothing else in this image
/// reads that window. [`core::mem::forget`] keeps the PSRAM
/// singleton claimed for the process lifetime (same latch-pin
/// pattern; the type has no `Drop` that unmaps).
///
/// # Safety
///
/// The `unsafe` block assumes `start` is non-null, `size` is at
/// least [`LAST_CARVE_BYTES`], and the window stays mapped. The
/// null / size checks above are the guard.
pub(crate) fn map_last_planes(psram: esp_hal::peripherals::PSRAM<'static>) {
    let config = esp_hal::psram::PsramConfig {
        mode: esp_hal::psram::PsramMode::OctalSpi,
        size: esp_hal::psram::PsramSize::Size(8 * 1024 * 1024),
        ram_frequency: esp_hal::psram::SpiRamFreq::Freq80m,
        ..Default::default()
    };
    let mapped = esp_hal::psram::Psram::new(psram, config);
    let (start, size) = mapped.raw_parts();
    if start.is_null() || size < LAST_CARVE_BYTES {
        println!("{LOG_PREFIX}: remote psram map failed size={size}");
        core::mem::forget(mapped);
        return;
    }
    // Exclusive carve of the first two planes. `start` is the MMU
    // window; `PLANE_BYTES` is 48_000 (64-byte aligned).
    // SAFETY: null / size checked above; window stays mapped.
    let (bw, red) = unsafe {
        let bw = &mut *start.cast::<[u8; display::PLANE_BYTES]>();
        let red = &mut *start
            .add(display::PLANE_BYTES)
            .cast::<[u8; display::PLANE_BYTES]>();
        (bw, red)
    };
    bw.fill(0);
    red.fill(0);
    REMOTE.lock(|cell| {
        let mut remote = cell.borrow_mut();
        remote.last = Some(LastInner {
            bw,
            red,
            kind: FrameKind::Mono,
            hold: PageRotation::Portrait0,
            scene: 0,
            target_step: 0xff,
            target_kind: WireTargetKind::TARGET_KIND_UNSPECIFIED,
            target_expect_x: 0,
            target_expect_y: 0,
            ready: false,
        });
    });
    println!("{LOG_PREFIX}: remote psram last={LAST_CARVE_BYTES}");
    core::mem::forget(mapped);
}

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
        // PSRAM map failed, or `map_last_planes` was not called.
        let Some(last) = remote.last.as_mut() else {
            return;
        };
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
        fill_compose_meta(last);
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

/// Record scene / targets mark for the next `Snapshot` (UART stays the
/// same). Splash persist `0` is Ferris; digits are pair-card only.
fn fill_compose_meta(last: &mut LastInner) {
    let scene = crate::pair::current_scene();
    last.scene = scene.map(Scene::persist_byte).unwrap_or(0);
    if scene == Some(Scene::Targets) {
        let mark = crate::targets::current_mark(last.hold);
        last.target_step = mark.id;
        last.target_kind = match mark.kind {
            TargetKind::Dot => WireTargetKind::TARGET_KIND_DOT,
            TargetKind::SlideX => WireTargetKind::TARGET_KIND_SLIDE_X,
            TargetKind::SlideY => WireTargetKind::TARGET_KIND_SLIDE_Y,
        };
        last.target_expect_x = mark.x;
        last.target_expect_y = mark.y;
    } else {
        last.target_step = 0xff;
        last.target_kind = WireTargetKind::TARGET_KIND_UNSPECIFIED;
        last.target_expect_x = 0;
        last.target_expect_y = 0;
    }
}

/// Wire scalars for the armed pull (planes stay in LAST).
fn snapshot_meta_from_last(nonce: u64, last: &LastInner) -> SnapshotMeta {
    let on_targets = last.target_step != 0xff;
    SnapshotMeta {
        nonce,
        width: display::WIDTH,
        height: display::HEIGHT,
        kind: frame_kind_to_wire(last.kind),
        hold: Some(hold_token(last.hold)),
        scene: Some(u32::from(last.scene)),
        target_step: on_targets.then_some(u32::from(last.target_step)),
        target_kind: last.target_kind,
        target_expect_x: on_targets.then_some(u32::from(last.target_expect_x)),
        target_expect_y: on_targets.then_some(u32::from(last.target_expect_y)),
    }
}

/// Visit the last compose under the same lock used to publish.
///
/// `None` before the first paint or after an inconsistent copy.
/// The `ExpectedFrame` borrows the LAST planes; do not store it
/// past this call (*The Embassy Book*: the mutex guard ends when
/// `f` returns). SoftAP GET (later) can use this instead of
/// touching [`REMOTE`] itself.
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
/// `None` when the slot is empty. [`crate::pair`] encodes
/// `Snapshot` from these slices — do not `Vec` the 48 KiB planes
/// (*The Embedded Rust Book*: the implementor owns the buffers).
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

/// Armed pull scalars plus plane lengths (no 48 KiB clone).
pub(crate) fn with_armed_snapshot_meta<R>(
    f: impl FnOnce(Option<(SnapshotMeta, usize, usize)>) -> R,
) -> R {
    REMOTE.lock(|cell| {
        let remote = cell.borrow();
        match (remote.slot.armed_nonce(), remote.last.as_ref()) {
            (Some(nonce), Some(last)) if last.ready => {
                let red_len = match last.kind {
                    FrameKind::Mono => 0,
                    FrameKind::Gray4 => display::PLANE_BYTES,
                };
                f(Some((
                    snapshot_meta_from_last(nonce, last),
                    display::PLANE_BYTES,
                    red_len,
                )))
            }
            _ => f(None),
        }
    })
}

/// Last compose hold (PAGE inject). Portrait0 before the first paint.
fn last_hold() -> PageRotation {
    REMOTE.lock(|cell| {
        cell.borrow()
            .last
            .as_ref()
            .map(|last| last.hold)
            .unwrap_or(PageRotation::Portrait0)
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
    rotation.hold_token()
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
    inject_synthetic(SyntheticTouch {
        sample,
        phase: TouchPhase::TOUCH_PHASE_DOWN,
    })
}

/// Queue a phased tap. Returns `false` when [`SYNTHETIC`] is full.
pub(crate) fn inject_synthetic(touch: SyntheticTouch) -> bool {
    SYNTHETIC.try_send(touch).is_ok()
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
pub(crate) fn take_synthetic() -> Option<SyntheticTouch> {
    SYNTHETIC.try_receive().ok()
}

/// Copy one Target / Scene UART line for GATT (never a PIN, never a MAC).
pub(crate) fn queue_log_line(event: &Event) {
    let t_ms = match event {
        Event::Target { t_ms, .. } | Event::Scene { t_ms, .. } => *t_ms,
        _ => return,
    };
    let mut buf = [0u8; LINE_CAPACITY];
    let Ok(line) = embassy_debug::format_event(event, &mut buf) else {
        return;
    };
    let mut queued = QueuedLog {
        t_ms,
        len: line.len() as u8,
        text: [0u8; LINE_CAPACITY],
    };
    queued.text[..line.len()].copy_from_slice(line.as_bytes());
    if LOG_LINES.try_send(queued).is_ok() {
        LOG_READY.signal(());
    }
}

/// BLE task waits here for a queued `LogLine`.
pub(crate) fn wait_log_ready() -> impl core::future::Future<Output = ()> {
    LOG_READY.wait()
}

/// Copy one queued UART line into `buf`. Returns `(t_ms, len)`.
#[must_use]
pub(crate) fn take_log_line(buf: &mut [u8]) -> Option<(u32, usize)> {
    let queued = LOG_LINES.try_receive().ok()?;
    let n = usize::from(queued.len);
    if n > buf.len() {
        return None;
    }
    buf[..n].copy_from_slice(&queued.text[..n]);
    Some((queued.t_ms, n))
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

pub(crate) use remote_debug_peripheral::EnvelopeOutcome;

/// Sticky image callbacks for [`dispatch_envelope`].
struct FwDevice;

impl Device for FwDevice {
    fn on_inject_touch(&mut self, sample: TouchSample, phase: TouchPhase, space: TouchSpace) {
        handle_inject_touch_mapped(sample, phase, space);
    }

    fn on_inject_button(&mut self, key_id: u8, down: bool) {
        handle_inject_button_id(key_id, down);
    }

    fn last_ready(&self) -> bool {
        REMOTE.lock(|cell| cell.borrow().last.as_ref().is_some_and(|last| last.ready))
    }

    fn slot_get(&mut self, nonce: u64, last_ready: bool) -> GetOutcome {
        let outcome = REMOTE.lock(|cell| cell.borrow_mut().slot.on_get(nonce, last_ready));
        let (op, n) = match outcome {
            GetOutcome::Armed => (SnapOp::Get, nonce),
            GetOutcome::Retry => (SnapOp::Retry, nonce),
            GetOutcome::Busy { armed } => (SnapOp::Busy { armed }, nonce),
            GetOutcome::Empty => (SnapOp::Empty, nonce),
            GetOutcome::Zero => (SnapOp::GetZero, 0),
        };
        emit_snap(op, n);
        outcome
    }

    fn slot_ack(&mut self, nonce: u64) -> AckOutcome {
        let outcome = REMOTE.lock(|cell| cell.borrow_mut().slot.on_ack(nonce));
        let (op, n) = match outcome {
            AckOutcome::Released => (SnapOp::Ack, nonce),
            AckOutcome::Miss { armed } => (SnapOp::AckMiss { armed }, nonce),
            AckOutcome::Stale => (SnapOp::AckStale, nonce),
            AckOutcome::Zero => (SnapOp::AckZero, 0),
        };
        emit_snap(op, n);
        outcome
    }

    fn slot_clear(&mut self) -> ClearOutcome {
        REMOTE.lock(|cell| {
            let _ = cell.borrow_mut().slot.on_clear();
        });
        emit_snap(SnapOp::Clear, 0);
        ClearOutcome::Cleared
    }

    fn on_reboot(&mut self) {
        crate::emit(Event::RemoteReboot {
            t_ms: crate::now_ms(),
        });
    }
}

/// Decode one framed Envelope and apply it through [`FwDevice`].
///
/// Injects go to the tap / key mux. Snapshot Get / Ack / Clear
/// update the frozen slot and emit the `snap` UART line. `Reboot`
/// emits `remote reboot` and asks the BLE task to ACK then
/// software-reset the **MCU**.
///
/// # Errors
///
/// [`FrameError`] when the bytes are not one version-1 envelope.
pub(crate) fn handle_envelope(bytes: &[u8]) -> Result<EnvelopeOutcome, FrameError> {
    dispatch_envelope::<FwDevice, StickyLayout>(&mut FwDevice, bytes)
}

/// Queue a decoded tap, or emit `touch drop src=syn`.
fn handle_inject_touch_mapped(mut sample: TouchSample, phase: TouchPhase, space: TouchSpace) {
    if space == TouchSpace::TOUCH_SPACE_PAGE {
        let Some((fx, fy)) = inject_framebuffer_for_page(sample.x, sample.y, last_hold()) else {
            emit_touch_drop();
            return;
        };
        sample.x = fx;
        sample.y = fy;
    }
    if framebuffer_to_uart_screen(sample.x, sample.y).is_none() {
        emit_touch_drop();
        return;
    }
    if !inject_synthetic(SyntheticTouch { sample, phase }) {
        emit_touch_drop();
    }
}

/// Print `btn … src=syn` and queue a short press.
fn handle_inject_button_id(gpio: u8, down: bool) {
    crate::emit(Event::Button {
        t_ms: crate::now_ms(),
        gpio,
        down,
        source: TouchSource::Synthetic,
    });
    let _ = inject_button(gpio, down);
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
