//! OTP panel path: every equivalent card follows the in-plane IMU hold.
//!
//! Splash, shapes, legend, tones, pair, Wi-Fi survey / SoftAP, and the
//! Ferris off-screen compose in page space (480×800 portrait or
//! 800×480 landscape) then map through
//! [`seeed_reterminal_sticky::display::page_to_framebuffer`]. FaceUp /
//! FaceDown keep the last in-plane page. Waveforms stay in the panel
//! OTP — this file never writes a `0x32` LUT. Pixel work lives in
//! [`crate::draw`].

#[cfg(feature = "pair")]
use crate::draw::draw_pair;
use crate::draw::{draw_legend, draw_shapes, draw_splash, draw_tones};
#[cfg(feature = "wifi")]
use crate::draw::{draw_wifi_ap, draw_wifi_survey};
use crate::{emit, now_ms};

// Embassy time + UART scene token.
use embassy_debug::{Event, Scene};
#[cfg(feature = "pair")]
use embassy_futures::select::{select, select3, select4, Either, Either3, Either4};
#[cfg(not(feature = "pair"))]
use embassy_futures::select::{select, select3, select4, Either, Either3, Either4};
use embassy_time::{with_timeout, Delay, Duration, Instant, Timer};
use embedded_hal::delay::DelayNs;

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
use ssd1677_gray4::planes::rotate180_mono;
use ssd1677_gray4::{Ssd1677, UpdateSequence};
use static_cell::ConstStaticCell;

const REFRESH_TIMEOUT: Duration = Duration::from_secs(15);
/// After clock-off, BUSY can stay high. Do not sit on it for a full refresh.
const RESUME_POLL: Duration = Duration::from_millis(2000);

static DRAW: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);
static TX: ConstStaticCell<[u8; display::PLANE_BYTES]> =
    ConstStaticCell::new([0; display::PLANE_BYTES]);

#[cfg(all(feature = "spi20", feature = "mic"))]
compile_error!("do not combine spi20 with mic");
#[cfg(all(feature = "spi20", feature = "radio"))]
compile_error!("do not combine spi20 with radio");

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
/// On the unit: the current card stays upright in the four in-plane
/// holds. FaceUp / FaceDown keep the last of those. In the MCU: OTP
/// gray4 for splash, legend, tones, and pair; OTP 1-bit for
/// shapes. Page Up 2 s enters panel standby and stays there until
/// Page Up 1 s (resume) or Page Up 5 s (MCU sleep). Page Down 5 s
/// paints Ferris, parks the panel, and drops the latch. `PAGE_ROTATION`
/// always redraws the current scene. BLE advertises only while
/// `Scene::Pair` is showing.
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

    let mut driver = Some(Ssd1677::new(bus, dc, rst, busy, Delay).expect("panel reset"));
    let mut rail = Some(rail);
    let mut scene = start;
    let mut rotation = start_rotation;
    let mut kind = scene_kind(scene);
    #[cfg(feature = "pair")]
    crate::pair::set_visible(scene == Scene::Pair);
    #[cfg(feature = "wifi")]
    crate::wifi::set_ui_scene(scene);
    #[cfg(feature = "wifi")]
    crate::wifi::set_ui_rotation(rotation);
    driver
        .as_mut()
        .expect("panel driver")
        .init(&kind.controller_config())
        .expect("panel init");

    let draw = DRAW.take();
    let tx = TX.take();

    refresh(
        driver.as_mut().expect("panel driver"),
        draw,
        tx,
        scene,
        rotation,
        &mut kind,
    )
    .await;

    loop {
        #[cfg(all(feature = "pair", feature = "wifi"))]
        let e4 = match select(
            select4(
                crate::SCENE.wait(),
                crate::PAGE_ROTATION.wait(),
                crate::sleep::SLEEP_REQUEST.wait(),
                select(
                    crate::STANDBY_REQUEST.wait(),
                    crate::sleep::POWER_OFF_REQUEST.wait(),
                ),
            ),
            select(crate::pair::PAIR_VIEW.wait(), crate::wifi::WIFI_VIEW.wait()),
        )
        .await
        {
            Either::First(e4) => e4,
            Either::Second(Either::First(())) => {
                // PIN / ok / fail arrived. Repaint only if the operator
                // is already on the pair card so a key-walk stays put.
                if scene == Scene::Pair {
                    refresh(
                        driver.as_mut().expect("panel driver"),
                        draw,
                        tx,
                        scene,
                        rotation,
                        &mut kind,
                    )
                    .await;
                }
                continue;
            }
            Either::Second(Either::Second(())) => {
                if matches!(scene, Scene::WifiSurvey | Scene::WifiAp)
                    && crate::wifi::state_rev() != 0
                {
                    refresh(
                        driver.as_mut().expect("panel driver"),
                        draw,
                        tx,
                        scene,
                        rotation,
                        &mut kind,
                    )
                    .await;
                }
                continue;
            }
        };
        #[cfg(all(feature = "pair", not(feature = "wifi")))]
        let e4 = match select(
            select4(
                crate::SCENE.wait(),
                crate::PAGE_ROTATION.wait(),
                crate::sleep::SLEEP_REQUEST.wait(),
                select(
                    crate::STANDBY_REQUEST.wait(),
                    crate::sleep::POWER_OFF_REQUEST.wait(),
                ),
            ),
            crate::pair::PAIR_VIEW.wait(),
        )
        .await
        {
            Either::First(e4) => e4,
            Either::Second(()) => {
                if scene == Scene::Pair {
                    refresh(
                        driver.as_mut().expect("panel driver"),
                        draw,
                        tx,
                        scene,
                        rotation,
                        &mut kind,
                    )
                    .await;
                }
                continue;
            }
        };
        #[cfg(all(not(feature = "pair"), feature = "wifi"))]
        let e4 = match select(
            select4(
                crate::SCENE.wait(),
                crate::PAGE_ROTATION.wait(),
                crate::sleep::SLEEP_REQUEST.wait(),
                select(
                    crate::STANDBY_REQUEST.wait(),
                    crate::sleep::POWER_OFF_REQUEST.wait(),
                ),
            ),
            crate::wifi::WIFI_VIEW.wait(),
        )
        .await
        {
            Either::First(e4) => e4,
            Either::Second(()) => {
                if matches!(scene, Scene::WifiSurvey | Scene::WifiAp)
                    && crate::wifi::state_rev() != 0
                {
                    refresh(
                        driver.as_mut().expect("panel driver"),
                        draw,
                        tx,
                        scene,
                        rotation,
                        &mut kind,
                    )
                    .await;
                }
                continue;
            }
        };
        #[cfg(not(any(feature = "pair", feature = "wifi")))]
        let e4 = select4(
            crate::SCENE.wait(),
            crate::PAGE_ROTATION.wait(),
            crate::sleep::SLEEP_REQUEST.wait(),
            select(
                crate::STANDBY_REQUEST.wait(),
                crate::sleep::POWER_OFF_REQUEST.wait(),
            ),
        )
        .await;
        match e4 {
            Either4::First(next) => {
                scene = next;
                #[cfg(feature = "pair")]
                crate::pair::set_visible(scene == Scene::Pair);
                #[cfg(feature = "wifi")]
                crate::wifi::set_ui_scene(scene);
                refresh(
                    driver.as_mut().expect("panel driver"),
                    draw,
                    tx,
                    scene,
                    rotation,
                    &mut kind,
                )
                .await;
            }
            Either4::Second(next) => {
                if next == rotation {
                    continue;
                }
                rotation = next;
                #[cfg(feature = "wifi")]
                crate::wifi::set_ui_rotation(rotation);
                refresh(
                    driver.as_mut().expect("panel driver"),
                    draw,
                    tx,
                    scene,
                    rotation,
                    &mut kind,
                )
                .await;
            }
            Either4::Third(()) => {
                if let Some((kept_driver, kept_rail)) = leave_for_sleep(
                    driver.take().expect("panel driver"),
                    rail.take().expect("panel rail"),
                    draw,
                    tx,
                    rotation,
                    &mut kind,
                    false,
                )
                .await
                {
                    driver = Some(kept_driver);
                    rail = Some(kept_rail);
                }
            }
            Either4::Fourth(Either::First(())) => {
                if let Some((kept_driver, kept_rail)) = sit_standby(
                    driver.take().expect("panel driver"),
                    rail.take().expect("panel rail"),
                    draw,
                    tx,
                    scene,
                    rotation,
                    &mut kind,
                )
                .await
                {
                    driver = Some(kept_driver);
                    rail = Some(kept_rail);
                }
            }
            Either4::Fourth(Either::Second(())) => {
                if let Some((kept_driver, kept_rail)) = leave_for_power_off(
                    driver.take().expect("panel driver"),
                    rail.take().expect("panel rail"),
                    draw,
                    tx,
                    rotation,
                    &mut kind,
                    false,
                )
                .await
                {
                    driver = Some(kept_driver);
                    rail = Some(kept_rail);
                }
            }
        }
    }
}

/// Gray4 for splash, legend, tones, pair, and Wi-Fi; OTP 1-bit full for shapes.
///
/// The Ferris off-screen is [`Scene::Splash`]: [`paint_off_card`] forces gray4.
fn scene_kind(scene: Scene) -> RefreshKind {
    match scene {
        Scene::Splash | Scene::Legend | Scene::Tones => RefreshKind::Gray4,
        #[cfg(feature = "pair")]
        Scene::Pair => RefreshKind::Gray4,
        #[cfg(feature = "wifi")]
        Scene::WifiSurvey | Scene::WifiAp => RefreshKind::Gray4,
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
            if !write_mono_scene(driver, draw, tx, scene, rotation) {
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

/// Print `embassy-debug: epd {tag} busy=` from a non-blocking poll.
///
/// After `standby` / clock-off, BUSY can sit high. This is a look, not
/// `wait_until_idle_async` (that can run to the refresh timeout).
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

/// Ferris, park, MCU sleep. Latch stays high.
///
/// Returns the driver only when the Ferris paint failed so the image
/// can stay awake. On success this parks and waits forever.
async fn leave_for_sleep<SPI, DC, RST, BUSY>(
    mut driver: Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    rail: EpdRail<Output<'static>, Enabled>,
    draw: &mut [u8; display::PLANE_BYTES],
    tx: &mut [u8; display::PLANE_BYTES],
    rotation: PageRotation,
    kind: &mut RefreshKind,
    from_standby: bool,
) -> Option<(
    Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    EpdRail<Output<'static>, Enabled>,
)>
where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error> + Wait<Error = DC::Error>,
{
    #[cfg(feature = "pair")]
    crate::pair::set_visible(false);
    crate::sleep::persist(Scene::Splash, rotation);
    if from_standby && !wake_after_standby(&mut driver, kind) {
        println!("embassy-debug: off paint failed");
        crate::sleep::cancel_sleep_request();
        crate::sleep::leave_standby();
        return Some((driver, rail));
    }
    if !paint_off_card(&mut driver, draw, tx, rotation, kind, true).await {
        println!("embassy-debug: off paint failed");
        crate::sleep::cancel_sleep_request();
        crate::sleep::leave_standby();
        return Some((driver, rail));
    }
    park_panel(driver, rail);
    crate::sleep::PANEL_PARKED.signal(());
    loop {
        Timer::after(Duration::from_secs(3_600)).await;
    }
}

/// Ferris, park, then the latch may drop.
///
/// Returns the driver only when the Ferris paint failed so the latch
/// stays high. On success this parks and waits forever.
async fn leave_for_power_off<SPI, DC, RST, BUSY>(
    mut driver: Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    rail: EpdRail<Output<'static>, Enabled>,
    draw: &mut [u8; display::PLANE_BYTES],
    tx: &mut [u8; display::PLANE_BYTES],
    rotation: PageRotation,
    kind: &mut RefreshKind,
    from_standby: bool,
) -> Option<(
    Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    EpdRail<Output<'static>, Enabled>,
)>
where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error> + Wait<Error = DC::Error>,
{
    #[cfg(feature = "pair")]
    crate::pair::set_visible(false);
    if from_standby && !wake_after_standby(&mut driver, kind) {
        println!("embassy-debug: off paint failed");
        crate::sleep::cancel_power_off_request();
        crate::sleep::leave_standby();
        return Some((driver, rail));
    }
    if !paint_off_card(&mut driver, draw, tx, rotation, kind, false).await {
        println!("embassy-debug: off paint failed");
        crate::sleep::cancel_power_off_request();
        crate::sleep::leave_standby();
        return Some((driver, rail));
    }
    park_panel(driver, rail);
    crate::sleep::POWER_OFF_READY.signal(());
    loop {
        Timer::after(Duration::from_secs(3_600)).await;
    }
}

/// [`Ssd1677::standby`], then wait for Page Up 1 s (resume), Page Up 5 s
/// (sleep), or Page Down 5 s (power-off).
///
/// `EPD_EN` stays high until sleep or power-off. Clock-off can leave
/// BUSY high; stock `0xC0` may not drop it. Resume tries
/// [`UpdateSequence::ENABLE_CLOCK`], then a hardware reset + init.
async fn sit_standby<SPI, DC, RST, BUSY>(
    mut driver: Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    rail: EpdRail<Output<'static>, Enabled>,
    draw: &mut [u8; display::PLANE_BYTES],
    tx: &mut [u8; display::PLANE_BYTES],
    scene: Scene,
    rotation: PageRotation,
    kind: &mut RefreshKind,
) -> Option<(
    Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    EpdRail<Output<'static>, Enabled>,
)>
where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error> + Wait<Error = DC::Error>,
{
    if driver.standby().is_err() {
        println!("embassy-debug: epd standby failed");
        crate::sleep::leave_standby();
        return Some((driver, rail));
    }
    emit(Event::Standby { t_ms: now_ms() });
    match select3(
        crate::STANDBY_RESUME.wait(),
        crate::sleep::SLEEP_REQUEST.wait(),
        crate::sleep::POWER_OFF_REQUEST.wait(),
    )
    .await
    {
        Either3::First(()) => {
            resume_from_standby(&mut driver, draw, tx, scene, rotation, kind).await;
            crate::sleep::leave_standby();
            Some((driver, rail))
        }
        Either3::Second(()) => leave_for_sleep(driver, rail, draw, tx, rotation, kind, true).await,
        Either3::Third(()) => {
            leave_for_power_off(driver, rail, draw, tx, rotation, kind, true).await
        }
    }
}

/// Bring the panel back after [`Ssd1677::standby`] and redraw the card.
///
/// Stock `0xC0` and `ENABLE_CLOCK` left BUSY high on this glass. Fall
/// back is RST + OTP `init` (`epd resume rst`).
async fn resume_from_standby<SPI, DC, RST, BUSY>(
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

/// RST + OTP init so a clock-off panel can take a Ferris frame.
///
/// Stock `0xC0` left BUSY high after standby. Do not sit on that.
fn wake_after_standby<SPI, DC, RST, BUSY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    kind: &mut RefreshKind,
) -> bool
where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin<Error = DC::Error>,
    BUSY: InputPin<Error = DC::Error>,
{
    let next = RefreshKind::Gray4;
    if driver.hardware_reset().is_err() || driver.init(&next.controller_config()).is_err() {
        println!("embassy-debug: epd re-init failed");
        return false;
    }
    *kind = next;
    true
}

/// Ferris splash in the current page, then wait BUSY.
///
/// Last frame before deep sleep or latch power-off so the glass shows
/// Ferris. Returns false if the write or wait failed so the caller can
/// stay awake (`EPD_EN` on) instead of parking a blank panel.
/// `sleeping` selects `scene=sleeping` versus `poweroff`.
async fn paint_off_card<SPI, DC, RST, BUSY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, Delay, ssd1677_gray4::Active>,
    bw: &mut [u8; display::PLANE_BYTES],
    red: &mut [u8; display::PLANE_BYTES],
    rotation: PageRotation,
    kind: &mut RefreshKind,
    sleeping: bool,
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
    draw_splash(bw, red, rotation);
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
            if sleeping {
                emit(Event::Sleeping { t_ms: now_ms() });
            } else {
                emit(Event::PowerOff { t_ms: now_ms() });
            }
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
        let mut pin = rail.release();
        crate::sleep::hold_output(&mut pin);
        core::mem::forget(pin);
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
/// Draw a 1-bit page, rotate 180°, then write both RAM planes
/// (second plane cleared).
///
/// `rotation` selects the page axes; [`draw_shapes`] maps through
/// [`seeed_reterminal_sticky::display::page_to_framebuffer`]. Do not
/// `mirror_x_plane` here: that reverse_bits each byte along panel X,
/// which is up/down on the USB-down page and flips 8-pixel-tall bands.
fn write_mono_scene<SPI, DC, RST, BUSY, DELAY>(
    driver: &mut Ssd1677<SPI, DC, RST, BUSY, DELAY, ssd1677_gray4::Active>,
    draw: &mut [u8; display::PLANE_BYTES],
    tx: &mut [u8; display::PLANE_BYTES],
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
        Scene::Shapes => draw_shapes(draw, rotation),
        Scene::Splash | Scene::Legend | Scene::Tones => {}
        #[cfg(feature = "pair")]
        Scene::Pair => {}
        #[cfg(feature = "wifi")]
        Scene::WifiSurvey | Scene::WifiAp => {}
    }
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

/// Splash, legend, tones, pair, or Wi-Fi. Gray4 pixels are already 180°-aware
/// in [`crate::draw`]. No `mirror_x_plane` — see [`write_mono_scene`].
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
        Scene::Legend => draw_legend(bw, red, rotation),
        Scene::Tones => draw_tones(bw, red, rotation),
        #[cfg(feature = "pair")]
        Scene::Pair => draw_pair(bw, red, rotation),
        #[cfg(feature = "wifi")]
        Scene::WifiSurvey => draw_wifi_survey(bw, red, rotation),
        #[cfg(feature = "wifi")]
        Scene::WifiAp => draw_wifi_ap(bw, red, rotation),
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
