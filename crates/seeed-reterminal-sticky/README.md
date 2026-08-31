# seeed-reterminal-sticky

Board support for the **Seeed Studio reTerminal Sticky** (ESP32-S3R8, 800×480
SSD1677 e-paper, GT911, CH343P UART).

This crate holds what is true about the *board* — pin numbers, the power latch,
rail settle times, panel geometry and OTP refresh modes, the touch transform,
the enclosure's orientation mapping — and nothing else. Chip registers live in
driver crates; MCU peripherals (UART, I2C, SPI, PDM, PWM, Wi-Fi) live in the
firmware's HAL. It depends on `embedded-hal` 1.0 only, is `#![no_std]`, and is
fully host-testable.

It is deliberately thin. It is not a second abstraction layer over `esp-hal`.

## What this crate is for

Firmware maps `pins::*` GPIO numbers onto HAL pins in one place, then uses the
typestate types here so bring-up order is a compile error rather than a
comment. Drivers for the chips on those buses are **not** re-exported; you
depend on them alongside this crate.

## Board features

These tables map schematic Rev 01 plus **Seeed / community claims** onto
crate types and suggested drivers. **This crate** means a typed helper, pin
constant, address, or rail.
**Not in this crate** means you talk to a chip driver or the MCU HAL yourself.

**On glass** is `yes` only when firmware exercises the feature and this
project has a passing UART or glass result. `no` means the pin, address, or
type exists so you can try; it is not a claim that the part is present or
that the crate encoding is correct.

### Power and charging

| Feature | This crate | Rest of the stack | On glass |
| --- | --- | --- | --- |
| Stay-alive latch (`PWR_HOLD` GPIO45, `PWR_LOCK` GPIO46) | `Latch::acquire` / `Latch::release`; every rail constructor needs a `Latched` witness | Firmware supplies the two output pins. Releasing the latch is a software power-off on battery. | yes |
| USB / external present | `pins::EXTERNAL_POWER_SENSE` (GPIO9, high = VBUS) | Digital input. Schematic: 5.1 kΩ / 5.1 kΩ `PWR_IN_VOLT` from `VIN_5V` (~½ VBUS); USB-C is 5 V sink only (Rd on CC1/CC2). Firmware still treats GPIO9 as a GPIO high. | yes |
| BQ25616 charger `/CE` (active low) and status | Pin numbers only: `pins::CHARGE_EN`, `pins::CHARGE_STATUS` | [`bq25616`](https://github.com/canardleteer/sticky-rs/tree/main/crates/bq25616) owns `/CE` typestate (`Drop` parks `Enabled`; VBUS interlock; `hold_disabled`). `CHARGE_STATUS` is STAT: low while `/CE` is enabled, high after park **and a settle**. embassy-debug `--features charge` is the attended pulse; default images stay parked. | STAT polarity yes; charge-to-done no |
| Dual-color charge LED (next to USB-C) | **Not in this crate** | Driven by the charger, not an MCU GPIO. Green/yellow while STAT was low. Off / done color unconfirmed. | charging color only |
| 750 mAh 1S pack / BQ27220 fuel gauge | I2C address `addresses::BQ27220` (`0x55`) on the sensor bus | [`bq27220`](https://github.com/canardleteer/sticky-rs/tree/main/crates/bq27220) on `pins::SENSOR_I2C_*`. Reads are safe; unseal / `CFGUPDATE` / FCC writes are opt-in. | yes |

### Display and touch

| Feature | This crate | Rest of the stack | On glass |
| --- | --- | --- | --- |
| 3.97" 800×480 mono e-paper (SSD1677, four-gray via dual planes + panel OTP) | `display`: geometry, `SPI_MAX_HZ` (10 MHz, mode 0), `RefreshKind::{Full, Partial, Gray4}`, `controller_config()` (OTP, `lut: None`) | [`ssd1677-gray4`](https://github.com/canardleteer/sticky-rs/tree/main/crates/ssd1677-gray4) for opcodes. Shared SPI with the card (`pins::SPI_SCLK` / `SPI_MOSI` / `SPI_MISO`, `pins::EPD_CS`). This crate does **not** ship a waveform LUT. | yes |
| Panel 3.3 V rail | `EpdRail` on GPIO47; no unconditional `disable` | After the controller deep-sleep command, pass `PanelParked::after_deep_sleep_command()` into `disable_after_panel_sleep`. | yes |
| GT911 capacitive touch (portrait 480×800 digitizer under landscape panel) | `TouchRail` (GPIO42), reset timings and addresses in `touch`, `touch::Register`, `touch::Command`, `touch::StatusWrite` / `StatusBits`, `touch::SlaveAddress`, `touch::StatusHeartbeat` / `STATUS_HEARTBEAT`, `touch::to_screen` / `to_framebuffer` | Dedicated I2C: `pins::TOUCH_I2C_*` (schematic). Rev.09 §6.1: INT=0 → `SlaveAddress::PairBaBb`, bus ≤ `I2C_MAX_HZ`, `Register::Points`, no init Status/Command write. Crate `init()` still writes `Command::ReadCoordinates`. Neither writes config RAM. This FPC delivers **5** contacts (`touch n=5`, `st=0x85`). INT-high + init Status-clear ACKed `Pair28_29` and stayed `st=0x00`. Do not silently flip `addresses::GT911_PRIMARY` (`0x14`). Read-only `gt911 st=` cadence is `STATUS_HEARTBEAT` (`EverySecs(10)` or `Off`). `to_screen` takes the **480×800** sample (not panel 800×480); USB-down ink corners land on 800×480. | yes |

### Sensors (sensor I2C, 400 kHz)

Sensor bus: `I2C_FREQUENCY_HZ`. Touch bus: `touch::I2C_HZ` (100 kHz)
or `touch::I2C_MAX_HZ` (Rev.09 §6.1 cap). `pins::SENSOR_I2C_SCL` is
GPIO0 and `pins::TOUCH_I2C_SDA` is GPIO3: both are **strapping
pins**. Never assign them to the SPI controller. After the
INT-during-reset dance, leave `pins::TOUCH_INT` (GPIO21) as a
floating input — the ESP32-S3 pad has no default pull.

| Feature | This crate | Rest of the stack | On glass |
| --- | --- | --- | --- |
| SHT40-AD1B-R2 humidity / temperature | `addresses::SHT40` (`0x44`) | [`sht4x`](https://crates.io/crates/sht4x). Four-pin DFN; no ALERT. simple-debug prints `sht t=` / `rh=` (milli °C / milli % RH). | yes |
| PCF8563M/TR real-time clock | `addresses::PCF8563` (`0x51`) | Time at `0x02`; VL is seconds bit 7. INT (`RTC_INTn`) is NC to the ESP32. simple-debug prints `rtc` (read only). | yes |
| LSM6DS3TR-C IMU | `addresses::LSM6DS3TRC` (`0x6A`); `imu::classify` maps a raw sample onto this enclosure | [`lsm6ds3tr`](https://crates.io/crates/lsm6ds3tr) over I2C. Do not drive GPIO7 as output (see below). | yes |

### Audio, storage, UI, debug

| Feature | This crate | Rest of the stack | On glass |
| --- | --- | --- | --- |
| PDM microphone (MSM261DDB020) | **Power and pins only.** `MicRail` on `pins::MIC_POWER_EN` (GPIO38, TPS22916CYFPR); clock `pins::MIC_CLK` (GPIO19), data `pins::MIC_DATA` (GPIO20). Settle time is unmeasured. There is **no** PDM/I2S driver, buffer, or sample API here. | See [PDM microphone (untested)](#pdm-microphone-untested). | no |
| Passive buzzer (FUET-5018) | Pin only: `pins::BUZZER` (GPIO48) | PWM (LEDC) in the HAL through a CJ2324. Hold low in sleep. No helper in this crate. | yes |
| MicroSD | `SdRail` (GPIO10), `pins::SD_CS`, `pins::SD_CARD_DETECT` (insert = 0), `sd::{INIT_HZ, identify, send_status, Identity}` (read-only SPI identify; CID product serial is not stored) | Same SPI as the panel; one CS at a time. Card rail 3.3 V. Filesystem mount: [`embedded-sdmmc`](https://crates.io/crates/embedded-sdmmc) after init ≤ 400 kHz. Empty slot with a pull-up reads high. embassy-debug `--features sd`: `sd cd=0`, `type=sdhc`, status ACK at 10 MHz and 20 MHz, FAT `vol=0` plus a ReadOnly file read. | yes |
| Right-edge buttons (AI Voice / Page Up / Page Down) | `pins::BUTTON_OK`, `BUTTON_UP`, `BUTTON_DOWN` — active low, external pull-ups. OK (GPIO4) is the `ext1` wake source. | GPIO input in the HAL. Seeed names and locations: [enclosure.md](https://github.com/canardleteer/sticky-rs/blob/main/.agents/skills/seeed-sticky-hardware/references/enclosure.md). Recessed **Reset** on the bottom edge is a hardware reset net, not a GPIO. | yes |
| UART0 via WCH CH343P | `pins::UART0_TX` / `UART0_RX` (GPIO43/44) | MCU UART driver at 115200. Schematic: 43 = TX (`USB_RXD`), 44 = RX (`USB_TXD`). Not native USB-Serial/JTAG. | yes |

### On-chip (ESP32-S3), not board-crate types

Seeed / community list these as ESP32-S3 or enclosure traits. This crate does
not wrap them:

| Feature | Notes | On glass |
| --- | --- | --- |
| Wi-Fi 802.11 and Bluetooth LE | Radio on the ESP32-S3; schematic on-board **ANT1** (2.4 GHz). Use the firmware stack (`esp-hal` / `esp-idf`); no pin map entry. Embassy-debug `--features radio` scanned both in one UART listen. | yes |
| 8 MB in-package octal PSRAM | MCU/HAL init. | no |
| 32 MB external quad flash | MCU/HAL. Factory NVS is per-unit. | no |
| Deep sleep | Hold documented GPIO levels across entry (see this crate's rustdoc). Wake on `BUTTON_OK`. Sequencing is firmware. | no |
| Enclosure magnets / IP40 glass | Mechanical only. Orientation for UI is `imu::classify`. | no |

## What the types enforce

- **The latch comes first.** `Latch::acquire` drives `PWR_HOLD` then `PWR_LOCK`
  and hands back a `Latched` witness. Every rail constructor requires that
  witness, so "bring up a peripheral before latching power" does not compile.
- **Releasing the latch is deliberate.** `Latch::release` is named for what it
  does: on battery it powers the board off.
- **The panel rail cannot be cut carelessly.** `EpdRail` has no `disable`; it
  has `disable_after_panel_sleep`, which needs a `PanelParked` token whose only
  constructor is `after_deep_sleep_command()`. Rails that are safe to cut
  (`TouchRail`, `SdRail`, `MicRail`) implement `CutAnytime` and get a plain
  `disable`.
- **GPIO7 has no output constructor.** Schematic Rev 01 ties IMU INT1 and
  gauge GPOUT to the same pin. `pins::AMBIGUOUS_INTERRUPT` is input-only.
- **Charge status is a raw level.** BQ25616 STAT is low while charging
  when `/CE` is enabled; see the `bq25616` crate.

## PDM microphone (untested)

This crate has no `Microphone` struct and no sample, PDM, or I2S API. It only
names `MicRail` and `pins::MIC_*`.

Schematic Rev 01: MSM261DDB020 on GPIO19 (clock) / GPIO20 (data),
TPS22916CYFPR on GPIO38 (`PDM_EN`). Embassy-debug and simple-debug
construct `MicRail` and leave it **disabled** in the default image.
Settle time, USB-Serial-JTAG pad reclaim after deep sleep, and
high-fidelity hole-vs-waveform are still open.

There is no loudspeaker.

Map:

- `MicRail` on `pins::MIC_POWER_EN` (GPIO38). Settle time is unmeasured.
  Hold the pin low when unused and across sleep.
- `pins::MIC_CLK` (GPIO19) and `pins::MIC_DATA` (GPIO20). Those pads are
  also USB-Serial-JTAG D−/D+; notes from elsewhere say to disable that pad
  after deep-sleep wake before attaching I2S. That sequence is untested
  here.
