//! Board support for the **Seeed Studio reTerminal Sticky**.
//!
//! This crate holds what is true about the *board*: pin numbers, the power
//! latch, rail settle times, panel geometry, the touch transform, and the
//! enclosure's orientation mapping. Chip register knowledge lives in the
//! device driver crates, and MCU knowledge lives in the firmware — so this
//! crate depends on `embedded-hal` 1.0 only and is fully host-testable.
//!
//! It is deliberately thin. It is not a second abstraction layer over
//! `esp-hal`. The crate README maps Seeed / community claims onto crate
//! types versus chip drivers and the MCU HAL, and marks which rows this
//! project has confirmed on glass. There is no microphone sample API
//! here: [`rails::MicRail`] and [`pins::MIC_CLK`] / [`pins::MIC_DATA`]
//! only. PDM capture is untested.
//!
//! # Bring-up order
//!
//! 1. [`power::Latch::acquire`] — `PWR_HOLD` then `PWR_LOCK`, before anything
//!    else. The returned [`power::Latched`] witness is required to construct
//!    any rail, so the order is enforced rather than documented.
//! 2. UART0 through the CH343P bridge.
//! 3. Buttons on [`pins::BUTTON_OK`], [`pins::BUTTON_UP`],
//!    [`pins::BUTTON_DOWN`] — all active low with external pull-ups.
//! 4. Two I2C masters. Sensors at [`I2C_FREQUENCY_HZ`] (400 kHz). GT911 at
//!    [`touch::I2C_HZ`] (100 kHz on glass). Never put [`pins::SENSOR_I2C_SCL`]
//!    (GPIO0) or [`pins::TOUCH_I2C_SDA`] (GPIO3) on the SPI bus: both are
//!    strapping pins (ESP32-S3 datasheet v2.2 section `3 Boot Configurations`).
//!    A zero-initialised SPI config that claims GPIO0 kills the sensor bus
//!    after display init. After the GT911 address dance, leave
//!    [`pins::TOUCH_INT`] as a floating input (v2.2 `Table 2-1. Pin Overview`:
//!    GPIO21 has no reset pull).
//! 5. SPI at [`display::SPI_MAX_HZ`], shared between panel and card with
//!    exactly one chip select asserted at a time.
//! 6. Panel rail, then the controller; touch rail, then the GT911 reset
//!    sequence in [`touch`].
//!
//! # Sleep order
//!
//! Reverse of bring-up, with one hard rule: the panel controller goes into
//! deep sleep **before** its rail is cut, which
//! [`rails::Rail::disable_after_panel_sleep`] makes explicit. Then hold the
//! GPIO levels (`SD_POWER_EN`, `MIC_POWER_EN`, `BUZZER`, `TOUCH_RST` low; the
//! latch pins high) across the sleep entry, and wake on
//! [`pins::BUTTON_OK`] as an `ext1` any-low source. After wake, GPIO19/20
//! are the USB-Serial-JTAG pads again: disable that pad before PDM RX.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(test)]
extern crate std;

pub mod display;
pub mod imu;
pub mod pins;
pub mod power;
pub mod rails;
pub mod touch;

pub use crate::power::{Latch, Latched};
pub use crate::rails::{EpdRail, MicRail, PanelParked, SdRail, TouchRail};

/// Why [`pins::AMBIGUOUS_INTERRUPT`] (GPIO7) is input-only here.
///
/// Seeed's hardware overview assigns it to the IMU interrupt; a community app
/// names it as the fuel gauge's `GPOUT`. Both cannot be right, and driving a
/// pin that another device is also driving is how transistors die. On-glass
/// IMU bring-up has polled I2C and left this pin alone, so this crate offers
/// no output constructor for it. Leave it an input until someone measures it
/// (`nyc-gpio7`).
pub const AMBIGUOUS_INTERRUPT_NOTE: &str =
    "GPIO7 owner is unconfirmed (IMU INT vs gauge GPOUT): input only";

/// I2C addresses on this board.
pub mod addresses {
    /// Sensirion SHT40 humidity and temperature, sensor bus.
    pub const SHT40: u8 = 0x44;
    /// NXP PCF8563 real-time clock, sensor bus.
    pub const PCF8563: u8 = 0x51;
    /// TI BQ27220 fuel gauge, sensor bus (SLUSCB7 `7.3.1.1 I2C Interface`
    /// 7-bit `0x55`).
    pub const BQ27220: u8 = 0x55;
    /// ST LSM6DS3TR-C IMU, sensor bus.
    pub const LSM6DS3TRC: u8 = 0x6a;
    /// Goodix GT911 on the **touch** bus after INT-high reset
    /// ([`crate::touch::SlaveAddress::Pair28_29`]). Rev.09 section `6.1 I2C
    /// Timing` names the 8-bit pair `0x28`/`0x29`; working units answer at
    /// 7-bit `0x14`.
    pub const GT911_PRIMARY: u8 = crate::touch::SlaveAddress::Pair28_29.seven_bit();
    /// GT911 after INT-low reset ([`crate::touch::SlaveAddress::PairBaBb`]).
    /// Rev.09 `6.1 I2C Timing` pair `0xBA`/`0xBB` (7-bit `0x5D`).
    pub const GT911_ALTERNATE: u8 = crate::touch::SlaveAddress::PairBaBb.seven_bit();
}

/// Sensor-bus I2C speed, in hertz.
///
/// ESP32-S3 datasheet v2.2 section `4.2.1.2 I2C Interface`: Fast mode is
/// 400 kbit/s. Touch stays at [`touch::I2C_HZ`] (100 kHz on glass).
pub const I2C_FREQUENCY_HZ: u32 = 400_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gauge_is_not_on_the_touch_bus() {
        // Putting the gauge on the touch bus has been a real mistake; the two
        // buses are physically separate here.
        assert_ne!(addresses::BQ27220, addresses::GT911_PRIMARY);
        assert_ne!(addresses::BQ27220, addresses::GT911_ALTERNATE);
    }

    #[test]
    fn sensor_bus_addresses_are_distinct() {
        let mut addresses = [
            addresses::SHT40,
            addresses::PCF8563,
            addresses::BQ27220,
            addresses::LSM6DS3TRC,
        ];
        addresses.sort_unstable();
        for pair in addresses.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
    }

    #[test]
    fn gt911_aliases_match_the_touch_module() {
        assert_eq!(
            addresses::GT911_PRIMARY,
            touch::SlaveAddress::Pair28_29.seven_bit()
        );
        assert_eq!(
            addresses::GT911_ALTERNATE,
            touch::SlaveAddress::PairBaBb.seven_bit()
        );
        assert_eq!(touch::I2C_HZ, 100_000);
        const { assert!(touch::I2C_HZ <= I2C_FREQUENCY_HZ) };
    }
}
