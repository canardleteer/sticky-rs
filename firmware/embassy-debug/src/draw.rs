//! Compose embassy-debug cards in the current IMU page, then map to OTP planes.
//!
//! # Architecture
//!
//! On the unit every equivalent card (splash, shapes, legend, tones, pair)
//! stays upright in the four in-plane holds. Deep sleep paints Ferris first. FaceUp / FaceDown keep
//! the last of those. This module never talks SPI: it only fills the two
//! SSD1677 RAM planes. [`crate::display`] owns ExclusiveDevice, BUSY, and
//! the OTP sequences. No `0x32` LUT.
//!
//! Draw in page space, then [`page_to_framebuffer`](seeed_reterminal_sticky::display::page_to_framebuffer):
//!
//! - Portrait holds: 480×800 ([`display::PAGE_WIDTH`] × [`display::PAGE_HEIGHT`]).
//! - Landscape holds: 800×480 ([`display::WIDTH`] × [`display::HEIGHT`]).
//!
//! Gray4 pixels go through [`set_gray`], which already writes
//! `(WIDTH-1-x, HEIGHT-1-y)`. 1-bit shapes write the pre-rotation canvas
//! and let [`rotate180_mono`](ssd1677_gray4::planes::rotate180_mono) finish
//! the same 180°. Do not also `mirror_x_plane` (that reverse_bits 8-pixel
//! bands on the USB-down page).
//!
//! Refresh kinds stay with the caller: OTP gray4 for splash / legend /
//! tones / pair / sleep; OTP 1-bit full for shapes. Koch timing is glass
//! only — no UART microsecond token.

use core::fmt::Write;

use embassy_time::Instant;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Text};
use seeed_reterminal_sticky::display::{self, PageRotation};
use ssd1677_gray4::planes::{gray, write_mono, PlaneMapping};

/// Packed 1-bit plane: `0xff` is white paper (no ink).
const WHITE: u8 = 0xff;

/// 360×240 packed 2bpp Ferris; provenance in `assets/SOURCE.md`.
const FERRIS: &[u8] = include_bytes!("../assets/ferris.g4");
/// Ferris width in page pixels. Must be a multiple of four (2 bpp packing).
const FERRIS_W: u16 = 360;
/// Ferris height in page pixels.
const FERRIS_H: u16 = 240;

const _: () = assert!(FERRIS_W % 4 == 0);

/// Recursion depth for the geometric-calibration Koch snowflake.
const KOCH_DEPTH: u32 = 3;

/// Q16 `cos(60°) = 1/2` (32768 / 65536). Used by the Koch apex rotate.
const COS_60_Q16: i64 = 32768;
/// Q16 `sin(60°) ≈ √3/2` (56756 / 65536). Pair with [`COS_60_Q16`].
const SIN_60_Q16: i64 = 56756;
/// Q16 `sin(-60°)`. Outward Koch bump rotates the third-segment vector.
const SIN_NEG_60_Q16: i64 = -56756;
/// Q16 half-unit, added before the `>> 16` so nearest-pixel rounding is
/// unbiased around `.5`.
const Q16_HALF: i32 = 32768;

/// Built-in 10×20 glyph cell height. `embedded-graphics` `Text` `y` is the
/// baseline, so a line that must clear the glyph uses this plus `fat - 1`.
const GLYPH_H: i32 = 20;

/// OTP 1-bit geometric calibration: nested frames, Koch, triangle, rect.
///
/// On the unit: title `Geometric calibration` plus a Koch depth-3 time
/// in microseconds (glass only). The triangle is a 1-bit checkerboard
/// stand-in for papermono’s light-gray fill; the rectangle is solid ink.
/// Portrait uses the 480×800 stack; landscape shifts Koch left and the
/// primitives right so the 800×480 page is not a squeezed portrait.
///
/// Time with [`embassy_time::Instant`] around the snowflake only. Do not
/// emit a UART Koch token.
pub(crate) fn draw_shapes(buf: &mut [u8], rotation: PageRotation) {
    buf.fill(WHITE);
    let (page_w, page_h) = rotation.page_size();
    stroke_rect_mono(buf, 0, 0, page_w, page_h, rotation);
    stroke_rect_mono(
        buf,
        16,
        16,
        page_w.saturating_sub(32),
        page_h.saturating_sub(32),
        rotation,
    );

    let (koch_c, koch_r, tri, rect, title_y, time_y) = if is_portrait(rotation) {
        (
            (240_u16, 200_u16),
            135_u16,
            (70_u16, 360_u16, 140_u16, 120_u16),
            (270_u16, 360_u16, 140_u16, 120_u16),
            580_i32,
            615_i32,
        )
    } else {
        (
            (200, 170),
            110,
            (420, 280, 140, 120),
            (600, 280, 140, 120),
            420,
            448,
        )
    };

    let start = Instant::now();
    draw_koch_snowflake(buf, KOCH_DEPTH, koch_c, koch_r, rotation);
    let elapsed_us = u32::try_from(start.elapsed().as_micros()).unwrap_or(u32::MAX);

    fill_triangle_up_dither(buf, tri.0, tri.1, tri.2, tri.3, rotation);
    fill_rect_mono(buf, rect.0, rect.1, rect.2, rect.3, rotation);

    let cx = i32::from(page_w / 2);
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let _ = Text::with_alignment(
        "Geometric calibration",
        Point::new(cx, title_y),
        style,
        Alignment::Center,
    )
    .draw(&mut MonoInk::new(buf, rotation));

    let mut label = [0u8; 48];
    let mut writer = BufWriter {
        buf: &mut label,
        pos: 0,
    };
    let _ = write!(
        writer,
        "Koch snowflake (depth {KOCH_DEPTH}): {elapsed_us} us"
    );
    if let Ok(text) = core::str::from_utf8(&writer.buf[..writer.pos]) {
        let _ = Text::with_alignment(text, Point::new(cx, time_y), style, Alignment::Center)
            .draw(&mut MonoInk::new(buf, rotation));
    }
}

/// Ferris + `sticky-rs` + two hints, stacked for the current page size.
///
/// Portrait and landscape reuse one stack (360×240 gray4 Ferris,
/// `TITLE_GAP` / `HINT_GAP` = 28). White Ferris pixels stay paper so
/// there is no bounding box. Hints name the right-edge keys and the IMU
/// hold — every card now follows that hold.
pub(crate) fn draw_splash(bw: &mut [u8], red: &mut [u8], rotation: PageRotation) {
    let (page_w, page_h) = rotation.page_size();
    draw_splash_stack(bw, red, rotation, page_w, page_h);
}

/// Document-style key / value legend for this enclosure.
///
/// On the unit: heading, rows (right-edge keys, sleep / standby /
/// power, touch, OTP), then a rule that the default image does not
/// read the gauge. No 72×72 nub boxes — those only lined up
/// USB-down. Portrait is a stacked document; landscape is one
/// key/value line per row so the 480-tall page still fits.
pub(crate) fn draw_legend(bw: &mut [u8], red: &mut [u8], rotation: PageRotation) {
    const ITEMS: [(&str, &str); 8] = [
        ("AI VOICE", "Top key. Hold ~3 s: power on"),
        ("PAGE UP", "2 s standby. 5 s sleep. 1 s wake"),
        ("PAGE DOWN", "Bottom key. Hold 5 s: power off"),
        ("SLEEP", "Latch stays high. USB stays up"),
        ("STANDBY", "EPD_EN high. Page Up 1 s leaves"),
        ("POWER OFF", "Latch low. USB plug or AI Voice"),
        ("TOUCH", "GT911. This FPC reports five contacts"),
        ("OTP PANEL", "Seeed sequences. No MCU 0x32 LUT"),
    ];

    clear_gray(bw, red, gray::WHITE, rotation);
    let (page_w, page_h) = rotation.page_size();
    let cx = i32::from(page_w / 2);
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let mut ink = GrayInk::new(bw, red, 1, rotation);

    let _ = Text::with_alignment(
        "HARDWARE LEGEND",
        Point::new(cx, 36),
        style,
        Alignment::Center,
    )
    .draw(&mut ink);
    drop(ink);
    fill_rect_gray(
        bw,
        red,
        30,
        48,
        page_w.saturating_sub(60),
        2,
        gray::BLACK,
        rotation,
    );

    let mut ink = GrayInk::new(bw, red, 1, rotation);
    if is_portrait(rotation) {
        let mut y = 88;
        for (key, value) in ITEMS {
            let _ = Text::new(key, Point::new(30, y), style).draw(&mut ink);
            let _ = Text::new(value, Point::new(30, y + 24), style).draw(&mut ink);
            y += 72;
        }
    } else {
        let mut y = 80;
        for (key, value) in ITEMS {
            let _ = Text::new(key, Point::new(24, y), style).draw(&mut ink);
            let _ = Text::new(value, Point::new(220, y), style).draw(&mut ink);
            y += 36;
        }
    }
    drop(ink);

    let rule_y = if is_portrait(rotation) {
        page_h.saturating_sub(80)
    } else {
        page_h.saturating_sub(48)
    };
    fill_rect_gray(
        bw,
        red,
        30,
        rule_y,
        page_w.saturating_sub(60),
        2,
        gray::BLACK,
        rotation,
    );
    let _ = Text::with_alignment(
        "Default image does not read the fuel gauge.",
        Point::new(cx, i32::from(rule_y) + 28),
        style,
        Alignment::Center,
    )
    .draw(&mut GrayInk::new(bw, red, 1, rotation));
}

/// Four OTP gray levels with a black stroke, stacked or four-across.
///
/// Portrait: four 140-tall bands (black → dark → light → white) with
/// the papermono margins. Landscape: four-across so the 800×480 page
/// is not four cropped portrait boxes. Waveform stays Seeed OTP gray4.
pub(crate) fn draw_tones(bw: &mut [u8], red: &mut [u8], rotation: PageRotation) {
    const TONES: [u8; 4] = [gray::BLACK, gray::DARK_GRAY, gray::LIGHT_GRAY, gray::WHITE];

    clear_gray(bw, red, gray::WHITE, rotation);
    let (page_w, page_h) = rotation.page_size();
    if is_portrait(rotation) {
        const BOX_X: u16 = 40;
        const BOX_H: u16 = 140;
        const YS: [u16; 4] = [80, 250, 420, 590];
        let box_w = page_w.saturating_sub(80);
        for (y, tone) in YS.iter().zip(TONES) {
            fill_rect_gray(bw, red, BOX_X, *y, box_w, BOX_H, tone, rotation);
            stroke_rect_gray(bw, red, BOX_X, *y, box_w, BOX_H, gray::BLACK, rotation);
        }
    } else {
        const MARGIN_X: u16 = 32;
        const MARGIN_Y: u16 = 48;
        const GAP: u16 = 16;
        let box_w = page_w
            .saturating_sub(MARGIN_X.saturating_mul(2))
            .saturating_sub(GAP.saturating_mul(3))
            / 4;
        let box_h = page_h.saturating_sub(MARGIN_Y.saturating_mul(2));
        for (i, tone) in TONES.iter().enumerate() {
            let x = MARGIN_X + u16::try_from(i).unwrap_or(0) * (box_w + GAP);
            fill_rect_gray(bw, red, x, MARGIN_Y, box_w, box_h, *tone, rotation);
            stroke_rect_gray(bw, red, x, MARGIN_Y, box_w, box_h, gray::BLACK, rotation);
        }
    }
}

/// BLE pair card: header, boxed PIN (or Paired banner), status, tutorial.
///
/// On the unit: idle is `sticky-rs` plus Settings copy and **empty**
/// digit boxes — not a fake PIN. Digits appear only after UART
/// `pair pin=`. [`FONT_10X20`] in page pixels (no 3× glyph scale) so
/// the how-to still fits landscape 800×480. Tokens stay
/// [`embassy_debug::PairFailWhy::as_str`].
#[cfg(feature = "pair")]
pub(crate) fn draw_pair(bw: &mut [u8], red: &mut [u8], rotation: PageRotation) {
    use crate::pair::{current_view, PairView};
    use embassy_debug::PAIR_ADV_NAME;

    clear_gray(bw, red, gray::WHITE, rotation);
    let layout = PairLayout::for_rotation(rotation);
    let view = current_view();

    // Frames first so later text is not under-stroked.
    fill_rect_gray(
        bw,
        red,
        layout.rule_x,
        layout.header_bar_y,
        layout.rule_w,
        2,
        gray::BLACK,
        rotation,
    );
    stroke_rect_gray(
        bw,
        red,
        layout.pin_frame_x,
        layout.pin_frame_y,
        layout.pin_frame_w,
        layout.pin_frame_h,
        gray::BLACK,
        rotation,
    );
    stroke_rect_gray(
        bw,
        red,
        layout.pin_frame_x.saturating_add(3),
        layout.pin_frame_y.saturating_add(3),
        layout.pin_frame_w.saturating_sub(6),
        layout.pin_frame_h.saturating_sub(6),
        gray::BLACK,
        rotation,
    );
    match view {
        PairView::Ok => {
            stroke_rect_gray(
                bw,
                red,
                layout.banner_x,
                layout.digit_y,
                layout.banner_w,
                layout.digit_h,
                gray::BLACK,
                rotation,
            );
        }
        PairView::Idle | PairView::Pin(_) | PairView::Fail(_) => {
            for i in 0..6_u16 {
                let x = layout
                    .digit_x0
                    .saturating_add(i.saturating_mul(layout.digit_pitch));
                stroke_rect_gray(
                    bw,
                    red,
                    x,
                    layout.digit_y,
                    layout.digit_w,
                    layout.digit_h,
                    gray::BLACK,
                    rotation,
                );
            }
        }
    }
    fill_rect_gray(
        bw,
        red,
        layout.rule_x,
        layout.mid_bar_y,
        layout.rule_w,
        2,
        gray::BLACK,
        rotation,
    );
    if matches!(view, PairView::Ok | PairView::Fail(_)) {
        stroke_rect_gray(
            bw,
            red,
            layout.status_x,
            layout.status_y,
            layout.status_w,
            layout.status_h,
            gray::BLACK,
            rotation,
        );
        stroke_rect_gray(
            bw,
            red,
            layout.status_x.saturating_add(2),
            layout.status_y.saturating_add(2),
            layout.status_w.saturating_sub(4),
            layout.status_h.saturating_sub(4),
            gray::BLACK,
            rotation,
        );
    }
    fill_rect_gray(
        bw,
        red,
        layout.rule_x,
        layout.tutorial_bar_y,
        layout.rule_w,
        1,
        gray::LIGHT_GRAY,
        rotation,
    );
    fill_rect_gray(
        bw,
        red,
        layout.rule_x,
        layout.footer_bar_y,
        layout.rule_w,
        2,
        gray::BLACK,
        rotation,
    );

    let mut ink = GrayInk::new(bw, red, 1, rotation);
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let cx = layout.cx;

    let _ = Text::with_alignment(
        "BLUETOOTH PAIRING",
        Point::new(cx, layout.title_y),
        style,
        Alignment::Center,
    )
    .draw(&mut ink);

    let mut device_line = [0u8; 32];
    let mut device_w = BufWriter {
        buf: &mut device_line,
        pos: 0,
    };
    let _ = write!(device_w, "Device: {PAIR_ADV_NAME}");
    if let Ok(line) = core::str::from_utf8(&device_w.buf[..device_w.pos]) {
        let _ = Text::with_alignment(
            line,
            Point::new(cx, layout.device_y),
            style,
            Alignment::Center,
        )
        .draw(&mut ink);
    }

    let instruction = match view {
        PairView::Pin(_) => "Enter this PIN code on your phone:",
        PairView::Ok => "Device paired and connected!",
        PairView::Fail(_) => "Pairing attempt failed",
        PairView::Idle => "Discoverable as 'sticky-rs'",
    };
    let _ = Text::with_alignment(
        instruction,
        Point::new(cx, layout.instruction_y),
        style,
        Alignment::Center,
    )
    .draw(&mut ink);

    match view {
        PairView::Pin(pin) => {
            let mut digits = [0u8; 6];
            let mut n = pin % 1_000_000;
            for i in (0..6).rev() {
                digits[i] = b'0' + (n % 10) as u8;
                n /= 10;
            }
            for (i, d) in digits.iter().enumerate() {
                let ch = core::str::from_utf8(core::slice::from_ref(d)).unwrap_or("0");
                let x = i32::from(layout.digit_x0)
                    + i32::from(layout.digit_pitch) * i32::try_from(i).unwrap_or(0)
                    + i32::from(layout.digit_w / 2);
                let _ = Text::with_alignment(
                    ch,
                    Point::new(x, layout.digit_baseline),
                    style,
                    Alignment::Center,
                )
                .draw(&mut ink);
            }
        }
        PairView::Ok => {
            let _ = Text::with_alignment(
                "P A I R E D",
                Point::new(cx, layout.digit_baseline),
                style,
                Alignment::Center,
            )
            .draw(&mut ink);
        }
        PairView::Idle | PairView::Fail(_) => {}
    }

    match view {
        PairView::Ok => {
            let _ = Text::with_alignment(
                "SUCCESS",
                Point::new(cx, layout.status_text_y),
                style,
                Alignment::Center,
            )
            .draw(&mut ink);
            let _ = Text::with_alignment(
                "Bluetooth connection encrypted.",
                Point::new(cx, layout.status_detail_y),
                style,
                Alignment::Center,
            )
            .draw(&mut ink);
        }
        PairView::Fail(why) => {
            let _ = Text::with_alignment(
                "FAILED",
                Point::new(cx, layout.status_text_y),
                style,
                Alignment::Center,
            )
            .draw(&mut ink);
            let _ = Text::with_alignment(
                why.as_str(),
                Point::new(cx, layout.status_detail_y),
                style,
                Alignment::Center,
            )
            .draw(&mut ink);
            let _ = Text::with_alignment(
                "Retry pairing from phone settings.",
                Point::new(cx, layout.status_hint_y),
                style,
                Alignment::Center,
            )
            .draw(&mut ink);
        }
        PairView::Pin(_) => {
            let _ = Text::with_alignment(
                "Status: Pairing in progress",
                Point::new(cx, layout.status_text_y),
                style,
                Alignment::Center,
            )
            .draw(&mut ink);
            let _ = Text::with_alignment(
                "Enter PIN shown above on phone",
                Point::new(cx, layout.status_detail_y),
                style,
                Alignment::Center,
            )
            .draw(&mut ink);
        }
        PairView::Idle => {
            let _ = Text::with_alignment(
                "Status: Ready to pair",
                Point::new(cx, layout.status_text_y),
                style,
                Alignment::Center,
            )
            .draw(&mut ink);
            let _ = Text::with_alignment(
                "Select 'sticky-rs' in phone Bluetooth",
                Point::new(cx, layout.status_detail_y),
                style,
                Alignment::Center,
            )
            .draw(&mut ink);
        }
    }

    let _ = Text::with_alignment(
        "HOW TO PAIR",
        Point::new(cx, layout.howto_y),
        style,
        Alignment::Center,
    )
    .draw(&mut ink);

    let steps = [
        "1. Open Settings -> Bluetooth on phone",
        "2. Select 'sticky-rs' under devices",
        "3. Wait for pairing passkey prompt",
        "4. Enter the 6-digit PIN shown above",
    ];
    if is_portrait(rotation) {
        let mut step_y = layout.step_y0;
        for step in steps {
            let _ =
                Text::new(step, Point::new(i32::from(layout.step_x), step_y), style).draw(&mut ink);
            step_y += 36;
        }
    } else {
        // Two columns so the 480-tall landscape page keeps the footer.
        for (i, step) in steps.iter().enumerate() {
            let col = i32::try_from(i % 2).unwrap_or(0);
            let row = i32::try_from(i / 2).unwrap_or(0);
            let x = i32::from(layout.step_x) + col * 380;
            let y = layout.step_y0 + row * 32;
            let _ = Text::new(*step, Point::new(x, y), style).draw(&mut ink);
        }
    }

    let _ = Text::with_alignment(
        "Page Up: Prev   |   Page Down: Next",
        Point::new(cx, layout.footer_y),
        style,
        Alignment::Center,
    )
    .draw(&mut ink);
}

/// Page-space geometry for the pair card (portrait 480×800 vs landscape 800×480).
///
/// Digit boxes stay 40×50 at 52 px pitch (papermono). Landscape compresses
/// vertical gaps so header, PIN, status, tutorial, and footer all fit.
#[cfg(feature = "pair")]
struct PairLayout {
    /// Horizontal center of the current page.
    cx: i32,
    /// Centered title baseline.
    title_y: i32,
    /// Hairline under the title.
    header_bar_y: u16,
    /// `Device: sticky-rs` baseline.
    device_y: i32,
    /// Status-specific instruction baseline.
    instruction_y: i32,
    /// Outer PIN frame origin X.
    pin_frame_x: u16,
    /// Outer PIN frame origin Y.
    pin_frame_y: u16,
    /// Outer PIN frame width.
    pin_frame_w: u16,
    /// Outer PIN frame height.
    pin_frame_h: u16,
    /// First digit-box origin X.
    digit_x0: u16,
    /// Digit-box origin Y (also the Paired banner Y).
    digit_y: u16,
    /// Digit-box width.
    digit_w: u16,
    /// Digit-box height.
    digit_h: u16,
    /// Distance from one digit-box left edge to the next.
    digit_pitch: u16,
    /// `embedded-graphics` baseline for a digit inside its box.
    digit_baseline: i32,
    /// Wide Paired banner origin X.
    banner_x: u16,
    /// Wide Paired banner width.
    banner_w: u16,
    /// Rule X (header / mid / footer share this).
    rule_x: u16,
    /// Rule width.
    rule_w: u16,
    /// Hairline under the PIN frame.
    mid_bar_y: u16,
    /// Terminal-state outline origin X.
    status_x: u16,
    /// Terminal-state outline origin Y.
    status_y: u16,
    /// Terminal-state outline width.
    status_w: u16,
    /// Terminal-state outline height.
    status_h: u16,
    /// SUCCESS / FAILED / Status: baseline.
    status_text_y: i32,
    /// Detail line under the status word.
    status_detail_y: i32,
    /// Optional third status line (fail retry).
    status_hint_y: i32,
    /// Light rule above the tutorial.
    tutorial_bar_y: u16,
    /// `HOW TO PAIR` baseline.
    howto_y: i32,
    /// First tutorial step X.
    step_x: u16,
    /// First tutorial step baseline.
    step_y0: i32,
    /// Footer hairline Y.
    footer_bar_y: u16,
    /// Footer navigation baseline.
    footer_y: i32,
}

#[cfg(feature = "pair")]
impl PairLayout {
    /// Pick the portrait or landscape constant table for this hold.
    fn for_rotation(rotation: PageRotation) -> Self {
        let (page_w, _) = rotation.page_size();
        let cx = i32::from(page_w / 2);
        if is_portrait(rotation) {
            Self {
                cx,
                title_y: 50,
                header_bar_y: 70,
                device_y: 110,
                instruction_y: 145,
                pin_frame_x: 60,
                pin_frame_y: 175,
                pin_frame_w: 360,
                pin_frame_h: 90,
                digit_x0: 90,
                digit_y: 195,
                digit_w: 40,
                digit_h: 50,
                digit_pitch: 52,
                digit_baseline: 227,
                banner_x: 90,
                banner_w: 300,
                rule_x: 30,
                rule_w: 420,
                mid_bar_y: 295,
                status_x: 100,
                status_y: 315,
                status_w: 280,
                status_h: 45,
                status_text_y: 345,
                status_detail_y: 395,
                status_hint_y: 425,
                tutorial_bar_y: 480,
                howto_y: 510,
                step_x: 45,
                step_y0: 545,
                footer_bar_y: 720,
                footer_y: 755,
            }
        } else {
            Self {
                cx,
                title_y: 28,
                header_bar_y: 42,
                device_y: 64,
                instruction_y: 86,
                pin_frame_x: 220,
                pin_frame_y: 96,
                pin_frame_w: 360,
                pin_frame_h: 80,
                digit_x0: 250,
                digit_y: 112,
                digit_w: 40,
                digit_h: 48,
                digit_pitch: 52,
                digit_baseline: 144,
                banner_x: 250,
                banner_w: 300,
                rule_x: 40,
                rule_w: 720,
                mid_bar_y: 188,
                status_x: 260,
                status_y: 200,
                status_w: 280,
                status_h: 40,
                status_text_y: 226,
                status_detail_y: 250,
                status_hint_y: 270,
                tutorial_bar_y: 278,
                howto_y: 300,
                step_x: 40,
                step_y0: 328,
                footer_bar_y: 420,
                footer_y: 452,
            }
        }
    }
}

/// Center Ferris + title + hints in a page. White Ferris pixels are skipped.
fn draw_splash_stack(
    bw: &mut [u8],
    red: &mut [u8],
    rotation: PageRotation,
    page_w: u16,
    page_h: u16,
) {
    const TITLE_GAP: i32 = 28;
    const HINT_GAP: i32 = 28;
    const LINE_GAP: i32 = 8;

    clear_gray(bw, red, gray::WHITE, rotation);

    let ferris_w = i32::from(FERRIS_W);
    let ferris_h = i32::from(FERRIS_H);
    let stack_h = ferris_h + TITLE_GAP + GLYPH_H + HINT_GAP + GLYPH_H + LINE_GAP + GLYPH_H;
    let top = (i32::from(page_h) - stack_h) / 2;
    let cx = i32::from(page_w) / 2;
    let ferris_x = (i32::from(page_w) - ferris_w) / 2;
    let title_y = top + ferris_h + TITLE_GAP + GLYPH_H;
    let hint1_y = title_y + HINT_GAP + GLYPH_H;
    let hint2_y = hint1_y + LINE_GAP + GLYPH_H;

    blit_ferris(bw, red, ferris_x, top, rotation);
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let _ = Text::with_alignment(
        "sticky-rs",
        Point::new(cx, title_y),
        style,
        Alignment::Center,
    )
    .draw(&mut GrayInk::new(bw, red, 1, rotation));
    let _ = Text::with_alignment(
        "Press a right-edge key",
        Point::new(cx, hint1_y),
        style,
        Alignment::Center,
    )
    .draw(&mut GrayInk::new(bw, red, 1, rotation));
    let _ = Text::with_alignment(
        "Tilt keeps the page upright",
        Point::new(cx, hint2_y),
        style,
        Alignment::Center,
    )
    .draw(&mut GrayInk::new(bw, red, 1, rotation));
}

/// Skip white (no box). Other tones are OTP gray4.
fn blit_ferris(bw: &mut [u8], red: &mut [u8], x0: i32, y0: i32, rotation: PageRotation) {
    for y in 0..FERRIS_H {
        for x in 0..FERRIS_W {
            let tone = ferris_tone(x, y);
            if tone == gray::WHITE {
                continue;
            }
            let Some(px) = u16::try_from(x0.saturating_add(i32::from(x))).ok() else {
                continue;
            };
            let Some(py) = u16::try_from(y0.saturating_add(i32::from(y))).ok() else {
                continue;
            };
            set_gray_page(bw, red, px, py, tone, rotation);
        }
    }
}

/// Packed 2 bpp Ferris sample at `(x, y)` (MSB pair first in the byte).
fn ferris_tone(x: u16, y: u16) -> u8 {
    let i = usize::from(y) * usize::from(FERRIS_W) + usize::from(x);
    let byte = FERRIS[i / 4];
    let shift = 6 - (i % 4) * 2;
    (byte >> shift) & 0b11
}

/// Order-`depth` Koch snowflake, Q16 vertices, circumradius `r`.
///
/// Equilateral base: top apex, then ±60° on the circumcircle. Each side
/// is [`koch_curve`]. Coordinates are page pixels, not panel 800×480.
fn draw_koch_snowflake(
    buf: &mut [u8],
    depth: u32,
    (cx, cy): (u16, u16),
    r: u16,
    rotation: PageRotation,
) {
    let cx_q = i32::from(cx) << 16;
    let cy_q = i32::from(cy) << 16;
    let r_i64 = i64::from(r);
    let r_sin = (r_i64 * SIN_60_Q16) as i32;
    let r_cos = (r_i64 * COS_60_Q16) as i32;
    let v0 = (cx_q, cy_q - (i32::from(r) << 16));
    let v1 = (cx_q + r_sin, cy_q + r_cos);
    let v2 = (cx_q - r_sin, cy_q + r_cos);
    koch_curve(buf, depth, v0, v1, rotation);
    koch_curve(buf, depth, v1, v2, rotation);
    koch_curve(buf, depth, v2, v0, rotation);
}

/// One Koch side. Depth 0 is a Bresenham segment; else split in thirds
/// and rotate the middle third by −60° (Q16) for the outward bump.
fn koch_curve(
    buf: &mut [u8],
    depth: u32,
    (x0, y0): (i32, i32),
    (x1, y1): (i32, i32),
    rotation: PageRotation,
) {
    if depth == 0 {
        let px0 = (x0 + Q16_HALF) >> 16;
        let py0 = (y0 + Q16_HALF) >> 16;
        let px1 = (x1 + Q16_HALF) >> 16;
        let py1 = (y1 + Q16_HALF) >> 16;
        draw_line_mono(buf, px0, py0, px1, py1, rotation);
        return;
    }

    let ux = (x1 - x0) / 3;
    let uy = (y1 - y0) / 3;
    let p1 = (x0 + ux, y0 + uy);
    let p3 = (p1.0 + ux, p1.1 + uy);
    let rot_x = ((i64::from(ux) * COS_60_Q16 - i64::from(uy) * SIN_NEG_60_Q16) >> 16) as i32;
    let rot_y = ((i64::from(ux) * SIN_NEG_60_Q16 + i64::from(uy) * COS_60_Q16) >> 16) as i32;
    let p2 = (p1.0 + rot_x, p1.1 + rot_y);

    koch_curve(buf, depth - 1, (x0, y0), p1, rotation);
    koch_curve(buf, depth - 1, p1, p2, rotation);
    koch_curve(buf, depth - 1, p2, p3, rotation);
    koch_curve(buf, depth - 1, p3, (x1, y1), rotation);
}

/// Single-pixel 1-bit line in page space (Bresenham).
fn draw_line_mono(
    buf: &mut [u8],
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    rotation: PageRotation,
) {
    let (page_w, page_h) = rotation.page_size();
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if x0 >= 0 && x0 < i32::from(page_w) && y0 >= 0 && y0 < i32::from(page_h) {
            set_black_page(buf, x0 as u16, y0 as u16, rotation);
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/// Upward isosceles triangle, checkerboard ink (1-bit “light”).
fn fill_triangle_up_dither(buf: &mut [u8], x: u16, y: u16, w: u16, h: u16, rotation: PageRotation) {
    if w < 2 || h == 0 {
        return;
    }
    for row in 0..h {
        let remain = h.saturating_sub(1).saturating_sub(row);
        let inset =
            (u32::from(w) * u32::from(remain) / (2 * u32::from(h))).min(u32::from(w / 2)) as u16;
        let ww = w.saturating_sub(inset.saturating_mul(2)).max(1);
        let xx0 = x.saturating_add(inset);
        let yy = y.saturating_add(row);
        for dx in 0..ww {
            let xx = xx0.saturating_add(dx);
            if xx.wrapping_add(yy) % 2 == 0 {
                set_black_page(buf, xx, yy, rotation);
            }
        }
    }
}

/// USB-C on a short edge (480×800 page), not a long-edge landscape hold.
fn is_portrait(rotation: PageRotation) -> bool {
    matches!(
        rotation,
        PageRotation::Portrait0 | PageRotation::Portrait180
    )
}

/// Fill the current page with one OTP gray tone.
fn clear_gray(bw: &mut [u8], red: &mut [u8], tone: u8, rotation: PageRotation) {
    let (page_w, page_h) = rotation.page_size();
    fill_rect_gray(bw, red, 0, 0, page_w, page_h, tone, rotation);
}

/// Fill a gray4 rectangle in the current page.
#[allow(clippy::too_many_arguments)]
fn fill_rect_gray(
    bw: &mut [u8],
    red: &mut [u8],
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    tone: u8,
    rotation: PageRotation,
) {
    let (page_w, page_h) = rotation.page_size();
    for yy in y..y.saturating_add(h).min(page_h) {
        for xx in x..x.saturating_add(w).min(page_w) {
            set_gray_page(bw, red, xx, yy, tone, rotation);
        }
    }
}

/// One-pixel gray4 outline so a white box stays visible on white paper.
#[allow(clippy::too_many_arguments)]
fn stroke_rect_gray(
    bw: &mut [u8],
    red: &mut [u8],
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    tone: u8,
    rotation: PageRotation,
) {
    if w == 0 || h == 0 {
        return;
    }
    let (page_w, page_h) = rotation.page_size();
    let x1 = x.saturating_add(w).saturating_sub(1);
    let y1 = y.saturating_add(h).saturating_sub(1);
    for xx in x..=x1.min(page_w.saturating_sub(1)) {
        set_gray_page(bw, red, xx, y, tone, rotation);
        set_gray_page(bw, red, xx, y1, tone, rotation);
    }
    for yy in y..=y1.min(page_h.saturating_sub(1)) {
        set_gray_page(bw, red, x, yy, tone, rotation);
        set_gray_page(bw, red, x1, yy, tone, rotation);
    }
}

/// Page pixel → Seeed OTP planes (already 180°-rotated writes).
fn set_gray_page(
    bw: &mut [u8],
    red: &mut [u8],
    px: u16,
    py: u16,
    tone: u8,
    rotation: PageRotation,
) {
    let Some((x, y)) = display::page_to_framebuffer(px, py, rotation) else {
        return;
    };
    set_gray(bw, red, x, y, tone);
}

/// One pixel in Seeed OTP plane mapping, written already 180° rotated.
fn set_gray(bw: &mut [u8], red: &mut [u8], x: u16, y: u16, tone: u8) {
    if x >= display::WIDTH || y >= display::HEIGHT {
        return;
    }
    let width = display::WIDTH as usize;
    let xr = usize::from(display::WIDTH - 1 - x);
    let yr = usize::from(display::HEIGHT - 1 - y);
    let (bw_bit, red_bit) = PlaneMapping::SEEED_OTP.bits_for(tone);
    write_mono(bw, width, xr, yr, bw_bit);
    write_mono(red, width, xr, yr, red_bit);
}

/// Filled black rectangle on the 1-bit page canvas.
fn fill_rect_mono(buf: &mut [u8], x: u16, y: u16, w: u16, h: u16, rotation: PageRotation) {
    let (page_w, page_h) = rotation.page_size();
    for yy in y..y.saturating_add(h).min(page_h) {
        for xx in x..x.saturating_add(w).min(page_w) {
            set_black_page(buf, xx, yy, rotation);
        }
    }
}

/// One-pixel black outline on the 1-bit page canvas.
fn stroke_rect_mono(buf: &mut [u8], x: u16, y: u16, w: u16, h: u16, rotation: PageRotation) {
    if w == 0 || h == 0 {
        return;
    }
    let (page_w, page_h) = rotation.page_size();
    let x1 = x.saturating_add(w).saturating_sub(1);
    let y1 = y.saturating_add(h).saturating_sub(1);
    for xx in x..=x1.min(page_w.saturating_sub(1)) {
        set_black_page(buf, xx, y, rotation);
        set_black_page(buf, xx, y1, rotation);
    }
    for yy in y..=y1.min(page_h.saturating_sub(1)) {
        set_black_page(buf, x, yy, rotation);
        set_black_page(buf, x1, yy, rotation);
    }
}

/// Page pixel → pre-rotation 800×480 1-bit plane (no 180° here).
fn set_black_page(buf: &mut [u8], px: u16, py: u16, rotation: PageRotation) {
    let Some((x, y)) = display::page_to_framebuffer(px, py, rotation) else {
        return;
    };
    set_black(buf, x, y);
}

/// Ink a pixel (`0`) on a 0xff-white 1-bit plane. Caller applies 180°.
fn set_black(buf: &mut [u8], x: u16, y: u16) {
    if x >= display::WIDTH || y >= display::HEIGHT {
        return;
    }
    let x = usize::from(x);
    let y = usize::from(y);
    let stride = display::WIDTH as usize / 8;
    buf[y * stride + x / 8] &= !(0x80u8 >> (x % 8));
}

/// `embedded-graphics` target over OTP gray4 in the current page.
///
/// [`BinaryColor::On`] is black ink. `fat` > 1 paints each source pixel
/// as a `fat`×`fat` block so mono glyphs survive the refresh.
struct GrayInk<'a> {
    /// Black/white SSD1677 plane (Seeed OTP gray4 pair).
    bw: &'a mut [u8],
    /// Second plane of the OTP gray4 pair.
    red: &'a mut [u8],
    /// Destination block size per source pixel.
    fat: u16,
    /// In-plane hold; [`OriginDimensions`] uses [`PageRotation::page_size`].
    rotation: PageRotation,
}

impl<'a> GrayInk<'a> {
    /// Borrow both planes. `fat` is clamped to at least 1.
    fn new(bw: &'a mut [u8], red: &'a mut [u8], fat: u16, rotation: PageRotation) -> Self {
        Self {
            bw,
            red,
            fat: fat.max(1),
            rotation,
        }
    }
}

impl OriginDimensions for GrayInk<'_> {
    fn size(&self) -> Size {
        let (w, h) = self.rotation.page_size();
        Size::new(u32::from(w), u32::from(h))
    }
}

impl DrawTarget for GrayInk<'_> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if color != BinaryColor::On || point.x < 0 || point.y < 0 {
                continue;
            }
            let Ok(x0) = u16::try_from(point.x) else {
                continue;
            };
            let Ok(y0) = u16::try_from(point.y) else {
                continue;
            };
            for dy in 0..self.fat {
                for dx in 0..self.fat {
                    set_gray_page(
                        self.bw,
                        self.red,
                        x0.saturating_add(dx),
                        y0.saturating_add(dy),
                        gray::BLACK,
                        self.rotation,
                    );
                }
            }
        }
        Ok(())
    }
}

/// `embedded-graphics` target over the 1-bit shapes canvas.
///
/// Same page axes as [`GrayInk`]. Ink is a cleared bit; the display task
/// still [`rotate180_mono`](ssd1677_gray4::planes::rotate180_mono) before SPI.
struct MonoInk<'a> {
    /// Packed 1-bit plane (`0xff` white).
    buf: &'a mut [u8],
    /// In-plane hold for [`page_to_framebuffer`](display::page_to_framebuffer).
    rotation: PageRotation,
}

impl<'a> MonoInk<'a> {
    /// Borrow the shapes plane for text in the current page.
    fn new(buf: &'a mut [u8], rotation: PageRotation) -> Self {
        Self { buf, rotation }
    }
}

impl OriginDimensions for MonoInk<'_> {
    fn size(&self) -> Size {
        let (w, h) = self.rotation.page_size();
        Size::new(u32::from(w), u32::from(h))
    }
}

impl DrawTarget for MonoInk<'_> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if color != BinaryColor::On || point.x < 0 || point.y < 0 {
                continue;
            }
            let Ok(x) = u16::try_from(point.x) else {
                continue;
            };
            let Ok(y) = u16::try_from(point.y) else {
                continue;
            };
            set_black_page(self.buf, x, y, self.rotation);
        }
        Ok(())
    }
}

/// Stack `write!` into a fixed slice. Truncates rather than allocating.
struct BufWriter<'a> {
    /// Destination bytes (ASCII).
    buf: &'a mut [u8],
    /// Next write index.
    pos: usize,
}

impl core::fmt::Write for BufWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remain = self.buf.len().saturating_sub(self.pos);
        let to_copy = bytes.len().min(remain);
        self.buf[self.pos..self.pos + to_copy].copy_from_slice(&bytes[..to_copy]);
        self.pos += to_copy;
        Ok(())
    }
}
