# Sources, conflicts, and gaps

## Precedence

The skill user is authoritative. When sources disagree, name both sides and
their layers; do not silently flatten the conflict. Full wording:
[SKILL.md](../SKILL.md#authority).

1. **Skill user** — they weigh the facts.
2. **Observed hardware** on this product (batch variation allowed). Live
   UART, `flash-id`, ACKs, meter/schematic, on-unit partitions and USB
   ([measure.md](measure.md)). Pin maps are firmware-derived (code that has
   run on this product), not ROM `board-info`.
3. **Official** board docs, vendor SDKs, and chip datasheets for parts
   confirmed on this model. Registers and timings when they have not been
   measured on glass. Official stock/SDK sequences prove **intent and
   ordering, never electrical fact**.
4. **Third-party** firmware, Playground apps, community skills, ESPHome,
   FreeInk. Often first with new valid detail; often stale or wrong.

Observed outranks a datasheet default. Both GT911 7-bit addresses ACK
here (Rev.09 §6.1: INT=0 → `0x5D` delivered contacts; INT=1 → `0x14`
ACK without points on an init Status-clear path). Do not apply a
datasheet to a chip that is not on this model.

This page is not a host-tool catalog. Consuming projects supply their own
capture path.

URL and firmware map: [catalog.md](catalog.md). Vendor datasheets:
[datasheets.md](../resources/datasheets.md). Open measurements:
[not-yet-confirmed.md](../resources/not-yet-confirmed.md). External skill
and its upstreams: [external.md](../resources/external.md). Vendor C++
sequences: [cpp-platformio.md](cpp-platformio.md).

## Citations

| Source | Layer | Use |
| --- | --- | --- |
| Live silicon on a Sticky ([measure.md](measure.md)) | Observed | Chip, USB, factory flash, ACK list; PSRAM/JEDEC confirmed (`esptool flash-id`) |
| Stock firmware image inspection ([measure.md](measure.md#stock-firmware-image-inspection)) | Official (intent) | Vendor driver sequences, interrupt/wake wiring, device configuration paths. Intent and ordering only — never electrical fact, never a source of bytes |
| [Seeed Hardware Overview](https://www.seeedstudio.com/sticky/docs/en/device-guide/hardware-overview/) | Official | Mechanics, 750 mAh, module list, IMU INT=GPIO7 |
| [Seeed schematic Rev 01](https://files.seeedstudio.com/wiki/reterminal_sticky/res/reTerminal_Sticky_Schematic_diagram_260609.pdf) ([datasheets.md](../resources/datasheets.md) `seeed-sticky-schematic`) | Official | Board nets and orderable PNs (CC BY-SA 4.0, 2026-06-05). No BOM in this PDF |
| [Seeed Quick Start appearance diagram](enclosure.md) ([vendored PNG](../resources/enclosure/appearance_en.png), [SOURCE.md](../resources/enclosure/SOURCE.md)) | Official | Enclosure locations: AI Voice / Page Up / Page Down on the right edge; SD left; Reset/mic/lanyard/LED/USB-C on the bottom. Not a pinout |
| [Product page](https://www.seeedstudio.com/reTerminal-Sticky-p-6861.html) | Official | Commercial identity (`p-6398` also appears in one board JSON) |
| ESP32-S3 datasheet v2.2 ([datasheets.md](../resources/datasheets.md)) | Official (confirmed MCU) | Straps GPIO0/3/45/46, GPIO21 no default pull, JTAG F0 on GPIO39–42, I2C 100/400 kbit/s, USB 19/20 |
| ESP32-S3 TRM ([datasheets.md](../resources/datasheets.md)) | Official (confirmed MCU) | GPIO hold, pad-JTAG eFuse, `ext1` wake details. Populate the local cache when citing it |
| Community firmware that ran on glass (Bunny / PlatformIO) | Third-party | Pin levels, 10 MHz SPI, display/touch, IMU, sleep rails |
| [varo6/reTerminal-sticky-skill](https://github.com/varo6/reTerminal-sticky-skill) | Third-party | ESP-IDF + Playground skill; GPIO7 flag; `seeed_epaper` API. File list: [external.md](../resources/external.md) |
| [Playground registry](https://github.com/Seeed-Projects/reterminal-sticky-playground-registry) | Third-party (catalog) | `integration.json`, `sticky-2048` in-tree source |
| [Lukilyy/reterminal-sticky-2048-eink-game](https://github.com/Lukilyy/reterminal-sticky-2048-eink-game) | Third-party | ESP-IDF app; GPIO7 as `PIN_BFG_INT` |
| FreeInk `STICKY` board profile | Third-party | Mic / SHT40 / GPIO40 *wiring intent*; 40 MHz / `NO_FLIP` still pending |
| ESPHome `seeed-reterminal-sticky` | Third-party | 10 MHz, `mirror_x`, sensor I2C example |
| [sira-fiinikkusu/reterminal-sticky-voice-companion](https://github.com/sira-fiinikkusu/reterminal-sticky-voice-companion) | Third-party (on-glass) | ESPHome: PDM 16 kHz left on GPIO19/20, GPIO38 rail, USB-Serial-JTAG pad reclaim after deep sleep |

Do not cite a host checkout path, a one-off dump directory, or another
person’s MAC / serial / NVS / flash image as if they were product facts.

## Conflicts

State both columns when a page or issue touches a row. The skill user
weighs them.

| Topic | Observed / on-glass | Other sources |
| --- | --- | --- |
| Flash size | **32 MB**, JEDEC `ef4019` | CrossPoint `n16r8` 16 MB is a build bug (third-party) |
| PSRAM | **8 MB octal** (`esptool flash-id`, `AP_3v3`); ~5 MiB in factory heap | `espflash board-info` prints `Embedded Flash` and omits PSRAM; CrossPoint often left PSRAM off |
| eFuse flash | **quad**, **3.3 V** | — |
| Factory runtime flash | **DIO** | Bunny builds **QIO** (software) |
| Factory CPU | **160 MHz** | Bunny **240 MHz** (software) |
| USB debug | CH343P `1a86:55d3`, no probe-rs | — |
| SPI clock | 10 MHz on-glass | FreeInk 40 MHz default (out of SSD1677 spec) |
| Display orientation | mirror_x+180° on-glass | FreeInk `NO_FLIP`; ESPHome `mirror_x` only |
| GPIO9 | Stock firmware runs it as a **digital any-edge interrupt**. Schematic: 5.1 kΩ / 5.1 kΩ `PWR_IN_VOLT` from `VIN_5V` (~2.5 V at 5 V VBUS), still a valid high | FreeInk analog `PWR_IN_VOLT` matches the sheet. Firmware stays digital |
| GPIO40 | UART `gpio40=1` with `/CE` parked (not charging) | Schematic: BQ25616 STAT. Low while charging when `/CE` is enabled; high-Z/high when done or parked |
| GT911 | UART `touch n=5`, `st=0x85` after INT-low (Rev.09 §6.1 → `0x5D`). This FPC delivers 5 (Rev.09 §1). Sample is **480×800**; `to_screen` maps that onto 800×480 (USB-down ink corners on glass). | INT-high → `0x14` ACK; init Status-clear path stayed `st=0x00`. A map that treated the sample as 800×480 compressed the keys axis to `y≈195`. |
| **GPIO7** | Unused in on-glass IMU poll (input, low, no edges) | Schematic: shared LSM6DS3TR-C INT1 (`6D_INTn`) and BQ27220 GPOUT (`BFG_INT`). Seeed and `sticky-2048` named the same pin. Do not drive. |
| ANT1 / radio | UART `wifi n=` and `ble n=` in one embassy-debug `--features radio` listen; `imu=` still running | Schematic on-board ANT1, shared 2.4 GHz |
| SHT40 | UART `sht t=` / `rh=` (~28.9 °C / ~27.9 % RH, one room) | Schematic SHT40-AD1B-R2 at `0x44`; `0xFD` measure |
| PCF8563 | UART `rtc` seconds tick; `vl=0` | Schematic PCF8563M/TR at `0x51`; NXP Rev 11 VL is seconds bit 7 |

Software rows (CPU, DIO/QIO, 16 MB n16r8) are decided as software choices.
Electrical rows that still need a meter or a missing glass PN are in
[not-yet-confirmed.md](../resources/not-yet-confirmed.md).

A full-chip image contains that unit’s NVS (Wi-Fi RF calibration, serial, MAC).
Do not flash someone else’s 32 MiB dump onto another board, and do not erase
your own: [what NVS holds](measure.md#what-nvs-holds-never-erase-it).
