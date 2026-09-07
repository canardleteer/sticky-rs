//! Insecure desk debug: last composed planes and a synthetic tap mux.
//!
//! `--features remote-debug` only. The default image does not compile
//! this module. There is no UART RX parser and no SoftAP / BLE
//! framebuffer protocol in this change — those callers land later and
//! must use the same types.
//!
//! # What this is
//!
//! On the unit: a later injector should hit the same START/STOP rect
//! as a finger. UART `touch` lines gain ` src=phys` / `src=syn` so a
//! desk log can tell them apart. Default-image `p0=` lines stay
//! unchanged when this feature is off.
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

use embassy_debug::{ExpectedFrame, FrameKind, TouchSample, LOG_PREFIX};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::channel::Channel;
use esp_println::println;
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

/// Last composed black/white plane (pre-rotation 800×480, packed).
///
/// Taken once into [`LAST`]. Not the panel’s SPI readout.
static LAST_BW: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);

/// Last composed red/gray plane, or unused zeros on a 1-bit card.
static LAST_RED: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);

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

/// Last compose. Display task writes; a later GET can read.
static LAST: Mutex<CriticalSectionRawMutex, RefCell<Option<LastInner>>> =
    Mutex::new(RefCell::new(None));

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

/// Copy planes into [`LAST`] under the critical-section mutex.
///
/// Expectation: `bw.len()` is [`display::PLANE_BYTES`]; `red` is
/// `Some` only for [`FrameKind::Gray4`]. A consistent frame sets
/// [`LastInner::ready`]. An inconsistent copy prints
/// `remote last inconsistent` and stays unpublished so a later
/// reader does not serve a torn pair.
fn publish_compose(
    bw: &[u8; display::PLANE_BYTES],
    red: Option<&[u8; display::PLANE_BYTES]>,
    kind: FrameKind,
    hold: PageRotation,
) {
    LAST.lock(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(LastInner {
                bw: LAST_BW.take(),
                red: LAST_RED.take(),
                kind,
                hold,
                ready: false,
            });
        }
        let last = slot.as_mut().expect("last compose cell");
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
/// [`LAST`] itself.
///
/// No caller in this change (no framebuffer protocol yet).
#[allow(dead_code)]
pub(crate) fn with_expected_frame<R>(
    f: impl FnOnce(Option<ExpectedFrame<'_, PageRotation>>) -> R,
) -> R {
    LAST.lock(|cell| {
        let inner = cell.borrow();
        match inner.as_ref() {
            Some(last) if last.ready => f(Some(ExpectedFrame {
                width: display::WIDTH,
                height: display::HEIGHT,
                kind: last.kind,
                hold: Some(last.hold),
                bw: last.bw.as_slice(),
                red: match last.kind {
                    FrameKind::Mono => None,
                    FrameKind::Gray4 => Some(last.red.as_slice()),
                },
            })),
            _ => f(None),
        }
    })
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
