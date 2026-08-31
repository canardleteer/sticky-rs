# ssd1677-gray4

Driver for the Solomon Systech **SSD1677** e-paper controller, with four gray
levels on **black-and-white film**.

The long “how e-paper works / why this panel” write-up is
[docs/ssd1677.md](https://github.com/canardleteer/sticky-rs/blob/main/docs/ssd1677.md).
Register facts come from the SSD1677
datasheet **Rev 1.0 (Nov 2018)** and are cited at each definition. `#![no_std]`,
`embedded-hal` 1.0 only: this crate does not know about ESP32-S3.

## How e-paper differs from an LCD

Pigment particles move when you apply a voltage for a measured time, then
**stay** when you stop. The timed recipe is a **waveform**. A bad recipe can
look fine for days and then leave permanent ghosts. Updates take seconds; the
controller holds **BUSY** high while they run. Do not talk on the bus or cut
the panel rail until it falls.

## Why four grays need two RAM planes

The SSD1677 was built for black/white/**red**. It stores two one-bit images
(commands `0x24` and `0x26`). Each pixel’s bit pair selects one of four
waveform slots (LUT0..LUT3 — look-up table index 0 through 3). This glass has
no red ink, so those four slots become four gray levels. There is no separate
“grayscale mode.” Dual-plane writes plus a waveform that matches them **are**
the mode.

The plane mapping and the waveform are one design, so `PlaneMapping` is
caller-visible. The Seeed Sticky factory path uses
`PlaneMapping::SEEED_OTP` (inverted polarity: `1` means white).

## Two places a waveform can live

**Factory OTP** (one-time programmable memory **on the panel**): the module
already holds the recipe. Firmware does not download a table. It picks a
stored sequence (`0x22`) and runs it (`0x20`). Analog rails come up with that
recipe. This is the confirmed Sticky path (`Config::lut = None`).

**MCU LUT** (a **look-up table** the **microcontroller** writes with command
`0x32`, 105 bytes): optional, attributed via `Lut::new`, **not shipped**. A
table from another SSD1677 module can drive this film outside its envelope.
Seeed’s driver never sends `0x32`; stock firmware does not contain FreeInk’s
105-byte file.

Do not extract waveform bytes from vendor firmware.

## Why not crates.io `ssd1677`

Those crates drive black/white(/red) panels. They have no four-gray dual-plane
path and no Sticky OTP sequences.

## What it gives you

- Datasheet-verified opcode subset; unverified commands are absent rather than
  guessed.
- Window and cursor in **datasheet address units** (10-bit, X <= `0x3BF`,
  Y <= `0x2A7`), passed through without a hidden divide-by-8.
- Pure plane building: `split_gray4`, `gray4_to_mono`, `rotate180_gray4`,
  `rotate180_mono`, `mirror_x_plane`, with exact byte counts asserted for the
  Sticky's 800×480 canvas.
- `embedded-graphics` `DrawTarget` over `Gray2` (feature `graphics`, default).
- Blocking BUSY polling with a timeout, or edge-triggered
  `wait_until_idle_async` (feature `async`).
- Deep sleep as a **type state**: `Asleep` has no command methods, and `wake`
  performs the hardware reset the datasheet requires.
- Stock **standby** / **resume** on `Active`: `0x22` Table 7-1
  `DISABLE_ANALOG_AND_CLOCK` / `ENABLE_CLOCK_AND_ANALOG` then `0x20`.
  RAM stays. Not a third `0x10` mode.
