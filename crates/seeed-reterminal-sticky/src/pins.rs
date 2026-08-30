//! GPIO assignments, as plain ESP32-S3 pin numbers.
//!
//! These are constants rather than a HAL type so this crate stays
//! MCU-agnostic and host-testable. The firmware maps them to real `esp-hal`
//! pins in one place, which is also the only place a typo can happen.
//!
//! Source: the board contract's pin and bus map.

/// Sensor I2C clock. **Also a strapping pin** (ESP32-S3 v2.2 section `3 Boot
/// Configurations` / `Table 3-1. Default Configuration of Strapping Pins`,
/// GPIO0 WPU) — never put it on the SPI bus.
pub const SENSOR_I2C_SCL: u8 = 0;
/// Sensor I2C data: SHT40, PCF8563, BQ27220, LSM6DS3TR-C.
pub const SENSOR_I2C_SDA: u8 = 1;

/// Touch I2C clock (dedicated bus). After reset: input, no internal pull.
pub const TOUCH_I2C_SCL: u8 = 2;
/// Touch I2C data (dedicated bus).
///
/// **Also a strapping pin** (ESP32-S3 v2.2 section `3 Boot Configurations`,
/// GPIO3). Floating at reset; ordinary IO after strapping hold time. Never
/// put it on the SPI bus.
pub const TOUCH_I2C_SDA: u8 = 3;

/// GPIO4, active low. The `ext1` wake source. Firmware name: AI / OK / power.
/// Seeed appearance diagram (glass facing you, USB-C down): **AI Voice Button**,
/// top of the three keys on the right edge.
pub const BUTTON_OK: u8 = 4;
/// GPIO5, active low. Firmware name: Up / left.
/// Seeed: **Page Up Button**, middle of the three on the right edge.
pub const BUTTON_UP: u8 = 5;
/// GPIO6, active low. Firmware name: Down / right.
/// Seeed: **Page Down Button**, bottom of the three on the right edge.
pub const BUTTON_DOWN: u8 = 6;

/// Shared interrupt net: LSM6DS3TR-C INT1 and BQ27220 GPOUT (schematic
/// Rev 01). **Input only** — see [`crate::AMBIGUOUS_INTERRUPT_NOTE`].
pub const AMBIGUOUS_INTERRUPT: u8 = 7;

/// MicroSD chip select, idle high.
pub const SD_CS: u8 = 8;
/// External power sense: high means USB/external present. Edge-capable.
/// Schematic: 5.1 kΩ / 5.1 kΩ from `VIN_5V` (`PWR_IN_VOLT`).
pub const EXTERNAL_POWER_SENSE: u8 = 9;
/// MicroSD power enable, active high. Hold low in sleep.
pub const SD_POWER_EN: u8 = 10;
/// MicroSD card detect. 10 kΩ pull-up; insert = low.
pub const SD_CARD_DETECT: u8 = 11;

/// Shared SPI MISO (used by the card; the panel is write-only in practice).
pub const SPI_MISO: u8 = 12;
/// Shared SPI clock: panel and card.
pub const SPI_SCLK: u8 = 13;
/// Shared SPI MOSI: panel and card.
pub const SPI_MOSI: u8 = 14;

/// E-paper chip select, active low.
pub const EPD_CS: u8 = 15;
/// E-paper data/command select.
pub const EPD_DC: u8 = 16;
/// E-paper reset.
pub const EPD_RST: u8 = 17;
/// E-paper BUSY, **active high**. Prefer an edge interrupt over polling.
pub const EPD_BUSY: u8 = 18;

/// PDM microphone clock (MSM261DDB020).
///
/// Also the ESP32-S3 USB D− / USB-Serial-JTAG pad. After deep sleep that
/// function reclaims the pin; firmware must disable the USB pad before
/// attaching PDM RX.
pub const MIC_CLK: u8 = 19;
/// PDM microphone data (MSM261DDB020).
///
/// Also the ESP32-S3 USB D+ / USB-Serial-JTAG pad. Same reclaim as
/// [`MIC_CLK`].
pub const MIC_DATA: u8 = 20;

/// GT911 interrupt. Selects the touch I2C address during reset, then input.
///
/// ESP32-S3 v2.2 `Table 2-1. Pin Overview`: no default pull at or after reset.
pub const TOUCH_INT: u8 = 21;

/// Microphone power enable, active high (TPS22916CYFPR on `PDM_EN`).
/// Hold low when unused and across sleep. After deep-sleep wake, drive
/// it low briefly before `enable` so the capsule is not left half-powered.
pub const MIC_POWER_EN: u8 = 38;

/// BQ25616 charge enable, **active low**.
///
/// TI BQ25616 SLUSDF7 `Table 7-1. Pin Functions`: CE driven LOW enables
/// charging. Default IO MUX is JTAG `MTCK` (ESP32-S3 v2.2 `Table 2-4. IO
/// MUX Functions`).
pub const CHARGE_EN: u8 = 39;
/// BQ25616 STAT (`CHARGE_STATE`). Low while charging when `/CE` is
/// enabled; high when done or `/CE` is parked. Default IO MUX is JTAG
/// `MTDO` (`Table 2-4. IO MUX Functions`).
pub const CHARGE_STATUS: u8 = 40;

/// GT911 reset.
///
/// Default IO MUX function is JTAG `MTDI` (v2.2 `Table 2-4. IO MUX
/// Functions`). Firmware must mux this pad to GPIO before the address-select
/// dance.
pub const TOUCH_RST: u8 = 41;
/// Touch power enable, active high. ~250 ms settle.
///
/// Default IO MUX function is JTAG `MTMS` (v2.2 `Table 2-4. IO MUX
/// Functions`). Mux to GPIO.
pub const TOUCH_POWER_EN: u8 = 42;

/// UART0 TX to the CH343P bridge.
pub const UART0_TX: u8 = 43;
/// UART0 RX from the CH343P bridge.
pub const UART0_RX: u8 = 44;

/// `PWR_HOLD`. Must be high to stay powered. **Also a strapping pin**
/// (ESP32-S3 v2.2 `3 Boot Configurations` / `Table 3-1`, GPIO45 default WPD).
pub const PWR_HOLD: u8 = 45;
/// `PWR_LOCK`. Must be high to stay powered. **Also a strapping pin**
/// (v2.2 `3 Boot Configurations` / `Table 3-1`, GPIO46 default WPD). Do not
/// pulse (`nyc-gpio46-pulse`).
pub const PWR_LOCK: u8 = 46;

/// E-paper power enable, active high. ~100 ms settle.
pub const EPD_POWER_EN: u8 = 47;

/// Passive buzzer (FUET-5018 through a CJ2324). PWM; hold low in sleep.
pub const BUZZER: u8 = 48;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strapping_pins_are_not_on_the_spi_bus() {
        // GPIO0 is sensor I2C SCL. A zero-initialised SPI config claims it and
        // kills the sensor bus after display init, so this is worth asserting.
        for spi_pin in [SPI_SCLK, SPI_MOSI, SPI_MISO, EPD_CS, SD_CS] {
            assert_ne!(spi_pin, SENSOR_I2C_SCL);
            assert_ne!(spi_pin, TOUCH_I2C_SDA);
            assert_ne!(spi_pin, PWR_HOLD);
            assert_ne!(spi_pin, PWR_LOCK);
        }
    }

    #[test]
    fn the_two_i2c_buses_are_disjoint() {
        for touch in [TOUCH_I2C_SCL, TOUCH_I2C_SDA] {
            assert_ne!(touch, SENSOR_I2C_SCL);
            assert_ne!(touch, SENSOR_I2C_SDA);
        }
    }
}
