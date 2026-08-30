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
| [nyc-spi-ceiling](#nyc-spi-ceiling) | Shared SPI clock ceiling with SD | [display.md](../references/display.md), [input-storage.md](../references/input-storage.md) |
| [nyc-gauge-profile](#nyc-gauge-profile) | BQ27220 CEDV values and chemistry on a factory unit | [sensors.md](../references/sensors.md) |
| [nyc-mic-pdm](#nyc-mic-pdm) | PDM high-fidelity rate / slot / hole | [sensors.md](../references/sensors.md) |
| [nyc-panel-glass](#nyc-panel-glass) | Glass part, analog rails, temp LUT | [display.md](../references/display.md) |
| [nyc-gt911-contacts](#nyc-gt911-contacts) | GT911 simultaneous contacts | [touch.md](../references/touch.md) |
| [nyc-esphome-orient](#nyc-esphome-orient) | ESPHome `mirror_x` vs on-glass 180° | [display.md](../references/display.md) |

Software knobs (CPU 160 vs 240 MHz, flash DIO vs QIO, partition tables, SKU
`p-6861` vs `p-6398`) are not listed. Those are firmware choices, not open
nets.

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

### nyc-spi-ceiling

SSD1677 serial max is 20 MHz. 10 MHz is stable on the shared bus. 40 MHz is
out of spec. 20 MHz with SD on the same controller is unverified.

- Panel-only then panel+SD at 10, 20 MHz. Confirmed with the highest clock
  that does not corrupt the panel or fail the card.

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
the buzzer), slot A/B, and hole vs waveform polarity.

- Record a known tone with ESP32-S3 PDM RX; note rate, slot, and which
  way the hole faces vs the waveform. Confirmed with those three at
  high fidelity.

### nyc-panel-glass

The refresh path is settled: Seeed `seeed_epaper` and stock firmware use
**OTP** sequences (no MCU 0x32 write) over mono film
([display.md](../references/display.md)). Schematic Rev 01 names analog
rails (VGH / VGL / VSH1 / VSH2 / VCOM / VPP / `EP_3V3`) but **no glass
PN**. Temperature-compensated waveform *sets* (beyond the one gray4
0x1A override) are still unpublished. Do not invent a four-gray MCU table
from a generic SSD1677 sheet.

OpenDisplay Toolbox / TRMNL bb_epaper label the Sticky panel
**GDEM0397T81P** (EP397). That is a partner **claim**, not a BOM or
electrical confirmation. Their gray4 path also uses a one-byte `0x1A = 0x5A`
and data entry `0x11 = 0x01`, which Seeed does not.

The SSD1677 does not read factory OTP back over SPI. Host UART/flash tools
do not talk to the panel. Closing this item is **not** “uncomment FreeInk
after a bench run.”

- Schematic, BOM, or marking for the glass part number. Temperature-band
  which-set stays unpublished until a vendor document or that marking
  says so.
- Confirmed with glass PN from those sources — still not a 105-byte
  `0x32` table.

### nyc-gt911-contacts

Controller silicon reports **up to 5** concurrent touches (Goodix GT911
Rev.09 §1). How many this FPC actually delivers is unmeasured.

- Multi-touch on glass, read point count from the GT911. Confirmed with max
  contacts the FPC actually delivers (≤ 5).

UART learning firmware ACKed `0x14` after INT-during-reset and a product-ID
read (`id=911`). Later attended polls still saw `gt911 st=0x00` every second
and INT stuck high while the MCU enabled an internal pull-up on GPIO21.
ESP32-S3 v2.2 Table 2-1 gives that pad **no** default pull; on-glass leaves
INT floating. That software miss is not proof the FPC has no touch. Max
simultaneous contacts still needs a poll path that prints `contacts=` on
this image.

### nyc-esphome-orient

ESPHome preset: `mirror_x`, 10 MHz, no packed 180° rotate. Bunny glass:
`mirror_x` + 180° packed rotate.

- Flash the ESPHome e-paper example, draw a known corner. Confirmed whether
  the preset already matches glass or needs the extra rotate.
