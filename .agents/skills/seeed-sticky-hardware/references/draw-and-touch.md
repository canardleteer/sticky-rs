# Draw and hit-test the first time

How to put a button on this glass and tap it. Algebra and sit UART
stay in [touch.md](touch.md) and [display.md](display.md). Use this
page to pick a path, then stay in one coordinate space.

Four spaces type-check as a `u16` pair unless you use the board
crate types (`DigitizerSample`, `FramebufferPoint`, `GlassPoint`,
`PagePoint`). Mixing any two is the first-sit miss.

| Space | Type | What it is |
| --- | --- | --- |
| GT911 raw | `DigitizerSample` | Portrait **480×800**. Do not scale `x` as 800. |
| Framebuffer | `FramebufferPoint` | Pre-rotation 800×480. Gray4 ink and remote-debug inject. |
| Glass | `GlassPoint` | UART `p0=` (`to_screen`). Not a hit-test. |
| Page | `PagePoint` | IMU hold logical pixels. Draw buttons here. |

Official `begin(800, 480)` is a software map, not silicon 800-wide
([sources.md](sources.md)). Do not invent a 105-byte LUT. Enclosure
holes and keys: `View::landmark`.

## Path A — Native 800×480

Use when the app does not rotate with the IMU.

1. **Draw on the 800×480 canvas.**
   Place the button in framebuffer pixels. `View::native()` is
   identity (`map_draw` is a bounds check).
2. **Hit-test the same canvas.**
   Convert the GT911 sample with
   `DigitizerSample::to_framebuffer` (or `View::map_touch_sample`).
   Call `View::hit_raw` or `View::hit_framebuffer`. A `HitRect` is
   800×480 on Native.
3. **Do not use UART `p0=`.**
   That token is `GlassPoint`. A log line that looks like the tap
   is already 180° from the ink.

## Path B — IMU page

Use when cards stay upright in the four in-plane holds.

1. **Draw in page pixels.**
   Portrait is 480×800. Landscape is 800×480. Build
   `View::from_hold` for the current IMU hold.
2. **Map ink with `map_draw` / `map_draw_point`.**
   Gray4 `set_gray` then writes `(W-1-x, H-1-y)`.
3. **Hit-test with `hit_framebuffer`.**
   Pass `to_framebuffer`, not `p0=`. Portrait holds flip Y
   (page-space mirror X on the 480-wide page). Landscape holds
   use only the OTP 180°. Do not OR the empty opposite side.
4. **Place the strip with `HitRect`.**
   Worked example: embassy-debug START is portrait
   `(50, page_h-150, page_w-100, 90)` slop 10; landscape
   `(80, page_h-100, page_w-160, 72)` slop 20.
   The in-tree sit is `scene=targets` (page-space dots and
   slides; UART `target show|hit|miss|loop`).

## What not to do

- Do not scale raw GT911 `cx` as if the range were 800.
- Do not hit-test UART `p0=` / `GlassPoint`.
- Do not OR both landscape canvases.
- Do not invent a default 105-byte LUT.
- Do not treat official `begin(800, 480)` as silicon width.

## Note (first START miss)

A 2026-09-04 sit printed `p0=679,189` while `imu=Portrait0` and
`touch n=1` with no radio. Those glass digits are not START.
Undo 180° (`FramebufferPoint` `(120, 290)`) then page ≈
`(189, 679)`, which is the strip.

## Note (targets top-left miss)

A 2026-09-08 `scene=targets` sit, `imu=Portrait0`, tapped the
visible top-left disk (`p0=73,399`) and printed
`page=399,73 expect=80,80` (miss, ~320 px). That is a page
mirror X. The centre disk still scored (`page=225,399
expect=240,400`) because it sits on the midline. Wi-Fi START
is wide enough that the same mirror still hit the strip.
Portrait gray4 hit-test now flips Y. Same-day reflash,
`imu=Portrait0`: all five dots hit (top-left `page=76,76
expect=80,80 d=5`), then `slide_x span=397`, `slide_y span=717`,
`target loop`. Other in-plane holds are not in that listen.
