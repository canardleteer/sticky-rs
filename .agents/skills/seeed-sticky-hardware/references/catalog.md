# Official docs and firmware catalog

Seeed’s **Playground** is a website catalog of whole-image firmware for this
product. Flashing an entry **replaces** whatever is on the board. There is no
on-device app sandbox. The public git backing store is the
[Playground registry](https://github.com/Seeed-Projects/reterminal-sticky-playground-registry).

This page is a map, not a pinout. Pins and rails stay in the other reference
files. Do not copy pin numbers from reTerminal **E-series** wiki pages.

## Official documentation

Seeed states these pages are still being written. When they disagree with
observed silicon or other sources, name both sides
([sources.md](sources.md)); the skill user weighs them.

| Page | URL |
| --- | --- |
| Support hub | https://www.seeedstudio.com/sticky/docs/ |
| Quick start | https://www.seeedstudio.com/sticky/docs/en/quick-start/ — appearance diagram vendored as [resources/enclosure/appearance_en.png](../resources/enclosure/appearance_en.png); layout: [enclosure.md](enclosure.md) |
| Release notes | https://www.seeedstudio.com/sticky/docs/en/quick-start/release-notes |
| Hardware overview | https://www.seeedstudio.com/sticky/docs/en/device-guide/hardware-overview/ |
| ESP-IDF basics | https://www.seeedstudio.com/sticky/docs/en/device-guide/esp-basics/ |
| Pages and peripherals | https://www.seeedstudio.com/sticky/docs/en/device-guide/esp-pages/ |
| Display refresh and low power | https://www.seeedstudio.com/sticky/docs/en/device-guide/esp-refresh/ |
| Playground (flash catalog) | https://www.seeedstudio.com/sticky/playground/ |
| Playground: CrossPoint Reader | https://www.seeedstudio.com/sticky/docs/en/playground-docs/crosspoint-reader/ |
| Playground: OpenDisplay | https://www.seeedstudio.com/sticky/docs/en/playground-docs/opendisplay/ — firmware is [OpenDisplay/Firmware](https://github.com/OpenDisplay/Firmware), not the Seeed registry |
| Playground: ESPHome | https://www.seeedstudio.com/sticky/docs/en/playground-docs/esphome/ — YAML generator; driver is [esphome `epaper_spi` SSD1677](https://github.com/esphome/esphome) |
| Playground: TRMNL | https://www.seeedstudio.com/sticky/docs/en/playground-docs/trmnl/ — registry firmware-only → [usetrmnl/trmnl-firmware](https://github.com/usetrmnl/trmnl-firmware) `env:seeed_sticky` |
| Product marketing | https://www.seeedstudio.com/sticky/ |
| Store listing | https://www.seeedstudio.com/reTerminal-Sticky-p-6861.html |

`wiki.seeedstudio.com/reterminal_sticky/` and `/sticky/` have been **404**.
Hardware Overview Resources publish the board schematic
([Rev 01 PDF](https://files.seeedstudio.com/wiki/reterminal_sticky/res/reTerminal_Sticky_Schematic_diagram_260609.pdf),
CC BY-SA 4.0). Cache id `seeed-sticky-schematic` in
[datasheets.md](../resources/datasheets.md). Nets on that PDF are official
electrical evidence. There is **no BOM** in this file.

## Firmware you can actually run

| Firmware | Kind | Notes |
| --- | --- | --- |
| Factory `reterminal_template` | Stock image (measured) | Dual-OTA LittleFS, 160 MHz, Winbond DIO. Vendor’s internal board name is **`reterminal_e1005`** (E1005) — pair that with the app name `reterminal_template` when searching for vendor docs, sources, or firmware. Restore via Playground factory package when published; do not flash another unit’s full-chip dump. [measure.md](measure.md). Official source `Seeed-Projects/OSHW-reTerminal-Sticky` is still **404**; binaries live in the registry. |
| `reTerminal_Sticky_Bunny` | Community ESP-IDF / PlatformIO | [limengdu/reTerminal_Sticky_Bunny](https://github.com/limengdu/reTerminal_Sticky_Bunny). Same `seeed_epaper` OTP SSD1677 as stock (no 0x32). On-glass latch, 10 MHz SPI, display/touch, IMU, sleep. [cpp-platformio.md](cpp-platformio.md) |
| ESPHome `seeed-reterminal-sticky` | YAML / Playground | **Not in the registry.** Playground generates YAML; the driver is ESPHome `epaper_spi` SSD1677 ([PR 16950](https://github.com/esphome/esphome/pull/16950)). B/W only (`EPaperMono`), booster Level 2, `0x22` F7/FF, sleep `0x03`. A 2026.8.2 “everything” generator YAML (Arduino + IDF 5.5.5) adds GT911 `0x5D` (INT 21 / RST 41, 100 kHz), PDM GPIO19/20 EN 38, SD DET/EN only (CS 8 named, unused), BQ27220 read-only templates, PCF8563 + HA write-back, and `ext1` GPIO4 `ANY_LOW` (`run_duration` 30 s). IMU is still a commented external component. `/CE` GPIO39 is inverted and not turned on at boot. Orientation may not match Bunny glass mapping ([nyc-esphome-orient](../resources/not-yet-confirmed.md#nyc-esphome-orient)). [sira-fiinikkusu/reterminal-sticky-voice-companion](https://github.com/sira-fiinikkusu/reterminal-sticky-voice-companion) is household ESPHome on this stack. Wiring: [sensors.md](sensors.md#pdm-microphone). |
| FreeInk `STICKY` / CrossPoint | Compiled board profile | Wiring intent for mic/SHT40/GPIO40; GPIO39 left undriven (prefer over Bunny boot-enable). Default 40 MHz SPI and `n16r8` 16 MB limits are wrong for this flash. MCU gray LUT is **not** factory data. |
| Playground `sticky-2048` | Community ESP-IDF in the registry | [Lukilyy/reterminal-sticky-2048-eink-game](https://github.com/Lukilyy/reterminal-sticky-2048-eink-game). **In-tree source** in the [Seeed Playground registry](https://github.com/Seeed-Projects/reterminal-sticky-playground-registry) at `integrations/sticky-2048/source/`. Buildable `seeed_epaper` OTP reference. Portrait 270° + `mirror_x` is an **app** choice. GPIO7 appears as `PIN_BFG_INT` there. |
| OpenDisplay | Partner firmware (not in registry) | [OpenDisplay/Firmware](https://github.com/OpenDisplay/Firmware), GPL-3.0. Toolbox preset `reterminal-sticky` / panel **GDEM0397T81P** via bb_epaper EP397. OTP, but gray4 uses `0x1A = 0x5A` (one byte) and partial `0x22 = 0xFC`. GT911 default `0x5D` — probe `0x14` first. |
| TRMNL | Partner, registry firmware-only | [usetrmnl/trmnl-firmware](https://github.com/usetrmnl/trmnl-firmware) `env:seeed_sticky`. Same bb_epaper EP397 path as OpenDisplay. Env uses `esp32s3_n16r8` on a **32 MB** part and SPI 8 MHz. Playground docs say erase-from-0 — that destroys factory NVS. |
| Lotus | Community, registry firmware-only | [inkOne/sticky-lotus](https://github.com/inkOne/sticky-lotus). Custom SSD1677 driver (not `seeed_epaper`): OTP, partial `0xFC`, writes 0x21, sleep `0x01` (a no-op for deep sleep). Independent note that activation is panel-wide. |
| Other Playground cards | Partner / community | CrossInk, Sticky Arcade, Followup, etc. Read each project’s pins before copying them. |

Official Seeedash firmware source (`Seeed-Projects/OSHW-reTerminal-Sticky`)
has been referenced while **404** (checked 2026-08-27). The dashboard demo
discussed in Seeed’s ESP-IDF guides is a layered `board/` / `devices/` /
`pages/` tree; use it for C++ structure, not as a substitute for
[pin-map.md](pin-map.md). Do not copy pins from E-series
[OSHW-reTerminal-Series-E-D](https://github.com/Seeed-Projects/OSHW-reTerminal-Series-E-D).

## Native ESP-IDF (no Playground)

Bare skeleton (latch, 32 MB, octal PSRAM, UART0, SPI GPIO0 trap) for reading
vendor C++ trees: [cpp-platformio.md](cpp-platformio.md#bare-esp-idf-skeleton).
This repository does not add an IDF project and does not flash with
`idf.py` / PlatformIO.

## External skill

[varo6/reTerminal-sticky-skill](https://github.com/varo6/reTerminal-sticky-skill)
and the Seeed/registry/2048 sources it distilled:
[external.md](../resources/external.md).
