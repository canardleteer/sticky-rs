//! OTP panel path: splash follows IMU; other pages stay USB-down.
//!
//! Splash is two drawings: a 480×800 portrait stack and an 800×480
//! landscape stack (Ferris + `sticky-rs` + hints). FaceUp / FaceDown
//! keep the last in-plane page. Shapes, legend, and tones stay USB-down
//! portrait. Waveforms stay in the panel OTP — this file never writes a
//! `0x32` LUT.

use crate::{emit, now_ms};

// Embassy time + UART scene token.
use embassy_debug::{Event, Scene, STANDBY_LOOK_MS};
use embassy_futures::select::{select4, Either4};
use embassy_time::{with_timeout, Delay, Duration, Instant, Timer};
use embedded_hal::delay::DelayNs;

// Draw splash with built-in mono fonts.
use embedded_graphics::mono_font::ascii::FONT_10X20;
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
use seeed_reterminal_sticky::display::{self, PageRotation, RefreshKind};
use seeed_reterminal_sticky::rails::{Enabled, EpdRail, PanelParked};
use ssd1677_gray4::planes::{gray, rotate180_mono, write_mono, PlaneMapping};
use ssd1677_gray4::{Ssd1677, UpdateSequence};
use static_cell::ConstStaticCell;

const WHITE: u8 = 0xff;
const REFRESH_TIMEOUT: Duration = Duration::from_secs(15);
/// After clock-off, BUSY can stay high. Do not sit on it for a full refresh.
const RESUME_POLL: Duration = Duration::from_millis(2000);

/// 240×160 packed 2bpp Ferris; provenance in `assets/SOURCE.md`.
const FERRIS: &[u8] = include_bytes!("../assets/ferris.g4");
const FERRIS_W: u16 = 360;
const FERRIS_H: u16 = 240;

static DRAW: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);
static TX: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);

#[cfg(feature = "spi20")]
const SPI_HZ: u32 = 20_000_000;
#[cfg(not(feature = "spi20"))]
const SPI_HZ: u32 = display::SPI_MAX_HZ;

/// Shared SPI pins for the panel (same bus as the card).
pub struct PanelParts {
    /// Shared SPI controller.
    pub spi: SPI2<'static>,
    /// SCLK.
    pub sclk: GPIO13<'static>,
    /// MOSI.
    pub mosi: GPIO14<'static>,
    /// MISO. Needed only when `--features sd`.
    #[cfg(feature = "sd")]
    pub miso: esp_hal::peripherals::GPIO12<'static>,
    /// Chip select. Idle-high except during a transfer.
    pub cs: GPIO15<'static>,
    /// Data/command.
    pub dc: GPIO16<'static>,
    /// Reset.
    pub rst: GPIO17<'static>,
    /// BUSY (active high). Do not talk on the bus while it is high.
    pub busy: GPIO18<'static>,
    /// Read-only identify. The card CS is never asserted on the default image.
    #[cfg(feature = "sd")]
    pub sd: crate::sd::SdParts,
}

/// Bring the panel up, paint the start card, then wait for a key or an IMU pose.
///
/// On the unit: splash stays upright in the four in-plane holds. FaceUp /
/// FaceDown keep the last of those. Other pages stay USB-down. In the
/// MCU: OTP gray4 for splash, the key legend, and tones; OTP 1-bit for
/// shapes. A 2 s Page Up runs panel standby then resume. A 4 s Page
/// Down paints the sleep card and parks the panel.
#[embassy_executor::task]
pub async fn display_task(
    parts: PanelParts,
    rail: EpdRail<Output<'static>, Enabled>,
    start: Scene,
    start_rotation: PageRotation,
) {
    #[cfg(feature = "sd")]
    let start_hz = seeed_reterminal_sticky::sd::INIT_HZ;
    #[cfg(not(feature = "sd"))]
    let start_hz = SPI_HZ;
    let spi = Spi::new(
        parts.spi,
        SpiConfig::default()
            .with_frequency(Rate::from_hz(start_hz))
            .with_mode(Mode::_0),
    )
    .expect("SPI configuration")
    .with_sck(parts.sclk)
    .with_mosi(parts.mosi);
    #[cfg(feature = "sd")]
    let mut spi = spi.with_miso(parts.miso);
    #[cfg(feature = "sd")]
    let mut delay = Delay;
    #[cfg(not(feature = "sd"))]
    let delay = Delay;
    #[cfg(feature = "sd")]
    {
        crate::sd::run(&mut spi, parts.sd, &mut delay);
        let _ = spi.apply_config(
            &SpiConfig::default()
                .with_frequency(Rate::from_hz(SPI_HZ))
                .with_mode(Mode::_0),
        );
    }
    println!("embassy-debug: spi={SPI_HZ}");

    let cs = Output::new(parts.cs, Level::High, OutputConfig::default());
    let bus = ExclusiveDevice::new(spi, cs, delay).expect("EPD CS");
    let dc = Output::new(parts.dc, Level::Low, OutputConfig::default());
    let rst = Output::new(parts.rst, Level::High, OutputConfig::default());
    let busy = Input::new(parts.busy, InputConfig::default().with_pull(Pull::None));

    let mut driver = Ssd1677::new(bus, dc, rst, busy, Delay).expect("panel reset");
    let mut scene = start;
    let mut rotation = start_rotation;
    let mut kind = scene_kind(scene);
    driver.init(&kind.controller_config()).expect("panel init");

    let draw = DRAW.take();
    let tx = TX.take();

    refresh(&mut driver, draw, tx, scene, rotation, &mut kind).await;

    loop {
        match select4(
            crate::SCENE.wait(),
            crate::PAGE_ROTATION.wait(),
            crate::sleep::SLEEP_REQUEST.wait(),
            crate::STANDBY_REQUEST.wait(),
        )
        .await
        {
            Either4::First(next) => {
                scene = next;
                refresh(&mut driver, draw, tx, scene, rotation, &mut kind).await;
            }
            Either4::Second(next) => {
                if next == rotation {
                    continue;
                }
                rotation = next;
                if scene == Scene::Splash {
                    refresh(&mut driver, draw, tx, scene, rotation, &mut kind).await;
                }
            }
            Either4::Third(()) => {
                crate::sleep::persist(scene, rotation);
                if paint_sleep_card(&mut driver, draw, tx, &mut kind).await {
                    park_panel(driver, rail);
                }
                crate::sleep::PANEL_PARKED.signal(());
                loop {
                    embassy_time::Timer::after(embassy_time::Duration::from_secs(3_600)).await;
                }
            }
            Either4::Fourth(()) => {
                run_standby_resume(&mut driver, draw, tx, scene, rotation, &mut kind).await;
            }
        }
    }
}

/// Gray4 for splash, legend text, and the four-tone page; 1-bit for shapes.
fn scene_kind(scene: Scene) -> RefreshKind {
    match scene {
        Scene::Splash | Scene::Legend | Scene::Tones => RefreshKind::Gray4,
        Scene::Shapes => RefreshKind::Full,
    }
}

/// Re-init the controller when the OTP sequence changes, write planes,
/// wait for BUSY, then print `scene=…` on UART.
async fn refresh<SPI, DC, RST, BUSY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    draw: &mut [u8; display::PLANE_BYTES],
    tx: &mut [u8; display::PLANE_BYTES],
    scene: Scene,
    rotation: PageRotation,
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
            if !write_gray4_scene(driver, draw, tx, scene, rotation) {
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

fn print_busy<SPI, DC, RST, BUSY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    tag: &str,
) where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error>,
{
    match driver.is_busy() {
        Ok(true) => println!("embassy-debug: epd {tag} busy=1"),
        Ok(false) => println!("embassy-debug: epd {tag} busy=0"),
        Err(_) => println!("embassy-debug: epd {tag} busy=?"),
    }
}

/// Poll BUSY. After clock-off, `wait_for_low` can sit until the refresh timeout.
async fn busy_cleared<SPI, DC, RST, BUSY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    timeout: Duration,
) -> bool
where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error>,
{
    if matches!(driver.is_busy(), Ok(false)) {
        return true;
    }
    let start = Instant::now();
    while Instant::now() - start < timeout {
        Timer::after(Duration::from_millis(20)).await;
        match driver.is_busy() {
            Ok(false) => return true,
            Ok(true) => {}
            Err(_) => return false,
        }
    }
    matches!(driver.is_busy(), Ok(false))
}

/// [`Ssd1677::standby`], look, [`Ssd1677::resume`], then the same card.
///
/// `EPD_EN` stays high. This is not [`Ssd1677::sleep`]. Clock-off can leave
/// BUSY high; stock `0xC0` may not drop it. Try [`UpdateSequence::ENABLE_CLOCK`]
/// next, then a hardware reset + init so the card can refresh.
async fn run_standby_resume<SPI, DC, RST, BUSY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    draw: &mut [u8; display::PLANE_BYTES],
    tx: &mut [u8; display::PLANE_BYTES],
    scene: Scene,
    rotation: PageRotation,
    kind: &mut RefreshKind,
) where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error> + Wait<Error = DC::Error>,
{
    if driver.standby().is_err() {
        println!("embassy-debug: epd standby failed");
        return;
    }
    emit(Event::Standby { t_ms: now_ms() });
    Timer::after(Duration::from_millis(u64::from(STANDBY_LOOK_MS))).await;
    print_busy(driver, "look");
    if driver.resume().is_err() {
        println!("embassy-debug: epd resume failed");
        return;
    }
    print_busy(driver, "c0");
    if busy_cleared(driver, RESUME_POLL).await {
        emit(Event::Resumed { t_ms: now_ms() });
        refresh(driver, draw, tx, scene, rotation, kind).await;
        return;
    }
    if driver
        .start_update_sequence(UpdateSequence::ENABLE_CLOCK)
        .is_err()
    {
        println!("embassy-debug: epd resume failed");
        return;
    }
    print_busy(driver, "clk");
    if busy_cleared(driver, RESUME_POLL).await && driver.resume().is_ok() {
        print_busy(driver, "c0b");
        if busy_cleared(driver, RESUME_POLL).await {
            println!("embassy-debug: epd resume clk");
            emit(Event::Resumed { t_ms: now_ms() });
            refresh(driver, draw, tx, scene, rotation, kind).await;
            return;
        }
    }
    println!("embassy-debug: epd resume rst");
    if driver.hardware_reset().is_err() || driver.init(&kind.controller_config()).is_err() {
        println!("embassy-debug: epd re-init failed");
        return;
    }
    emit(Event::Resumed { t_ms: now_ms() });
    refresh(driver, draw, tx, scene, rotation, kind).await;
}

/// Sleep card, then wait BUSY. Returns false if the write or wait failed.
async fn paint_sleep_card<SPI, DC, RST, BUSY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    bw: &mut [u8; display::PLANE_BYTES],
    red: &mut [u8; display::PLANE_BYTES],
    kind: &mut RefreshKind,
) -> bool
where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error> + Wait<Error = DC::Error>,
{
    let next = RefreshKind::Gray4;
    if next != *kind {
        if driver.init(&next.controller_config()).is_err() {
            println!("embassy-debug: epd re-init failed");
            return false;
        }
        *kind = next;
    }
    draw_sleep_card(bw, red);
    if driver
        .write_gray4_frame(&display::FULL_WINDOW, bw, red)
        .is_err()
    {
        println!("embassy-debug: epd write failed");
        return false;
    }
    if driver.start_update_sequence(next.sequence()).is_err() {
        println!("embassy-debug: epd update failed");
        return false;
    }
    match with_timeout(REFRESH_TIMEOUT, driver.wait_until_idle_async()).await {
        Ok(Ok(())) => {
            emit(Event::Sleeping { t_ms: now_ms() });
            true
        }
        Ok(Err(_)) | Err(_) => {
            println!("embassy-debug: epd busy timeout");
            false
        }
    }
}

/// [`Ssd1677::sleep`] ([`ssd1677_gray4::Command::DeepSleepMode`] /
/// [`ssd1677_gray4::DeepSleep::Enter`]), wait
/// [`display::SLEEP_HOLD_MS`], cut `EPD_EN`, hold the pad.
fn park_panel<SPI, DC, RST, BUSY>(
    driver: Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    rail: EpdRail<Output<'static>, Enabled>,
) where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error>,
{
    let Ok(asleep) = driver.sleep() else {
        println!("embassy-debug: epd sleep failed");
        return;
    };
    let _ = asleep.release();
    Delay.delay_ms(display::SLEEP_HOLD_MS);
    let disabled = rail
        .disable_after_panel_sleep(PanelParked::after_deep_sleep_command())
        .expect("driving the panel rail cannot fail");
    let mut pin = disabled.release();
    crate::sleep::hold_output(&mut pin);
    core::mem::forget(pin);
}

/// USB-down portrait: box plus the resume hint.
fn draw_sleep_card(bw: &mut [u8], red: &mut [u8]) {
    const ROT: PageRotation = PageRotation::Portrait0;
    clear_gray(bw, red, gray::WHITE, ROT);
    fill_rect_gray(bw, red, 40, 280, 400, 200, gray::WHITE, ROT);
    stroke_rect_gray(bw, red, 40, 280, 400, 200, gray::BLACK, ROT);
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let cx = i32::from(display::PAGE_WIDTH / 2);
    let _ = Text::with_alignment("sleeping,", Point::new(cx, 340), style, Alignment::Center)
        .draw(&mut GrayInk::new(bw, red, 2, ROT));
    let _ = Text::with_alignment(
        "hold page down",
        Point::new(cx, 380),
        style,
        Alignment::Center,
    )
    .draw(&mut GrayInk::new(bw, red, 2, ROT));
    let _ = Text::with_alignment("to resume", Point::new(cx, 420), style, Alignment::Center)
        .draw(&mut GrayInk::new(bw, red, 2, ROT));
}

/// Draw a 1-bit portrait page, rotate 180°, then write both RAM planes
/// (second plane cleared).
///
/// Do not `mirror_x_plane` here: that reverse_bits each byte along panel
/// X, which is up/down on the USB-down page and flips 8-pixel-tall bands.
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

/// Splash or four-tone page. Pixels are already 180°-aware in
/// [`set_gray`]. No `mirror_x_plane` — see [`write_mono_scene`].
fn write_gray4_scene<SPI, DC, RST, BUSY, DELAY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, DELAY, ssd1677_gray4::Active>,
    bw: &mut [u8; display::PLANE_BYTES],
    red: &mut [u8; display::PLANE_BYTES],
    scene: Scene,
    rotation: PageRotation,
) -> bool
where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error>,
    DELAY: embedded_hal::delay::DelayNs,
{
    match scene {
        Scene::Splash => draw_splash(bw, red, rotation),
        Scene::Legend => draw_legend(bw, red),
        Scene::Tones => draw_tones(bw, red),
        Scene::Shapes => {}
    }

    if driver
        .write_gray4_frame(&display::FULL_WINDOW, bw, red)
        .is_err()
    {
        println!("embassy-debug: epd write failed");
        return false;
    }
    true
}

/// Paint one 1-bit portrait page (0xff = white, cleared bit = ink).
fn draw_scene(scene: Scene, buf: &mut [u8]) {
    buf.fill(WHITE);
    match scene {
        Scene::Shapes => {
            fill_rect(buf, 40, 80, 180, 140);
            fill_rect(buf, 160, 280, 140, 200);
            fill_rect(buf, 80, 560, 280, 160);
        }
        Scene::Splash | Scene::Legend | Scene::Tones => {}
    }
}

/// Three squares beside the right-edge keys, Seeed names to the left.
///
/// Centers come from the Seeed appearance diagram (front view): screen
/// y 75–429, key nubs AI Voice 90–120, Page Up 141–183, Page Down
/// 189–231. All three sit in the top half of the glass.
fn draw_legend(bw: &mut [u8], red: &mut [u8]) {
    const BOX: u16 = 72;
    const MARGIN: u16 = 8;
    const GAP: i32 = 12;
    const X: u16 = display::PAGE_WIDTH - MARGIN - BOX;
    const KEYS: [(&str, u16); 3] = [("AI Voice", 68), ("Page Up", 197), ("Page Down", 305)];

    clear_gray(bw, red, gray::WHITE, PageRotation::Portrait0);
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let text_x = i32::from(X) - GAP;
    for (label, center) in KEYS {
        let y = center.saturating_sub(BOX / 2);
        fill_rect_gray(
            bw,
            red,
            X,
            y,
            BOX,
            BOX,
            gray::BLACK,
            PageRotation::Portrait0,
        );
        let _ = Text::with_alignment(
            label,
            Point::new(text_x, i32::from(center) + 6),
            style,
            Alignment::Right,
        )
        .draw(&mut GrayInk::new(bw, red, 1, PageRotation::Portrait0));
    }
}

/// Splash: portrait 480×800 or landscape 800×480, then map to the panel.
fn draw_splash(bw: &mut [u8], red: &mut [u8], rotation: PageRotation) {
    match rotation {
        PageRotation::Portrait0 | PageRotation::Portrait180 => {
            draw_splash_portrait(bw, red, rotation);
        }
        PageRotation::Landscape0 | PageRotation::Landscape180 => {
            draw_splash_landscape(bw, red, rotation);
        }
    }
}

/// USB-C down / up: Ferris, title, hints stacked on the 480×800 page.
fn draw_splash_portrait(bw: &mut [u8], red: &mut [u8], rotation: PageRotation) {
    draw_splash_stack(bw, red, rotation, display::PAGE_WIDTH, display::PAGE_HEIGHT);
}

/// USB-C right / left: the same stack composed on the 800×480 page.
fn draw_splash_landscape(bw: &mut [u8], red: &mut [u8], rotation: PageRotation) {
    draw_splash_stack(bw, red, rotation, display::WIDTH, display::HEIGHT);
}

/// Center Ferris + `sticky-rs` + two hints in a page. White Ferris pixels
/// are skipped.
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
    const TITLE_FAT: u16 = 1;
    const HINT_FAT: u16 = 1;
    const GLYPH_H: i32 = 20;

    clear_gray(bw, red, gray::WHITE, rotation);

    let ferris_w = i32::from(FERRIS_W);
    let ferris_h = i32::from(FERRIS_H);
    let title_h = GLYPH_H + i32::from(TITLE_FAT) - 1;
    let hint_h = GLYPH_H + i32::from(HINT_FAT) - 1;
    let stack_h = ferris_h + TITLE_GAP + title_h + HINT_GAP + hint_h + LINE_GAP + hint_h;
    let top = (i32::from(page_h) - stack_h) / 2;
    let cx = i32::from(page_w) / 2;
    let ferris_x = (i32::from(page_w) - ferris_w) / 2;
    let title_y = top + ferris_h + TITLE_GAP + title_h;
    let hint1_y = title_y + HINT_GAP + hint_h;
    let hint2_y = hint1_y + LINE_GAP + hint_h;

    blit_ferris(bw, red, ferris_x, top, rotation);
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let _ = Text::with_alignment(
        "sticky-rs",
        Point::new(cx, title_y),
        style,
        Alignment::Center,
    )
    .draw(&mut GrayInk::new(bw, red, TITLE_FAT, rotation));
    let _ = Text::with_alignment(
        "Press a right-edge key",
        Point::new(cx, hint1_y),
        style,
        Alignment::Center,
    )
    .draw(&mut GrayInk::new(bw, red, HINT_FAT, rotation));
    let _ = Text::with_alignment(
        "to change drawings",
        Point::new(cx, hint2_y),
        style,
        Alignment::Center,
    )
    .draw(&mut GrayInk::new(bw, red, HINT_FAT, rotation));
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

fn ferris_tone(x: u16, y: u16) -> u8 {
    let i = usize::from(y) * usize::from(FERRIS_W) + usize::from(x);
    let byte = FERRIS[i / 4];
    let shift = 6 - (i % 4) * 2;
    (byte >> shift) & 0b11
}

/// Four portrait boxes, one OTP gray level each (black → white).
fn draw_tones(bw: &mut [u8], red: &mut [u8]) {
    clear_gray(bw, red, gray::WHITE, PageRotation::Portrait0);
    const BOX_W: u16 = 360;
    const BOX_H: u16 = 140;
    const BOX_X: u16 = 60;
    const BOXES: [(u16, u8); 4] = [
        (80, gray::BLACK),
        (250, gray::DARK_GRAY),
        (420, gray::LIGHT_GRAY),
        (590, gray::WHITE),
    ];
    for (y, tone) in BOXES {
        fill_rect_gray(
            bw,
            red,
            BOX_X,
            y,
            BOX_W,
            BOX_H,
            tone,
            PageRotation::Portrait0,
        );
        stroke_rect_gray(
            bw,
            red,
            BOX_X,
            y,
            BOX_W,
            BOX_H,
            gray::BLACK,
            PageRotation::Portrait0,
        );
    }
}

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

/// Black outline so the white box is visible on a white field.
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

/// Filled black rectangle on the 1-bit portrait canvas.
fn fill_rect(buf: &mut [u8], x: u16, y: u16, w: u16, h: u16) {
    for yy in y..y.saturating_add(h).min(display::PAGE_HEIGHT) {
        for xx in x..x.saturating_add(w).min(display::PAGE_WIDTH) {
            set_black_page(buf, xx, yy);
        }
    }
}

fn set_black_page(buf: &mut [u8], px: u16, py: u16) {
    let Some((x, y)) = display::portrait_to_framebuffer(px, py) else {
        return;
    };
    set_black(buf, x, y);
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

/// `embedded-graphics` target over OTP gray4 in the current page.
///
/// [`BinaryColor::On`] is black ink. `fat` > 1 paints each source pixel
/// as a `fat`×`fat` block so mono glyphs survive the refresh.
struct GrayInk<'a> {
    bw: &'a mut [u8],
    red: &'a mut [u8],
    fat: u16,
    rotation: PageRotation,
}

impl<'a> GrayInk<'a> {
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
