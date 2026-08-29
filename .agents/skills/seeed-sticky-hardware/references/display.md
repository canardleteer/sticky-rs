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

SSD1677 serial clock max is 20 MHz. **10 MHz** is the rate that has been
stable on this shared bus. Do not start at 40 MHz.
[nyc-spi-ceiling](../resources/not-yet-confirmed.md#nyc-spi-ceiling).

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

## Waveforms

Refresh is expensive; glass holds pixels without power.

- **Full** black/white: OTP `UpdateSequence::DISPLAY_MODE_1_WITH_TEMP` (`0xF7`). Cold boot: white clear.
- **Partial** / DU black/white: OTP `UpdateSequence::DISPLAY_MODE_2_WITH_TEMP` (`0xFF`). Still panel-wide; send a
  full comparison frame, not a dirty rectangle alone. Lotus independently
  documents that Master Activation drives the whole panel from 0x24; a RAM
  window only scopes the write. Do not copy Lotus/bb_epaper `0xFC`.
- **Gray4**: OTP `SEEED_GRAY4_TEMPERATURE` then `UpdateSequence::SEEED_GRAY4` (`0xD7`), dual planes with
  Seeed’s inverted polarity (`PlaneMapping::SEEED_OTP`). Not an MCU 0x32 table.
- Deep sleep: `0x10` = `DEEP_SLEEP_ENTER` (`0x03`), wait ~100 ms, then `EPD_EN=0`.

Seeed’s open `seeed_epaper` SSD1677 driver and stock `reterminal_template`
agree on that OTP path. They do **not** write a 105-byte LUT. A FreeInk MCU
table for Sticky was compared to stock app0 and is **absent**; it stays
commented. Full register table, hazards, and the commented bytes:
[docs/ssd1677.md](../../../docs/ssd1677.md).

Do not invent a four-gray LUT from a generic SSD1677 example. Crate
`ssd1677` on crates.io is a black/white(/red) skeleton, not this path.

Panel glass part number and temperature LUT *set* (beyond the one gray4
0x1A override): [nyc-panel-glass](../resources/not-yet-confirmed.md#nyc-panel-glass).
