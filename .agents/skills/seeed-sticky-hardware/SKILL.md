---
name: seeed-sticky-hardware
description: >-
  Use when writing or reviewing firmware or software for the Seeed Studio
  reTerminal Sticky (ESP32-S3R8, 800x480 SSD1677 e-paper, GT911, CH343P UART),
  including GPIO and bus maps, power-latch bring-up, display/touch/IMU/battery
  wiring, shared SPI, deep sleep, flashing geometry, destroy-the-board
  hazards, or when sources disagree about this board. Vendor datasheet
  citations use the skill catalog and a gitignored local PDF/markdown
  cache; populate that cache when the work is registers, opcodes, or
  timings. Also use when the user mentions Sticky,
  reTerminal Sticky, seeed-sticky, or vendor C++ / PlatformIO / Playground
  firmware as evidence of how this board is wired.
---

# Seeed Sticky Hardware

Board contract for the Seeed Studio reTerminal Sticky. Read this file first.
Hardware facts live in the subsystem pages. Do not mix a stack’s APIs into
the pin map.

Host discovery and flash I/O belong to the consuming project’s tools, not
this skill. Vendor C++ / PlatformIO trees are wiring evidence, not a flash
path here.

## How to read this skill

1. **Authority** — [references/sources.md](references/sources.md). Precedence
   and the conflict inventory. The skill user weighs disagreements.
2. **Hazards** — [references/safety.md](references/safety.md). Destroy the
   board or irreplaceable data. A consuming repo may symlink this page as
   `docs/SAFETY.md`.
3. **Observed silicon** — [references/measure.md](references/measure.md). Chip,
   flash, USB UART, factory image, and which peripherals ACK **on a Sticky
   in hand**. That beats SDK/DevKit profiles when they disagree on those
   fields.
4. **Enclosure** — [references/enclosure.md](references/enclosure.md). Where
   keys, holes, USB-C, and the SD slot sit. Vendored Seeed diagram:
   [resources/enclosure/appearance_en.png](resources/enclosure/appearance_en.png).
   The glass is the panel, not a key.
5. **Pin map and rails** — remaining hardware pages (not available from ROM
   `board-info`; they come from firmware that has run on this product).
6. **Official docs and firmware catalog** —
   [references/catalog.md](references/catalog.md).
7. **Vendor datasheets** — [resources/datasheets.md](resources/datasheets.md).
   Registers, opcodes, timings for parts confirmed on this model. **Vendor
   the local cache** when that work needs a sheet (see
   [Vendor datasheets](#vendor-datasheets-local-cache)).
8. **Vendor C++ evidence** —
   [references/cpp-platformio.md](references/cpp-platformio.md). Sequences
   from ESP-IDF / PlatformIO trees that ran on a physical unit
   (third-party unless the vendor published them).
9. **Measurement backlog** — remaining open nets and confirmation recipes
   live in
   [resources/not-yet-confirmed.md](resources/not-yet-confirmed.md).
10. **External skills and sources** — including
   [varo6/reTerminal-sticky-skill](https://github.com/varo6/reTerminal-sticky-skill)
   and ESPHome audio confirmed on a physical unit in
   [sira-fiinikkusu/reterminal-sticky-voice-companion](https://github.com/sira-fiinikkusu/reterminal-sticky-voice-companion)
   — [resources/external.md](resources/external.md).

Do not mix a stack’s APIs into the pin map. Do not commit another person’s
MAC, serial number, USB serial string, NVS, or flash image.

Write evidence as **confirmed on a physical unit**, **on a physical
device**, or **not measured**. Do not write *on glass* or *close on
glass* for that. *Glass* is the front panel, not a synonym for the unit.

## Authority

When sources disagree, name both sides and their layers. Do not flatten a
conflict to one number. The skill user is authoritative: they decide how to
weigh the facts. This skill presents the stack; it does not silently pick a
winner against the user.

**Precedence (highest first):**

1. **The skill user.** They resolve the conflict. Ask when a choice would
   change wiring, flash, or a hazardous write.
2. **Observed hardware** on this product, with batch variation allowed. Live
   UART, `flash-id`, ACKs, meter/schematic, and on-unit partition/USB facts
   ([measure.md](references/measure.md)). Name the unit class or batch when a
   fact is not known to be universal. Pin maps remain firmware-derived (code
   that has run on this product), not ROM `board-info`.
3. **Official** board documentation, vendor SDKs, and **chip datasheets for
   parts confirmed on this model.** Registers, opcodes, and timings belong
   here when they have not been measured on a physical unit. Official stock/SDK
   sequences still prove **intent and ordering, never electrical fact**.
   That stock firmware treats a net as a digital interrupt does not prove
   there is no divider on it. Do not apply a datasheet to a part that is
   not confirmed on this model.
4. **Third-party** firmware, Playground apps, community skills, ESPHome,
   FreeInk profiles. Often first to carry new valid detail; also the usual
   source of stale or wrong maps. When FreeInk and Bunny disagree on
   charger GPIO, prefer FreeInk unless a physical unit says otherwise.

An observed address or pin (2) outranks a datasheet default (3): both
GT911 7-bit addresses ACK here depending on INT at RST (Rev.09 §6.1:
INT=0 → `0x5D` delivered contacts; INT=1 → `0x14` ACK, `st=0x00` on
an init Status-clear path). Datasheets outrank a random SSD1677
example (4) for opcodes on the confirmed panel controller.

Inventory: [sources.md](references/sources.md). New mismatches get a row
there or a recipe in
[not-yet-confirmed.md](resources/not-yet-confirmed.md). Speak up when a
page, crate, or user issue sits on a known conflict.

Do not invent GPIOs. Do not use a generic ESP32-S3 DevKit pinout. Do not
invent registers. If the local datasheet cache is missing, ask the user to
populate it rather than guessing.

## Product snapshot

| Item | Value |
| --- | --- |
| Product | Seeed reTerminal Sticky |
| MCU | ESP32-S3R8, rev v0.2, QFN56, 40 MHz crystal (confirmed `esptool flash-id`) |
| RAM | Internal SRAM + **8 MB in-package octal PSRAM** at 3.3 V (confirmed `esptool flash-id`, `AP_3v3`) |
| Flash | **32 MB** external quad SPI, Winbond W25Q256-class (`ef 4019`; eFuse quad, 3.3 V) |
| Display | 3.97" 800×480, 235 ppi **mono** E-Ink film; 4-gray is synthesized (dual plane + panel OTP), **SSD1677**-compatible SPI |
| Touch | **GT911** on its own I2C; sensor reports **480×800** (portrait); map that sample onto 800×480 (`to_screen`); **5** simultaneous contacts on this FPC (Rev.09 §1). INT low at RST → `0x5D` (Rev.09 §6.1; [touch.md](references/touch.md#on-a-physical-unit-embassy-debug)) |
| USB debug | WCH **CH343P** on UART0 (`1a86:55d3`), not native USB-Serial/JTAG; udev by-id uses `_` before the USB serial |
| Battery | 750 mAh 1S Li-ion, **BQ27220** gauge, **BQ25616** charger. STAT (GPIO40) low while `/CE` enabled, high after park + settle ([power-and-sleep.md](references/power-and-sleep.md)). Default images park `/CE` |
| Audio | PDM MEMS **MSM261DDB020** (GPIO19/20, EN 38 / TPS22916; hole on bottom edge); **no loudspeaker** (FUET-5018 on GPIO48). On a physical unit: 16 kHz / left energy is live; GPIO48 1 kHz dump shows a ~16-sample period; a phone tone through the hole shows ~36–40 samples in a buzzer-off dump. Not high-fidelity ([sensors.md](references/sensors.md#pdm-microphone)) |
| Radio | On-board **ANT1**, shared 2.4 GHz Wi-Fi / BLE. On a physical unit: embassy-debug `--features radio` printed `wifi n=` and `ble n=` in one listen ([pin-map.md](references/pin-map.md#on-a-physical-unit-embassy-debug-radio-feature)) |
| Enclosure | 106 × 65.5 × 7.3 mm, 70 g, IP40, glass front, N52 corner magnets. Keys on the **right** edge (AI Voice / Page Up / Page Down); SD on the **left**; Reset, mic, lanyard, charge LED, USB-C on the **bottom**. [enclosure.md](references/enclosure.md) |

Xtensa target when using Rust: `xtensa-esp32s3-none-elf` (`no_std`) or the
ESP-IDF Rust target (`std`). **No probe-rs** on the USB-C connector.

## Hard rules (all stacks)

1. **Latch power first.** Drive GPIO45 `PWR_HOLD` high, then GPIO46 `PWR_LOCK`
   high, before logging or bus init. If they stay low, the board dies when USB
   is unplugged. Releasing the latch is a deliberate power-off, not a fault:
   stock firmware latches first and then *releases* when the power button was
   not the boot cause. Do not copy that policy by accident, or a USB-powered
   boot will shut down on you.
2. **GPIO0, GPIO3, GPIO45, and GPIO46 are strapping pins** (ESP32-S3
   datasheet v2.2 §3). GPIO0 is sensor I2C SCL. GPIO3 is GT911 SDA (floating
   at reset; do not wiggle it until `tH` ≥ 3 ms after `CHIP_PU`). GPIO45
   (`PWR_HOLD`) and GPIO46 (`PWR_LOCK`) default to weak pull-down — drive
   them high; do not treat GPIO46 as a general-purpose toggle.
3. **Display and MicroSD share one SPI controller** (SCLK 13, MOSI 14,
   MISO 12) with separate CS. Clock the panel at **10 MHz, SPI mode 0**.
   Never assign GPIO0 to that SPI bus.
4. **Touch is portrait on a landscape panel.** The GT911 sample is
   **480×800**. Map that onto 800×480, then account for the 180°
   framebuffer rotation. Do not scale `cx` as if the range were 800.
   Rev.09 §6.1: INT low at RST → **`0x5D`** (contacts on this FPC);
   INT high → **`0x14`**. After address select, leave GPIO21 (INT)
   floating: the ESP32-S3 pad has no default pull. Mux GPIO41/42 off
   JTAG before RST / `TOUCH_EN`.
5. **Log and flash on UART0 through the CH343P** (monitor 115200). QinHeng
   `1a86:55d3` is not an Espressif VID. Consuming host tools pick that UART.
   Do not treat USB-C as native USB-Serial/JTAG or `probe-rs`.
6. **Ship a 32 MB-aware partition table.** Do not inherit 16 MB DevKit limits.
7. **Never erase this flash.** The factory NVS at `0x9000` holds that unit's
   Wi-Fi RF calibration, device identity, and persisted gauge state. None of it
   is regenerable, and the only restore is a full-chip image of **that same
   unit**. No `erase-flash`, no full-chip erase, and no writes below `0x90000`
   on a board you care about. Custom images belong in factory `app0` only.

## Vendor datasheets (local cache)

Catalog: [resources/datasheets.md](resources/datasheets.md). Cached PDFs and
extracted markdown live in [resources/datasheets/](resources/datasheets/README.md)
(`pdf/`, `md/`; gitignored).

**Vendor that cache** when the work is registers, opcodes, timings,
strapping/I2C/SPI limits, SSD1677 command tables, or a datasheet-versus-unit
conflict. Search `resources/datasheets/md/<id>.md` rather than loading a
whole TRM. The cache does **not** replace the pin map, enclosure, or a
Playground app layout.

It does not help for board wiring you already have from observed firmware,
or for third-party project structure. For **official Seeed / Playground HTML
docs** as an offline markdown corpus, use the user-global
`skill-corpus-vendoring` skill — do not invent a second datasheet pipeline.

When citing a register, opcode, or timing:

1. Read the catalog (gaps and already-verified SSD1677 facts live there).
2. If `resources/datasheets/md/<id>.md` exists, search that file.
3. If the markdown (or PDF) is missing, **ask the user to populate the
   cache** before inventing a constant. Do not download vendor files unless
   they asked.

```shell
# from this skill directory
python3 scripts/fetch_datasheets.py status
# only if the user asked to populate the cache:
python3 scripts/fetch_datasheets.py fetch
```

`status` is local-only. Some vendor portals need a browser save into `pdf/`
and then `fetch_datasheets.py convert`. SHA-256 of the cached files is
committed in [resources/datasheets.sha256](resources/datasheets.sha256) for
later IPFS CIDv1.

## Bring-up order (hardware)

1. GPIO45 then GPIO46 high; ~100 ms settle. On deep-sleep wake, restore those
   outputs **before** releasing RTC/GPIO holds.
2. Charger: GPIO39 **low**. GPIO9 high means external power present (digital,
   edge-capable: stock firmware runs it as an any-edge interrupt). The net
   is a 5.1 kΩ / 5.1 kΩ divider from `VIN_5V` (`PWR_IN_VOLT`); 2.5 V at
   5 V VBUS still reads high.
3. Park MicroSD (CS high, power enable high, detect as input).
4. Sensor I2C at 400 kHz: SDA=1, SCL=0. PCF8563 `0x51`, BQ27220 `0x55`,
   SHT40 `0x44` (a Sensirion measure command; a 1-byte read NAKs),
   LSM6DS3TR-C `0x6A`.
5. EPD rail GPIO47 high, ~100 ms; SPI 10 MHz; SSD1677. Cold boot: full white
   clear.
6. GT911 rail GPIO42 high (schematic); I2C SDA=3 SCL=2 at ≤400 kHz
   (Rev.09 §6.1); INT-during-reset address select (INT low first);
   no init status-clear; poll Status then Points. Contacts here:
   INT=0 → `0x5D`.
7. Buzzer GPIO48 PWM; IMU; then app storage.

Factory firmware also ACKs the PDM microphone and SD slot.

## Subsystem map

| Question | Read |
| --- | --- |
| What can destroy the board or irreplaceable data | [references/safety.md](references/safety.md) |
| How to read chip, flash, USB, factory image on your unit | [references/measure.md](references/measure.md) |
| Where keys, holes, USB-C, and the SD slot sit | [references/enclosure.md](references/enclosure.md) |
| GPIO, I2C, SPI, part numbers | [references/pin-map.md](references/pin-map.md) |
| Latch, charger, deep-sleep rails | [references/power-and-sleep.md](references/power-and-sleep.md) |
| Panel, orientation, framebuffer | [references/display.md](references/display.md) |
| GT911 address and coordinate map | [references/touch.md](references/touch.md) |
| IMU axes, RTC, gauge, SHT40, mic | [references/sensors.md](references/sensors.md) |
| Buttons, buzzer, SD, USB-C | [references/input-storage.md](references/input-storage.md) |
| UART, flash geometry, PSRAM | [references/flashing.md](references/flashing.md) |
| Rust stacks (not a host toolchain) | [references/rust.md](references/rust.md) |
| Vendor C++ / PlatformIO sequences (wiring evidence) | [references/cpp-platformio.md](references/cpp-platformio.md) |
| Official URLs, Playground, firmware list | [references/catalog.md](references/catalog.md) |
| Vendor datasheets (catalog; local PDF/markdown cache) | [resources/datasheets.md](resources/datasheets.md) |
| Conflicts and citations | [references/sources.md](references/sources.md) |
| Measurement backlog | [resources/not-yet-confirmed.md](resources/not-yet-confirmed.md) |
| varo6 skill, voice-companion audio, distilled sources | [resources/external.md](resources/external.md) |

## Silicon defaults

- **PSRAM:** **8 MB octal** on hardware (`esptool flash-id`, `AP_3v3`).
  ~96 KiB gray4 frames fit there; a 48 KiB 1-bit frame can stay in internal
  RAM. 80 MHz is a firmware config, not an eFuse field.
- **CPU:** factory image has run at **160 MHz**. Community firmware has used
  240 MHz. Either is a software choice; `esptool flash-id` listing 240 MHz is
  chip capability, not the factory app clock. Read `cpu freq` from UART on the unit.
- **Flash:** eFuse **quad**, eFuse voltage **3.3 V** (`esptool`). Factory
  runtime has logged **DIO**. QIO is a software choice, not an eFuse fact.
- **Canvas:** 800×480, top-left origin, 2-bit packed (4 pixels/byte, MSB-first,
  Black=0 … White=3). Transmit a 180°-rotated copy with `mirror_x`.
- **Wake:** GPIO4 `ext1` ANY_LOW. Touch cannot wake if the GT911 rail is off.
- **Strapping (v2.2 §3):** GPIO0 (WPU), GPIO3 (floating, touch SDA), GPIO45
  (WPD, `PWR_HOLD`), GPIO46 (WPD, `PWR_LOCK`). Latched at chip reset; pins
  are ordinary IO after `tH` ≥ 3 ms.

## Do not

- Treat USB-C as CMSIS-DAP / JTAG / RTT, or log on native USB CDC.
- Leave the ESP32-S3 USB-Serial-JTAG pad enabled on GPIO19/20 if the PDM
  microphone must work after deep sleep ([sensors.md](references/sensors.md#pdm-microphone)).
- Use native USB-Serial/JTAG or `probe-rs` on USB-C while GPIO19/20 are
  PDM. Debug stays on the CH343 (UART0). Enabling the mic rail and
  running PDM RX is not a [safety.md](references/safety.md)
  destroy-the-board row; mute energy is a failed experiment.
- Leave GPIO38 floating across deep sleep (the load switch / capsule can
  sit half-powered). Hold it low when unused.
- Copy PDM clock/data from reTerminal **E-series** wiki pages (GPIO42/41).
- Assume one GT911 7-bit address without the INT-during-reset sequence.
  Both `0x14` and `0x5D` ACK depending on INT; contacts here came from
  INT=0 → `0x5D`.
- Leave GPIO41 (`MTDI`) / GPIO42 (`MTMS`) on their default JTAG pad functions
  when using them as GT911 RST / `TOUCH_EN`. Mux to GPIO. Same class of
  caution as GPIO19/20 USB pads (v2.2 §2.3.4).
- Enable an MCU pull-up on GPIO21 (GT911 INT) by default: Table 2-1 has no
  reset pull. On a physical unit, leave INT floating after address select.
- Overlap display and SD SPI transactions.
- Invent a four-gray LUT from a generic SSD1677 example, or mix an MCU
  0x32 table with Sticky OTP gray4.
- Use crate `bq27xxx` (wrong gauge family: Impedance Track BQ27426/427;
  this board has a CEDV BQ27220).
- Run `erase-flash` / full-chip erase, or write below `0x90000` (hard rule 7).
- Invent registers or opcodes when the vendor PDF is unread. If the local
  datasheet cache is missing, ask the user to populate it.
- Treat a C++ file layout or PIO env as hardware, or treat `esp-hal` as the
  only legal Rust stack.
- Drive GPIO7 as an output, or assume it is free GPIO. Schematic: IMU INT1
  and gauge GPOUT share it. Leave it an input.
- Treat a key as being on the glass. The three tactile buttons are the right
  edge ([enclosure.md](references/enclosure.md)). Recessed Reset is the bottom
  pinhole, not GPIO4.
