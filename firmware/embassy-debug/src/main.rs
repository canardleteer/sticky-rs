//! reTerminal Sticky Embassy event-logger image.
//!
//! On the unit: splash stays upright in the four in-plane holds; right-edge
//! keys change the drawing; taps and tilts print on UART0; a short beep
//! answers a key or the first finger on the glass.
//!
//! In the MCU: latch power, park the charger and unused rails, bring up
//! the two I2C buses and the panel OTP path. No invented LUT.
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
use embassy_debug::{
    format_event, format_git, format_latched, Event, ImuPose, Scene, TouchPoint, BUZZER_TONE_MS,
    GIT_CAPACITY, IMU_REPORT_SECS, LATCHED_CAPACITY, LINE_CAPACITY, LOG_PREFIX, MAX_TOUCH_POINTS,
    TONE_DUMP_WINDOWS,
};

// Embassy runtime: tasks, channels, time.
use embassy_executor::Spawner;
use embassy_futures::select::{select3, Either3};
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
use seeed_reterminal_sticky::rails::{Disabled, Enabled, Rail, SdRail, TouchRail};
use seeed_reterminal_sticky::touch::{
    Register, SlaveAddress, COMMAND_READ_COORDINATES, I2C_HZ as TOUCH_I2C_HZ, INT_SETTLE_MS,
    POST_RESET_SETTLE_MS, RESET_HOLD_MS, RESET_RELEASE_MS, STATUS_CLEAR,
};
use seeed_reterminal_sticky::{imu, Latch, I2C_FREQUENCY_HZ};

esp_bootloader_esp_idf::esp_app_desc!();

static EVENTS: Channel<CriticalSectionRawMutex, Event, 32> = Channel::new();
static BEEPS: Channel<CriticalSectionRawMutex, Beep, 4> = Channel::new();
static DROPPED: AtomicU32 = AtomicU32::new(0);

/// How many PDM windows the mic task should print as `pcm` rows.
pub(crate) static TONE_CAPTURE: AtomicU32 = AtomicU32::new(0);

/// Short chirp (keys / glass) or the 1 kHz AI Voice capture tone.
#[derive(Clone, Copy)]
enum Beep {
    Chirp,
    Tone,
}

/// The page the display task should paint next.
pub(crate) static SCENE: Signal<CriticalSectionRawMutex, Scene> = Signal::new();

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

/// Ask the buzzer for the 1 kHz capture tone (`--features mic`).
#[cfg(feature = "mic")]
fn ask_tone() {
    let _ = BEEPS.try_send(Beep::Tone);
}

/// Pins we must hold for the run so they stay in a safe idle state.
///
/// On the unit: charging stays off, the card is not mounted, the mic is
/// dark. GPIO7 is not a key — it is an ambiguous interrupt net.
struct ParkedHazards {
    _charger: Charger<Output<'static>, bq25616::Disabled>,
    _gpio7: Input<'static>,
    _sd_cs: Output<'static>,
    _sd_rail: SdRail<Output<'static>, Disabled>,
    #[cfg(not(feature = "mic"))]
    _mic_rail: MicRail<Output<'static>, Disabled>,
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
/// not recording. Do not drive GPIO7 (IMU INT vs gauge GPOUT is open).
fn park_charger_and_unused(
    ce: esp_hal::peripherals::GPIO39<'static>,
    gpio7: esp_hal::peripherals::GPIO7<'static>,
    sd_cs: esp_hal::peripherals::GPIO8<'static>,
    sd_en: esp_hal::peripherals::GPIO10<'static>,
    #[cfg(not(feature = "mic"))] mic_en: esp_hal::peripherals::GPIO38<'static>,
    latch: &Latched,
) -> ParkedHazards {
    let charger = Charger::new(Output::new(ce, Level::High, OutputConfig::default()))
        .expect("driving /CE cannot fail");
    let gpio7 = Input::new(gpio7, InputConfig::default().with_pull(Pull::Up));
    // CS idle-high. Do not mount the card.
    let sd_cs = Output::new(sd_cs, Level::High, OutputConfig::default());
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
        _charger: charger,
        _gpio7: gpio7,
        _sd_cs: sd_cs,
        _sd_rail: sd_rail,
        #[cfg(not(feature = "mic"))]
        _mic_rail: mic_rail,
    }
}

/// Power the touch rail, INT-during-reset, then read the product ID.
///
/// On the unit: the glass becomes a digitizer. In the MCU: 100 kHz,
/// address `0x14`, no config-RAM write. Leave INT floating (GPIO21 has
/// no reset pull).
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
    touch_int.apply_output_config(&OutputConfig::default());
    touch_int.set_output_enable(true);
    touch_int.set_high();
    touch_rst.set_low();
    delay.delay_ms(RESET_HOLD_MS);
    touch_rst.set_high();
    delay.delay_ms(RESET_RELEASE_MS);
    touch_int.set_output_enable(false);
    touch_int.apply_input_config(&InputConfig::default().with_pull(Pull::None));
    touch_int.set_input_enable(true);
    delay.delay_ms(INT_SETTLE_MS);
    delay.delay_ms(POST_RESET_SETTLE_MS);

    let gt911_addr = SlaveAddress::Pair28_29.seven_bit();
    let mut id = [0u8; 4];
    let gt911_ack = touch_i2c
        .write_read(gt911_addr, &Register::Id.addr_bytes(), &mut id)
        .is_ok();
    println!(
        "{}: {:#04x} {}",
        LOG_PREFIX,
        gt911_addr,
        if gt911_ack { "ack" } else { "nak" }
    );

    match touch_i2c.write(gt911_addr, &Register::Status.write_u8(STATUS_CLEAR)) {
        Ok(()) => println!("{LOG_PREFIX}: gt911 status cleared"),
        Err(_) => println!("{LOG_PREFIX}: gt911 status clear failed"),
    }
    match touch_i2c.write(
        gt911_addr,
        &Register::Command.write_u8(COMMAND_READ_COORDINATES),
    ) {
        Ok(()) => println!("{LOG_PREFIX}: gt911 command read-coordinates"),
        Err(_) => println!("{LOG_PREFIX}: gt911 command failed"),
    }

    (touch_i2c, touch_rst, touch_int, touch_rail)
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
    sensor_i2c: I2c<'static, Blocking>,
    ledc: esp_hal::peripherals::LEDC<'static>,
    buzzer: esp_hal::peripherals::GPIO48<'static>,
    panel: crate::display::PanelParts,
    epd_rail: seeed_reterminal_sticky::rails::EpdRail<Output<'static>, Enabled>,
}

/// Start UART log, keys, glass, IMU, beep, and panel. First page is splash.
fn spawn_tasks(spawner: &Spawner, parts: SpawnParts) {
    spawner.spawn(log_task().expect("log task"));
    spawner
        .spawn(button_task(parts.ai_voice, parts.page_up, parts.page_down).expect("button task"));
    spawner.spawn(
        touch_task(
            parts.touch_i2c,
            parts.touch_rst,
            parts.touch_int,
            parts.touch_rail,
        )
        .expect("touch task"),
    );
    spawner.spawn(imu_task(parts.sensor_i2c).expect("imu task"));
    spawner.spawn(buzzer_task(parts.ledc, parts.buzzer).expect("buzzer task"));
    spawner.spawn(crate::display::display_task(parts.panel, parts.epd_rail).expect("display task"));
    SCENE.signal(Scene::Splash);
}

/// Latch, park hazards, bring up buses, then hand the unit to Embassy.
///
/// On the unit: power stays on, UART says we latched, the glass shows
/// Ferris + `sticky-rs`, and a right-edge key changes the drawing.
#[esp_hal::main]
async fn main(spawner: Spawner) {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_alloc::heap_allocator!(size: 8 * 1024);

    let mut delay = Delay::new();
    let latch = acquire_latch(peripherals.GPIO45, peripherals.GPIO46, &mut delay);

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

    let _parked = park_charger_and_unused(
        peripherals.GPIO39,
        peripherals.GPIO7,
        peripherals.GPIO8,
        peripherals.GPIO10,
        #[cfg(not(feature = "mic"))]
        peripherals.GPIO38,
        latch.witness(),
    );

    #[cfg(feature = "mic")]
    let mic_rail = crate::mic::enable_rail(peripherals.GPIO38, latch.witness(), &mut delay);

    let sensor_i2c = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(Rate::from_hz(I2C_FREQUENCY_HZ)),
    )
    .expect("I2C0 configuration")
    .with_sda(peripherals.GPIO1)
    .with_scl(peripherals.GPIO0);

    let touch_i2c = I2c::new(
        peripherals.I2C1,
        I2cConfig::default().with_frequency(Rate::from_hz(TOUCH_I2C_HZ)),
    )
    .expect("I2C1 configuration")
    .with_sda(peripherals.GPIO3)
    .with_scl(peripherals.GPIO2);

    let (touch_i2c, touch_rst, touch_int, touch_rail) = touch_i2c_after_int_reset(
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
    let page_up = Input::new(
        peripherals.GPIO5,
        InputConfig::default().with_pull(Pull::Up),
    );
    let page_down = Input::new(
        peripherals.GPIO6,
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
            sensor_i2c,
            ledc: peripherals.LEDC,
            buzzer: peripherals.GPIO48,
            panel: crate::display::PanelParts {
                spi: peripherals.SPI2,
                sclk: peripherals.GPIO13,
                mosi: peripherals.GPIO14,
                cs: peripherals.GPIO15,
                dc: peripherals.GPIO16,
                rst: peripherals.GPIO17,
                busy: peripherals.GPIO18,
            },
            epd_rail,
        },
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

    let _keep_latched = latch;
    loop {
        Timer::after(Duration::from_secs(60)).await;
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
/// Page Up goes to the previous drawing; Page Down (and AI Voice on the
/// default image) go to the next. With `--features mic`, AI Voice plays
/// the 1 kHz capture tone and does not change the page.
#[embassy_executor::task]
async fn button_task(
    mut ai_voice: Input<'static>,
    mut page_up: Input<'static>,
    mut page_down: Input<'static>,
) {
    let mut scene = Scene::Splash;

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
        if down {
            match gpio {
                4 => {
                    #[cfg(feature = "mic")]
                    ask_tone();
                    #[cfg(not(feature = "mic"))]
                    {
                        ask_beep();
                        scene = scene.next();
                        SCENE.signal(scene);
                    }
                }
                5 => {
                    ask_beep();
                    scene = scene.prev();
                    SCENE.signal(scene);
                }
                6 => {
                    ask_beep();
                    scene = scene.next();
                    SCENE.signal(scene);
                }
                _ => {}
            }
        }
    }
}

/// Poll the glass. A new contact set prints `touch` and beeps on first down.
#[embassy_executor::task]
async fn touch_task(
    mut i2c: I2c<'static, Blocking>,
    _rst: Output<'static>,
    _int: Flex<'static>,
    _rail: TouchRail<Output<'static>, Enabled>,
) {
    let addr = SlaveAddress::Pair28_29.seven_bit();
    let touch = gt911::Gt911Blocking::new(addr);
    let mut last_n = 0u8;
    let mut last_points = [TouchPoint::default(); MAX_TOUCH_POINTS];

    loop {
        match touch.get_multi_touch(&mut i2c) {
            Ok(points) => {
                let n = core::cmp::min(points.len(), MAX_TOUCH_POINTS) as u8;
                let mut mapped = [TouchPoint::default(); MAX_TOUCH_POINTS];
                for (i, point) in points.iter().take(MAX_TOUCH_POINTS).enumerate() {
                    let (x, y) = seeed_reterminal_sticky::touch::to_screen(
                        u32::from(point.x),
                        u32::from(point.y),
                    );
                    mapped[i] = TouchPoint {
                        x: x as u16,
                        y: y as u16,
                    };
                }
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
                    }
                }
            }
            Err(gt911::Error::NotReady) => {}
            Err(_) => {}
        }
        Timer::after(Duration::from_millis(30)).await;
    }
}

/// Tilt the card: splash follows in-plane pose; UART about every
/// [`IMU_REPORT_SECS`].
#[embassy_executor::task]
async fn imu_task(i2c: I2c<'static, Blocking>) {
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
    let mut last_rotation = Some(PageRotation::Portrait0);
    loop {
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
    if lstimer0
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty8Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(1),
        })
        .is_err()
    {
        println!("{LOG_PREFIX}: buzzer timer failed");
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
        let kind = BEEPS.receive().await;
        let _ = channel0.set_duty(50);
        match kind {
            Beep::Chirp => {
                Timer::after(Duration::from_millis(80)).await;
            }
            Beep::Tone => {
                TONE_CAPTURE.store(TONE_DUMP_WINDOWS, Ordering::Relaxed);
                Timer::after(Duration::from_millis(u64::from(BUZZER_TONE_MS))).await;
            }
        }
        let _ = channel0.set_duty(0);
    }
}

mod display;
#[cfg(feature = "mic")]
mod mic;
