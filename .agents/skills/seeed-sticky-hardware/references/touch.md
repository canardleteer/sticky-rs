# Touch (GT911)

Vendor document: Goodix GT911 datasheet **Rev.09 (11 Mar 2015)**
([catalog](../resources/datasheets.md)). Rev.07 **deleted the register map**;
this PDF still has I2C addressing, INT/sleep behaviour, and “up to 5”
contacts. Coordinate registers (`0x8140` / `0x814E` / `0x8150`), the
command-`0` encoding, and status bit `0x80` are **not** in Rev.09 — those
numbers come from on-glass `GT911_REG_*` names, not from this datasheet.

## Wiring

| Signal | GPIO | Role |
| --- | ---: | --- |
| SDA | 3 | Dedicated I2C, 100 kHz on glass. **ESP32-S3 strapping pin** (v2.2 §3.4). |
| SCL | 2 | Dedicated I2C. After reset: input, no internal pull. |
| INT | 21 | Address select during reset, then input. **No MCU default pull** (v2.2 Table 2-1). Leave it floating after the dance unless a schematic shows a board pull. |
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

On-glass mapping (working units at **`0x14`**):

| INT level at RST rising | 8-bit write/read | 7-bit |
| --- | --- | ---: |
| 0 | `0xBA`/`0xBB` | `0x5D` |
| 1 | `0x28`/`0x29` | `0x14` |

Probe `0x14` first, then `0x5D`.

I2C must stay at or below **400 kbps** (Rev.09 §6.1). On-glass clocks this
bus at **100 kHz**.

`/RSTB` is active-low and wants an external 10 kΩ pull-up (pin table).
Initialization, including self-calibration of the idle capacitance, finishes
in **< 200 ms** (features + §8.6). Do not expect a first valid scan before
that window.

Reset timing that worked on this board (conservative vs the unread
diagrams):

1. RST=0, INT=select level, hold **20 ms**.
2. RST=1, wait **20 ms**.
3. INT as a **floating** input (no MCU pull-up), wait **80 ms**, then an extra
   **30 ms**. GPIO21 has no silicon default pull (ESP32-S3 v2.2 Table 2-1).
4. I2C probe, read ID. Self-cal finishes in **< 200 ms** (Rev.09); this
   sequence stays inside that window.

UART learning firmware ACKed `0x14` after this dance by reading product ID
at `0x8140`. Bunny on glass
([limengdu/reTerminal_Sticky_Bunny](https://github.com/limengdu/reTerminal_Sticky_Bunny))
then **clears status** (`0x814E = 0`) and polls at **30 ms** on a **100 kHz**
touch bus. `read_points` returns **0** when buffer bit `0x80` is clear (idle,
not an error) and emits a tap on **finger-up** if the path stayed still —
that is why it feels a little slow. It does **not** write `0x8040` and does
**not** rewrite GT911 config RAM; resolution fallback is software mapping only.

The `gt911` crate `init()` adds a command-`0` write at `0x8040` before the
product-ID check and status clear. That encoding is **not** in Rev.09 (the
map is gone). The PDF still names `0x8040` as a command port for Gesture
mode (command `8` to `0x8046` then `0x8040`, §8.1). Espressif’s
`ENTER_SLEEP` name for the same address is not a Rev.09 claim. Sleep in
this datasheet is: drive INT **low**, then the screen-off I2C command; wake
by driving INT **high** for 2–5 ms, at least 58 ms after screen-off. That
is not the same as cutting `TOUCH_EN`.

INT notify polarity is a **config bit** (§8.2): `0` = rising (idle low),
`1` = falling (idle high). A stuck-high INT with a host pull-up matches
falling-edge idle, or a chip that never pulses. GPIO21 has **no** reset
pull on the ESP32-S3 (v2.2 Table 2-1); an MCU `Pull::Up` is firmware, not
silicon default, and can hold INT high so a rising-edge notify never looks
like a pulse. After address select, leave INT floating. On-glass also calls
`gpio_hold_dis` on RST/INT before the dance; GPIO hold itself is an ESP32-S3
**TRM** topic (that PDF is not in the local cache yet).

`get_multi_touch` returns `Err(NotReady)` when status bit `0x80` is clear
(idle in the crate / on-glass drivers; **not** a Rev.09 bit name). The
operator image prints `gt911 st=0xNN` each heartbeat so a miss is status vs
count. Silicon allows **up to 5** concurrent touches (Rev.09 §1). How many
this FPC delivers is still
[nyc-gt911-contacts](../resources/not-yet-confirmed.md#nyc-gt911-contacts).

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
