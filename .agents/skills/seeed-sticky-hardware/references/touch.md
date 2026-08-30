# Touch (GT911)

Vendor document: Goodix GT911 datasheet **Rev.09 (11 Mar 2015)**
([catalog](../resources/datasheets.md)). Rev.07 **deleted the register map**;
this PDF still has I2C addressing, INT/sleep behaviour, and “up to 5”
contacts. Discuss the hex as crate enums, not raw ports:

| Hex | Type | What it is |
| --- | --- | --- |
| `0x8040` | `Register::Command` | **Address** of the command port. Rev.09 §8.1 still names this port for Gesture (opcode `Command::Gesture` = `8`, also written to `0x8046`). The crate `init()` opcode `Command::ReadCoordinates` = `0` is **not** in Rev.09. |
| `0x814E` | `Register::Status` | **Address** of the buffer handshake. A host write is `StatusWrite` (`Clear` = `0`). A read is `StatusBits` (bitfield: bit 7 ready, bits 3–0 count) — crate / on-glass, **not** a Rev.09 table. |
| `0x8150` | `Register::Points` | First contact record (on-glass). Coords are LE at `POINT_X_OFFSET` / `POINT_Y_OFFSET`. |

Rev.07 **deleted the register map** (Rev.09 revision history). Do not treat
`0x814E` as a mode enum. Local cache: `resources/datasheets/pdf/gt911.pdf`
(id `gt911`).

## Wiring

Schematic Rev 01. Speeds are Rev.09 §6.1 (at or below 400 kbps).

| Signal | GPIO | Role |
| --- | ---: | --- |
| SDA | 3 | Dedicated I2C. **ESP32-S3 strapping pin** (v2.2 §3.4). |
| SCL | 2 | Dedicated I2C. After reset: input, no internal pull. |
| INT | 21 | Address select during reset, then input. **No MCU default pull** (v2.2 Table 2-1). Leave it floating after address select unless the schematic shows a board pull. |
| RST | 41 | Reset. Default pad function is JTAG `MTDI`; mux to GPIO before the dance. |
| `TOUCH_EN` | 42 | Active-high power. ~250 ms settle. Default pad function is JTAG `MTMS`; mux to GPIO. |

Digitizer is **portrait 480×800** under a **landscape 800×480** panel.
Must **not** share the sensor I2C bus (GPIO0 is a strap). GPIO3 is also a
strap: do not toggle SDA until strapping hold time has passed.

## Address selection

Rev.09 §6.1: two **8-bit** slave pairs, named in code as
`SlaveAddress::Pair28_29` (`0x28`/`0x29`) and `SlaveAddress::PairBaBb`
(`0xBA`/`0xBB`). The 7-bit addresses used on the wire are
`SlaveAddress::seven_bit`. The host selects the pair with Reset and INT
during power-on / reset (timing diagrams on datasheet p.10; the extracted
markdown has no T2/T3 numbers).

On-glass mapping (both 7-bit addresses ACK on this unit):

| INT level at RST rising | 8-bit write/read | 7-bit |
| --- | --- | ---: |
| 0 | `0xBA`/`0xBB` | `0x5D` |
| 1 | `0x28`/`0x29` | `0x14` |

INT=0 → `0x5D` delivered contacts (`touch n=5`). INT=1 → `0x14`
ACKed; an init `StatusWrite::Clear` path stayed at `st=0x00`. Probe
the address the dance selected. Do not silently flip
`addresses::GT911_PRIMARY` (`0x14`).

I2C must stay at or below **400 kbps** (Rev.09 §6.1). embassy-debug
uses that cap. simple-debug uses 100 kHz (inside the cap).

`/RSTB` is active-low and wants an external 10 kΩ pull-up (pin table).
Initialization, including self-calibration of the idle capacitance, finishes
in **< 200 ms** (features + §8.6). Do not expect a first valid scan before
that window.

Address-select reset (Rev.09 §6.1; extracted markdown has no T2/T3).
embassy-debug holds that worked on glass, inside the 200 ms window:

1. RST=0, INT driven at the select level, hold **10 ms**.
2. RST=1, INT still driven, wait **10 ms**, then **50 ms**.
3. INT as a **floating** input (no MCU pull-up), wait **50 ms**.
4. I2C probe, read ID at `Register::Id`.

A longer conservative hold (20 / 20 / 80 / 30 ms) ACKed `0x14` after
INT-high select and is what simple-debug uses.

Neither path writes `Register::Command` or GT911 config RAM.
embassy-debug does **not** write `StatusWrite::Clear` at begin. Poll
`Register::Status`; if bit 7 is set, read `Register::Points`, then
clear. simple-debug clears Status at init and has not printed a
contact line.

Third-party C++ sequences that used the same INT-low-first combination
are wiring evidence only
([cpp-platformio.md](cpp-platformio.md#freeink--crosspoint)). They
are not the source of these numbers.

The `gt911` crate `init()` writes `Command::ReadCoordinates` at
`Register::Command` before the product-ID check and status clear. That
encoding is **not** in Rev.09 (the map is gone). The PDF still names
`Register::Command` as a command port for Gesture mode
(`Command::Gesture` to `0x8046` then `0x8040`, §8.1). Espressif’s
`ENTER_SLEEP` name for the same address is not a Rev.09 claim. Sleep in
this datasheet is: drive INT **low**, then the screen-off I2C command; wake
by driving INT **high** for 2–5 ms, at least 58 ms after screen-off. That
is not the same as cutting `TOUCH_EN`.

INT notify polarity is a **config bit** (§8.2): `0` = rising (idle low),
`1` = falling (idle high). A stuck-high INT with a host pull-up matches
falling-edge idle, or a chip that never pulses. GPIO21 has **no** reset
pull on the ESP32-S3 (v2.2 Table 2-1); an MCU `Pull::Up` is firmware, not
silicon default, and can hold INT high so a rising-edge notify never looks
like a pulse. After address select, leave INT floating. GPIO hold on
RST/INT is an ESP32-S3 **TRM** topic (that PDF is not in the local
cache yet).

`get_multi_touch` returns `Err(NotReady)` when status bit `0x80` is clear
(idle in the crate / on-glass drivers; **not** a Rev.09 bit name). The
operator image prints `gt911 st=0xNN` each heartbeat so a miss is status vs
count. embassy-debug prints the same token when
`touch::STATUS_HEARTBEAT` is on (read-only; it does not write
`Register::Status` for that line).

## On glass (embassy-debug)

2026-08-30, default embassy-debug. Rev.09 §6.1 INT-low first,
`I2C_MAX_HZ`, `Register::Points` at `POINT_X_OFFSET`. Boot token:
`gt911 addr dance`.

- INT=0: `SlaveAddress::PairBaBb` **ACK** (`0x5d`), `Pair28_29` NAK.
- First heartbeat `gt911 st=0x80` (ready, count 0).
- Attended taps: `touch n=1` … `touch n=0`, then `n=2`, then **`n=5`**
  with `gt911 st=0x85`. This FPC delivers the silicon max (Rev.09 §1).

INT-high + init `StatusWrite::Clear` + 100 kHz ACKed `0x14` and stayed
at `st=0x00` with no `touch n=`. simple-debug operator
`gt911_contacts` timed out the same way. That was the dance, not a
dead panel.

Power the rail **before** this dance. Point data starts at **byte 0** (no
track-id prefix).

Board sleep cuts the rail: hold `TOUCH_EN` and `TOUCH_RST` low. Touch-to-wake
is off whenever the rail is off. Do not mix that with the datasheet I2C Sleep
sequence above unless the rail stays up.

## Sensor resolution

Hardware reports **480×800**. If the resolution register is junk, keep
controller config and map in software as 480×800. Do not rewrite GT911 config
RAM just to pretend the sensor is 800×480. Rev.09 §8.4 (Stationary
Configuration) is a host-to-chip parameter lock, not a license to invent a
186-byte table. Tx channel order also has to match the sensor (§5.2); that
is module programming, not an MCU guess.

## Coordinate transform (on-glass)

Map a controller sample `(cx, cy)` to physical screen `(sx, sy)` that matches
the **pre-rotation** 800×480 canvas, then undo the display’s 180° transmit
rotation.

`W=800`, `H=480`, portrait `Pw=480`, `Ph=800`; `scale` is rounded integer map:

1. `portrait_x = scale(cx, W, Pw-1)`
2. `portrait_y = scale(H - min(cy, H), H, Ph-1)`
3. `fb_x = W - portrait_y - 1`
4. `fb_y = portrait_x`
5. `sx = W - fb_x - 1`
6. `sy = H - fb_y - 1`

Swap-XY + flip-both onto 0–799 × 0–479 is the same geometry **without** step
5–6. If display rotation changes, this transform must change with it.

After mapping, taps are physical 800×480. Rotated pages convert physical →
logical with the same rotation as drawing.

Polling rate, tap slop, and stuck-contact recovery are software policy, not
hardware. Re-run the reset/address sequence if the controller stops ACKing.
