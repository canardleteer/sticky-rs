//! Board-agnostic logical canvas and touch map.
//!
//! A product crate (`seeed-reterminal-sticky`, later papermono-rs) implements
//! [`PanelView`]. Shared UI talks to the trait: logical size, draw remap,
//! touch remap, enclosure landmarks, and in-plane hold changes.
//!
//! This crate has **no** SPI, GPIO, or pixel buffers. It does not own a
//! panel. Rotation algebra and digitizer physics stay in the implementor.
//! Last-compose snapshot and tagged touches live in [`debug`]: they are
//! companions, not methods on [`PanelView`].
//!
//! Native identity (zero-cost panel RAM) is an implementor policy, not a
//! trait variant. Official “starting position is horizontal” must not be
//! baked in here.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod debug;

pub use debug::{
    ExpectedFrame, FrameKind, PanelCapture, PanelTouchAlign, TouchSample, TouchSource,
};

/// Logical canvas plus touch remap for one product.
///
/// `Hold` is that board’s in-plane pose (USB-down portrait, …). `Landmark`
/// is that enclosure’s named points. Face-up / face-down / unknown stay
/// `None` on [`PanelView::current_hold`] and [`PanelView::hold_changed`].
pub trait PanelView {
    /// In-plane hold. `Copy + Eq` so IMU compare needs no alloc.
    type Hold: Copy + Eq;

    /// Enclosure landmark in logical pixels.
    type Landmark: Copy;

    /// Logical page size `(width, height)`.
    fn logical_size(&self) -> (u16, u16);

    /// Logical pixel → panel framebuffer (pre-rotation RAM).
    fn map_draw(&self, x: u16, y: u16) -> Option<(u16, u16)>;

    /// Pre-rotation framebuffer tap → logical page.
    ///
    /// Pass the product’s digitizer→framebuffer map, not glass / UART
    /// screen space.
    fn map_touch_framebuffer(&self, fx: u16, fy: u16) -> Option<(u16, u16)>;

    /// Raw digitizer sample → logical page.
    fn map_touch_raw(&self, cx: u32, cy: u32) -> Option<(u16, u16)>;

    /// Landmark in this view’s logical pixels.
    fn landmark(&self, mark: Self::Landmark) -> Option<(u16, u16)>;

    /// Current in-plane hold, or `None` on the native / identity canvas.
    fn current_hold(&self) -> Option<Self::Hold>;

    /// Opt into an enclosure hold (leaves native identity).
    fn set_hold(&mut self, hold: Self::Hold);

    /// Whether this view is the identity canvas (no rotation math).
    fn is_native(&self) -> bool;

    /// True when an in-plane hold change should repaint.
    ///
    /// `None` is flat / unknown: keep the last page.
    #[inline]
    #[must_use]
    fn hold_changed(from: Option<Self::Hold>, to: Option<Self::Hold>) -> bool {
        match (from, to) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Hold {
        A,
        B,
    }

    struct Native;

    impl PanelView for Native {
        type Hold = Hold;
        type Landmark = ();

        fn logical_size(&self) -> (u16, u16) {
            (8, 4)
        }

        fn map_draw(&self, x: u16, y: u16) -> Option<(u16, u16)> {
            (x < 8 && y < 4).then_some((x, y))
        }

        fn map_touch_framebuffer(&self, fx: u16, fy: u16) -> Option<(u16, u16)> {
            self.map_draw(fx, fy)
        }

        fn map_touch_raw(&self, cx: u32, cy: u32) -> Option<(u16, u16)> {
            self.map_touch_framebuffer(cx as u16, cy as u16)
        }

        fn landmark(&self, _mark: ()) -> Option<(u16, u16)> {
            Some((0, 0))
        }

        fn current_hold(&self) -> Option<Hold> {
            None
        }

        fn set_hold(&mut self, _hold: Hold) {}

        fn is_native(&self) -> bool {
            true
        }
    }

    fn draw_via<V: PanelView>(view: &V, x: u16, y: u16) -> Option<(u16, u16)> {
        view.map_draw(x, y)
    }

    #[test]
    fn trait_path_is_identity_on_a_native_stub() {
        let view = Native;
        assert_eq!(draw_via(&view, 1, 2), Some((1, 2)));
        assert_eq!(view.map_touch_framebuffer(7, 3), Some((7, 3)));
        assert!(view.is_native());
        assert!(!Native::hold_changed(Some(Hold::A), Some(Hold::A)));
        assert!(Native::hold_changed(Some(Hold::A), Some(Hold::B)));
        assert!(!Native::hold_changed(Some(Hold::A), None));
    }
}
