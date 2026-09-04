//! reTerminal Sticky Embassy event-logger image.
//!
//! On the unit: cards stay upright in the four in-plane holds; right-edge
//! keys change the drawing; Page Up 2 s is panel standby, Page Up 5 s
//! is MCU sleep (one hold can do both); Page Up 1 s leaves either;
//! Page Down 5 s drops the latch. Power-on is USB-C plug or the stock
//! ~3 s AI Voice hold. Taps and tilts print on UART0; a short beep
//! answers a key or the first finger on the glass.
//!
//! In the MCU: latch power, park the charger and unused rails, bring up
//! the two I2C buses and the panel OTP path. `--features charge` may
//! pulse `/CE` for two seconds when VBUS is present after a cold boot
//! or a 1 s Page Up resume hold, then parks again. A wake that
//! re-sleeps does not pulse `/CE`.
//! No invented LUT.
//!
//! # Before flashing anything
//!
//! Agent flash contract and envelope: the sibling `AGENTS.md`.
//! `espflash save-image` needs [`esp_bootloader_esp_idf::esp_app_desc`].

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};

// Charger /CE typestate (parked disabled).
use bq25616::Charger;

// Host-tested UART line format.
#[cfg(feature = "mic")]
use embassy_debug::TONE_DUMP_WINDOWS;
use embassy_debug::{
    classify_page_up_hold, classify_power_off_hold, classify_standby_exit_hold, format_event,
    format_git, format_latched, Event, ImuPose, PageUpHold, PowerOffHold, ResumeHold, Scene,
    StandbyExitHold, TouchPoint, BUZZER_CHIRP_MS, BUZZER_TONE_HZ, GIT_CAPACITY, IMU_REPORT_SECS,
    LATCHED_CAPACITY, LINE_CAPACITY, LOG_PREFIX, MAX_TOUCH_POINTS, PAGE_DOWN_POWER_OFF_MS,
    PAGE_UP_SLEEP_MS,
};

// Embassy runtime: tasks, channels, time.
use embassy_executor::Spawner;
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};

// esp-hal: GPIO, I2C, LEDC buzzer, timer group.
use embedded_hal::delay::DelayNs;
use esp_backtrace as _;
use esp_hal::delay::Delay;
use esp_hal::gpio::{DriveMode, Flex, Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::ledc::channel::{self, ChannelIFace};
use esp_hal::ledc::timer::{self, TimerIFace};
use esp_hal::ledc::{LSGlobalClkSource, Ledc, LowSpeed};
#[cfg(any(feature = "radio", feature = "pair", feature = "wifi"))]
use esp_hal::ram;
use esp_hal::rtc_cntl::sleep::LowPower;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::Blocking;
use esp_println::println;

// IMU on the sensor bus.
use lsm6ds3tr::interface::i2c::I2cInterface;
use lsm6ds3tr::{AccelSampleRate, AccelScale, AccelSettings, LsmSettings, LSM6DS3TR};

// Board latch, rails, GT911 reset dance.
use seeed_reterminal_sticky::display::PageRotation;
use seeed_reterminal_sticky::power::Latched;
#[cfg(not(feature = "mic"))]
use seeed_reterminal_sticky::rails::MicRail;
#[cfg(not(feature = "sd"))]
use seeed_reterminal_sticky::rails::SdRail;
use seeed_reterminal_sticky::rails::{Disabled, Enabled, Rail, TouchRail};
use seeed_reterminal_sticky::touch::{
    Register, SlaveAddress, StatusBits, StatusWrite, ADDR_SELECT_INT_FLOAT_MS,
    ADDR_SELECT_INT_HIGH_AT_RST, ADDR_SELECT_INT_HOLD_AFTER_RST_MS, ADDR_SELECT_RESET_HOLD_MS,
    ADDR_SELECT_RESET_RELEASE_MS, I2C_MAX_HZ as TOUCH_I2C_HZ, POINT_RECORD_LEN, POINT_X_OFFSET,
    POINT_Y_OFFSET, PRODUCT_ID_LEN, STATUS_HEARTBEAT, STATUS_POLL_MS,
};
use seeed_reterminal_sticky::{imu, Latch, I2C_FREQUENCY_HZ};

esp_bootloader_esp_idf::esp_app_desc!();

static EVENTS: Channel<CriticalSectionRawMutex, Event, 32> = Channel::new();
static BEEPS: Channel<CriticalSectionRawMutex, Beep, 4> = Channel::new();
static DROPPED: AtomicU32 = AtomicU32::new(0);

/// How many PDM windows the mic task should print as `pcm` rows.
#[cfg(feature = "mic")]
pub(crate) static TONE_CAPTURE: AtomicU32 = AtomicU32::new(0);

/// Short chirp (keys / glass). Mic PCM dump does not use the buzzer.
#[derive(Clone, Copy)]
enum Beep {
    Chirp,
}

/// The page the display task should paint next.
pub(crate) static SCENE: Signal<CriticalSectionRawMutex, Scene> = Signal::new();

/// Display task: run panel `standby()` on the current card and wait.
pub(crate) static STANDBY_REQUEST: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Display task: leave panel standby (`resume` / RST path).
pub(crate) static STANDBY_RESUME: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// In-plane splash page. Face-up / face-down / unknown do not signal.
pub(crate) static PAGE_ROTATION: Signal<CriticalSectionRawMutex, PageRotation> = Signal::new();

/// Milliseconds since Embassy time started (UART `t=`).
pub(crate) fn now_ms() -> u32 {
    Instant::now().as_millis() as u32
}

/// Push an event toward the log task. Overflow increments [`DROPPED`].
pub(crate) fn emit(event: Event) {
    if EVENTS.try_send(event).is_err() {
        DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}

/// Ask the buzzer for one short chirp (not a loudspeaker).
fn ask_beep() {
    let _ = BEEPS.try_send(Beep::Chirp);
}

/// Ask the mic task to print PCM rows (`--features mic`). No buzzer.
#[cfg(feature = "mic")]
fn ask_pcm_dump() {
    TONE_CAPTURE.store(TONE_DUMP_WINDOWS, Ordering::Relaxed);
}

/// Pins we must hold for the run so they stay in a safe idle state.
///
/// On the unit: charging stays off, the card is not mounted, the mic is
/// dark. GPIO7 is not a key — IMU INT1 and gauge GPOUT share it.
struct ParkedHazards {
    /// Held so `/CE` stays parked. `--features charge` reassigns after the pulse.
    charger: Charger<Output<'static>, bq25616::Disabled>,
    gpio7: Input<'static>,
    #[cfg(not(feature = "sd"))]
    sd_cs: Output<'static>,
    #[cfg(not(feature = "sd"))]
    sd_rail: SdRail<Output<'static>, Disabled>,
    #[cfg(not(feature = "mic"))]
    mic_rail: MicRail<Output<'static>, Disabled>,
}

/// Latch before any rail so the board stays up on battery.
///
/// PWR_HOLD (GPIO45) then PWR_LOCK (GPIO46). Never pulse PWR_LOCK.
fn acquire_latch(
    hold: esp_hal::peripherals::GPIO45<'static>,
    lock: esp_hal::peripherals::GPIO46<'static>,
    delay: &mut Delay,
) -> Latch<Output<'static>, Output<'static>> {
    Latch::acquire(
        Output::new(hold, Level::Low, OutputConfig::default()),
        Output::new(lock, Level::Low, OutputConfig::default()),
        delay,
    )
    .expect("driving the latch pins cannot fail")
}

/// Park /CE disabled, GPIO7 input-only, and unused CS/rails idle.
///
/// On the unit: we are not charging, not using the left-edge card, and
/// not recording. Do not drive GPIO7 (IMU INT1 and gauge GPOUT share it).
fn park_charger_and_unused(
    ce: esp_hal::peripherals::GPIO39<'static>,
    gpio7: esp_hal::peripherals::GPIO7<'static>,
    #[cfg(not(feature = "sd"))] sd_cs: esp_hal::peripherals::GPIO8<'static>,
    #[cfg(not(feature = "sd"))] sd_en: esp_hal::peripherals::GPIO10<'static>,
    #[cfg(not(feature = "mic"))] mic_en: esp_hal::peripherals::GPIO38<'static>,
    latch: &Latched,
) -> ParkedHazards {
    let charger = Charger::new(Output::new(ce, Level::High, OutputConfig::default()))
        .expect("driving /CE cannot fail");
    let gpio7 = Input::new(gpio7, InputConfig::default().with_pull(Pull::Up));
    // CS idle-high. `--features sd` takes these pins for identify.
    #[cfg(not(feature = "sd"))]
    let sd_cs = Output::new(sd_cs, Level::High, OutputConfig::default());
    #[cfg(not(feature = "sd"))]
    let sd_rail: SdRail<_, _> = Rail::new(
        Output::new(sd_en, Level::Low, OutputConfig::default()),
        latch,
    )
    .expect("driving the SD rail cannot fail");
    #[cfg(not(feature = "mic"))]
    let mic_rail: MicRail<_, _> = Rail::new(
        Output::new(mic_en, Level::Low, OutputConfig::default()),
        latch,
    )
    .expect("driving the mic rail cannot fail");
    ParkedHazards {
        charger,
        gpio7,
        #[cfg(not(feature = "sd"))]
        sd_cs,
        #[cfg(not(feature = "sd"))]
        sd_rail,
        #[cfg(not(feature = "mic"))]
        mic_rail,
    }
}

/// Hold parked `/CE`, GPIO7, and unused rails across deep sleep.
fn hold_parked(parked: ParkedHazards) {
    let mut ce = parked.charger.release();
    crate::sleep::hold_output(&mut ce);
    core::mem::forget(ce);

    let mut gpio7 = parked.gpio7;
    crate::sleep::hold_input(&mut gpio7);
    core::mem::forget(gpio7);

    #[cfg(not(feature = "sd"))]
    {
        let mut cs = parked.sd_cs;
        crate::sleep::hold_output(&mut cs);
        core::mem::forget(cs);
        let mut sd = parked.sd_rail.release();
        crate::sleep::hold_output(&mut sd);
        core::mem::forget(sd);
    }
    #[cfg(not(feature = "mic"))]
    {
        let mut mic = parked.mic_rail.release();
        crate::sleep::hold_output(&mut mic);
        core::mem::forget(mic);
    }
}

/// Rev.09 §6.1 address select: hold INT at the pair level through RST.
fn reset_with_int_level(
    rst: &mut Output<'static>,
    int: &mut Flex<'static>,
    int_high: bool,
    delay: &mut Delay,
) {
    int.apply_output_config(&OutputConfig::default());
    int.set_output_enable(true);
    rst.set_low();
    apply_int_level(int, int_high);
    delay.delay_ms(ADDR_SELECT_RESET_HOLD_MS);
    rst.set_high();
    delay.delay_ms(ADDR_SELECT_RESET_RELEASE_MS);
    apply_int_level(int, int_high);
    delay.delay_ms(ADDR_SELECT_INT_HOLD_AFTER_RST_MS);
    int.set_output_enable(false);
    int.apply_input_config(&InputConfig::default().with_pull(Pull::None));
    int.set_input_enable(true);
    delay.delay_ms(ADDR_SELECT_INT_FLOAT_MS);
}

fn apply_int_level(int: &mut Flex<'static>, high: bool) {
    if high {
        int.set_high();
    } else {
        int.set_low();
    }
}

fn probe_gt911_addr(i2c: &mut I2c<'static, Blocking>, addr: u8) -> bool {
    let mut id = [0u8; PRODUCT_ID_LEN];
    i2c.write_read(addr, &Register::Id.addr_bytes(), &mut id)
        .is_ok()
}

/// Power the touch rail, then Rev.09 §6.1 INT-during-reset address select.
///
/// [`ADDR_SELECT_INT_HIGH_AT_RST`] then [`SlaveAddress::probe_order`].
/// Bus at `TOUCH_I2C_HZ` (`I2C_MAX_HZ`, Rev.09 §6.1 cap). No init
/// [`StatusWrite::Clear`]. No [`Register::Command`]. No config-RAM write.
fn touch_i2c_after_int_reset(
    mut touch_i2c: I2c<'static, Blocking>,
    rst: esp_hal::peripherals::GPIO41<'static>,
    int: esp_hal::peripherals::GPIO21<'static>,
    rail: esp_hal::peripherals::GPIO42<'static>,
    latch: &Latched,
    delay: &mut Delay,
) -> (
    I2c<'static, Blocking>,
    Output<'static>,
    Flex<'static>,
    TouchRail<Output<'static>, Enabled>,
    Option<u8>,
) {
    let touch_rail: TouchRail<_, _> = Rail::new(
        Output::new(rail, Level::Low, OutputConfig::default()),
        latch,
    )
    .expect("driving the touch rail cannot fail");
    let touch_rail = touch_rail
        .enable(delay)
        .expect("driving the touch rail cannot fail");

    let mut touch_rst = Output::new(rst, Level::Low, OutputConfig::default());
    let mut touch_int = Flex::new(int);
    println!("{LOG_PREFIX}: gt911 addr dance");

    let mut gt911_addr = None;
    for int_high in ADDR_SELECT_INT_HIGH_AT_RST {
        reset_with_int_level(&mut touch_rst, &mut touch_int, int_high, delay);
        println!("{}: gt911 int={}", LOG_PREFIX, u8::from(int_high));
        for pair in SlaveAddress::probe_order() {
            let addr = pair.seven_bit();
            let ack = probe_gt911_addr(&mut touch_i2c, addr);
            println!(
                "{}: {:#04x} {}",
                LOG_PREFIX,
                addr,
                if ack { "ack" } else { "nak" }
            );
            if ack && gt911_addr.is_none() {
                gt911_addr = Some(addr);
            }
        }
        if gt911_addr.is_some() {
            break;
        }
    }

    // Rev.09 has no init Status/Command write at this port.
    println!("{LOG_PREFIX}: gt911 no init status clear");
    println!("{LOG_PREFIX}: gt911 no command write");

    (touch_i2c, touch_rst, touch_int, touch_rail, gt911_addr)
}

/// Raise the panel 3.3 V rail after the latch witness.
///
/// On the unit: the glass can refresh. OTP sequences live in the display
/// task; this only switches the load.
fn enable_epd_rail(
    pin: esp_hal::peripherals::GPIO47<'static>,
    latch: &Latched,
    delay: &mut Delay,
) -> seeed_reterminal_sticky::rails::EpdRail<Output<'static>, Enabled> {
    use seeed_reterminal_sticky::rails::EpdRail;
    let rail: EpdRail<_, _> =
        Rail::new(Output::new(pin, Level::Low, OutputConfig::default()), latch)
            .expect("driving the panel rail cannot fail");
    rail.enable(delay)
        .expect("driving the panel rail cannot fail")
}

/// Peripherals handed to Embassy tasks after buses are up.
struct SpawnParts {
    /// Right-edge top: AI Voice Button (GPIO4, UART `btn 4`).
    ai_voice: Input<'static>,
    /// Right-edge middle: Page Up Button (GPIO5, UART `btn 5`).
    page_up: Input<'static>,
    /// Right-edge bottom: Page Down Button (GPIO6, UART `btn 6`).
    page_down: Input<'static>,
    touch_i2c: I2c<'static, Blocking>,
    touch_rst: Output<'static>,
    touch_int: Flex<'static>,
    touch_rail: TouchRail<Output<'static>, Enabled>,
    gt911_addr: Option<u8>,
    sensor_i2c: I2c<'static, Blocking>,
    ledc: esp_hal::peripherals::LEDC<'static>,
    buzzer: esp_hal::peripherals::GPIO48<'static>,
    panel: crate::display::PanelParts,
    epd_rail: seeed_reterminal_sticky::rails::EpdRail<Output<'static>, Enabled>,
}

/// Start UART log, keys, glass, IMU, beep, and panel.
fn spawn_tasks(spawner: &Spawner, parts: SpawnParts, start: Scene, rotation: PageRotation) {
    spawner.spawn(log_task().expect("log task"));
    spawner.spawn(
        button_task(parts.ai_voice, parts.page_up, parts.page_down, start).expect("button task"),
    );
    spawner.spawn(
        touch_task(
            parts.touch_i2c,
            parts.touch_rst,
            parts.touch_int,
            parts.touch_rail,
            parts.gt911_addr,
        )
        .expect("touch task"),
    );
    spawner.spawn(imu_task(parts.sensor_i2c, rotation).expect("imu task"));
    spawner.spawn(buzzer_task(parts.ledc, parts.buzzer).expect("buzzer task"));
    spawner.spawn(
        crate::display::display_task(parts.panel, parts.epd_rail, start, rotation)
            .expect("display task"),
    );
}

/// Latch, park hazards, bring up buses, then hand the unit to Embassy.
///
/// On the unit: power stays on, UART says we latched, the glass shows
/// Ferris + `sticky-rs` (or the restored card after a deep-sleep wake),
/// and a right-edge key changes the drawing. Page Up 5 s paints
/// Ferris and sleeps. Page Down 5 s drops the latch. BLE
/// advertises only on the pair card.
#[esp_hal::main]
async fn main(spawner: Spawner) {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    #[cfg(not(any(feature = "radio", feature = "pair", feature = "wifi")))]
    esp_alloc::heap_allocator!(size: 8 * 1024);
    // Same class of heap as the esp-hal embassy_coex example.
    #[cfg(any(feature = "radio", feature = "pair", feature = "wifi"))]
    {
        esp_alloc::heap_allocator!(#[ram(reclaimed)] size: 64 * 1024);
        esp_alloc::heap_allocator!(size: 64 * 1024);
    }

    let mut delay = Delay::new();
    let latch = acquire_latch(peripherals.GPIO45, peripherals.GPIO46, &mut delay);
    let lpwr = LowPower::new(peripherals.LPWR);

    {
        let mut buf = [0u8; LATCHED_CAPACITY];
        if let Ok(line) = format_latched(&mut buf) {
            println!("{line}");
        }
        let mut buf = [0u8; GIT_CAPACITY];
        if let Ok(line) = format_git(
            env!("EMBASSY_DEBUG_GIT"),
            env!("EMBASSY_DEBUG_GIT_DIRTY") == "1",
            &mut buf,
        ) {
            println!("{line}");
        }
    }

    #[cfg_attr(not(feature = "charge"), allow(unused_mut))]
    let mut parked = park_charger_and_unused(
        peripherals.GPIO39,
        peripherals.GPIO7,
        #[cfg(not(feature = "sd"))]
        peripherals.GPIO8,
        #[cfg(not(feature = "sd"))]
        peripherals.GPIO10,
        #[cfg(not(feature = "mic"))]
        peripherals.GPIO38,
        latch.witness(),
    );

    #[cfg(feature = "mic")]
    let mic_rail = crate::mic::enable_rail(peripherals.GPIO38, latch.witness(), &mut delay);

    #[cfg_attr(not(feature = "charge"), allow(unused_mut))]
    let mut sensor_i2c = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(Rate::from_hz(I2C_FREQUENCY_HZ)),
    )
    .expect("I2C0 configuration")
    .with_sda(peripherals.GPIO1)
    .with_scl(peripherals.GPIO0);

    let resume = crate::sleep::resume_snap();
    let mut page_up = crate::sleep::page_up_input(peripherals.GPIO5);
    let page_down = crate::sleep::page_down_input(peripherals.GPIO6);
    let (start_scene, start_rotation) = if let Some(snap) = resume {
        {
            let mut buf = [0u8; LINE_CAPACITY];
            if let Ok(line) = format_event(&Event::Woke { t_ms: 0 }, &mut buf) {
                println!("{line}");
            }
        }
        match crate::sleep::wait_resume_hold(&page_up, &mut delay) {
            ResumeHold::Ready => (snap.scene, snap.rotation),
            ResumeHold::Abort | ResumeHold::Waiting => {
                crate::sleep::park_epd_en_low(peripherals.GPIO47);
                #[cfg(feature = "mic")]
                {
                    let disabled = mic_rail
                        .disable()
                        .expect("driving the mic rail cannot fail");
                    let mut pin = disabled.release();
                    crate::sleep::hold_output(&mut pin);
                    core::mem::forget(pin);
                }
                hold_parked(parked);
                crate::sleep::hold_latch(latch);
                crate::sleep::arm_page_up_and_sleep(&mut page_up, lpwr);
            }
        }
    } else {
        (Scene::Splash, PageRotation::Portrait0)
    };

    #[cfg(feature = "charge")]
    {
        parked.charger = crate::charge::run(
            parked.charger,
            peripherals.GPIO9,
            peripherals.GPIO40,
            &mut sensor_i2c,
            &mut delay,
        );
    }

    let touch_i2c = I2c::new(
        peripherals.I2C1,
        I2cConfig::default().with_frequency(Rate::from_hz(TOUCH_I2C_HZ)),
    )
    .expect("I2C1 configuration")
    .with_sda(peripherals.GPIO3)
    .with_scl(peripherals.GPIO2);

    let (touch_i2c, touch_rst, touch_int, touch_rail, gt911_addr) = touch_i2c_after_int_reset(
        touch_i2c,
        peripherals.GPIO41,
        peripherals.GPIO21,
        peripherals.GPIO42,
        latch.witness(),
        &mut delay,
    );

    // Right-edge keys (glass facing you, USB-C down): AI Voice, Page Up,
    // Page Down. Active-low with external pull-ups.
    let ai_voice = Input::new(
        peripherals.GPIO4,
        InputConfig::default().with_pull(Pull::Up),
    );
    let epd_rail = enable_epd_rail(peripherals.GPIO47, latch.witness(), &mut delay);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    spawn_tasks(
        &spawner,
        SpawnParts {
            ai_voice,
            page_up,
            page_down,
            touch_i2c,
            touch_rst,
            touch_int,
            touch_rail,
            gt911_addr,
            sensor_i2c,
            ledc: peripherals.LEDC,
            buzzer: peripherals.GPIO48,
            panel: crate::display::PanelParts {
                spi: peripherals.SPI2,
                sclk: peripherals.GPIO13,
                mosi: peripherals.GPIO14,
                #[cfg(feature = "sd")]
                miso: peripherals.GPIO12,
                cs: peripherals.GPIO15,
                dc: peripherals.GPIO16,
                rst: peripherals.GPIO17,
                busy: peripherals.GPIO18,
                #[cfg(feature = "sd")]
                sd: crate::sd::SdParts {
                    cs: peripherals.GPIO8,
                    cd: peripherals.GPIO11,
                    rail: crate::sd::park_rail(peripherals.GPIO10, latch.witness()),
                },
            },
            epd_rail,
        },
        start_scene,
        start_rotation,
    );

    #[cfg(feature = "mic")]
    spawner.spawn(
        crate::mic::mic_task(
            peripherals.I2S0,
            peripherals.DMA_CH0,
            peripherals.GPIO19,
            peripherals.GPIO20,
            mic_rail,
        )
        .expect("mic task"),
    );

    #[cfg(feature = "radio")]
    spawner.spawn(crate::radio::radio_task(peripherals.WIFI, peripherals.BT).expect("radio task"));

    // BLE only on this path. Advertise `sticky-rs`; PIN after PassKeyDisplay.
    // Wi-Fi owns `WIFI` separately (`init_wifi`); do not double-init the
    // same peripheral.
    #[cfg(feature = "pair")]
    spawner.spawn(crate::pair::pair_task(peripherals.BT).expect("pair task"));

    #[cfg(feature = "wifi")]
    crate::wifi::init_wifi(peripherals.WIFI, spawner);

    match select(
        crate::sleep::PANEL_PARKED.wait(),
        crate::sleep::POWER_OFF_READY.wait(),
    )
    .await
    {
        Either::First(()) => {
            crate::sleep::WAKE_ARMED.wait().await;
            hold_parked(parked);
            crate::sleep::hold_latch(latch);
            crate::sleep::enter_deep_sleep(lpwr);
        }
        Either::Second(()) => {
            hold_parked(parked);
            crate::sleep::release_latch(latch);
            loop {
                Timer::after(Duration::from_secs(3_600)).await;
            }
        }
    }
}

/// Print timestamped lines on UART0 (CH343, USB-C).
#[embassy_executor::task]
async fn log_task() {
    loop {
        let event = EVENTS.receive().await;
        let mut buf = [0u8; LINE_CAPACITY];
        if let Ok(line) = format_event(&event, &mut buf) {
            println!("{line}");
        }
        let dropped = DROPPED.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            let overflow = Event::Overflow {
                t_ms: now_ms(),
                dropped,
            };
            if let Ok(line) = format_event(&overflow, &mut buf) {
                println!("{line}");
            }
        }
    }
}

/// Right-edge keys: log `btn 4`/`5`/`6`, beep, and change the page.
///
/// A short Page Up goes to the previous drawing; a short Page Down (and
/// AI Voice on the default image) go to the next. Hold Page Up 2 s for
/// panel standby; keep holding to 5 s for MCU sleep. Page Up 1 s leaves
/// standby or sleep. Hold Page Down 5 s to drop the latch. With
/// `--features mic`, AI Voice dumps PCM and does not play the buzzer
/// or change the page.
#[embassy_executor::task]
async fn button_task(
    mut ai_voice: Input<'static>,
    mut page_up: Input<'static>,
    mut page_down: Input<'static>,
    mut scene: Scene,
) {
    loop {
        let gpio = match select3(
            ai_voice.wait_for_any_edge(),
            page_up.wait_for_any_edge(),
            page_down.wait_for_any_edge(),
        )
        .await
        {
            Either3::First(_) => 4,
            Either3::Second(_) => 5,
            Either3::Third(_) => 6,
        };
        Timer::after(Duration::from_millis(20)).await;
        let down = match gpio {
            4 => ai_voice.is_low(),
            5 => page_up.is_low(),
            _ => page_down.is_low(),
        };
        emit(Event::Button {
            t_ms: now_ms(),
            gpio,
            down,
        });
        if !down {
            continue;
        }
        match gpio {
            4 => {
                #[cfg(feature = "mic")]
                ask_pcm_dump();
                #[cfg(not(feature = "mic"))]
                {
                    ask_beep();
                    scene = scene.next();
                    SCENE.signal(scene);
                }
            }
            5 => {
                if crate::sleep::is_in_standby() {
                    hold_page_up_from_standby(&mut page_up).await;
                } else {
                    hold_page_up_awake(&mut page_up, &mut scene).await;
                }
            }
            6 => hold_page_down(&mut page_down, &mut scene).await,
            _ => {}
        }
    }
}

/// Awake Page Up: short = previous card; 2 s = standby; 5 s = sleep.
///
/// The same hold can enter standby at 2 s and continue to sleep at 5 s.
async fn hold_page_up_awake(page_up: &mut Input<'static>, scene: &mut Scene) {
    let mut held = 20u32;
    let mut standby_signaled = false;
    loop {
        match classify_page_up_hold(held, page_up.is_low()) {
            PageUpHold::Waiting => {
                Timer::after(Duration::from_millis(20)).await;
                held = held.saturating_add(20);
            }
            PageUpHold::Short => {
                ask_beep();
                emit(Event::Button {
                    t_ms: now_ms(),
                    gpio: 5,
                    down: false,
                });
                *scene = scene.prev();
                SCENE.signal(*scene);
                break;
            }
            PageUpHold::RequestStandby => {
                if !standby_signaled {
                    crate::sleep::enter_standby();
                    crate::STANDBY_REQUEST.signal(());
                    standby_signaled = true;
                }
                if !page_up.is_low() {
                    emit(Event::Button {
                        t_ms: now_ms(),
                        gpio: 5,
                        down: false,
                    });
                    break;
                }
                Timer::after(Duration::from_millis(20)).await;
                held = held.saturating_add(20);
                if held > PAGE_UP_SLEEP_MS {
                    held = PAGE_UP_SLEEP_MS;
                }
            }
            PageUpHold::RequestSleep => {
                crate::sleep::request_sleep();
                crate::sleep::wait_release_and_arm(page_up).await;
                emit(Event::Button {
                    t_ms: now_ms(),
                    gpio: 5,
                    down: false,
                });
                loop {
                    Timer::after(Duration::from_secs(3_600)).await;
                }
            }
        }
    }
}

/// Standby Page Up: release before 1 s stays; release after 1 s resumes;
/// hold 5 s sleeps.
async fn hold_page_up_from_standby(page_up: &mut Input<'static>) {
    let mut held = 20u32;
    loop {
        match classify_standby_exit_hold(held, page_up.is_low()) {
            StandbyExitHold::Waiting => {
                Timer::after(Duration::from_millis(20)).await;
                held = held.saturating_add(20);
                if held > PAGE_UP_SLEEP_MS {
                    held = PAGE_UP_SLEEP_MS;
                }
            }
            StandbyExitHold::Abort => {
                emit(Event::Button {
                    t_ms: now_ms(),
                    gpio: 5,
                    down: false,
                });
                break;
            }
            StandbyExitHold::Resume => {
                crate::sleep::leave_standby();
                crate::STANDBY_RESUME.signal(());
                emit(Event::Button {
                    t_ms: now_ms(),
                    gpio: 5,
                    down: false,
                });
                break;
            }
            StandbyExitHold::RequestSleep => {
                crate::sleep::request_sleep();
                crate::sleep::wait_release_and_arm(page_up).await;
                emit(Event::Button {
                    t_ms: now_ms(),
                    gpio: 5,
                    down: false,
                });
                loop {
                    Timer::after(Duration::from_secs(3_600)).await;
                }
            }
        }
    }
}

/// Awake Page Down: short = next card; 5 s = latch power-off.
async fn hold_page_down(page_down: &mut Input<'static>, scene: &mut Scene) {
    let mut held = 20u32;
    loop {
        match classify_power_off_hold(held, page_down.is_low()) {
            PowerOffHold::Waiting => {
                Timer::after(Duration::from_millis(20)).await;
                held = held.saturating_add(20);
                if held > PAGE_DOWN_POWER_OFF_MS {
                    held = PAGE_DOWN_POWER_OFF_MS;
                }
            }
            PowerOffHold::Short => {
                ask_beep();
                emit(Event::Button {
                    t_ms: now_ms(),
                    gpio: 6,
                    down: false,
                });
                *scene = scene.next();
                SCENE.signal(*scene);
                break;
            }
            PowerOffHold::RequestPowerOff => {
                crate::sleep::request_power_off();
                if page_down.is_low() {
                    page_down.wait_for_high().await;
                }
                emit(Event::Button {
                    t_ms: now_ms(),
                    gpio: 6,
                    down: false,
                });
                loop {
                    Timer::after(Duration::from_secs(3_600)).await;
                }
            }
        }
    }
}

/// Poll `Register::Status`, then `Register::Points` (coords at byte 0).
/// Clear Status only after a ready frame.
#[embassy_executor::task]
async fn touch_task(
    mut i2c: I2c<'static, Blocking>,
    mut rst: Output<'static>,
    mut int: Flex<'static>,
    rail: TouchRail<Output<'static>, Enabled>,
    addr: Option<u8>,
) {
    let Some(addr) = addr else {
        println!("{LOG_PREFIX}: gt911 absent");
        return;
    };
    let mut last_n = 0u8;
    let mut last_points = [TouchPoint::default(); MAX_TOUCH_POINTS];
    let mut last_status_uart: Option<Instant> = None;

    loop {
        if crate::sleep::is_requested() {
            rst.set_low();
            crate::sleep::hold_output(&mut rst);
            core::mem::forget(rst);
            int.set_output_enable(false);
            int.set_pad_hold(true);
            core::mem::forget(int);
            let disabled = rail.disable().expect("driving the touch rail cannot fail");
            let mut en = disabled.release();
            crate::sleep::hold_output(&mut en);
            core::mem::forget(en);
            loop {
                Timer::after(Duration::from_secs(3_600)).await;
            }
        }
        let status_due = STATUS_HEARTBEAT.interval_secs().is_some_and(|secs| {
            last_status_uart.is_none_or(|t| {
                Instant::now().saturating_duration_since(t) >= Duration::from_secs(u64::from(secs))
            })
        });
        if status_due {
            let mut st = [0u8];
            if i2c
                .write_read(addr, &Register::Status.addr_bytes(), &mut st)
                .is_ok()
            {
                last_status_uart = Some(Instant::now());
                emit(Event::Gt911Status {
                    t_ms: now_ms(),
                    status: st[0],
                });
            }
        }

        let mut st = [0u8];
        if i2c
            .write_read(addr, &Register::Status.addr_bytes(), &mut st)
            .is_ok()
        {
            let bits = StatusBits::from_byte(st[0]);
            if bits.buffer_ready() {
                let n = core::cmp::min(bits.touch_count() as usize, MAX_TOUCH_POINTS);
                let mut mapped = [TouchPoint::default(); MAX_TOUCH_POINTS];
                let mut fb0 = None;
                if n > 0 {
                    let mut raw = [0u8; MAX_TOUCH_POINTS * POINT_RECORD_LEN];
                    if i2c
                        .write_read(
                            addr,
                            &Register::Points.addr_bytes(),
                            &mut raw[..n * POINT_RECORD_LEN],
                        )
                        .is_ok()
                    {
                        for i in 0..n {
                            let rec = &raw[i * POINT_RECORD_LEN..];
                            let cx =
                                u16::from_le_bytes([rec[POINT_X_OFFSET], rec[POINT_X_OFFSET + 1]]);
                            let cy =
                                u16::from_le_bytes([rec[POINT_Y_OFFSET], rec[POINT_Y_OFFSET + 1]]);
                            // GT911 reports portrait 480×800; to_screen maps
                            // that onto physical 800×480 (UART `p0=`).
                            let (x, y) = seeed_reterminal_sticky::touch::to_screen(
                                u32::from(cx),
                                u32::from(cy),
                            );
                            mapped[i] = TouchPoint {
                                x: x as u16,
                                y: y as u16,
                            };
                            if i == 0 {
                                // Pre-rotation canvas for START/STOP. Works
                                // for landscape holds; do not hit-test `p0=`.
                                let (fx, fy) = seeed_reterminal_sticky::touch::to_framebuffer(
                                    u32::from(cx),
                                    u32::from(cy),
                                );
                                fb0 = Some((fx as u16, fy as u16));
                            }
                        }
                    }
                }
                let n = n as u8;
                if n != last_n || mapped != last_points {
                    let became_contact = n > 0 && last_n == 0;
                    last_n = n;
                    last_points = mapped;
                    emit(Event::Touch {
                        t_ms: now_ms(),
                        n,
                        points: mapped,
                    });
                    if became_contact {
                        ask_beep();
                        #[cfg(feature = "wifi")]
                        if let Some(scene) = crate::wifi::ui_scene() {
                            let rotation = crate::wifi::ui_rotation();
                            if let Some((fx, fy)) = fb0 {
                                if crate::draw::wifi_action_hit(fx, fy, rotation) {
                                    match scene {
                                        Scene::WifiSurvey => {
                                            let cmd = match crate::wifi::wifi_mode() {
                                                crate::wifi::WifiMode::SurveyScanning => {
                                                    crate::wifi::WifiCommand::StopSurvey
                                                }
                                                _ => crate::wifi::WifiCommand::StartSurvey,
                                            };
                                            crate::wifi::send_wifi_cmd(cmd);
                                        }
                                        Scene::WifiAp => {
                                            let cmd = match crate::wifi::wifi_mode() {
                                                crate::wifi::WifiMode::Hotspot => {
                                                    crate::wifi::WifiCommand::StopHotspot
                                                }
                                                _ => crate::wifi::WifiCommand::StartHotspot,
                                            };
                                            crate::wifi::send_wifi_cmd(cmd);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
                let _ = i2c.write(addr, &Register::Status.write_u8(StatusWrite::Clear.byte()));
            }
        }
        Timer::after(Duration::from_millis(STATUS_POLL_MS)).await;
    }
}

/// Tilt the card: the current page follows in-plane pose; UART about every
/// [`IMU_REPORT_SECS`].
#[embassy_executor::task]
async fn imu_task(i2c: I2c<'static, Blocking>, start_rotation: PageRotation) {
    const POLL_MS: u64 = 250;

    let settings = LsmSettings::default().with_accel(
        AccelSettings::new()
            .with_sample_rate(AccelSampleRate::_26Hz)
            .with_scale(AccelScale::_2G),
    );
    let mut imu_dev = LSM6DS3TR::new(I2cInterface::new(i2c)).with_settings(settings);
    match imu_dev.init_accel() {
        Ok(()) => println!("{LOG_PREFIX}: imu accel init ok"),
        Err(_) => println!("{LOG_PREFIX}: imu accel init failed"),
    }

    let mut last_uart: Option<Instant> = None;
    let mut last_rotation = Some(start_rotation);
    loop {
        if crate::sleep::is_requested() {
            loop {
                Timer::after(Duration::from_secs(3_600)).await;
            }
        }
        if let Ok(xyz) = imu_dev.read_accel_raw() {
            let classified = imu::classify(xyz.x, xyz.y, xyz.z);
            if let Some(rotation) = classified.and_then(imu::Orientation::page_rotation) {
                if last_rotation != Some(rotation) {
                    last_rotation = Some(rotation);
                    crate::PAGE_ROTATION.signal(rotation);
                }
            }
            let uart_due = last_uart
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(u64::from(IMU_REPORT_SECS)));
            if uart_due {
                emit(Event::Imu {
                    t_ms: now_ms(),
                    pose: classified.map(pose),
                    x: xyz.x,
                    y: xyz.y,
                    z: xyz.z,
                });
                last_uart = Some(Instant::now());
            }
        }
        Timer::after(Duration::from_millis(POLL_MS)).await;
    }
}

/// Board classifier pose → UART token (`imu=FaceUp`, …).
fn pose(orientation: imu::Orientation) -> ImuPose {
    match orientation {
        imu::Orientation::Portrait0 => ImuPose::Portrait0,
        imu::Orientation::Portrait180 => ImuPose::Portrait180,
        imu::Orientation::Landscape0 => ImuPose::Landscape0,
        imu::Orientation::Landscape180 => ImuPose::Landscape180,
        imu::Orientation::FaceUp => ImuPose::FaceUp,
        imu::Orientation::FaceDown => ImuPose::FaceDown,
    }
}

/// One 80 ms chirp on the passive buzzer (GPIO48). Not a speaker.
#[embassy_executor::task]
async fn buzzer_task(
    ledc: esp_hal::peripherals::LEDC<'static>,
    pin: esp_hal::peripherals::GPIO48<'static>,
) {
    let mut ledc = Ledc::new(ledc);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let mut lstimer0 = ledc.timer::<LowSpeed>(timer::Number::Timer0);
    let mut pin = Output::new(pin, Level::Low, OutputConfig::default());
    if lstimer0
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty8Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_hz(BUZZER_TONE_HZ),
        })
        .is_err()
    {
        println!("{LOG_PREFIX}: buzzer timer failed");
        crate::sleep::hold_output(&mut pin);
        core::mem::forget(pin);
        loop {
            BEEPS.receive().await;
        }
    }

    let mut channel0 = ledc.channel::<LowSpeed>(channel::Number::Channel0, pin);
    if channel0
        .configure(channel::config::Config {
            timer: &lstimer0,
            duty_pct: 0,
            drive_mode: DriveMode::PushPull,
        })
        .is_err()
    {
        println!("{LOG_PREFIX}: buzzer channel failed");
        loop {
            BEEPS.receive().await;
        }
    }

    loop {
        let kind = match select(BEEPS.receive(), Timer::after(Duration::from_millis(50))).await {
            Either::First(kind) => kind,
            Either::Second(()) => {
                if crate::sleep::is_requested() {
                    let _ = channel0.set_duty(0);
                    loop {
                        Timer::after(Duration::from_secs(3_600)).await;
                    }
                }
                continue;
            }
        };
        if crate::sleep::is_requested() {
            let _ = channel0.set_duty(0);
            loop {
                Timer::after(Duration::from_secs(3_600)).await;
            }
        }
        let _ = channel0.set_duty(50);
        match kind {
            Beep::Chirp => {
                Timer::after(Duration::from_millis(u64::from(BUZZER_CHIRP_MS))).await;
            }
        }
        let _ = channel0.set_duty(0);
    }
}

#[cfg(feature = "charge")]
mod charge;
mod display;
mod draw;
#[cfg(feature = "mic")]
mod mic;
#[cfg(feature = "pair")]
mod pair;
#[cfg(feature = "radio")]
mod radio;
#[cfg(feature = "sd")]
mod sd;
mod sleep;
#[cfg(feature = "wifi")]
mod wifi;
