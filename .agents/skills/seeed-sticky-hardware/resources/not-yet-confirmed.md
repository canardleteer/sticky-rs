# Not yet confirmed

Measurement backlog for this product. **Closed items leave this file**; the
fact goes into the matching `references/` page. Do not grow a confirmed-history
section here.

GPIO7 is also called out on the pin map (do not drive it). Its confirmation
recipe stays in this list so a later probe can iterate one table.

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
| [nyc-gpio7](#nyc-gpio7) | Who owns GPIO7 (IMU INT vs gauge GPOUT) | [pin-map.md](../references/pin-map.md), [sensors.md](../references/sensors.md) |
| [nyc-gpio9-mode](#nyc-gpio9-mode) | GPIO9 divider / ADC capability | [pin-map.md](../references/pin-map.md), [power-and-sleep.md](../references/power-and-sleep.md) |
| [nyc-gpio40-polarity](#nyc-gpio40-polarity) | GPIO40 `CHARGE_STATE` polarity | [power-and-sleep.md](../references/power-and-sleep.md) |
| [nyc-gpio46-pulse](#nyc-gpio46-pulse) | GPIO46 pulse vs hold-high | [power-and-sleep.md](../references/power-and-sleep.md) |
| [nyc-latch-deadline](#nyc-latch-deadline) | Max delay from reset to latch | [power-and-sleep.md](../references/power-and-sleep.md) |
| [nyc-sleep-current](#nyc-sleep-current) | Deep-sleep current | [power-and-sleep.md](../references/power-and-sleep.md) |
| [nyc-gpio43-44](#nyc-gpio43-44) | UART0 is GPIO43/44 to CH343P | [pin-map.md](../references/pin-map.md) |
| [nyc-sd-detect](#nyc-sd-detect) | Card-detect polarity, SD ratings | [input-storage.md](../references/input-storage.md) |
| [nyc-spi-ceiling](#nyc-spi-ceiling) | Shared SPI clock ceiling with SD | [display.md](../references/display.md), [input-storage.md](../references/input-storage.md) |
| [nyc-pcf8563-wake](#nyc-pcf8563-wake) | RTC INT / CLKOUT / backup | [sensors.md](../references/sensors.md) |
| [nyc-gauge-profile](#nyc-gauge-profile) | BQ27220 CEDV values and chemistry on a factory unit | [sensors.md](../references/sensors.md) |
| [nyc-sht40-package](#nyc-sht40-package) | SHT40 package suffix / alert | [sensors.md](../references/sensors.md) |
| [nyc-mic-pdm](#nyc-mic-pdm) | PDM rate, channel, acoustics | [sensors.md](../references/sensors.md) |
| [nyc-buzzer-part](#nyc-buzzer-part) | Buzzer part / resonance | [input-storage.md](../references/input-storage.md) |
| [nyc-panel-glass](#nyc-panel-glass) | Glass part, analog rails, temp LUT | [display.md](../references/display.md) |
| [nyc-gt911-contacts](#nyc-gt911-contacts) | GT911 simultaneous contacts | [touch.md](../references/touch.md) |
| [nyc-usb-pd](#nyc-usb-pd) | USB-C PD vs 5 V charge | [input-storage.md](../references/input-storage.md) |
| [nyc-esphome-orient](#nyc-esphome-orient) | ESPHome `mirror_x` vs on-glass 180° | [display.md](../references/display.md) |
| [nyc-schematic](#nyc-schematic) | Published schematic / BOM / rev | [sources.md](../references/sources.md) |

Software knobs (CPU 160 vs 240 MHz, flash DIO vs QIO, partition tables, SKU
`p-6861` vs `p-6398`) are not listed. Those are firmware choices, not open
nets.

## Recipes

### nyc-gpio7

Seeed Hardware Overview: LSM6DS3TR-C **INT = GPIO7**. Playground `sticky-2048`
`pin_config.h`: **`PIN_BFG_INT`** on GPIO7 (BQ27220 GPOUT). Both chips sit on
sensor I2C. On-glass IMU code has polled I2C and left INT unused.

- Leave GPIO7 as input with pull-up. Never drive it as an output.
- Confirm on hardware: continuity from GPIO7 to IMU INT1/INT2 vs BQ27220 GPOUT
  (or read the schematic net). Optionally enable one interrupt at a time and
  watch edges while tilting vs changing SOC.
- Confirmed when one owner is named and the other is documented as NC or
  shared.

UART learning firmware (input + pull-up, IMU I2C poll) read GPIO7 **low**
with no edges while sitting still **and** during a tilt that changed the IMU
classifier (`FaceUp` → `Landscape0`) and a USB-C unplug/replug. Do not treat
that as an owner.

### nyc-gpio9-mode

Firmware usage is settled: treat GPIO9 as a digital, edge-driven external-power
input ([power-and-sleep.md](../references/power-and-sleep.md)). What remains is
electrical, which no firmware read can answer: FreeInk treats the same net as
an analog `PWR_IN_VOLT`, so there may still be a divider that would let it be
read as a ratio.

- Meter GPIO9 against 3.3 V while connecting and removing USB. A plain rail
  sense reads as a GPIO level; a divider reads mid-scale and tracks VBUS.
- Confirmed with either “no divider, digital only” or a stated divider ratio
  plus the usable ADC range.

UART learning firmware logged `vbus=1` while USB was connected. Unplug was
exercised: the host CH343 dropped and returned (that is the vbus step). The
firmware `vbus 1 -> 0` line is usually lost with the UART. After replug,
heartbeats were `vbus=1` again. That reconfirms GPIO9 as a digital “USB
present” level. It does not close the divider question.

### nyc-gpio40-polarity

Compiled maps wire GPIO40 as BQ25616 charge status. Polarity unmeasured.
ESPHome infers charging from gauge current instead.

- Scope or `gpio_get_level` on GPIO40 with USB in, USB out, and charge
  enable (GPIO39) high vs low. Compare to BQ27220 current.
- Confirmed when “high means …” is stated for charging / done / fault.

UART learning firmware logged `gpio40=1` and gauge `i=0` while USB was
present and `/CE` stayed disabled. USB-C was unplugged and replugged (host
UART dropped); after return, `gpio40=1` and `i=0` still. Charge was not
enabled, so polarity vs charging is still open.

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

### nyc-gpio43-44

UART0 TX/RX are ESP32-S3 defaults. Not independently scoped to the CH343P.

- Continuity or a known UART loopback while logging. Confirmed when 43=TX and
  44=RX to the bridge, or when they are different.

### nyc-sd-detect

GPIO11 card-detect polarity, card voltage, and max capacity are unspecified.
Factory ACK of the slot is not a polarity.

- Insert/remove a card with pull-up on GPIO11; note the level. The pin is
  interrupt-capable, so an edge handler can log the level on both transitions
  in one pass. Try a known size class. Confirmed with “insert = 0 or 1”, plus
  any voltage/capacity limit from schematic or a failed mount.

UART learning firmware (pull-up, card not inserted or removed) logged
`sd_cd=1` with no edges. Polarity on insert is still open.

### nyc-spi-ceiling

SSD1677 serial max is 20 MHz. 10 MHz is stable on the shared bus. 40 MHz is
out of spec. 20 MHz with SD on the same controller is unverified.

- Panel-only then panel+SD at 10, 20 MHz. Confirmed with the highest clock
  that does not corrupt the panel or fail the card.

### nyc-pcf8563-wake

PCF8563 INT, CLKOUT, and backup nets are undocumented. Do not assume RTC
wake of the ESP32.

- Schematic or continuity from PCF8563 INT to an MCU GPIO. Confirmed wired or
  NC. If wired, which GPIO (must not silently steal GPIO7 without updating
  nyc-gpio7).

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

### nyc-sht40-package

Address `0x44` is enough to talk. Package suffix and alert pin unknown.

- Marking on the can, or schematic. Confirmed with orderable PN and alert net
  or NC.

A 1-byte I2C read at `0x44` NAKs; a Sensirion high-precision measure (`0xFD`)
ACKs. That is still not a package suffix.

### nyc-mic-pdm

Pins 19/20/38 work at the HAL-ACK level. Sample rate, channel, and acoustic
orientation unmeasured.

ESPHome firmware that has run on production Stickys
([sira-fiinikkusu/reterminal-sticky-voice-companion](https://github.com/sira-fiinikkusu/reterminal-sticky-voice-companion))
uses PDM RX at **16 kHz**, 16-bit, **left**, clock GPIO19, data GPIO20, and
documents that deep-sleep wake remuxes those pins back to USB-Serial-JTAG
until the USB pad is disabled. That is a working recipe, not this close.

- Record a known tone with ESP32-S3 PDM RX; note rate, slot, and which way
  the hole faces vs the waveform. Confirmed with those three.

### nyc-buzzer-part

GPIO48 PWM works. Part and resonance unpublished. ~1 kHz has been used.

- Marking or schematic, or a frequency sweep for peak SPL. Confirmed with PN
  or a recommended drive frequency.

### nyc-panel-glass

The refresh path is settled: Seeed `seeed_epaper` and stock firmware use
**OTP** sequences (no MCU 0x32 write) over mono film
([display.md](../references/display.md)). Glass part number, analog rail
voltages, and temperature-compensated waveform *sets* (beyond the one gray4
0x1A override) are still unpublished. Do not invent a four-gray MCU table
from a generic SSD1677 sheet.

OpenDisplay Toolbox / TRMNL bb_epaper label the Sticky panel
**GDEM0397T81P** (EP397). That is a partner **claim**, not a BOM or
electrical confirmation. Their gray4 path also uses a one-byte `0x1A = 0x5A`
and data entry `0x11 = 0x01`, which Seeed does not.

The SSD1677 does not read factory OTP back over SPI. Host UART/flash tools
do not talk to the panel. Closing this item is **not** “uncomment FreeInk
after a bench run.”

- Schematic, BOM, or marking for the glass part number and named analog
  rails. Temperature-band which-set stays unpublished until a vendor document
  or that schematic says so.
- Confirmed with glass PN and analog rail names from those sources — still
  not a 105-byte `0x32` table.

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

### nyc-usb-pd

USB-C is power + CDC + download. PD negotiation unpublished.

- USB PD analyzer or charger current at 5 V vs a PD source. Confirmed
  “5 V only” or a PDO list.

### nyc-esphome-orient

ESPHome preset: `mirror_x`, 10 MHz, no packed 180° rotate. Bunny glass:
`mirror_x` + 180° packed rotate.

- Flash the ESPHome e-paper example, draw a known corner. Confirmed whether
  the preset already matches glass or needs the extra rotate.

### nyc-schematic

Hardware Overview lists a schematic PDF. BOM and board revision unpublished.

- Retrieve the PDF (no host-relative paths in this skill). Walk nets for
  GPIO7, 9, 40, 43/44, SD detect, RTC INT, mic EN. Close other NYC items from
  those nets, then drop this recipe or replace it with the public URL.
