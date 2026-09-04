//! Attended BQ25616 `/CE` pulse (`--features charge`).
//!
//! Parks first, prints STAT and VBUS, enables only when GPIO9 is high,
//! waits [`embassy_debug::CHARGE_PULSE_MS`], parks, settles, and
//! [`bq25616::Charger::hold_disabled`] if STAT is still low. The
//! firmware calls this after a cold boot or a 1 s Page Up resume
//! hold, not on a wake that re-sleeps. Gauge `Current()` is a
//! one-shot on the sensor bus before the IMU task owns it. On a
//! physical unit (USB): `gpio40=1→0→1`; `i=` was `0` at 200 ms and
//! `5702` at 2 s (not a 555 mA proof). Do not combine with `mic`,
//! `radio`, `pair`, `wifi`, or `sd`.
//!
//! FreeInk `FREEINK_DEVICE_STICKY` is the SDK wiring we follow here:
//! GPIO40 STAT **low** = charging, GPIO39 left undriven at idle. Bunny
//! `board_charger_init` drives `/CE` low at boot; that is not a recipe
//! for this image.

#[cfg(all(feature = "charge", feature = "mic"))]
compile_error!("do not combine charge with mic");
#[cfg(all(feature = "charge", feature = "radio"))]
compile_error!("do not combine charge with radio");
#[cfg(all(feature = "charge", feature = "sd"))]
compile_error!("do not combine charge with sd");
#[cfg(all(feature = "charge", feature = "pair"))]
compile_error!("do not combine charge with pair");
#[cfg(all(feature = "charge", feature = "wifi"))]
compile_error!("do not combine charge with wifi");

use bq25616::{ChargeStatus, Charger, ExternalPower, Level};
use bq27220::Bq27220;
use embassy_debug::{
    format_ce_off, format_ce_on, format_ce_parked, format_ce_skip, CHARGE_PULSE_MS,
    CHARGE_SETTLE_MS, LINE_CAPACITY,
};
use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c;
use esp_hal::gpio::{Input, InputConfig, Output, Pull};
use esp_hal::peripherals::{GPIO40, GPIO9};
use esp_println::println;

/// Enable window after settle, in milliseconds.
const CHARGE_HOLD_MS: u32 = CHARGE_PULSE_MS - CHARGE_SETTLE_MS;

/// Pulse `/CE` when VBUS is present, then return a parked charger.
pub fn run<I2C: I2c, D: DelayNs>(
    charger: Charger<Output<'static>, bq25616::Disabled>,
    vbus: GPIO9<'static>,
    stat: GPIO40<'static>,
    i2c: &mut I2C,
    delay: &mut D,
) -> Charger<Output<'static>, bq25616::Disabled> {
    let mut vbus = ExternalPower::new(Input::new(
        vbus,
        InputConfig::default().with_pull(Pull::None),
    ));
    let mut stat = ChargeStatus::new(Input::new(stat, InputConfig::default().with_pull(Pull::Up)));

    let gpio40_high = stat_is_high(&mut stat);
    let vbus_present = vbus.is_present().unwrap_or(false);
    let i_ma = gauge_current_ma(i2c);
    print_line(|buf| format_ce_parked(gpio40_high, vbus_present, i_ma, buf));

    if !vbus_present {
        print_line(format_ce_skip);
        return charger;
    }

    let charger = match charger.enable_charging_if_external_power(&mut vbus) {
        Ok(on) => on,
        Err(err) => {
            print_line(format_ce_skip);
            return err.into_charger();
        }
    };

    delay.delay_ms(CHARGE_SETTLE_MS);
    print_line(|buf| format_ce_on(stat_is_high(&mut stat), gauge_current_ma(i2c), buf));
    delay.delay_ms(CHARGE_HOLD_MS);
    print_line(|buf| format_ce_on(stat_is_high(&mut stat), gauge_current_ma(i2c), buf));

    let mut charger = charger.disable_charging().expect("driving /CE cannot fail");
    delay.delay_ms(CHARGE_SETTLE_MS);
    let mut gpio40_high = stat_is_high(&mut stat);
    if !gpio40_high {
        charger.hold_disabled().expect("driving /CE cannot fail");
        delay.delay_ms(CHARGE_SETTLE_MS);
        gpio40_high = stat_is_high(&mut stat);
    }
    print_line(|buf| format_ce_off(gpio40_high, gauge_current_ma(i2c), buf));
    charger
}

fn stat_is_high(stat: &mut ChargeStatus<Input<'static>>) -> bool {
    matches!(stat.level(), Ok(Level::High))
}

fn gauge_current_ma<I2C: I2c>(i2c: &mut I2C) -> Option<i16> {
    Bq27220::new(i2c).current_ma().ok()
}

fn print_line(format: impl FnOnce(&mut [u8]) -> Result<&str, embassy_debug::FormatError>) {
    let mut buf = [0u8; LINE_CAPACITY];
    if let Ok(line) = format(&mut buf) {
        println!("{line}");
    }
}
