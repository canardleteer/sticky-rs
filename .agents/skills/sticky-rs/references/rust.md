# Rust software paths

Hardware stays in
[`seeed-sticky-hardware`](../../seeed-sticky-hardware/SKILL.md). This page
is how to drive that hardware from **Rust** in this repository, including
the Cargo host toolchain. Command catalog: [xtask.md](xtask.md).

Two stacks are valid. In-tree Xtensa images are **not migrated yet**.
When they land, `firmware/simple-debug` is **`no_std` / `esp-hal`**:

| Stack | When |
| --- | --- |
| `no_std`: `esp-hal` + `esp-rtos` / Embassy | Bare-metal async; this repo’s default |
| `std`: `esp-idf-hal` + `esp-idf-svc` | Share ESP-IDF drivers/partition story with vendor C++ firmware |

Encode the hardware
[pin-map](../../seeed-sticky-hardware/references/pin-map.md) in
`seeed-reterminal-sticky`. Chip drivers (`bq25616`, `bq27220`,
`ssd1677-gray4`) stay MCU-agnostic. Adopt a crates.io driver only with a
recorded verdict in [`docs/CRATES.md`](../../../../docs/CRATES.md). Register
facts come from the hardware
[datasheet catalog](../../seeed-sticky-hardware/resources/datasheets.md);
if the local cache is missing, ask the user to populate it.

Do not mix this page with PlatformIO / `idf.py`. Those trees are wiring
evidence in
[cpp-platformio.md](../../seeed-sticky-hardware/references/cpp-platformio.md),
not a flash path here. UART geometry:
[flashing.md](../../seeed-sticky-hardware/references/flashing.md). Observed
silicon:
[measure.md](../../seeed-sticky-hardware/references/measure.md).

## Host toolchain (`cargo xtask`)

USB-C is a WCH CH343P (`1a86:55d3`), not Espressif USB-Serial/JTAG and not
probe-rs. QinHeng is not an Espressif VID. `cargo xtask` picks a unique
Sticky CH343 (or `--port` / `ESPFLASH_PORT`) and refuses a non-QinHeng plug
**before** DTR. Prefer

```text
/dev/serial/by-id/usb-1a86_USB_Single_Serial_<SERIAL>-if00
```

when more than one Sticky is present. Monitor **115200**. Full catalog and
UART lock: [xtask.md](xtask.md).

`sticky-host` talks to the chip through the **`espflash` library** (what
`cargo-espflash` wraps). `cargo xtask` is clap over that crate. Do not run
the `espflash` / `cargo espflash` / `esptool` CLIs against the board when
xtask covers the job. Never `espflash flash` (it installs a default
bootloader and table). Never `erase-flash`.

Three different resets, confirmed on the CH343 UART:

- **Opening the ACM node** does not reprint stock firmware logs, but
  Linux `cdc-acm` asserts DTR+RTS on that open and pulses EN (`POWERON`)
  on embassy / custom `app0`. Default `monitor` / `learn-uart` claim USB
  CDC instead.
- **EN/RTS pulse, IO0 high** (xtask UART sample) boots the app. Stock
  `key=serial_number` appears ~4.5–6.5 s later.
- **ROM download DTR/RTS** (`--probe`, backup, confirm, restore, flash-app)
  enters the stub, then hard-resets. Glass keeps the last frame.

Do not open a port unless a human asked.

### One-time install

```shell
cargo install espup --locked
cargo install espflash --locked
espup install
# then source the script `espup` printed (example: . $HOME/export-esp.sh)
```

`espup` provides the Xtensa compiler. `espflash` is for **host-only**
`save-image` (no port), which `cargo xtask build-fw` invokes. This
repository does not use [`cargo-espflash`](https://crates.io/crates/cargo-espflash)
to write flash. Do not use `probe-rs` on this connector.

The host user needs `dialout` (or equivalent). This repository has no Cargo
`runner`; `cargo run` cannot flash.

### `no_std` (`esp-hal`) — Cargo

Target: `xtensa-esp32s3-none-elf`.

```shell
# after sourcing the script `espup` printed, from the sticky-rs repo root
rustc --print target-list | grep esp32s3   # expect xtensa-esp32s3-none-elf
cargo xtask build-fw simple-debug --features operator
# equivalent:
# cargo +esp build -p simple-debug-fw --profile release-fw --locked \
#   --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc --features operator
# espflash save-image --chip esp32s3 --flash-size 32mb --skip-update-check \
#   target/xtensa-esp32s3-none-elf/release-fw/simple-debug-fw \
#   target/xtensa-esp32s3-none-elf/release-fw/simple-debug.bin
# after an original exists:
# cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/simple-debug.bin --yes
# cargo xtask learn-uart --image target/xtensa-esp32s3-none-elf/release-fw/simple-debug.bin --yes --report FILE
# cargo xtask monitor
```

`save-image` is host-only (no port). It refuses an `esp-hal` ELF without
`esp_bootloader_esp_idf::esp_app_desc!()`. Do not `--merge` (default
bootloader and table). `cargo xtask monitor` at 115200 has shown the factory
2nd-stage jump to `0x90000`. Opening the ACM TTY (`--acm-tty`) produces a
`POWERON` ROM log because `cdc-acm` asserts DTR+RTS on open. Default
monitor claims USB CDC instead and leaves the modem lines off.

Board info: `cargo xtask detect-connected --probe`, not
`cargo espflash board-info`. `--probe` prints `Embedded Flash` and omits
PSRAM; product-class PSRAM / JEDEC are already in
[measure.md](../../seeed-sticky-hardware/references/measure.md). That is
not a Cargo flash flag.

Ship a **32 MB-aware** partition table. Do not inherit `n16r8` 16 MB limits.
Do not add a `.cargo/config.toml` runner that calls `espflash flash`.

### `std` (`esp-idf-hal`) — Cargo

Target: `xtensa-esp32s3-espidf`. ESP-IDF is pulled by `esp-idf-sys` (not
`idf.py` in this path). There is no in-tree `std` image yet. If you build
one, still load it with `save-image` + `cargo xtask flash-app`, not
`cargo espflash flash`.

```shell
cargo install ldproxy --locked
# ESP-IDF version is whatever esp-idf-sys / the template pins (aim 5.4-class)
cargo build --release
espflash save-image --chip esp32s3 --flash-size 32mb --skip-update-check \
  ELF out.bin
# cargo xtask flash-app --image out.bin --yes
```

`ldproxy` is required for the GNU ld wrapper this target uses. First `cargo
build` downloads IDF and can take a long time. sdkconfig still needs octal
PSRAM and 32 MB flash — same board facts as
[cpp-platformio.md](../../seeed-sticky-hardware/references/cpp-platformio.md),
expressed through `esp-idf-sys` (env / `ESP_IDF_SDKCONFIG_DEFAULTS`), not
PlatformIO.

Do not flash this stack with `pio run` or `idf.py`.

## `no_std` (`esp-hal`) crates

- `esp-hal` (`esp32s3`)
- `esp-rtos` with `embassy` (replaces unmaintained `esp-hal-embassy`)
- Embassy executor / time / sync; `embassy-net` + `esp-radio` for Wi-Fi/BLE
  (`esp-wifi` is the old name)
- `embedded-hal` 1.0 / `embedded-hal-async` 1.0
- `esp-println` + `log` on UART0 (not RTT)

Pin `Cargo.lock`. PSRAM, LEDC, I2S/PDM, and some sleep APIs have lived behind
`esp-hal` `unstable`. When firmware members exist, `firmware/simple-debug`
is blocking `esp-hal` only — no Embassy, no RTOS. `firmware/embassy-debug`
is the Embassy image (`esp-rtos` + executor); the panel is `--features epd`.

SPI: construct the bus with **only** SCLK/MOSI/MISO and the CS pins in the
[pin map](../../seeed-sticky-hardware/references/pin-map.md). Do not attach
GPIO0.

Sleep: GPIO hold + `Ext1WakeupSource` on GPIO4 ANY_LOW. Rails in
[power-and-sleep.md](../../seeed-sticky-hardware/references/power-and-sleep.md).

Shared SPI: `embedded-hal-bus` or `embassy-embedded-hal` — one mutex, two
`SpiDevice`s.

## `std` (`esp-idf-hal`) crates

Uses ESP-IDF under the hood (same ROM bootloader, partition table, PSRAM
Kconfig story as
[cpp-platformio.md](../../seeed-sticky-hardware/references/cpp-platformio.md)).

- `esp-idf-sys` / `esp-idf-hal` / `esp-idf-svc`
- UART0 logging matches IDF `Serial0` / physical UART0
- SPI2: set unused quad/data pins to **`-1`** (IDF zero-init otherwise takes
  GPIO0 and kills sensor I2C after display init)
- PSRAM: octal, `BOARD_HAS_PSRAM` equivalent in sdkconfig
- Flash: 32 MB; do not inherit `n16r8` 16 MB limits

IDF driver knowledge (GT911 reset dance, SSD1677 busy=1, BQ27220 registers)
transfers from the vendor C++ trees; wrap it in Rust rather than re-deriving
pins.

## First image (either Rust stack)

Firmware, not host tools:

1. GPIO45 then GPIO46 high **before** the executor / `main` work.
2. UART0 through the CH343P.
3. Buttons GPIO4/5/6 active-low.
4. Two I2C masters: sensors at 400 kHz, GT911 at 100 kHz (Bunny on-glass);
   probe the listed addresses. GPIO0 and GPIO3 are straps; leave GPIO21
   (GT911 INT) floating after the address dance.
5. SPI at 10 MHz; never GPIO0 or GPIO3 on that bus.
6. SSD1677 1-bit full refresh, then gray4 +
   [display.md](../../seeed-sticky-hardware/references/display.md).
7. GT911 from [touch.md](../../seeed-sticky-hardware/references/touch.md).
8. 32 MB partition table; octal PSRAM; then SD arbitration.

Load a custom image only after a factory original exists, with
`cargo xtask flash-app --image FILE --yes` (factory `app0` at `0x90000`).
`cargo xtask build-fw` fails until firmware members exist. Intended images:
`firmware/simple-debug` (blocking `esp-hal` latch + I2C facts + UART
heartbeat of raw levels; host-tested line format in `crates/simple-debug`)
and `firmware/embassy-debug` (Embassy log task, buttons, GT911, IMU every
5 s, buzzer; panel only with `--features epd`; host-tested lines in
`crates/embassy-debug` when that crate lands).

## Datasheet catalog vs crates

Vendor documents and gaps live in the hardware skill
[datasheet catalog](../../seeed-sticky-hardware/resources/datasheets.md).
That catalog does not name this repo’s crates. Which crate cites which
sheet, and the adoption verdict, live here and in
[`docs/CRATES.md`](../../../../docs/CRATES.md). Prefer crate rustdoc for
the exact literature revision a driver was typed against.

| Catalog id | Crate | Notes |
| --- | --- | --- |
| `ssd1677` | `ssd1677-gray4` | Rev 1.0 Table 7-1 opcodes, 105-byte `Lut`, window ranges, dual planes |
| `bq27220-sluscb7` | `bq27220` | Standard commands, DeviceType; rustdoc cites SLUSCB7 |
| `bq27220-sluubd4` | `bq27220` | CEDV data-memory not yet typed |
| `bq25616` | `bq25616` | Active-low `/CE`; rustdoc cites SLUSDF7 |
| `lsm6ds3tr-c` | `lsm6ds3tr` + `seeed-reterminal-sticky` | Orientation classification in the board crate |
| `gt911` | `gt911` + `seeed-reterminal-sticky` | Addresses and transform in the board crate |
| `sht4x` | `sht4x` 0.2.0 | `Precision::{High,Medium,Low}` → `0xFD` / `0xF6` / `0xE0`; do not print `0x89` serial |
| `pcf8563` | `pcf8563-dd` | **pass** pending seconds-register VL / integrity check ([CRATES.md](../../../../docs/CRATES.md)) |
| `esp32-s3-datasheet` / `esp32-s3-trm` | board crate / firmware | Strapping, GPIO21, JTAG pads, `ext1` |

## Crates vs parts

In this repository, prefer the workspace crates and the verdicts in
[`docs/CRATES.md`](../../../../docs/CRATES.md).

| Part | Crate | Notes |
| --- | --- | --- |
| Board pins / latch / rails | `seeed-reterminal-sticky` | This repo. Keep chip drivers MCU-agnostic |
| SSD1677 | `ssd1677-gray4` (this repo) | Dual-plane four-gray; Sticky uses **OTP** (no default MCU LUT). 10 MHz SPI. Wait on BUSY with `embedded-hal-async` `Wait`, not a spin loop. Not crates.io `ssd1677` |
| GT911 | `gt911` | Own EN/RST/INT + Sticky transform in the board crate. Mux GPIO41/42 off JTAG F0 before driving. Crate `init()` writes command `0` at `0x8040` then clears `0x814E` — not a config-RAM write. On-glass `begin()` clears `0x814E` only (no `0x8040`). `NotReady` is idle (count 0). **Rev.09 deleted the register map**; those encodings are on-glass / crate, not that PDF. Bus **100 kHz** (datasheet cap 400 kbps), 30 ms poll, taps on finger-up. Silicon max 5 contacts. INT after reset: floating input (`Pull::None`; ESP32-S3 GPIO21 has no default pull) |
| SHT40 | `sht4x` | Sensor I2C `0x44` |
| PCF8563 | `pcf8563-dd` | Sensor I2C `0x51` |
| LSM6DS3TR-C | `lsm6ds3tr` | Mutex the shared sensor I2C. Do not drive GPIO7 |
| BQ27220 | `bq27220` (this repo) | Not `bq27xxx` (wrong family: CEDV vs Impedance Track). Reads by default; gate data-memory writes |
| BQ25616 | `bq25616` (this repo) | GPIO39 low; GPIO9 digital. No I2C |
| Buzzer | LEDC (`esp-hal` or IDF LEDC) | GPIO48 |
| MicroSD | `embedded-sdmmc` | Init ≤ 400 kHz. CS arbitration is the application’s job |
| PDM mic | I2S PDM RX | GPIO38 enable. GPIO19/20 are USB-Serial-JTAG pads: disable the USB pad after deep-sleep wake before attaching PDM. Working ESPHome recipe (16 kHz, left) in [sensors.md](../../seeed-sticky-hardware/references/sensors.md#pdm-microphone); not a crate |
| Wi-Fi / BLE | `esp-radio` or `esp-idf-svc` | |
| Graphics | `embedded-graphics` | Host simulator without glass |
