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
