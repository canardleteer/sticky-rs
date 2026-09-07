# panel-view

Board-agnostic logical canvas and touch map for e-paper UIs.

A product crate implements `PanelView`: logical
size, draw remap, touch remap, enclosure landmarks, and in-plane hold
changes. Shared UI talks to the trait. This crate has no SPI, GPIO, or
pixel buffers.

The Sticky implementation is `seeed-reterminal-sticky::view::View`
(`https://github.com/canardleteer/sticky-rs`). PaperMono can implement the
same trait later without taking Sticky pins or GT911 math.

Native identity (zero-cost panel RAM) is an implementor policy. Do not bake
in “starting position is horizontal.”

Companions (always compiled, not a Cargo feature): `TouchSource` /
`TouchSample` (physical glass vs a debugger inject, both in pre-rotation
framebuffer pixels), `ExpectedFrame` / `PanelCapture` (last composed
planes; the implementor owns the buffers), and `PanelTouchAlign` (source
does not change the framebuffer map). `PanelView` itself stays remap-only
and does not own pixel buffers. The Sticky `View` type is `Copy` and does
not implement `PanelCapture`.
