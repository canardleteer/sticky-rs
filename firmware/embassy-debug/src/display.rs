//! OTP panel path: 1-bit full refresh, plus a four-tone OTP gray4 scene.
//!
//! What you see on the glass: splash (Ferris + `sticky-rs` + a hint), then
//! shapes, a right-edge legend, and four gray boxes. Waveforms stay in the
//! panel OTP — this file never writes a `0x32` LUT.

// Embassy time + UART scene token.
use embassy_debug::{Event, Scene};
use embassy_time::{with_timeout, Delay, Duration};

// Draw splash with built-in mono fonts and a 1-bit Ferris BMP.
use embedded_graphics::image::Image;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Text};

// SPI panel: exclusive CS, BUSY wait, OTP sequences.
use embedded_hal::digital::InputPin;
use embedded_hal_async::digital::Wait;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::peripherals::{GPIO13, GPIO14, GPIO15, GPIO16, GPIO17, GPIO18, SPI2};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode;
use esp_hal::time::Rate;
use esp_println::println;
use seeed_reterminal_sticky::display::{self, RefreshKind};
use seeed_reterminal_sticky::rails::{Enabled, EpdRail};
use ssd1677_gray4::planes::{gray, mirror_x_plane, rotate180_mono, write_mono, PlaneMapping};
use ssd1677_gray4::Ssd1677;
use static_cell::ConstStaticCell;
use tinybmp::Bmp;

use crate::{emit, now_ms};

const WHITE: u8 = 0xff;
const REFRESH_TIMEOUT: Duration = Duration::from_secs(15);

/// 72×48 1-bit Ferris; provenance in `assets/SOURCE.md`.
const FERRIS_BMP: &[u8] = include_bytes!("../assets/ferris.bmp");

static DRAW: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);
static TX: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);

/// Shared SPI pins for the panel (same bus as the card; this image
/// never asserts SD CS).
pub struct PanelParts {
    /// Shared SPI controller.
    pub spi: SPI2<'static>,
    /// SCLK.
    pub sclk: GPIO13<'static>,
    /// MOSI.
    pub mosi: GPIO14<'static>,
    /// Chip select. Idle-high except during a transfer.
    pub cs: GPIO15<'static>,
    /// Data/command.
    pub dc: GPIO16<'static>,
    /// Reset.
    pub rst: GPIO17<'static>,
    /// BUSY (active high). Do not talk on the bus while it is high.
    pub busy: GPIO18<'static>,
}

/// Bring the panel up, paint splash, then wait for Page Up / Page Down /
/// AI Voice to ask for another page.
///
/// On the unit: the glass shows Ferris and `sticky-rs` first. In the MCU:
/// OTP 1-bit full refresh, then OTP gray4 only for the four-tone page.
#[embassy_executor::task]
pub async fn display_task(parts: PanelParts, _rail: EpdRail<Output<'static>, Enabled>) {
    let spi = Spi::new(
        parts.spi,
        SpiConfig::default()
            .with_frequency(Rate::from_hz(display::SPI_MAX_HZ))
            .with_mode(Mode::_0),
    )
    .expect("SPI configuration")
    .with_sck(parts.sclk)
    .with_mosi(parts.mosi);

    let cs = Output::new(parts.cs, Level::High, OutputConfig::default());
    let bus = ExclusiveDevice::new(spi, cs, Delay).expect("EPD CS");
    let dc = Output::new(parts.dc, Level::Low, OutputConfig::default());
    let rst = Output::new(parts.rst, Level::High, OutputConfig::default());
    let busy = Input::new(parts.busy, InputConfig::default().with_pull(Pull::None));

    let mut driver = Ssd1677::new(bus, dc, rst, busy, Delay).expect("panel reset");
    let mut kind = RefreshKind::Full;
    driver.init(&kind.controller_config()).expect("panel init");

    let draw = DRAW.take();
    let tx = TX.take();

    let mut scene = Scene::Splash;
    refresh(&mut driver, draw, tx, scene, &mut kind).await;

    loop {
        scene = crate::SCENE.wait().await;
        refresh(&mut driver, draw, tx, scene, &mut kind).await;
    }
}

/// 1-bit full refresh for splash / shapes / legend; OTP gray4 for tones.
fn scene_kind(scene: Scene) -> RefreshKind {
    match scene {
        Scene::Tones => RefreshKind::Gray4,
        Scene::Splash | Scene::Shapes | Scene::Legend => RefreshKind::Full,
    }
}

/// Re-init the controller when the OTP sequence changes, write planes,
/// wait for BUSY, then print `scene=…` on UART.
async fn refresh<SPI, DC, RST, BUSY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    draw: &mut [u8; display::PLANE_BYTES],
    tx: &mut [u8; display::PLANE_BYTES],
    scene: Scene,
    kind: &mut RefreshKind,
) where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error> + Wait<Error = DC::Error>,
{
    let next = scene_kind(scene);
    if next != *kind {
        if driver.init(&next.controller_config()).is_err() {
            println!("embassy-debug: epd re-init failed");
            return;
        }
        *kind = next;
    }

    match next {
        RefreshKind::Gray4 => {
            if !write_tones(driver, draw, tx) {
                return;
            }
        }
        RefreshKind::Full | RefreshKind::Partial => {
            if !write_mono_scene(driver, draw, tx, scene) {
                return;
            }
        }
    }

    if let Some(temp) = next.temperature_override() {
        if driver.write_temperature_register(temp).is_err() {
            println!("embassy-debug: epd temperature failed");
            return;
        }
    }

    if driver.start_update_sequence(next.sequence()).is_err() {
        println!("embassy-debug: epd update failed");
        return;
    }

    match with_timeout(REFRESH_TIMEOUT, driver.wait_until_idle_async()).await {
        Ok(Ok(())) => {
            emit(Event::Scene {
                t_ms: now_ms(),
                scene,
            });
        }
        Ok(Err(_)) | Err(_) => println!("embassy-debug: epd busy timeout"),
    }
}

/// Draw a 1-bit page in landscape, rotate 180°, mirror X, then write both
/// RAM planes (second plane cleared). Matches the Seeed OTP polarity.
fn write_mono_scene<SPI, DC, RST, BUSY, DELAY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, DELAY, ssd1677_gray4::Active>,
    draw: &mut [u8; display::PLANE_BYTES],
    tx: &mut [u8; display::PLANE_BYTES],
    scene: Scene,
) -> bool
where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error>,
    DELAY: embedded_hal::delay::DelayNs,
{
    draw_scene(scene, draw);
    if rotate180_mono(draw, display::WIDTH as usize, display::HEIGHT as usize, tx).is_err() {
        println!("embassy-debug: epd rotate failed");
        return false;
    }
    mirror_x_plane(tx);

    draw.fill(0);
    if driver.write_black_white_plane(tx).is_err() {
        println!("embassy-debug: epd write failed");
        return false;
    }
    if driver.write_second_plane(draw).is_err() {
        println!("embassy-debug: epd write failed");
        return false;
    }
    true
}

/// Four landscape boxes, one OTP gray each. Planes are already 180°-aware
/// in [`set_gray`]; no extra canvas.
fn write_tones<SPI, DC, RST, BUSY, DELAY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, DELAY, ssd1677_gray4::Active>,
    bw: &mut [u8; display::PLANE_BYTES],
    red: &mut [u8; display::PLANE_BYTES],
) -> bool
where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error>,
    DELAY: embedded_hal::delay::DelayNs,
{
    draw_tones(bw, red);
    mirror_x_plane(bw);
    mirror_x_plane(red);

    if driver
        .write_gray4_frame(&display::FULL_WINDOW, bw, red)
        .is_err()
    {
        println!("embassy-debug: epd write failed");
        return false;
    }
    true
}

/// Paint one 1-bit page into `buf` (0xff = white, cleared bit = ink).
fn draw_scene(scene: Scene, buf: &mut [u8]) {
    buf.fill(WHITE);
    match scene {
        Scene::Splash => draw_splash(buf),
        Scene::Shapes => {
            fill_rect(buf, 60, 60, 200, 160);
            fill_rect(buf, 320, 140, 160, 220);
            fill_rect(buf, 560, 80, 180, 320);
        }
        Scene::Legend => {
            fill_rect(buf, 680, 40, 80, 80);
            fill_rect(buf, 680, 200, 80, 80);
            fill_rect(buf, 680, 360, 80, 80);
        }
        Scene::Tones => {}
    }
}

/// First page: small Ferris, then `sticky-rs`, then a smaller hint.
///
/// On the unit: look at the glass, then press a right-edge key. In the MCU:
/// `embedded-graphics` into the existing 1-bit plane; OTP full refresh.
fn draw_splash(buf: &mut [u8]) {
    const HINT: &str = "Press a right-edge key to change drawings";
    const TITLE_GAP: i32 = 16;
    const HINT_GAP: i32 = 16;
    const TITLE_H: i32 = 20;
    const HINT_H: i32 = 10;

    let mut plane = MonoPlane { buf };
    let Ok(bmp) = Bmp::<BinaryColor>::from_slice(FERRIS_BMP) else {
        println!("embassy-debug: ferris bmp failed");
        return;
    };
    let size = bmp.size();
    let ferris_w = size.width as i32;
    let ferris_h = size.height as i32;
    let stack_h = ferris_h + TITLE_GAP + TITLE_H + HINT_GAP + HINT_H;
    let top = (i32::from(display::HEIGHT) - stack_h) / 2;
    let ferris_x = (i32::from(display::WIDTH) - ferris_w) / 2;
    let title_y = top + ferris_h + TITLE_GAP + TITLE_H;
    let hint_y = title_y + HINT_GAP + HINT_H;

    let _ = Image::new(&bmp, Point::new(ferris_x, top)).draw(&mut plane);
    let title = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let hint = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let _ = Text::with_alignment(
        "sticky-rs",
        Point::new(i32::from(display::WIDTH) / 2, title_y),
        title,
        Alignment::Center,
    )
    .draw(&mut plane);
    let _ = Text::with_alignment(
        HINT,
        Point::new(i32::from(display::WIDTH) / 2, hint_y),
        hint,
        Alignment::Center,
    )
    .draw(&mut plane);
}

/// Four landscape boxes, one OTP gray level each (black → white).
fn draw_tones(bw: &mut [u8], red: &mut [u8]) {
    bw.fill(0);
    red.fill(0);
    const BOX_W: u16 = 140;
    const BOX_H: u16 = 280;
    const BOX_Y: u16 = 100;
    const BOXES: [(u16, u8); 4] = [
        (60, gray::BLACK),
        (240, gray::DARK_GRAY),
        (420, gray::LIGHT_GRAY),
        (600, gray::WHITE),
    ];
    for (x, tone) in BOXES {
        fill_rect_gray(bw, red, x, BOX_Y, BOX_W, BOX_H, tone);
        stroke_rect_gray(bw, red, x, BOX_Y, BOX_W, BOX_H, gray::BLACK);
    }
}

/// Fill a gray4 rectangle (already 180°-rotated pixel writes).
fn fill_rect_gray(bw: &mut [u8], red: &mut [u8], x: u16, y: u16, w: u16, h: u16, tone: u8) {
    for yy in y..y.saturating_add(h).min(display::HEIGHT) {
        for xx in x..x.saturating_add(w).min(display::WIDTH) {
            set_gray(bw, red, xx, yy, tone);
        }
    }
}

/// Black outline so the white box is visible on a white field.
fn stroke_rect_gray(bw: &mut [u8], red: &mut [u8], x: u16, y: u16, w: u16, h: u16, tone: u8) {
    if w == 0 || h == 0 {
        return;
    }
    let x1 = x.saturating_add(w).saturating_sub(1);
    let y1 = y.saturating_add(h).saturating_sub(1);
    for xx in x..=x1.min(display::WIDTH.saturating_sub(1)) {
        set_gray(bw, red, xx, y, tone);
        set_gray(bw, red, xx, y1, tone);
    }
    for yy in y..=y1.min(display::HEIGHT.saturating_sub(1)) {
        set_gray(bw, red, x, yy, tone);
        set_gray(bw, red, x1, yy, tone);
    }
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

/// Filled black rectangle on the 1-bit splash/shapes/legend canvas.
fn fill_rect(buf: &mut [u8], x: u16, y: u16, w: u16, h: u16) {
    for yy in y..y.saturating_add(h).min(display::HEIGHT) {
        for xx in x..x.saturating_add(w).min(display::WIDTH) {
            set_black(buf, xx, yy);
        }
    }
}

/// Ink a pixel (`0`) on a 0xff-white 1-bit plane.
fn set_black(buf: &mut [u8], x: u16, y: u16) {
    if x >= display::WIDTH || y >= display::HEIGHT {
        return;
    }
    let x = usize::from(x);
    let y = usize::from(y);
    let stride = display::WIDTH as usize / 8;
    buf[y * stride + x / 8] &= !(0x80u8 >> (x % 8));
}

/// `embedded-graphics` target over the existing 48 KiB 1-bit plane.
///
/// [`BinaryColor::On`] is ink (same as [`set_black`]). The splash pipeline
/// still rotates and mirrors after this draw.
struct MonoPlane<'a> {
    buf: &'a mut [u8],
}

impl OriginDimensions for MonoPlane<'_> {
    fn size(&self) -> Size {
        Size::new(u32::from(display::WIDTH), u32::from(display::HEIGHT))
    }
}

impl DrawTarget for MonoPlane<'_> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x < 0 || point.y < 0 {
                continue;
            }
            let Ok(x) = u16::try_from(point.x) else {
                continue;
            };
            let Ok(y) = u16::try_from(point.y) else {
                continue;
            };
            if color == BinaryColor::On {
                set_black(self.buf, x, y);
            }
        }
        Ok(())
    }
}
