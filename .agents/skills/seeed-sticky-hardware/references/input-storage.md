# Buttons, buzzer, MicroSD, USB-C

## Buttons

Three active-low keys, external 10 kΩ to 3.3 V. They are **tactile switches
on the right edge**, not the glass. Locations:
[enclosure.md](enclosure.md)
(vendored Seeed diagram). Default pose: glass facing you, USB-C down.

| Official name | GPIO | Seeed enclosure name | Electrical extra |
| --- | ---: | --- | --- |
| AI | 4 | AI Voice Button (right edge, top) | Shared confirm / power / voice; RTC `ext1` wake (ANY_LOW) |
| Up | 5 | Page Up Button (right edge, middle) | Page key |
| Down | 6 | Page Down Button (right edge, bottom) | Page key |

AI and “OK/power” are the **same** GPIO4. On stock firmware a ~3 s hold of
that key powers the unit on. Debounce times and whether sleep is a GPIO4
hold or a GPIO5+GPIO6 chord are application policy.

Those GPIO names are **firmware claims**; the enclosure names above are
Seeed's diagram. UART prints `btn 4` / `btn 5` / `btn 6`. Attended
sessions have observed `btn 4` / `btn 5` / `btn 6` when the operator
pressed the top / middle / bottom right-edge keys as labeled on the Seeed
diagram. Operator notes from a host learn session stay with that unit’s
original dump, not in this skill.

Recessed **Reset** is a pinhole on the bottom edge (same edge as the
microphone hole, lanyard hole, charge LED, and USB-C). It is a hardware
reset, not these GPIOs. See [enclosure.md](enclosure.md).

## Buzzer

Passive **FUET-5018** on **GPIO48**, driven through a CJ2324. PWM (LEDC)
works; hold low in deep sleep. Resonance / SPL:
[nyc-buzzer-spl](../resources/not-yet-confirmed.md#nyc-buzzer-spl). A ~1 kHz
carrier has been used as a volume-via-duty drive.

## MicroSD (SPI, shared with EPD)

Slot is on the **left** long edge ([enclosure.md](enclosure.md)).

| Signal | GPIO | Notes |
| --- | ---: | --- |
| SCLK / MOSI / MISO | 13 / 14 / 12 | Same as EPD |
| CS | 8 | Idle high |
| Power enable | 10 | Active high |
| Card detect | 11 | 10 kΩ pull-up; insert = **0**; interrupt- and wake-capable |

No 4-bit SDMMC.

Before the panel owns the bus: CS high, EN high, detect as input, ~10 ms
settle. For sleep, hold EN low.

To mount a card:

1. Deselect EPD CS.
2. SD_EN high, wait ≥ 10 ms.
3. SPI init at **≤ 400 kHz**, then raise. On glass, `send_status` ACKed
   at 10 MHz and 20 MHz after that init (embassy-debug `--features sd`).
4. Serialize with display transactions on the one controller.

Card rail is **3.3 V** (`TPS22916CYFPR`). Insert pulls GPIO11 low (empty
slot with the MCU pull-up reads `sd_cd=1`). Max capacity is unpublished.
Factory firmware ACKs the slot and hotplug; flash LittleFS on the factory
image is **not** the SD card.

Detect is more than a level. Stock firmware puts an interrupt handler on
GPIO11, drives a hotplug monitor task from it, and arms the pin as a sleep
wake source. Plan for insert/remove events rather than polling at mount time.

UART learning firmware (GPIO11 pull-up, card not changed) logged `sd_cd=1`
with no edges. That matches an empty slot. embassy-debug `--features sd`
with a card in printed `sd cd=0`, a read-only identify (`type=sdhc`),
then FAT volume 0: root listed and one `ReadOnly` file read (64 bytes
into a scratch buffer, contents not printed). No writes.

## USB-C

On the **bottom** short edge (rightmost item in the appearance diagram).
Power, USB 1.1 CDC to the CH343P, and ROM download. Not ESP32-S3 native
USB-Serial/JTAG — but GPIO19/20 **are** those pads on the silicon, and they
are wired to the PDM microphone. After deep sleep the USB function reclaims
them; see [sensors.md](sensors.md#pdm-microphone). Use a data-capable cable
to flash. Unplugging USB-C drops the host CH343 serial. UART learning
firmware usually loses the `vbus 1 -> 0` line; the host seeing the QinHeng
node drop and return **is** the vbus step. After replug, heartbeats are
`vbus=1` again. CC1/CC2 are 5.1 kΩ Rd to GND: **5 V sink only**, no PD
controller.
