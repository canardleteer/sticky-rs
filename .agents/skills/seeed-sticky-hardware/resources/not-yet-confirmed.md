# Not yet confirmed

Measurement backlog for this product. **Closed items leave this file**; the
fact goes into the matching `references/` page. Do not grow a confirmed-history
section here.

Schematic Rev 01 closed GPIO7, GPIO9, GPIO40, GPIO43/44, SD detect, RTC
INT, SHT40 package, buzzer PN, USB-C PD, and the public schematic URL.
Those facts live in the subsystem pages and
[datasheets.md](datasheets.md#verified-against-the-schematic).

Do not record another person’s MAC, serial, NVS, or flash image. UART
geometry: [measure.md](../references/measure.md).

Companion skill and its upstreams: [external.md](external.md).

## How to close an item

1. Run the recipe (or read a schematic net that answers it).
2. Write the result into the **Write the answer** target.
3. Delete the row and the recipe section from this file.

Status is only `open`. If blocked on tools, say so in the recipe, do not add
a second status column.

## Index

| ID | Topic | Write the answer |
| --- | --- | --- |
| [nyc-gpio46-pulse](#nyc-gpio46-pulse) | GPIO46 pulse vs hold-high | [power-and-sleep.md](../references/power-and-sleep.md) |
| [nyc-latch-deadline](#nyc-latch-deadline) | Max delay from reset to latch | [power-and-sleep.md](../references/power-and-sleep.md) |
| [nyc-sleep-current](#nyc-sleep-current) | Deep-sleep current | [power-and-sleep.md](../references/power-and-sleep.md) |
| [nyc-gauge-profile](#nyc-gauge-profile) | BQ27220 CEDV values and chemistry on a factory unit | [sensors.md](../references/sensors.md) |
| [nyc-mic-pdm](#nyc-mic-pdm) | PDM high-fidelity rate / slot / hole | [sensors.md](../references/sensors.md) |
| [nyc-panel-glass](#nyc-panel-glass) | Official glass PN (candidate sheet on file) | [display.md](../references/display.md) |
| [nyc-esphome-orient](#nyc-esphome-orient) | ESPHome `mirror_x` vs on-glass 180° | [display.md](../references/display.md) |
| [nyc-charge-stat](#nyc-charge-stat) | Charge-to-done and gauge current scale | [power-and-sleep.md](../references/power-and-sleep.md) |
| [nyc-gpio7-edge](#nyc-gpio7-edge) | GPIO7 edges with IMU or gauge armed | [sensors.md](../references/sensors.md) |
| [nyc-buzzer-spl](#nyc-buzzer-spl) | Buzzer resonance / SPL | [input-storage.md](../references/input-storage.md) |

Software knobs (CPU 160 vs 240 MHz, flash DIO vs QIO, partition tables, SKU
`p-6861` vs `p-6398`, simple-debug still on the INT-high GT911 dance)
are not listed. Those are firmware choices, not open nets. Do not add a
row that asks anyone to invent the deleted GT911 register map.

## Recipes

### nyc-gpio46-pulse

This skill and ESPHome/Bunny: **GPIO45 and GPIO46 stay high**. Some write-ups
pulse GPIO46 (low→high→low) after raising GPIO45.

- On battery, with USB unplugged after boot: compare hold-both-high vs the
  pulse sequence. Does the rail drop with either recipe?
- Confirmed when pulse is either equivalent, required, or harmful.

### nyc-latch-deadline

Maximum legal time from chip reset to asserting the latch is unpublished.

- Cold boot on battery, vary delay before driving 45/46 high, note the last
  delay that still stays powered.
- Confirmed with a measured upper bound (and that bring-up still does it
  first).

### nyc-sleep-current

Deep-sleep current with latch held and peripheral rails off is unmeasured.

- Current meter in the battery path (not USB) after the sleep rail table in
  [power-and-sleep.md](../references/power-and-sleep.md).
- Confirmed with a number and the rail recipe used.

### nyc-gauge-profile

Family and mechanism are settled: **CEDV**, with stock firmware maintaining
Full Charge Capacity through the CFGUPDATE path and persisting it off-chip
([sensors.md](../references/sensors.md)). What remains is the pack profile's
actual content on a factory unit.

- Read the CEDV core and thresholds plus design capacity from a factory board,
  and check whether SOC tracks a known discharge without any writes.
- Confirmed with the shipped values (or “OTP is empty and the app supplies
  them”) and the cell chemistry ID. Do not enter CFGUPDATE to answer this.

UART learning firmware read DeviceType `0x0220` plus standard-command V/I/SoC
while sitting on USB (`/CE` off). CEDV data-memory was not read (the in-repo
driver does not implement that block).

### nyc-mic-pdm

Pins 19/20/38 work on glass (embassy-debug `--features mic`). Energy
jumps on a whistle. AI Voice’s 1 kHz buzzer shows a ~16-sample period
in the PCM dump (1 kHz if the clock is 16 kHz); left slot hears it.
That is **not** high-fidelity confirmation. Facts:
[sensors.md](../references/sensors.md#on-glass-embassy-debug-mic-feature).
Schematic Rev 01 names **MSM261DDB020** and **TPS22916CYFPR** on
`PDM_EN`.

ESPHome firmware that has run on production Stickys
([sira-fiinikkusu/reterminal-sticky-voice-companion](https://github.com/sira-fiinikkusu/reterminal-sticky-voice-companion))
uses the same 16 kHz / left / GPIO19/20 recipe and documents that
deep-sleep wake remuxes those pins back to USB-Serial-JTAG until the
USB pad is disabled.

Still open: a clean known-tone through the hole (not board coupling to
the buzzer), slot A/B, hole vs waveform polarity, rail settle time
after GPIO38 rises, and USB-Serial-JTAG pad reclaim after deep-sleep
wake (third-party notes say disable the pad before PDM RX).

- Record a known tone with ESP32-S3 PDM RX; note rate, slot, and which
  way the hole faces vs the waveform. Time GPIO38 high to first valid
  window. After a deep-sleep wake, confirm whether PDM works only once
  the USB pad is disabled. Confirmed with those at high fidelity.

### nyc-panel-glass

The refresh path is settled: Seeed `seeed_epaper` and stock firmware use
**OTP** sequences (no MCU 0x32 write) over mono film
([display.md](../references/display.md)). Schematic Rev 01 names analog
rails (VGH / VGL / VSH1 / VSH2 / VCOM / VPP / `EP_3V3`) but **no glass
PN**. Temperature-compensated waveform *sets* (beyond the one gray4
0x1A override) are still unpublished. Do not invent a four-gray MCU table
from a generic SSD1677 sheet.

**Hint (not a PN):** third-party sources converge on the Good Display
3.97" 800×480 SSD1677 T81 / T81P family. Use those strings to search a
marking or sheet. Do not treat any of them as this board's BOM line.

- OpenDisplay Toolbox / TRMNL bb_epaper label the Sticky panel
  **GDEM0397T81P** (EP397). Partner firmware claim. Their gray4 path
  uses a one-byte `0x1A = 0x5A` and data entry `0x11 = 0x01`, which
  Seeed does not.
- Waveshare's 3.97" e-Paper HAT+ raw panel publishes the same
  mechanical numbers as that family (86.40 × 51.84 mm active, 96.62 ×
  56.24 × 0.92 mm outline, 0.108 mm pitch, 24-pin 0.5 mm FPC). The
  wiki does not print a Good Display SKU. Published refresh times
  (2.8 / 0.6 / 3.5 s, 0–40 °C) do not match Good Display's
  GDEY0397T81P sheet (1.5 / 0.3 / 3 s, 0–50 °C).
- Arduino [GxEPD2](https://github.com/ZinggJM/GxEPD2) class
  `GxEPD2_397_GDEM0397T81` is written for "SPI e-paper panels from
  Dalian Good Display and boards from Waveshare." The header names
  panel **GDEM0397T81** (no `P`) and links Good Display product
  [613](https://www.good-display.com/product/613.html), which today
  lists **GDEY0397T81P**. The driver is based on a Good Display demo,
  not a Seeed BOM. Picker comment: `FPC-7750`.

`GDEM` vs `GDEY` and the `P` suffix are different Good Display SKUs
(film / OTP / thickness). Do not collapse them.

**Datasheet (candidate, keep on file):** Good Display
**GDEY0397T81P** Rev 1.0 (2026-08-13). Not a confirmed Sticky part.
Do not add it to [datasheets.md](datasheets.md) until a marking or BOM
names this SKU.

- Product page: good-display.com/product/613.html
- CDN PDF (bare curl may 403; a browser `User-Agent` worked):
  `https://v4.cecdn.yun300.cn/100001_1909185148/GDEY0397T81P.pdf`
- Local cache (gitignored):
  `resources/datasheets/pdf/gdey0397t81p.pdf` and `md/gdey0397t81p.md`.
  If missing, copy the skill user's `~/Downloads/GDEY0397T81P.pdf`.
- SHA-256
  `e28ea298457108bb3431b8c4de065834f62390b6caf2dc58e216d134fe559dbc`

Desk check against schematic Rev 01
`J3` (24P top-contact): pins 2–4 / 8–24 match the sheet (GDR, RESE,
BS1, BUSY, RESET#, DC#, CS#, SCL, SDA, VCI, VSS, VDD, VPP, VSH1,
VGH, VSL, VGL, VCOM). Seeed names pin 5 **VSH2**; the sheet names it
**VDHR** (red-source leftover). That is the SSD1677 second source
rail, not a different connector. Sheet pin 19 is **VPP FOR TEST**;
Seeed brings it out to `TP17`. Pins 6–7 are NC on the sheet; Seeed
has `R66` 10 kΩ there.

Do **not** ask to inspect the FPC tail. It is not visible on an
assembled unit and will not be. Do **not** treat wall-clock BUSY as
a part-number proof. Do **not** send this sheet's MCU LUT, `0x32`,
or `0x03` / `0x04` / `0x2C` analog bytes. Command-table pages in
the PDF are images; do not invent opcodes from them. SSD1677 `0x2E`
can read a 10-byte User ID; that is not the waveform table and is
not a close.

The SSD1677 does not read the factory **waveform** table back over
SPI. Host UART/flash tools do not talk to the panel. Closing this
item is **not** “uncomment FreeInk after a bench run.”

Keep the candidate sheet on file. Close only with **official**
evidence that names this board's glass SKU (Seeed BOM, a schematic
that prints the PN, or a vendor doc for this product). Third-party
labels stay hints. Temperature-band which-set stays unpublished
until that same class of document says so. Still not a 105-byte
`0x32` table.

### nyc-esphome-orient

ESPHome preset: `mirror_x`, 10 MHz, no packed 180° rotate. Bunny glass:
`mirror_x` + 180° packed rotate.

- Flash the ESPHome e-paper example, draw a known corner. Confirmed whether
  the preset already matches glass or needs the extra rotate.

### nyc-charge-stat

Schematic: GPIO40 is BQ25616 STAT. UART read **high** with `/CE` parked
and gauge `i=0`. That is not a charge proof. Default in-repo images
park `/CE`. embassy-debug `--features charge` on USB (2026-08-30,
settle after disable): STAT followed enable and park
(`gpio40=1` → `0` → `1`). Gauge `i=` went `0` → `5702` while
enabled (not a 555 mA charge-set number). First sit left STAT low
and the charger LED green/yellow.

- Charge-to-done and a credible current scale are still open. Do
  not leave `/CE` enabled in default images.

### nyc-gpio7-edge

Schematic: GPIO7 is IMU INT1 and gauge GPOUT. Polled IMU on glass
changed pose with **no** GPIO7 edges; interrupts were not armed. Leave
the pin an input. Do not enable both chips as push-pull.

- Arm **one** chip as open-drain. Tilt (IMU) or finish a charge (gauge
  GPOUT — only with [nyc-charge-stat](#nyc-charge-stat) parked after).
  Confirmed when GPIO7 edges match that source.

### nyc-buzzer-spl

GPIO48 PWM beeps on glass. Embassy-debug `--features mic` already
heard the 1 kHz AI Voice tone on the PDM path (left slot, ~16-sample
period). That is enclosure / EMI coupling, not a calibrated SPL.

- Sweep a known PWM on GPIO48 and record on-board `mic rms=` /
  `peak=` (or a PCM dump) vs frequency. Confirmed with a relative
  resonance (which Hz peaked). Absolute dB SPL stays unmeasured
  unless someone later meters. Not a destroy-the-board row.
