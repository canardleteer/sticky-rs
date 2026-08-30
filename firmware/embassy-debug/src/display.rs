//! OTP 1-bit panel path. Compiled only with `--features epd`.

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
use ssd1677_gray4::planes::{mirror_x_plane, rotate180_mono};
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
    let config = RefreshKind::Full.controller_config();
    driver.init(&config).expect("panel init");

    let draw = DRAW.take();
    let tx = TX.take();

    let mut scene = Scene::Splash;
    refresh(&mut driver, draw, tx, scene).await;

    loop {
        scene = crate::SCENE.wait().await;
        refresh(&mut driver, draw, tx, scene).await;
    }
}

async fn refresh<SPI, DC, RST, BUSY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    draw: &mut [u8; display::PLANE_BYTES],
    tx: &mut [u8; display::PLANE_BYTES],
    scene: Scene,
) where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error> + Wait<Error = DC::Error>,
{
    draw_scene(scene, draw);
    if rotate180_mono(draw, display::WIDTH as usize, display::HEIGHT as usize, tx).is_err() {
        println!("embassy-debug: epd rotate failed");
        return;
    }
    mirror_x_plane(tx);

    draw.fill(0);
    if driver.write_black_white_plane(tx).is_err() {
        println!("embassy-debug: epd write failed");
        return;
    }
    if driver.write_second_plane(draw).is_err() {
        println!("embassy-debug: epd write failed");
        return;
    }

    if driver
        .start_update_sequence(RefreshKind::Full.sequence())
        .is_err()
    {
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
    }
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
