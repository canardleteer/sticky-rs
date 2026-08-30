//! reTerminal Sticky simple-debug image.
//!
//! Blocking `esp-hal` `#[main]`: latch, park hazardous pins, print boot-time
//! bus facts, then a UART heartbeat of raw GPIO/gauge/IMU levels. No Embassy,
//! no RTOS, no panel LUT.
//!
//! # Before flashing anything
//!
//! Agent flash contract and envelope: the sibling `AGENTS.md`.
//! `espflash save-image` needs [`esp_bootloader_esp_idf::esp_app_desc`].

#![no_std]
#![no_main]

use core::cell::RefCell;

use bq25616::{ChargeStatus, Charger, ExternalPower, Level as ChargeLevel};
use bq27220::Bq27220;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::InputPin;
use embedded_hal_bus::i2c::RefCellDevice;
use esp_backtrace as _;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Flex, Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::main;
use esp_hal::time::Rate;
use esp_hal::Blocking;
use esp_println::println;
use lsm6ds3tr::interface::i2c::I2cInterface;
use lsm6ds3tr::{AccelSampleRate, AccelScale, AccelSettings, LsmSettings, LSM6DS3TR};
use seeed_reterminal_sticky::rails::{EpdRail, MicRail, Rail, SdRail, TouchRail};
use seeed_reterminal_sticky::touch::{
    Register, SlaveAddress, I2C_HZ as TOUCH_I2C_HZ, INT_SETTLE_MS, POST_RESET_SETTLE_MS,
    RESET_HOLD_MS, RESET_RELEASE_MS,
};
#[cfg(feature = "operator")]
use seeed_reterminal_sticky::touch::{COMMAND_READ_COORDINATES, STATUS_CLEAR};
use seeed_reterminal_sticky::{addresses, display, imu, Latch, I2C_FREQUENCY_HZ};
use sht4x::{Precision, Sht4x};
use simple_debug::{
    collect_edges, format_edge, format_git, format_gt911_id, format_heartbeat, Edge, GpioLevels,
    ImuPose, Snapshot, EDGE_CAPACITY, GIT_CAPACITY, GT911_ID_CAPACITY, HEARTBEAT_CAPACITY,
    LOG_PREFIX,
};
#[cfg(feature = "operator")]
use simple_debug::{
    format_contacts, format_gt911_int, format_gt911_status, format_prompt, CONTACTS_CAPACITY,
    GT911_INT_CAPACITY, GT911_STATUS_CAPACITY, PROMPT_CAPACITY,
};

esp_bootloader_esp_idf::esp_app_desc!();

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_alloc::heap_allocator!(size: 8 * 1024);
    let mut delay = Delay::new();

    // 1. Power latch, before anything else. Rails below need the witness.
    let latch = Latch::acquire(
        Output::new(peripherals.GPIO45, Level::Low, OutputConfig::default()),
        Output::new(peripherals.GPIO46, Level::Low, OutputConfig::default()),
        &mut delay,
    )
    .expect("driving the latch pins cannot fail");

    println!("{LOG_PREFIX}: latched (PWR_HOLD then PWR_LOCK)");
    {
        let mut buf = [0u8; GIT_CAPACITY];
        if let Ok(line) = format_git(
            env!("SIMPLE_DEBUG_GIT"),
            env!("SIMPLE_DEBUG_GIT_DIRTY") == "1",
            &mut buf,
        ) {
            println!("{line}");
        }
    }

    let charger = Charger::new(Output::new(
        peripherals.GPIO39,
        Level::High,
        OutputConfig::default(),
    ))
    .expect("driving /CE cannot fail");

    let mut external_power = ExternalPower::new(Input::new(
        peripherals.GPIO9,
        InputConfig::default().with_pull(Pull::None),
    ));
    println!(
        "{LOG_PREFIX}: external power present: {:?}",
        external_power.is_present()
    );

    let gpio7 = Input::new(
        peripherals.GPIO7,
        InputConfig::default().with_pull(Pull::Up),
    );
    let charge_status = ChargeStatus::new(Input::new(
        peripherals.GPIO40,
        InputConfig::default().with_pull(Pull::None),
    ));
    let sd_cd = Input::new(
        peripherals.GPIO11,
        InputConfig::default().with_pull(Pull::Up),
    );
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

    // Park CS idle-high. Do not mount the card or clock the panel.
    let _sd_cs = Output::new(peripherals.GPIO8, Level::High, OutputConfig::default());
    let _epd_cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());
    let _buzzer = Output::new(peripherals.GPIO48, Level::Low, OutputConfig::default());

    let sd_rail: SdRail<_, _> = Rail::new(
        Output::new(peripherals.GPIO10, Level::Low, OutputConfig::default()),
        latch.witness(),
    )
    .expect("driving the SD rail cannot fail");
    let sd_rail = sd_rail
        .enable(&mut delay)
        .expect("driving the SD rail cannot fail");

    let mic_rail: MicRail<_, _> = Rail::new(
        Output::new(peripherals.GPIO38, Level::Low, OutputConfig::default()),
        latch.witness(),
    )
    .expect("driving the mic rail cannot fail");

    let mut sensor_i2c = I2c::new(
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

    let epd_rail: EpdRail<_, _> = Rail::new(
        Output::new(peripherals.GPIO47, Level::Low, OutputConfig::default()),
        latch.witness(),
    )
    .expect("driving the panel rail cannot fail");
    let epd_rail = epd_rail
        .enable(&mut delay)
        .expect("driving the panel rail cannot fail");

    let (touch_rst, touch_int, gt911_addr) = reset_and_probe_gt911(
        &mut touch_i2c,
        peripherals.GPIO41,
        peripherals.GPIO21,
        &mut delay,
    );

    ack_walk_sensor_bus(&mut sensor_i2c, &mut delay);

    let sensor_i2c = RefCell::new(sensor_i2c);
    let mut gauge = Bq27220::new(RefCellDevice::new(&sensor_i2c));
    match gauge.verify_device_type(&mut delay) {
        Ok(()) => println!("{LOG_PREFIX}: gauge DeviceType 0x0220"),
        Err(error) => println!("{LOG_PREFIX}: gauge type error {error:?}"),
    }

    // ±2 g matches the board classifier's LSB scale. Accel only: do not call
    // init() / init_irqs() (those would program INT routing; GPIO7 stays input).
    let imu_settings = LsmSettings::default().with_accel(
        AccelSettings::new()
            .with_sample_rate(AccelSampleRate::_26Hz)
            .with_scale(AccelScale::_2G),
    );
    let mut imu_dev = LSM6DS3TR::new(I2cInterface::new(RefCellDevice::new(&sensor_i2c)))
        .with_settings(imu_settings);
    match imu_dev.init_accel() {
        Ok(()) => println!("{LOG_PREFIX}: imu accel init ok"),
        Err(_) => println!("{LOG_PREFIX}: imu accel init failed"),
    }

    println!(
        "{LOG_PREFIX}: rails {} / {} / {} ({}x{} panel, {} Hz SPI ceiling)",
        touch_rail.name(),
        epd_rail.name(),
        sd_rail.name(),
        display::WIDTH,
        display::HEIGHT,
        display::SPI_MAX_HZ,
    );
    println!("{LOG_PREFIX}: loop (no LUT, no charge enable)");
    #[cfg(feature = "operator")]
    println!("{LOG_PREFIX}: operator (gpio poll 20ms, gt911 contacts)");

    let _ = (charger, mic_rail);
    // RST stays driven high and INT stays input for the whole run.
    let _keep_touch_rst = &touch_rst;

    #[cfg(feature = "operator")]
    {
        // After INT-during-reset: clear Status, then Command = read coordinates.
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
        for step in [
            "buttons_ok",
            "buttons_up",
            "buttons_down",
            "vbus",
            "imu",
            "gpio7",
            "sd_detect",
            "gt911_contacts",
        ] {
            let mut buf = [0u8; PROMPT_CAPACITY];
            if let Ok(line) = format_prompt(step, &mut buf) {
                println!("{line}");
            }
        }
    }

    let mut gpio = PolledGpios {
        btn4,
        btn5,
        btn6,
        external_power,
        gpio7,
        charge_status,
        sd_cd,
    };
    let mut touch = TouchKeep {
        i2c: touch_i2c,
        int: touch_int,
        addr: gt911_addr,
    };
    run_poll_loop(&mut delay, &mut gpio, &mut gauge, &mut imu_dev, &mut touch);
}

struct PolledGpios {
    btn4: Input<'static>,
    btn5: Input<'static>,
    btn6: Input<'static>,
    external_power: ExternalPower<Input<'static>>,
    gpio7: Input<'static>,
    charge_status: ChargeStatus<Input<'static>>,
    sd_cd: Input<'static>,
}

/// Held for the whole poll so INT stays an input after reset.
struct TouchKeep {
    #[cfg_attr(not(feature = "operator"), allow(dead_code))]
    i2c: I2c<'static, Blocking>,
    #[cfg_attr(not(feature = "operator"), allow(dead_code))]
    int: Flex<'static>,
    #[cfg_attr(not(feature = "operator"), allow(dead_code))]
    addr: u8,
}

/// INT-during-reset, then product-ID read. Do not write config RAM.
fn reset_and_probe_gt911(
    touch_i2c: &mut I2c<'static, Blocking>,
    rst: esp_hal::peripherals::GPIO41<'static>,
    int: esp_hal::peripherals::GPIO21<'static>,
    delay: &mut Delay,
) -> (Output<'static>, Flex<'static>, u8) {
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
    // GPIO21 has no reset pull (ESP32-S3 v2.2 Table 2-1). Leave INT floating.
    touch_int.apply_input_config(&InputConfig::default().with_pull(Pull::None));
    touch_int.set_input_enable(true);
    delay.delay_ms(INT_SETTLE_MS);
    delay.delay_ms(POST_RESET_SETTLE_MS);

    let gt911_addr = SlaveAddress::Pair28_29.seven_bit();
    let mut id = [0u8; 4];
    let gt911_ack = touch_i2c
        .write_read(gt911_addr, &Register::Id.addr_bytes(), &mut id)
        .is_ok();
    log_ack(gt911_addr, gt911_ack);
    if gt911_ack {
        let mut buf = [0u8; GT911_ID_CAPACITY];
        if let Ok(line) = format_gt911_id(&id, &mut buf) {
            println!("{line}");
        }
    }
    (touch_rst, touch_int, gt911_addr)
}

/// Boot-time ACKs only. Do not print SHT serial; a 1-byte read NAKs.
fn ack_walk_sensor_bus(sensor_i2c: &mut I2c<'static, Blocking>, delay: &mut Delay) {
    // Sensirion SHT4x: 0xFD is high-precision measure (`Precision::High` in
    // sht4x 0.2.0). A 1-byte I2C read is not a valid command and NAKs.
    let sht40_ack = {
        let mut sht = Sht4x::<_, Delay>::new(&mut *sensor_i2c);
        sht.measure(Precision::High, delay).is_ok()
    };
    log_ack(addresses::SHT40, sht40_ack);

    for address in [
        addresses::PCF8563,
        addresses::BQ27220,
        addresses::LSM6DS3TRC,
    ] {
        let mut probe = [0u8; 1];
        log_ack(address, sensor_i2c.read(address, &mut probe).is_ok());
    }
}

fn run_poll_loop(
    delay: &mut Delay,
    gpio: &mut PolledGpios,
    gauge: &mut Bq27220<RefCellDevice<'_, I2c<'static, Blocking>>>,
    imu_dev: &mut LSM6DS3TR<I2cInterface<RefCellDevice<'_, I2c<'static, Blocking>>>>,
    touch: &mut TouchKeep,
) -> ! {
    #[cfg(not(feature = "operator"))]
    let _ = touch;

    let mut prev = read_levels(
        &mut gpio.btn4,
        &mut gpio.btn5,
        &mut gpio.btn6,
        &mut gpio.external_power,
        &mut gpio.gpio7,
        &mut gpio.charge_status,
        &mut gpio.sd_cd,
    );
    let mut t_s = 0_u32;

    #[cfg(feature = "operator")]
    let gt911 = gt911::Gt911Blocking::new(touch.addr);
    #[cfg(feature = "operator")]
    let mut last_contacts: Option<u8> = None;
    #[cfg(feature = "operator")]
    let mut last_gt911_status: Option<u8> = None;
    #[cfg(feature = "operator")]
    let mut gt911_fail_polls: u32 = 0;

    #[cfg(not(feature = "operator"))]
    const POLL_MS: u32 = 1_000;
    #[cfg(feature = "operator")]
    const POLL_MS: u32 = 20;
    #[cfg(feature = "operator")]
    const HEARTBEAT_EVERY: u32 = 1_000 / POLL_MS;

    let mut polls: u32 = 0;
    loop {
        let now = read_levels(
            &mut gpio.btn4,
            &mut gpio.btn5,
            &mut gpio.btn6,
            &mut gpio.external_power,
            &mut gpio.gpio7,
            &mut gpio.charge_status,
            &mut gpio.sd_cd,
        );
        let mut edges = [Edge::ButtonDown { gpio: 0 }; 7];
        let n = collect_edges(&prev, &now, &mut edges);
        for edge in edges.iter().take(n) {
            let mut buf = [0u8; EDGE_CAPACITY];
            if let Ok(line) = format_edge(*edge, &mut buf) {
                println!("{line}");
            }
        }
        prev = now;

        #[cfg(feature = "operator")]
        {
            let mut st = [0u8];
            if touch
                .i2c
                .write_read(touch.addr, &Register::Status.addr_bytes(), &mut st)
                .is_ok()
            {
                last_gt911_status = Some(st[0]);
            }
            let count = match gt911.get_multi_touch(&mut touch.i2c) {
                Ok(points) => {
                    gt911_fail_polls = 0;
                    Some(points.len() as u8)
                }
                // Crate: no new buffer (`STATUS_BUFFER_READY` clear). Not a bus error.
                Err(gt911::Error::NotReady) => last_contacts,
                Err(_) => {
                    if gt911_fail_polls.is_multiple_of(250) {
                        println!("{LOG_PREFIX}: gt911 poll failed");
                    }
                    gt911_fail_polls = gt911_fail_polls.saturating_add(1);
                    last_contacts
                }
            };
            if let Some(n) = count {
                if last_contacts != Some(n) {
                    let mut buf = [0u8; CONTACTS_CAPACITY];
                    if let Ok(line) = format_contacts(n, &mut buf) {
                        println!("{line}");
                    }
                    last_contacts = Some(n);
                }
            }
        }

        #[cfg(not(feature = "operator"))]
        let heartbeat = true;
        #[cfg(feature = "operator")]
        let heartbeat = polls.is_multiple_of(HEARTBEAT_EVERY);
        if heartbeat {
            let voltage_mv = gauge.voltage_mv().unwrap_or_default();
            let current_ma = gauge.current_ma().unwrap_or_default();
            let soc_pct = gauge.state_of_charge_pct().unwrap_or_default();
            let imu = imu_dev
                .read_accel_raw()
                .ok()
                .and_then(|xyz| imu::classify(xyz.x, xyz.y, xyz.z).map(pose));

            let snapshot = Snapshot {
                t_s,
                vbus: now.vbus,
                gpio7: now.gpio7,
                gpio40: now.gpio40,
                sd_cd: now.sd_cd,
                soc_pct,
                voltage_mv,
                current_ma,
                imu,
            };
            let mut buf = [0u8; HEARTBEAT_CAPACITY];
            if let Ok(line) = format_heartbeat(&snapshot, &mut buf) {
                println!("{line}");
            }
            #[cfg(feature = "operator")]
            if let Some(status) = last_gt911_status {
                let mut buf = [0u8; GT911_STATUS_CAPACITY];
                if let Ok(line) = format_gt911_status(status, &mut buf) {
                    println!("{line}");
                }
                let int_high = touch.int.is_high();
                let mut buf = [0u8; GT911_INT_CAPACITY];
                if let Ok(line) = format_gt911_int(int_high, &mut buf) {
                    println!("{line}");
                }
            }
            t_s = t_s.saturating_add(1);
        }

        delay.delay_ms(POLL_MS);
        polls = polls.saturating_add(1);
    }
}

fn log_ack(address: u8, ok: bool) {
    println!(
        "{}: {:#04x} {}",
        LOG_PREFIX,
        address,
        if ok { "ack" } else { "nak" }
    );
}

fn read_levels<B4, B5, B6, Vbus, G7, Stat, Sd>(
    btn4: &mut B4,
    btn5: &mut B5,
    btn6: &mut B6,
    vbus: &mut ExternalPower<Vbus>,
    gpio7: &mut G7,
    gpio40: &mut ChargeStatus<Stat>,
    sd_cd: &mut Sd,
) -> GpioLevels
where
    B4: InputPin,
    B5: InputPin,
    B6: InputPin,
    Vbus: InputPin,
    G7: InputPin,
    Stat: InputPin,
    Sd: InputPin,
{
    GpioLevels {
        btn4: btn4.is_high().unwrap_or(true),
        btn5: btn5.is_high().unwrap_or(true),
        btn6: btn6.is_high().unwrap_or(true),
        vbus: vbus.is_present().unwrap_or(false),
        gpio7: gpio7.is_high().unwrap_or(true),
        gpio40: matches!(gpio40.level(), Ok(ChargeLevel::High)),
        sd_cd: sd_cd.is_high().unwrap_or(true),
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
