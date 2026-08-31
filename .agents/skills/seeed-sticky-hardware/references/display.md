# Display (SSD1677, 800×480, 4-gray)

## Panel

| Item | Value |
| --- | --- |
| Size | 3.97 inch, 235 ppi |
| Native resolution | **800 × 480** landscape |
| Film | **Mono** (black/white) E-Ink |
| Colors | Black/white, plus **4-level grayscale synthesized** by dual-plane writes with the panel OTP |
| Controller | SSD1677-compatible SPI |
| Power | GPIO47 `EPD_EN`, active high, ~100 ms settle |
| SPI | Shared bus, **10 MHz**, mode 0, CS GPIO15 |
| D/C | GPIO16 |
| RST | GPIO17, 10 ms low / 10 ms high works |
| BUSY | GPIO18, busy = **1** (timeouts of several seconds are normal). Edge-capable: stock firmware waits on a BUSY **interrupt**, not a spin loop |
| Frontlight | None |

SSD1677 serial clock max is 20 MHz. Panel refresh at **10 MHz and
20 MHz** is clean on glass (card inserted). Read-only card identify
inits at 400 kHz, then `send_status` ACKs at 10 MHz and 20 MHz
(embassy-debug `--features sd`, 2026-08-30). Board `SPI_MAX_HZ` stays
10 MHz. Do not start at 40 MHz. FAT list and a ReadOnly file read
are on glass (embassy-debug `--features sd`); see
[input-storage.md](input-storage.md).

## Bring-up

1. Deselect MicroSD (CS high). Leave SD power parked (see
   [input-storage.md](input-storage.md)).
2. `EPD_EN` = 1, wait ~100 ms.
3. SPI with only SCLK/MOSI/MISO/CS as listed. Do not attach GPIO0.
4. Reset, then wait BUSY. Panel needs **horizontal mirror** plus a **180°**
   framebuffer rotation to match glass (see below).
5. On a cold boot, full **white** clear. E-paper retains the last frame across
   reset and ROM download.

Drive `EPD_EN` from GPIO, not as an afterthought of the controller driver.

## Framebuffer (logical)

Logical canvas is 800×480, origin top-left, independent of FPC mount.

| Format | Packing | Size |
| --- | --- | --- |
| Gray4 | 2 bits/pixel, 4 pixels/byte, **MSB-first** | stride 200, **96,000** bytes |
| Mono 1 bpp | MSB-first | stride 100, **48,000** bytes |

| Name | Value |
| --- | ---: |
| Black | 0 |
| DarkGray | 1 |
| LightGray | 2 |
| White | 3 |

Gray4 → mono: values ≥ 2 white, 0–1 black. Two 96 KiB gray4 buffers (draw +
rotated TX) fit in octal PSRAM. Keep SPI DMA bounce buffers in internal RAM.

## Orientation (on-glass)

Working mapping:

1. Draw in normal 800×480 coordinates.
2. **Rotate the packed buffer 180°** (reverse byte order and reverse the four
   2-bit pixels in each byte) before SPI.
3. Controller **mirror_x**.

A compiled profile shipped `NO_FLIP` and called mount unknown. Trust the
mirror + 180° combination that actually matched glass. If you change either
step, change the [touch transform](touch.md) with it. ESPHome `mirror_x` only:
[nyc-esphome-orient](../resources/not-yet-confirmed.md#nyc-esphome-orient).

IMU-driven UI rotation is a logical-canvas concern: keep the physical
framebuffer mapping fixed, then rotate drawing/touch into page axes.
2026-08-30, embassy-debug splash, glass facing the operator:

| USB-C | UART | Splash |
| --- | --- | --- |
| Toward the floor | `imu=Portrait0` | Readable |
| Toward the ceiling | `imu=Portrait180` | Readable |
| Toward the operator’s right | `imu=Landscape0` | Readable |
| Toward the operator’s left | `imu=Landscape180` | Readable |

Tokens match the enclosure map. Identity landscape printed Latin
right-to-left; `page_to_framebuffer` now mirrors X. Confirmed LTR
on glass after that map.

## Waveforms

Refresh is expensive; glass holds pixels without power.

- **Full** black/white: OTP `UpdateSequence::DISPLAY_MODE_1_WITH_TEMP`
  (`0xF7`). Cold boot: white clear.
- **Partial** / DU black/white: OTP
  `UpdateSequence::DISPLAY_MODE_2_WITH_TEMP` (`0xFF`). Still panel-wide;
  send a full comparison frame, not a dirty rectangle alone. Lotus
  independently documents that Master Activation drives the whole panel
  from 0x24; a RAM window only scopes the write. Do not copy
  Lotus/bb_epaper `0xFC`.
- **Gray4**: OTP `SEEED_GRAY4_TEMPERATURE` then
  `UpdateSequence::SEEED_GRAY4` (`0xD7`), dual planes with Seeed’s inverted
  polarity (`PlaneMapping::SEEED_OTP`). Not an MCU 0x32 table.
- Deep sleep: `0x10` = `DEEP_SLEEP_ENTER` (`0x03`), wait ~100 ms, then
  `EPD_EN=0`. That drops controller RAM.
- Stock panel **standby** (keeps RAM): `0x22` =
  `UpdateSequence::DISABLE_ANALOG_AND_CLOCK` (`0x03`) then Master
  Activation `0x20`. **Resume:** `0x22` =
  `UpdateSequence::ENABLE_CLOCK_AND_ANALOG` (`0xC0`) then `0x20`.
  Same Table 7-1 stage bits the crate already names. Not a new
  opcode. Stock `ssd1677_standby` / `ssd1677_resume` send those
  bytes. The 2048 `seeed_epaper` copy has no vtable slots.
  UC8179 slots are NULL (`driver standby not supported`). Crate
  `Ssd1677::standby` / `resume` stay on `Active`. Deep sleep is
  still `sleep` → `Asleep`.

Seeed’s open `seeed_epaper` SSD1677 driver and stock `reterminal_template`
agree on that OTP path. They do **not** write a 105-byte LUT. A FreeInk MCU
table for Sticky was compared to stock app0 and is **absent**; it stays
commented. Full register table, hazards, and the commented bytes:
[docs/ssd1677.md](../../../../docs/ssd1677.md).

Do not invent a four-gray LUT from a generic SSD1677 example. Crate
`ssd1677` on crates.io is a black/white(/red) skeleton, not this path.

Schematic Rev 01 names the panel analog rails on the 24-pin FPC: **VGH**,
**VGL**, **VSH1**, **VSH2**, **VCOM**, **VPP**, and `EP_3V3`. There is
**no glass part number** on that sheet. A Good Display
**GDEY0397T81P** sheet is on file as a candidate only. Close
[nyc-panel-glass](../resources/not-yet-confirmed.md#nyc-panel-glass)
with official evidence that names this board's SKU. Do not inspect
the FPC. Do not treat BUSY time as a PN.
