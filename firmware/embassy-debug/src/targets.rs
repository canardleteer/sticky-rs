//! Interactive touch-validation walk (`Scene::Targets`).
//!
//! # Architecture
//!
//! PaperMono's Targets card is a blocking walk that ends on a white
//! clear. This image keeps the same five dots and two midline slides,
//! but:
//!
//! - Marks live in **page** pixels for the current
//!   [`PageRotation`] (`View::from_hold`). Rotating remaps the same
//!   step; do not keep a 480×800 table on a landscape hold.
//! - Completing [`TARGET_LAST_ID`] prints `target loop` and restarts
//!   at id 0. There is no white end card. Page Up / Page Down still
//!   leave the scene.
//! - The display task stays the owner of SPI (*The Embassy Book*:
//!   one panel owner). This module only advances an atomic step and
//!   signals [`TARGET_VIEW`].
//!
//! Hit-test uses [`FramebufferPoint`] from
//! [`seeed_reterminal_sticky::touch::to_framebuffer`], not UART `p0=`.
//! Dots score on first contact. Slides accumulate while the finger
//! stays down.

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU8, Ordering};

use embassy_debug::{
    dist_px, dot_hit, next_target_id, slide_axis_value, slide_complete, slide_on_line,
    slide_page_len, target_mark, Event, TargetKind, TargetMark, TargetVerb, TARGET_LAST_ID,
};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use seeed_reterminal_sticky::display::PageRotation;
use seeed_reterminal_sticky::view::View;
use seeed_reterminal_sticky::{FramebufferPoint, PagePoint};

use crate::{emit, now_ms};

/// Wake the display task after a scored mark so it can paint the next.
pub static TARGET_VIEW: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// True while [`embassy_debug::Scene::Targets`] is the painted card.
static VISIBLE: AtomicBool = AtomicBool::new(false);

/// Current mark (`0..=6`).
static STEP: AtomicU8 = AtomicU8::new(0);

/// Packed IMU hold (same encoding as [`crate::sleep`] / Wi-Fi UI).
static ROT: AtomicU8 = AtomicU8::new(0);

/// Smallest on-line sample on the slide axis this mark.
static SLIDE_MIN: AtomicU16 = AtomicU16::new(u16::MAX);

/// Largest on-line sample on the slide axis this mark.
static SLIDE_MAX: AtomicU16 = AtomicU16::new(0);

/// Display task: the operator is on or off this card.
///
/// Entering resets to the centre dot. Leaving drops partial slide
/// span so a later walk does not inherit it.
pub fn set_visible(on: bool) {
    VISIBLE.store(on, Ordering::Release);
    STEP.store(0, Ordering::Release);
    reset_slide();
}

/// Display task: remember the hold used for the last paint.
pub fn set_rotation(rotation: PageRotation) {
    ROT.store(rotation_byte(rotation), Ordering::Release);
    reset_slide();
}

/// Current mark for compose. Safe to call from the display task.
#[must_use]
pub fn current_mark(rotation: PageRotation) -> TargetMark {
    let (page_w, page_h) = rotation.page_size();
    target_mark(STEP.load(Ordering::Acquire), page_w, page_h)
}

/// UART `target show` for the painted mark (page pixels of `rotation`).
pub fn emit_show(rotation: PageRotation) {
    let mark = current_mark(rotation);
    emit(Event::Target {
        t_ms: now_ms(),
        verb: TargetVerb::Show,
        id: mark.id,
        kind: mark.kind,
        page_x: mark.x,
        page_y: mark.y,
        expect_x: mark.x,
        expect_y: mark.y,
        metric: mark.r,
    });
}

/// Feed a **pre-rotation framebuffer** sample.
///
/// `became_contact` is a lift→down edge. Dots ignore held samples so
/// scoring one disk cannot immediately score the next while the
/// finger is still down. Slides need the held stream.
pub fn feed(fx: u16, fy: u16, became_contact: bool) {
    if !VISIBLE.load(Ordering::Acquire) {
        return;
    }
    let rotation = rotation_from_byte(ROT.load(Ordering::Acquire));
    let Some(page) = View::from_hold(rotation).map_touch_point(FramebufferPoint { x: fx, y: fy })
    else {
        return;
    };
    let mark = current_mark(rotation);
    match mark.kind {
        TargetKind::Dot => feed_dot(page, mark, became_contact),
        TargetKind::SlideX | TargetKind::SlideY => feed_slide(page, mark, rotation),
    }
}

/// Score or miss a disk. One UART line per new contact.
fn feed_dot(page: PagePoint, mark: TargetMark, became_contact: bool) {
    if !became_contact {
        return;
    }
    if let Some(d) = dot_hit(page.x, page.y, mark) {
        emit_verb(TargetVerb::Hit, mark, page, d);
        advance(mark.id);
        return;
    }
    emit_verb(
        TargetVerb::Miss,
        mark,
        page,
        dist_px(page.x, page.y, mark.x, mark.y),
    );
}

/// Accumulate on-line span. Completes at both inset ends.
fn feed_slide(page: PagePoint, mark: TargetMark, rotation: PageRotation) {
    if !slide_on_line(page.x, page.y, mark) {
        return;
    }
    let Some(v) = slide_axis_value(page.x, page.y, mark) else {
        return;
    };
    let mut min_v = SLIDE_MIN.load(Ordering::Acquire);
    let mut max_v = SLIDE_MAX.load(Ordering::Acquire);
    if v < min_v {
        min_v = v;
        SLIDE_MIN.store(min_v, Ordering::Release);
    }
    if v > max_v {
        max_v = v;
        SLIDE_MAX.store(max_v, Ordering::Release);
    }
    let (page_w, page_h) = rotation.page_size();
    let page_len = slide_page_len(mark, page_w, page_h);
    if slide_complete(min_v, max_v, page_len) {
        let span = max_v.saturating_sub(min_v);
        emit_verb(TargetVerb::Hit, mark, page, span);
        advance(mark.id);
    }
}

/// Next mark, or wrap. Signals the display task to repaint.
fn advance(id: u8) {
    let next = next_target_id(id);
    if next == 0 && id == TARGET_LAST_ID {
        emit(Event::Target {
            t_ms: now_ms(),
            verb: TargetVerb::Loop,
            id: 0,
            kind: TargetKind::Dot,
            page_x: 0,
            page_y: 0,
            expect_x: 0,
            expect_y: 0,
            metric: 0,
        });
    }
    STEP.store(next, Ordering::Release);
    reset_slide();
    TARGET_VIEW.signal(());
}

fn emit_verb(verb: TargetVerb, mark: TargetMark, page: PagePoint, metric: u16) {
    emit(Event::Target {
        t_ms: now_ms(),
        verb,
        id: mark.id,
        kind: mark.kind,
        page_x: page.x,
        page_y: page.y,
        expect_x: mark.x,
        expect_y: mark.y,
        metric,
    });
}

fn reset_slide() {
    SLIDE_MIN.store(u16::MAX, Ordering::Release);
    SLIDE_MAX.store(0, Ordering::Release);
}

fn rotation_byte(rotation: PageRotation) -> u8 {
    match rotation {
        PageRotation::Portrait0 => 0,
        PageRotation::Portrait180 => 1,
        PageRotation::Landscape0 => 2,
        PageRotation::Landscape180 => 3,
    }
}

fn rotation_from_byte(byte: u8) -> PageRotation {
    match byte {
        1 => PageRotation::Portrait180,
        2 => PageRotation::Landscape0,
        3 => PageRotation::Landscape180,
        _ => PageRotation::Portrait0,
    }
}
