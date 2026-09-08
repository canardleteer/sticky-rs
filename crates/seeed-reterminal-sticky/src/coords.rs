//! Four named coordinate spaces on this glass.
//!
//! Mixing any two is the first-sit miss (UART `p0=` is not a gray4
//! hit-test). First-time recipe lives in the hardware skill
//! `draw-and-touch.md`. This module is the compile-time split.

use crate::display::{screen_to_framebuffer, HEIGHT, WIDTH};
use crate::touch::{to_framebuffer, to_screen};

/// GT911 sample. Portrait **480×800**, not panel 800×480.
///
/// Do not scale `x` as if the range were 800.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DigitizerSample {
    /// Raw `cx` (0..=479).
    pub x: u32,
    /// Raw `cy` (0..=799).
    pub y: u32,
}

/// Pre-rotation 800×480 panel RAM (same space as gray4 DRAW).
///
/// Gray4 `set_gray` then writes `(W-1-x, H-1-y)`. Remote-debug inject
/// uses this space, not UART `p0=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FramebufferPoint {
    /// Canvas X (0..=799).
    pub x: u16,
    /// Canvas Y (0..=479).
    pub y: u16,
}

/// Glass after the panel 180° transmit. UART `p0=` is this space.
///
/// Do not hit-test a button with this type. Convert through
/// [`GlassPoint::to_framebuffer`] only when the digits are already
/// glass (a log line), not when the raw sample is still in hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlassPoint {
    /// Glass X (0..=799).
    pub x: u16,
    /// Glass Y (0..=479).
    pub y: u16,
}

/// Logical page for the current [`crate::view::View`] origin.
///
/// Native is 800×480 (same axes as framebuffer). A hold is 480×800
/// (portrait) or 800×480 (landscape). Draw buttons here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PagePoint {
    /// Page X.
    pub x: u16,
    /// Page Y.
    pub y: u16,
}

/// Axis-aligned button in **page** pixels (Native: 800×480 canvas).
///
/// `slop` grows the hit box for a fat finger. Same space as the draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitRect {
    /// Page left.
    pub x: u16,
    /// Page top.
    pub y: u16,
    /// Width.
    pub w: u16,
    /// Height.
    pub h: u16,
    /// Extra pixels on each side.
    pub slop: u16,
}

impl DigitizerSample {
    /// Pre-rotation canvas ([`to_framebuffer`]).
    #[must_use]
    pub fn to_framebuffer(self) -> FramebufferPoint {
        let (x, y) = to_framebuffer(self.x, self.y);
        FramebufferPoint {
            x: x as u16,
            y: y as u16,
        }
    }

    /// Glass ([`to_screen`]). UART `p0=` matches this.
    #[must_use]
    pub fn to_glass(self) -> GlassPoint {
        let (x, y) = to_screen(self.x, self.y);
        GlassPoint {
            x: x as u16,
            y: y as u16,
        }
    }
}

impl FramebufferPoint {
    /// Glass (undo the transmit 180°).
    #[must_use]
    pub const fn to_glass(self) -> Option<GlassPoint> {
        if self.x >= WIDTH || self.y >= HEIGHT {
            return None;
        }
        Some(GlassPoint {
            x: WIDTH - 1 - self.x,
            y: HEIGHT - 1 - self.y,
        })
    }
}

impl GlassPoint {
    /// Pre-rotation canvas ([`screen_to_framebuffer`]).
    ///
    /// Prefer [`DigitizerSample::to_framebuffer`] when the raw sample
    /// is still in hand.
    #[must_use]
    pub const fn to_framebuffer(self) -> Option<FramebufferPoint> {
        match screen_to_framebuffer(self.x, self.y) {
            Some((x, y)) => Some(FramebufferPoint { x, y }),
            None => None,
        }
    }
}

impl HitRect {
    /// Inclusive-start, exclusive-end box grown by [`Self::slop`].
    #[must_use]
    pub const fn contains(self, point: PagePoint) -> bool {
        let x0 = self.x.saturating_sub(self.slop);
        let y0 = self.y.saturating_sub(self.slop);
        let x1 = self.x.saturating_add(self.w).saturating_add(self.slop);
        let y1 = self.y.saturating_add(self.h).saturating_add(self.slop);
        point.x >= x0 && point.x < x1 && point.y >= y0 && point.y < y1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glass_is_not_framebuffer() {
        let glass = GlassPoint { x: 679, y: 189 };
        let fb = glass.to_framebuffer().expect("on panel");
        assert_eq!(fb, FramebufferPoint { x: 120, y: 290 });
        assert_ne!((glass.x, glass.y), (fb.x, fb.y));
    }
}
