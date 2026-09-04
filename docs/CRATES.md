# Crate audit

Every third-party driver needs a recorded verdict before adoption. Catalog
presence is not a verdict: both [drive-rs](https://tweedegolf.github.io/drive-rs/)
and [awesome-embedded-rust](https://github.com/rust-embedded/awesome-embedded-rust)
mostly miss this board's parts, and the ones they do list are not all correct
for it.

Verdicts are **pass** (use as-is), **pass-with-wrapper** (use, but board
specifics stay in `seeed-reterminal-sticky`), or **fail** (write our own).

## Adopted

| Part | Crate | Version audited | Verdict | Basis |
| --- | --- | --- | --- | --- |
| GT911 touch | [`gt911`](https://crates.io/crates/gt911) | 0.3.0 | **pass-with-wrapper** | `Gt911Blocking::new(i2c_addr: u8)` and the async `Gt911` take an explicit address, so `0x14` is constructible — the open question from planning. Blocking and async surfaces both exist; multi-touch returns a `heapless::Vec` of up to 5 points (matches Rev.09 §1 silicon max). `init()` writes command `0` at `0x8040` and clears status `0x814E`; it does not write config RAM. `Error::NotReady` means buffer bit `0x80` is clear (idle). **Rev.09 deleted the register map** (Rev.07), so command-`0` and bit `0x80` are crate / on-unit encodings, not a Rev.09 table; Espressif `ENTER_SLEEP` is not a claim of this PDF. Power enable, the INT-during-reset address dance, and the Sticky coordinate transform stay in the board crate (`to_screen` takes the GT911 **480×800** sample). After that dance, leave GPIO21 floating (`Pull::None`; ESP32-S3 v2.2 Table 2-1 has no default pull on that pad). simple-debug writes `StatusWrite::Clear` at `Register::Status` only (no `Register::Command`); 100 kHz. embassy-debug poll is board `Register` I2C at `I2C_MAX_HZ` (crate `init()` not used); INT-low (Rev.09 §6.1) delivered `touch n=5`. Read-only `gt911 st=` cadence is board `touch::STATUS_HEARTBEAT` (`EverySecs(10)` or `Off`). 100 kHz is inside the datasheet 400 kbps cap. |
| LSM6DS3TR-C IMU | [`lsm6ds3tr`](https://crates.io/crates/lsm6ds3tr) | 0.2.2 | **pass-with-wrapper** | `interface` module provides **both** `i2c` and `spi` back ends, so the SPI-only examples were misleading; I2C at `0x6A` is supported. Enclosure axis mapping and the 0.70 g placement threshold stay in the board crate. Do not touch GPIO7. |
| SHT40 | [`sht4x`](https://crates.io/crates/sht4x) | 0.2.0 | **pass** | `embedded-hal` 1.0, address `0x44`. Command bytes match the Sensirion SHT4x datasheet (`Precision::High` → `0xFD` high-precision measure, `0xF6` / `0xE0` medium/low, `0x94` soft reset, `0x89` serial number). Used on silicon: a high-precision measure ACKed at `0x44` where a 1-byte read NAKed. Do not print `serial_number` from this crate. |
| PCF8563 RTC | [`pcf8563-dd`](https://crates.io/crates/pcf8563-dd) | 0.3.0 | **pass** for the register map; **do not take the crate in this workspace** (`bisync` 0.3 is yanked) | NXP Rev 11: seconds bit 7 is VL. simple-debug reads `0x02` raw. On a physical unit: seconds tick and **`vl=0`**. |
| MicroSD | [`embedded-sdmmc`](https://crates.io/crates/embedded-sdmmc) | 0.10.0 | **pass-with-wrapper** | Init at <= 400 kHz then raise. Shares one SPI controller with the panel, so CS arbitration is the application's job via `embedded-hal-bus`. |

The two rows that were “pending register spot-check” are closed on
silicon: `sht4x` `0xFD` printed live milli °C / milli % RH; PCF8563
VL is seconds bit 7 and read **`vl=0`** from `0x02` (no `pcf8563-dd`
in the lockfile).

## Rejected

| Crate | Version | Why |
| --- | --- | --- |
| [`bq27xxx`](https://crates.io/crates/bq27xxx) | 0.0.2 | Targets BQ27426/427, which are **Impedance Track** gauges. The Sticky carries a BQ27220, a **CEDV** gauge — different data memory and different maintenance model. Wrong family, not merely incomplete. |
| [`ssd1677`](https://crates.io/crates/ssd1677) | 0.1.0 | Black/white(/red) skeleton with no four-gray path. This panel needs dual-plane writes plus OTP (or an optional panel-specific 105-byte LUT). Also occupies the obvious crate name, hence `ssd1677-gray4`. |
| [`ssd1677-driver`](https://crates.io/crates/ssd1677-driver) | 0.1.0 | Same gap as above. |
| [`epd-waveshare`](https://crates.io/crates/epd-waveshare) | — | Different controller and panel families; its waveform assumptions do not transfer. |
| [`ssd1675`](https://crates.io/crates/ssd1675) | — | Different controller. Byte-addressed windowing assumptions do not hold for SSD1677's 10-bit address units. |
| [`lsm6ds3trc`](https://crates.io/crates/lsm6ds3trc) | 0.1.0 | Not needed: `lsm6ds3tr` already supports I2C. Fewer dependencies beats a second candidate at 0.1.0. |

## Written here

| Crate | Why not off the shelf |
| --- | --- |
| [`bq27220`](../crates/bq27220) | No correct crate exists for this CEDV part. Reads are safe, writes are hazardous, so the split matters more than convenience. |
| [`ssd1677-gray4`](../crates/ssd1677-gray4) | Four-gray on mono film via dual planes plus OTP (optional MCU LUT) is the whole point, and no existing crate does it. |
| [`bq25616`](../crates/bq25616) | A GPIO-only charger has no I2C driver to adopt; the value is making active-low `/CE` impossible to get wrong (`Drop` parks, VBUS interlock, `hold_disabled`). |
| [`seeed-reterminal-sticky`](../crates/seeed-reterminal-sticky) | No board crate exists for this product. |
| [`simple-debug`](../crates/simple-debug) | UART heartbeat, GPIO edges, and [`IdleListen`](../crates/simple-debug/src/idle.rs) for unattended `vet-idle-log`. Host-tested because the Xtensa image cannot run `cargo test` on the host compiler. |
| [`embassy-debug`](../crates/embassy-debug) | Timestamped button / touch / IMU / mic / radio / BLE pair-card / read-only SD identify / charge-sit lines and [`IdleListen`](../crates/embassy-debug/src/idle.rs) for unattended `vet-idle-log`. Host-tested because the Xtensa image cannot run `cargo test` on the host compiler. |

## Infrastructure

`sticky-host` uses [`espflash`](https://crates.io/crates/espflash) 4.5 as a
library (`default-features = false`, feature `serialport`).
[`cargo-espflash`](https://crates.io/crates/cargo-espflash) is the Cargo
plugin binary wrapping that crate; it has no library target. Do not enable
espflash's `cli` feature (no SemVer). Never call the full-chip erase APIs.
`cargo xtask` is clap over `sticky-host`.

When firmware members land, both images take `esp-hal`, `esp-println`,
`esp-backtrace`, `esp-bootloader-esp-idf`, and `esp-alloc` from git tag
`esp-hal-v1.2.0-rc.0`. `embassy-debug` also takes `esp-rtos` from that tag
(crates.io `esp-rtos` 0.3.0 still pins `esp-hal` ~1.1). The images share
that source so one workspace lockfile does not hit a `links = "esp-println"`
conflict (crates.io `esp-println` 0.18 vs the tag's 0.17).
`esp-bootloader-esp-idf` is only for `esp_app_desc!()`, so the factory
ESP-IDF 2nd-stage bootloader and `espflash save-image` accept the payload.
Do not `--merge`. `esp-alloc` is present because `lsm6ds3tr` 0.2.2 pulls
`alloc`. `embassy-debug-fw --features radio` also takes `esp-radio` from
that tag plus `trouble-host` / `bt-hci` for concurrent scan.
`--features pair` takes the same radio crates for BLE peripheral +
DisplayOnly passkey (no Wi-Fi, no `coex`).
`sticky-host` serializes learn-uart YAML with
[`noyalib`](https://crates.io/crates/noyalib) 0.0.30 (serde, no `unsafe` in
sticky-host). Operator prompts use [`anstyle`](https://crates.io/crates/anstyle)
1.0.14 and [`anstream`](https://crates.io/crates/anstream) 1.0.0 (`anstream`
honors TTY, `NO_COLOR`, and `CLICOLOR`).
[`rustix`](https://crates.io/crates/rustix) 1.1.4 (`termios`) puts the
operator TTY in cbreak so `learn-uart` can skip a wait on `s` without Enter
(no `unsafe` in sticky-host).

`embedded-hal` 1.0, `embedded-hal-async`, `embedded-hal-bus`,
`embedded-graphics-core`, and dev-only `embedded-hal-mock` (with
`default-features = false, features = ["eh1", "embedded-hal-async"]` so the
0.2 and `embedded-time` shims stay out of the graph).

## Counts

5 adopted from crates.io, 6 explicitly rejected, 6 written here.
