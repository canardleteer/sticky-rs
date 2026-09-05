# Vendor C/C++, ESP-IDF, and PlatformIO

Hardware pins and levels are in the other reference files. This page is how
existing **C/C++** firmware drives them — wiring evidence (official when
Seeed published the tree; otherwise third-party), not a flash path for
this skill.

Do not treat `idf.py flash`, PlatformIO upload, or `espflash flash` as this
skill’s host path. Observed silicon: [measure.md](measure.md). Docs and
firmware catalog: [catalog.md](catalog.md).

## Trees

| Tree | Role |
| --- | --- |
| Official `Sticky_dashboard_demo` / `Sticky_peripheral_demo` | Vendor ESP-IDF v5.4 zips (CDN 2026-08). Official `pin_config.h` and sequences. [Official ESP-IDF demos](#official-esp-idf-demos) |
| Factory `reterminal_template` 1.1.0 | **Measured** on hardware — [measure.md](measure.md) |
| `reTerminal_Sticky_Bunny` (PlatformIO, ESP-IDF 5.4.1) | Sequences confirmed on a physical unit: latch, 10 MHz SPI, display/touch, IMU, sleep |
| FreeInk SDK `FREEINK_DEVICE_STICKY` / CrossPoint | Compiled `BoardProfile STICKY`; some items still pending |
| ESPHome `seeed-reterminal-sticky` | 10 MHz, `mirror_x`, sensor I2C examples |
| Playground `sticky-2048` | Buildable `seeed_epaper` / `gt911` / `bq27220`; GPIO7 as `PIN_BFG_INT` |

## Official ESP-IDF demos

Seeed’s three ESP-IDF guides walk the same dashboard zip (not the
still-404 `OSHW-reTerminal-Sticky` git):
[basics](https://www.seeedstudio.com/sticky/docs/en/device-guide/esp-basics/),
[pages](https://www.seeedstudio.com/sticky/docs/en/device-guide/esp-pages/),
[refresh](https://www.seeedstudio.com/sticky/docs/en/device-guide/esp-refresh/).
Zip URLs and SHA-256: [catalog.md](catalog.md). Flashing them replaces
`app0`. This skill does not flash them.

Official prose on those pages that is board-relevant (intent, not
electrical fact):

- **Pages:** I2C1 GPIO0/1 is shared by BQ27220, PCF8563, SHT40, and the
  IMU; I2C0 GPIO2/3 is GT911 only. SPI2 is shared by the panel and
  MicroSD (different CS). `sticky_sdcard_read_text()` must mount, read,
  and unmount **before** a panel refresh, and must not
  `spi_bus_free` SPI2. Unused `spi_bus_config_t` data pins must be
  `-1` or GPIO0 is stolen from sensor SCL. The Battery page’s
  `charging` flag is GPIO9 VBUS, shown separately from the gauge
  percent. IMU is a 100 ms poll with five matching samples before
  `OrientationChanged` (no GPIO7). Microphone capture is on-demand
  (~1 s), rail on then off.
- **Refresh:** Home is the only gray4 full refresh
  (`sticky_display_refresh`). Other pages are mono full. RTC minute
  updates are mono partial of a **complete** comparison frame. Do not
  partial-refresh a gray4 page. AI hold ~2 s: beep, save RTC state,
  mono sleep page, stop touch, `sticky_display_sleep`, then hold latch
  high and EPD/touch/mic/SD/buzzer **off**. Wait for AI release, then
  GPIO4 `ext1` ANY_LOW. The guide’s FAQ: GT911 errors or an immediate
  wake mean touch was still polling or the hold was still low.

`main/pin_config.h` (both zips) matches [pin-map.md](pin-map.md) for the
nets it names: latch 45/46, AI/Up/Down 4/5/6, `/CE` 39, VBUS detect 9,
sensor I2C 0/1, touch 2/3/21/41/42, EPD 13/14/12/15/16/17/18/47, SD
8/10/11, mic 19/20/38, buzzer 48. Addresses: BQ27220 `0x55`, PCF8563
`0x51`, SHT40 `0x44`, LSM6DS3TR-C `0x6A`. **GPIO7 and GPIO40 are not in
the macros.** Do not treat the omission as free GPIO; schematic still
shares IMU INT1 / gauge GPOUT on 7 and STAT on 40.

Dashboard `board_init()`: on deep-sleep wake, preload 45/46 high before
`gpio_hold_dis`; then hold both high, 100 ms; park SD CS and **SD EN
high**; sensor I2C1 at default with internal pull-ups. It never pulses
GPIO46.

`sticky_charger_init()` drives GPIO39 **low** at boot (official enable)
and reads GPIO9 as a bool named `charging` (VBUS present, not STAT). Do
not copy that boot-enable into a default debug image
([power-and-sleep.md](power-and-sleep.md)).

`sticky_display_init()`: EPD_EN, SPI2 10 MHz mode 0, unused data pins
**-1** (official comment: zero-init steals GPIO0 from sensor SCL),
`mirror_x`, 180° rotate, 96 KiB gray4 in SPIRAM. OTP `seeed_epaper`:
gray4 `0x1A = {0x67,0x00}` then `0x22 = 0xD7`; mono full `0xF7`;
partial `0xFF`. Comment in `sticky_display_refresh_partial()`: SSD1677
partial is **panel-wide**; send a full comparison frame. Sleep `0x10 =
0x03`. No 0x32 LUT.

`sticky_touch` I2C0, EN 250 ms, INT-during-reset (`INT=0 → 0x5D`).
`GT911::begin(800, 480)` is the landscape canvas map; the driver reads
`0x8146` (`sensor=` vs `map=`) and does not rewrite config RAM.
`read_points` already `mapCoord`s onto 800×480.
`transform_touch_coordinate` then treats those points as portrait and
scales with 800 as X. That is SDK layering (landscape as the starting
canvas), not a 800-wide FPC. Do not apply that second scale to raw
480×800 samples ([touch.md](touch.md)).

Deep sleep: wait for AI release, hold latch high and rails off, wake
GPIO4 `ext1` ANY_LOW only. IMU is polled over I2C (no GPIO7). PDM is
16 kHz mono, GPIO38 off until capture.

`sdkconfig.defaults`: 32 MB, octal PSRAM 80 MHz, `SINGLE_APP_LARGE` (not
factory dual-OTA). Flash mode is not pinned (IDF default, not a DIO/QIO
fact).

## Bare ESP-IDF skeleton

Native IDF **v5.4**, target `esp32s3`. This is a hardware-correct starting
tree for reading vendor C++ firmware, not a Playground submission, not a
copy of `sticky-2048`, and not a flash path for this skill.

```text
project/
  CMakeLists.txt
  sdkconfig.defaults
  main/
    CMakeLists.txt
    main.cpp
    pin_config.h
```

Top-level `CMakeLists.txt`:

```cmake
cmake_minimum_required(VERSION 3.16)
include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(sticky_app)
```

`main/CMakeLists.txt` — list every `.cpp` in `SRCS` (a missing file links and
does nothing):

```cmake
idf_component_register(
    SRCS "main.cpp"
    INCLUDE_DIRS "."
    REQUIRES driver esp_driver_gpio esp_hw_support esp_rom freertos
)
```

`sdkconfig.defaults` — board facts, not 2048 app policy. Flash **DIO** matches
factory runtime; QIO is a software choice. CPU frequency is a software choice
(factory 160 MHz, Bunny 240 MHz). Leave Wi-Fi/BT to the app.

```ini
CONFIG_IDF_TARGET="esp32s3"
CONFIG_ESPTOOLPY_FLASHSIZE_32MB=y
CONFIG_ESPTOOLPY_FLASHMODE_DIO=y
CONFIG_SPIRAM=y
CONFIG_SPIRAM_MODE_OCT=y
CONFIG_SPIRAM_SPEED_80M=y
CONFIG_ESP_CONSOLE_UART_DEFAULT=y
CONFIG_ESP_CONSOLE_UART_BAUDRATE=115200
CONFIG_ESP_MAIN_TASK_STACK_SIZE=8192
CONFIG_FREERTOS_HZ=1000
```

Ship a **32 MB-aware** partition table. Do not inherit `n16r8` 16 MB limits.
Do not copy factory dual-OTA LittleFS unless that is the product goal.

Latch **before** any log or bus init. Drive GPIO45 and GPIO46 **high and keep
them high**. Do not start from write-ups that pulse GPIO46 until
[nyc-gpio46-pulse](../resources/not-yet-confirmed.md#nyc-gpio46-pulse) is
closed.

```cpp
// pin_config.h — GPIO numbers from pin-map.md
#define PIN_POWER_HOLD 45
#define PIN_POWER_LOCK 46
#define PIN_GPIO7_DO_NOT_DRIVE 7  // IMU INT vs gauge GPOUT; input only
```

```cpp
void board_power_latch(void)
{
    gpio_config_t cfg = {};
    cfg.pin_bit_mask = (1ULL << PIN_POWER_HOLD) | (1ULL << PIN_POWER_LOCK);
    cfg.mode = GPIO_MODE_OUTPUT;
    gpio_config(&cfg);
    gpio_hold_dis((gpio_num_t)PIN_POWER_HOLD);
    gpio_hold_dis((gpio_num_t)PIN_POWER_LOCK);
    gpio_set_level((gpio_num_t)PIN_POWER_HOLD, 1);
    gpio_set_level((gpio_num_t)PIN_POWER_LOCK, 1);
    vTaskDelay(pdMS_TO_TICKS(100));
}
```

Call `board_power_latch()` first in `app_main()`. On deep-sleep wake, drive
those pins high **before** `gpio_hold_dis`.

SPI2 for the panel: unused `quadwp` / `quadhd` / `data4`–`data7` must be
**`-1`**, or GPIO0 is stolen from sensor SCL.

Flash and monitor over the CH343P (`1a86:55d3`). QinHeng is not an Espressif
VID. Monitor **115200**. In those C++ trees that is `idf.py -p PORT flash
monitor`. Consuming projects use their own host tools.

Playground `integration.json` and Web Serial publishing:
[catalog.md](catalog.md). This skeleton does not include that schema.

## Factory (`reterminal_template`)

Hard facts (chip, partitions, HAL ACK list, 160 MHz, Winbond DIO) are in
[measure.md](measure.md). This section is only how that image behaves as
ESP-IDF firmware: `Serial0` / UART0 115200, dual-OTA LittleFS, `power_en_lock`
inside HAL init, NVS device-info keys (per-device; do not copy another unit’s
values). Do not copy its partition table unless the product goal is
factory-compatible OTA.

## Bunny (PlatformIO, ran on device)

`platformio.ini`: `espressif32@6.11.0`, `board = sticky_esp32s3`, framework
`espidf`, monitor **115200**. Envs: `sticky-release` (default), `sticky-debug`,
`sticky-power-test`.

Board JSON: 32 MB flash, `BOARD_HAS_PSRAM`, **240 MHz**, flash **QIO**, upload
460800. Product URL in that JSON (`p-6398`) disagrees with the README
(`p-6861`).

`sdkconfig.defaults`: `CONFIG_ESPTOOLPY_FLASHSIZE_32MB`, QIO, octal PSRAM
80 MHz, main task stack 8192, FreeRTOS 1000 Hz.

Bunny partitions (not factory):

| Name | Type | Offset | Size |
| --- | --- | --- | --- |
| nvs | nvs | `0x9000` | 24 KiB |
| phy_init | phy | `0xF000` | 4 KiB |
| factory | factory | `0x10000` | 8 MiB |
| assets | spiffs | `0x810000` | ~23.9 MiB |

### `app_main` order

1. `board_power_init()` — GPIO45/46 high, 100 ms; on sleep-wake preload before
   `gpio_hold_dis`
2. `board_charger_init()` — GPIO39 low, GPIO9 digital VBUS
3. `board_shared_spi_prepare()` — SD CS/EN high, detect pull-up, GPIO ISR
4. `board_sensor_bus_init()` — I2C1 SDA=1 SCL=0
5. RTC, BQ27220 (non-fatal if missing)
6. `sticky_display_init()` — EPD_EN, SPI2 at 10 MHz; unused SPI data pins **-1**
7. White full clear unless woke from deep sleep
8. GT911 (I2C0, EN 250 ms), buzzer, IMU, NVS, apps

### Pin macros (`pin_config.h`)

`PIN_POWER_HOLD` 45, `PIN_POWER_LOCK` 46, `PIN_TOP_BUTTON` 4,
`PIN_SIDE_BUTTON_LEFT/RIGHT` 5/6, `PIN_BAT_CHG_EN` 39, `PIN_EXTERNAL_POWER` 9,
sensor I2C 0/1, EPD 14/13/12/15/16/17/18/47, SD CS/EN/DETECT 8/10/11, touch
2/3/42/21/41, buzzer 48. GPIO7 is **not** in Bunny’s macros; do not drive it.
Addresses: BQ27220 `0x55`, PCF8563 `0x51`, LSM6DS3 `0x6A`.

### ESP-IDF gotchas proven here

- SPI `quadwp` / `quadhd` / `data4`–`data7` must be **`-1`**, or GPIO0 is
  stolen from sensor SCL.
- Display: `SPI2_HOST`, 10 MHz, mode 0, `mirror_x`, busy level 1, 96 KiB gray4
  buffers in SPIRAM, 180° rotate before TX.
- Touch: `I2C_NUM_0`, **100 kHz**, probe **0x14 then 0x5D**, extra **30 ms**
  after the reset dance, poll **30 ms**, sensor map 480×800. INT left as a
  **floating** input (no MCU pull-up). `gpio_hold_dis` on RST/INT before the
  dance. After reset, write `0x814E = 0`. Do not write `0x8040`. Idle poll is
  count 0 (status bit `0x80` clear), not a bus error. Taps fire on finger-up.
- Sleep chord: GPIO5+GPIO6 held 2 s; wait for release; hold latch high,
  peripheral enables low; wake GPIO4 `ext1` ANY_LOW.
- Buzzer: LEDC low-speed timer 0 / channel 0, 10-bit duty, GPIO48.
- Charger: `gpio_get_level(GPIO9) != 0` ⇒ external power.

`components/seeed_epaper` documents SSD1677 as “reTerminal E1005”; Sticky uses
that driver class. That driver is **OTP** (no 0x32). Do not port a FreeInk
`Ssd1677Driver` MCU LUT onto this glass. Stock `reterminal_template`
1.1.0 adds `ssd1677_standby` / `ssd1677_resume` (`0x22 = 0x03` then
`0x20`; `0x22 = 0xC0` then `0x20`). UC8179 leaves both NULL. The 2048
tree’s SSD1677 driver has only `.sleep` (`0x10 = 0x03`).

Source map: `src/board/` power, charger, buses, pins; `src/display/`;
`src/input/` touch+buttons; `src/sensors/` IMU; `src/devices/` battery, RTC,
buzzer.

## FreeInk / CrossPoint

`BoardProfile STICKY` in `BoardConfig.h`: SSD1677 800×480, pins as above,
`displaySpiHz = 0` → **40 MHz default** (comment: pin 10 MHz if shared bus
is flaky), `NO_FLIP` pending, GT911 `0x5D` alt `0x14`, raw 0–799×0–479,
swapXY+flip both, `usbDetect` unassigned (GPIO9 as ADC), GPIO40 charge
status, GPIO4 `DigitalConfirmPowerHold`, PDM mic 19/20/38, SHT40 `0x44`.

GT911 bring-up is `InputManager::beginGt911` (CrossPoint calls the same
SDK path; there is no second driver):

- `gpio_hold_dis` on `TOUCH_EN` (GPIO42), then rail HIGH, **50 ms**.
- Wire at **400 kHz**.
- `resetWithIntLevel`: RST low 10 ms with INT driven, RST high 10 ms,
  INT still driven 50 ms, INT input 50 ms. Try INT=0 (`0x5D`) first,
  then INT=1 (`0x14`). Probe both candidate addresses after each dance.
- **No** write to `0x8040`. **No** status clear at `begin`.
- `pollGt911` (8 ms): read `0x814E`; if bit 7 clear, return without
  writing. If ready, read `0x8150` (coords at byte 0 on Sticky), then
  write `0x814E = 0`.

That combination matched the embassy-debug listen that printed
`touch n=5`. The numbers above are **this tree’s** evidence, not the
board contract. Contract facts are Rev.09, schematic Rev 01, and UART
in [touch.md](touch.md).

CrossPoint inherited `esp32-s3-devkitc1-n16r8` and **16 MB** upload limits —
wrong for this flash. PSRAM often left off because a 48 KiB 1-bit FB fits in
DRAM. `holdPowerRails()` is the first `setup()` step (GPIO45/46). Logging is
physical `Serial0` / ROM UART, not USB CDC.

## ESPHome

Preset `seeed-reterminal-sticky`: EPD CS/DC/RST/BUSY/EN 15/16/17/18/47,
`mirror_x`, 10 MHz. Sensor I2C GPIO1/0 at 400 kHz; touch I2C GPIO3/2 at
100 kHz. Boot automation (priority 600) raises GPIO45/46. A 2026.8.2
Playground “everything” YAML also raises mic GPIO38, touch GPIO42, and
SD GPIO10, enables PDM (19/20), GT911 `0x5D` (INT 21 / RST 41), SHT40,
PCF8563, BQ27220 register reads, and GPIO4 `ext1` deep sleep after 30 s.
IMU stays commented (no native LSM6DS3). SD CS GPIO8 is named in the
header only. Charge enable on GPIO39 is inverted and not turned on at
boot. Do not flash a Playground `.factory.bin` at `0x0`: that image’s
table puts `nvs` at `0x1fa0000` and would destroy factory NVS. Extract
the app at `0x10000` and write factory `app0` only.

[sira-fiinikkusu/reterminal-sticky-voice-companion](https://github.com/sira-fiinikkusu/reterminal-sticky-voice-companion)
does use the PDM mic and deep sleep on this product. Audio wiring and the
USB-Serial-JTAG pad reclaim: [sensors.md](sensors.md#pdm-microphone).

## Flashing those C++ trees

This skill does not run `pio` or `idf.py`. The commands below are how the
Bunny / IDF trees load **their own** builds if you are in those trees.

```shell
# in the Bunny / IDF tree only
pio run -e sticky-release -t upload
# or
idf.py -p "$PORT" flash monitor
```

Same CH343P UART. Monitor 115200. Never `erase-flash`: factory NVS holds
Wi-Fi RF calibration, device identity, and persisted gauge state. See
[what NVS holds](measure.md#what-nvs-holds-never-erase-it).
