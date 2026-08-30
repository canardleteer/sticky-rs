//! OTP panel path: 1-bit full refresh, plus a four-tone OTP gray4 scene.

use embassy_debug::{Event, Scene};
use embassy_time::{with_timeout, Delay, Duration};
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

use crate::{emit, now_ms};

const WHITE: u8 = 0xff;
const REFRESH_TIMEOUT: Duration = Duration::from_secs(15);

static DRAW: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);
static TX: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);

/// Pins and the latch witness needed to bring the panel up.
pub struct PanelParts {
    /// Shared SPI controller.
    pub spi: SPI2<'static>,
    /// SCLK.
    pub sclk: GPIO13<'static>,
    /// MOSI.
    pub mosi: GPIO14<'static>,
    /// Chip select.
    pub cs: GPIO15<'static>,
    /// Data/command.
    pub dc: GPIO16<'static>,
    /// Reset.
    pub rst: GPIO17<'static>,
    /// BUSY (active high).
    pub busy: GPIO18<'static>,
}

/// White clear, then splash, then wait for [`crate::SCENE`] updates.
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

fn scene_kind(scene: Scene) -> RefreshKind {
    match scene {
        Scene::Tones => RefreshKind::Gray4,
        Scene::Splash | Scene::Shapes | Scene::Legend => RefreshKind::Full,
    }
}

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

fn draw_scene(scene: Scene, buf: &mut [u8]) {
    buf.fill(WHITE);
    match scene {
        Scene::Splash => {
            frame(buf, 8);
            line(buf, 40, 40, 760, 440);
            line(buf, 760, 40, 40, 440);
        }
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

fn fill_rect_gray(bw: &mut [u8], red: &mut [u8], x: u16, y: u16, w: u16, h: u16, tone: u8) {
    for yy in y..y.saturating_add(h).min(display::HEIGHT) {
        for xx in x..x.saturating_add(w).min(display::WIDTH) {
            set_gray(bw, red, xx, yy, tone);
        }
    }
}

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

fn frame(buf: &mut [u8], inset: u16) {
    let x1 = display::WIDTH - 1 - inset;
    let y1 = display::HEIGHT - 1 - inset;
    for x in inset..=x1 {
        set_black(buf, x, inset);
        set_black(buf, x, y1);
    }
    for y in inset..=y1 {
        set_black(buf, inset, y);
        set_black(buf, x1, y);
    }
}

fn fill_rect(buf: &mut [u8], x: u16, y: u16, w: u16, h: u16) {
    for yy in y..y.saturating_add(h).min(display::HEIGHT) {
        for xx in x..x.saturating_add(w).min(display::WIDTH) {
            set_black(buf, xx, yy);
        }
    }
}

fn line(buf: &mut [u8], x0: u16, y0: u16, x1: u16, y1: u16) {
    let mut x0 = i32::from(x0);
    let mut y0 = i32::from(y0);
    let x1 = i32::from(x1);
    let y1 = i32::from(y1);
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if let (Ok(x), Ok(y)) = (u16::try_from(x0), u16::try_from(y0)) {
            set_black(buf, x, y);
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

fn set_black(buf: &mut [u8], x: u16, y: u16) {
    if x >= display::WIDTH || y >= display::HEIGHT {
        return;
    }
    let x = usize::from(x);
    let y = usize::from(y);
    let stride = display::WIDTH as usize / 8;
    buf[y * stride + x / 8] &= !(0x80u8 >> (x % 8));
}
