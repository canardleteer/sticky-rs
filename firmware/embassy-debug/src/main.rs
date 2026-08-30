//! reTerminal Sticky Embassy event-logger image.
//!
//! Latch, then UART0 lines for buttons, GT911, and IMU. The panel is behind
//! `--features epd`. Flash only via `cargo xtask flash-app` after a matching
//! snapshot exists (original or capture).
//!
//! # Before flashing anything
//!
//! 1. Take a full-chip original (`cargo xtask backup-factory-firmware`) and
//!    keep it out of git.
//! 2. Flash `app0` only with `cargo xtask flash-app`. Never erase, never write
//!    below `0x90000`.
//! 3. `espflash save-image` needs [`esp_bootloader_esp_idf::esp_app_desc`].

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};

use bq25616::Charger;
use embassy_debug::{
    format_event, format_git, format_latched, Event, ImuPose, TouchPoint, GIT_CAPACITY,
    IMU_REPORT_SECS, LATCHED_CAPACITY, LINE_CAPACITY, LOG_PREFIX, MAX_TOUCH_POINTS,
};
use embassy_executor::Spawner;
use embassy_futures::select::{select3, Either3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
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
use lsm6ds3tr::interface::i2c::I2cInterface;
use lsm6ds3tr::{AccelSampleRate, AccelScale, AccelSettings, LsmSettings, LSM6DS3TR};
use seeed_reterminal_sticky::rails::{Enabled, MicRail, Rail, SdRail, TouchRail};
use seeed_reterminal_sticky::touch::{
    Register, SlaveAddress, COMMAND_READ_COORDINATES, I2C_HZ as TOUCH_I2C_HZ, INT_SETTLE_MS,
    POST_RESET_SETTLE_MS, RESET_HOLD_MS, RESET_RELEASE_MS, STATUS_CLEAR,
};
use seeed_reterminal_sticky::{imu, Latch, I2C_FREQUENCY_HZ};

#[cfg(feature = "epd")]
use embassy_debug::Scene;
#[cfg(feature = "epd")]
use embassy_sync::signal::Signal;

esp_bootloader_esp_idf::esp_app_desc!();

static EVENTS: Channel<CriticalSectionRawMutex, Event, 32> = Channel::new();
static BEEPS: Channel<CriticalSectionRawMutex, (), 4> = Channel::new();
static DROPPED: AtomicU32 = AtomicU32::new(0);

#[cfg(feature = "epd")]
pub(crate) static SCENE: Signal<CriticalSectionRawMutex, Scene> = Signal::new();

/// Milliseconds since Embassy time started.
pub(crate) fn now_ms() -> u32 {
    Instant::now().as_millis() as u32
}

/// Push an event toward the log task. Overflow increments [`DROPPED`].
pub(crate) fn emit(event: Event) {
    if EVENTS.try_send(event).is_err() {
        DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}

fn ask_beep() {
    let _ = BEEPS.try_send(());
}

#[esp_hal::main]
async fn main(spawner: Spawner) {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_alloc::heap_allocator!(size: 8 * 1024);

    let mut delay = Delay::new();
    let latch = Latch::acquire(
        Output::new(peripherals.GPIO45, Level::Low, OutputConfig::default()),
        Output::new(peripherals.GPIO46, Level::Low, OutputConfig::default()),
        &mut delay,
    )
    .expect("driving the latch pins cannot fail");

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

    let _charger = Charger::new(Output::new(
        peripherals.GPIO39,
        Level::High,
        OutputConfig::default(),
    ))
    .expect("driving /CE cannot fail");

    let _gpio7 = Input::new(
        peripherals.GPIO7,
        InputConfig::default().with_pull(Pull::Up),
    );

    #[cfg(not(feature = "epd"))]
    let _epd_cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());

    let _sd_cs = Output::new(peripherals.GPIO8, Level::High, OutputConfig::default());
    let _sd_rail: SdRail<_, _> = Rail::new(
        Output::new(peripherals.GPIO10, Level::Low, OutputConfig::default()),
        latch.witness(),
    )
    .expect("driving the SD rail cannot fail");
    let _mic_rail: MicRail<_, _> = Rail::new(
        Output::new(peripherals.GPIO38, Level::Low, OutputConfig::default()),
        latch.witness(),
    )
    .expect("driving the mic rail cannot fail");

    let sensor_i2c = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(Rate::from_hz(I2C_FREQUENCY_HZ)),
    )
    .expect("I2C0 configuration")
    .with_sda(peripherals.GPIO1)
    .with_scl(peripherals.GPIO0);

    let mut touch_i2c = I2c::new(
        peripherals.I2C1,
        I2cConfig::default().with_frequency(Rate::from_hz(TOUCH_I2C_HZ)),
    )
    .expect("I2C1 configuration")
    .with_sda(peripherals.GPIO3)
    .with_scl(peripherals.GPIO2);

    let touch_rail: TouchRail<_, _> = Rail::new(
        Output::new(peripherals.GPIO42, Level::Low, OutputConfig::default()),
        latch.witness(),
    )
    .expect("driving the touch rail cannot fail");
    let touch_rail = touch_rail
        .enable(&mut delay)
        .expect("driving the touch rail cannot fail");

    let mut touch_rst = Output::new(peripherals.GPIO41, Level::Low, OutputConfig::default());
    let mut touch_int = Flex::new(peripherals.GPIO21);
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

    let btn4 = Input::new(
        peripherals.GPIO4,
        InputConfig::default().with_pull(Pull::Up),
    );
    let btn5 = Input::new(
        peripherals.GPIO5,
        InputConfig::default().with_pull(Pull::Up),
    );
    let btn6 = Input::new(
        peripherals.GPIO6,
        InputConfig::default().with_pull(Pull::Up),
    );

    #[cfg(feature = "epd")]
    let epd_rail = {
        use seeed_reterminal_sticky::rails::EpdRail;
        let rail: EpdRail<_, _> = Rail::new(
            Output::new(peripherals.GPIO47, Level::Low, OutputConfig::default()),
            latch.witness(),
        )
        .expect("driving the panel rail cannot fail");
        rail.enable(&mut delay)
            .expect("driving the panel rail cannot fail")
    };

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    spawner.spawn(log_task().expect("log task"));
    spawner.spawn(button_task(btn4, btn5, btn6).expect("button task"));
    spawner.spawn(touch_task(touch_i2c, touch_rst, touch_int, touch_rail).expect("touch task"));
    spawner.spawn(imu_task(sensor_i2c).expect("imu task"));
    spawner.spawn(buzzer_task(peripherals.LEDC, peripherals.GPIO48).expect("buzzer task"));

    #[cfg(feature = "epd")]
    {
        spawner.spawn(
            crate::display::display_task(
                crate::display::PanelParts {
                    spi: peripherals.SPI2,
                    sclk: peripherals.GPIO13,
                    mosi: peripherals.GPIO14,
                    cs: peripherals.GPIO15,
                    dc: peripherals.GPIO16,
                    rst: peripherals.GPIO17,
                    busy: peripherals.GPIO18,
                },
                epd_rail,
            )
            .expect("display task"),
        );
        SCENE.signal(Scene::Splash);
    }

    #[cfg(not(feature = "epd"))]
    {
        let _ = (peripherals.SPI2, peripherals.GPIO13, peripherals.GPIO14);
        let _ = (
            peripherals.GPIO16,
            peripherals.GPIO17,
            peripherals.GPIO18,
            peripherals.GPIO47,
        );
    }

    let _keep_latched = latch;
    loop {
        Timer::after(Duration::from_secs(60)).await;
    }
}

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

#[embassy_executor::task]
async fn button_task(mut btn4: Input<'static>, mut btn5: Input<'static>, mut btn6: Input<'static>) {
    #[cfg(feature = "epd")]
    let mut scene = Scene::Splash;

    loop {
        let gpio = match select3(
            btn4.wait_for_any_edge(),
            btn5.wait_for_any_edge(),
            btn6.wait_for_any_edge(),
        )
        .await
        {
            Either3::First(_) => 4,
            Either3::Second(_) => 5,
            Either3::Third(_) => 6,
        };
        Timer::after(Duration::from_millis(20)).await;
        let down = match gpio {
            4 => btn4.is_low(),
            5 => btn5.is_low(),
            _ => btn6.is_low(),
        };
        emit(Event::Button {
            t_ms: now_ms(),
            gpio,
            down,
        });
        if down {
            ask_beep();
            #[cfg(feature = "epd")]
            {
                match gpio {
                    5 => {
                        scene = scene.prev();
                        SCENE.signal(scene);
                    }
                    6 => {
                        scene = scene.next();
                        SCENE.signal(scene);
                    }
                    _ => {}
                }
            }
        }
    }
}

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

#[embassy_executor::task]
async fn imu_task(i2c: I2c<'static, Blocking>) {
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

    loop {
        if let Ok(xyz) = imu_dev.read_accel_raw() {
            emit(Event::Imu {
                t_ms: now_ms(),
                pose: imu::classify(xyz.x, xyz.y, xyz.z).map(pose),
                x: xyz.x,
                y: xyz.y,
                z: xyz.z,
            });
        }
        Timer::after(Duration::from_secs(u64::from(IMU_REPORT_SECS))).await;
    }
}

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
        BEEPS.receive().await;
        let _ = channel0.set_duty(50);
        Timer::after(Duration::from_millis(80)).await;
        let _ = channel0.set_duty(0);
    }
}

#[cfg(feature = "epd")]
mod display;
