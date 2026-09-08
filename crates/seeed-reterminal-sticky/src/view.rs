//! Uniform logical canvas and touch map for this glass.
//!
//! [`View::native`] is the **zero-cost** default: the pre-rotation 800×480
//! panel RAM canvas. `map_draw` and `map_touch_framebuffer` are identity
//! (bounds check only). The four [`PageRotation`] holds are opt-in remaps
//! from a builder preference; they are the cost of an enclosure-relative
//! page, not of constructing a [`View`].
//!
//! | Preference | `map_draw` | Extra gray4 hit-test |
//! | --- | --- | --- |
//! | **[`ViewOrigin::Native`] (default)** | `(x, y)` | none |
//! | [`PageRotation::Portrait0`] (USB-C bottom) | swap + 180-ish | flip Y (page mirror X) |
//! | [`PageRotation::Portrait180`] | swap | flip Y (page mirror X) |
//! | [`PageRotation::Landscape0`] (USB-C right) | mirror X | OTP 180° |
//! | [`PageRotation::Landscape180`] (USB-C left) | flip Y | OTP 180° |
//!
//! Native is **not** official `begin(800, 480)` / “starting position is
//! horizontal.” That landscape software map is [`PageRotation::Landscape0`],
//! the most expensive gray4 hold. Do not make it the identity.
//!
//! Still paid on Native (not View rotation math):
//!
//! - Raw GT911 **480×800 → 800×480** ([`crate::touch::to_framebuffer`])
//! - Gray4 `set_gray` internal 180° (already inside the driver)
//!
//! UART `p0=` stays glass ([`crate::touch::to_screen`]). This type maps
//! ink and hit-test, not that token. Prefer
//! [`crate::coords::FramebufferPoint`] and [`crate::coords::HitRect`]
//! (`map_draw_point` / `hit_framebuffer`) so a
//! [`crate::coords::GlassPoint`] cannot compile as a tap.
//!
//! Existing [`page_to_framebuffer`](crate::display::page_to_framebuffer) /
//! [`gray4_touch_framebuffer`](crate::display::gray4_touch_framebuffer)
//! stay the algebra. [`View`] calls them. It does not own SPI.
//! [`View`] implements [`panel_view::PanelView`] so shared UI (later
//! papermono-rs) can talk to the trait. It does not implement
//! [`panel_view::PanelCapture`]: this type is `Copy` remap-only.
//! Last compose lives in firmware (or a host stub).

use crate::coords::{DigitizerSample, FramebufferPoint, HitRect, PagePoint};
use crate::display::{
    framebuffer_to_page, gray4_touch_framebuffer, page_to_framebuffer, PageRotation, HEIGHT,
    PAGE_HEIGHT, PAGE_WIDTH, WIDTH,
};
use panel_view::PanelView;

/// How a [`View`] addresses the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewOrigin {
    /// Pre-rotation 800×480 panel RAM. Zero-cost identity.
    Native,
    /// Enclosure-relative page for this in-plane hold.
    Hold(PageRotation),
}

/// Enclosure landmark in **logical** pixels for the current [`View`].
///
/// Positions are diagram fractions from the appearance drawing (glass
/// facing you, USB-C on the bottom short edge, keys on the right). They
/// are not measured millimetres on a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landmark {
    /// USB Type-C on the USB-C short edge (diagram #5).
    UsbC,
    /// Recessed Reset pinhole (diagram #1). Hardware reset, not a GPIO.
    Reset,
    /// PDM microphone hole (diagram #2).
    Microphone,
    /// AI Voice / OK key, top of the keys long-edge trio.
    ButtonAi,
    /// Page Up, middle of the keys trio.
    ButtonUp,
    /// Page Down, bottom of the keys trio.
    ButtonDown,
    /// MicroSD slot on the opposite long edge.
    SdSlot,
}

/// Builds a [`View`]. Default origin is [`ViewOrigin::Native`] (no rotation
/// math).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewBuilder {
    origin: ViewOrigin,
}

impl ViewBuilder {
    /// Native default.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            origin: ViewOrigin::Native,
        }
    }

    /// Keep the zero-cost panel-RAM canvas (same as [`ViewBuilder::new`]).
    #[inline]
    #[must_use]
    pub const fn native(mut self) -> Self {
        self.origin = ViewOrigin::Native;
        self
    }

    /// Enclosure-relative page. Costs the hold remap, including Landscape0
    /// OTP 180° on gray4 hit-test.
    #[inline]
    #[must_use]
    pub const fn logical_up(mut self, rotation: PageRotation) -> Self {
        self.origin = ViewOrigin::Hold(rotation);
        self
    }

    /// Finish the builder.
    #[inline]
    #[must_use]
    pub const fn build(self) -> View {
        View {
            origin: self.origin,
        }
    }
}

impl Default for ViewBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Logical canvas plus touch remap for one origin.
///
/// Copy-sized. Native construction does not allocate and does not rotate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    origin: ViewOrigin,
}

impl View {
    /// Zero-cost panel framebuffer (800×480). No rotation math.
    #[inline]
    #[must_use]
    pub const fn native() -> Self {
        Self {
            origin: ViewOrigin::Native,
        }
    }

    /// Enclosure-relative page for `rotation`.
    #[inline]
    #[must_use]
    pub const fn from_hold(rotation: PageRotation) -> Self {
        Self {
            origin: ViewOrigin::Hold(rotation),
        }
    }

    /// Builder. Default is [`View::native`].
    #[inline]
    #[must_use]
    pub const fn builder() -> ViewBuilder {
        ViewBuilder::new()
    }

    /// Stated origin.
    #[inline]
    #[must_use]
    pub const fn origin(self) -> ViewOrigin {
        self.origin
    }

    /// Whether this view is the identity canvas.
    #[inline]
    #[must_use]
    pub const fn is_native(self) -> bool {
        matches!(self.origin, ViewOrigin::Native)
    }

    /// In-plane hold, or `None` on Native.
    #[inline]
    #[must_use]
    pub const fn current_hold(self) -> Option<PageRotation> {
        match self.origin {
            ViewOrigin::Native => None,
            ViewOrigin::Hold(rotation) => Some(rotation),
        }
    }

    /// Switch to an enclosure hold. Leaves Native; this is the opt-in to cost.
    #[inline]
    #[must_use]
    pub const fn with_hold(self, rotation: PageRotation) -> Self {
        Self::from_hold(rotation)
    }

    /// In-place [`View::with_hold`].
    #[inline]
    pub fn set_hold(&mut self, rotation: PageRotation) {
        *self = self.with_hold(rotation);
    }

    /// Logical page size `(width, height)`.
    #[inline]
    #[must_use]
    pub const fn logical_size(self) -> (u16, u16) {
        match self.origin {
            ViewOrigin::Native => (WIDTH, HEIGHT),
            ViewOrigin::Hold(rotation) => rotation.page_size(),
        }
    }

    /// Logical pixel → pre-rotation 800×480 panel RAM.
    ///
    /// Native is identity. A hold calls
    /// [`page_to_framebuffer`](crate::display::page_to_framebuffer).
    #[inline]
    #[must_use]
    pub const fn map_draw(self, x: u16, y: u16) -> Option<(u16, u16)> {
        match self.origin {
            ViewOrigin::Native => {
                if x >= WIDTH || y >= HEIGHT {
                    None
                } else {
                    Some((x, y))
                }
            }
            ViewOrigin::Hold(rotation) => page_to_framebuffer(x, y, rotation),
        }
    }

    /// Raw GT911 sample → logical page (gray4 hit-test canvas).
    ///
    /// Always pays [`crate::touch::to_framebuffer`] (digitizer 480×800).
    /// Native then returns that canvas. A hold also applies
    /// [`gray4_touch_framebuffer`](crate::display::gray4_touch_framebuffer)
    /// then [`framebuffer_to_page`](crate::display::framebuffer_to_page) so
    /// portrait flips Y (page mirror X) and Landscape0 uses only the
    /// OTP 180°.
    #[inline]
    #[must_use]
    pub fn map_touch_raw(self, cx: u32, cy: u32) -> Option<(u16, u16)> {
        let (fx, fy) = crate::touch::to_framebuffer(cx, cy);
        self.map_touch_framebuffer(fx as u16, fy as u16)
    }

    /// [`PagePoint`] → pre-rotation [`FramebufferPoint`].
    #[inline]
    #[must_use]
    pub const fn map_draw_point(self, point: PagePoint) -> Option<FramebufferPoint> {
        match self.map_draw(point.x, point.y) {
            Some((x, y)) => Some(FramebufferPoint { x, y }),
            None => None,
        }
    }

    /// Pre-rotation framebuffer tap → logical page.
    ///
    /// Pass [`crate::touch::to_framebuffer`], not UART `p0=` /
    /// [`crate::touch::to_screen`]. Native is identity.
    #[inline]
    #[must_use]
    pub const fn map_touch_framebuffer(self, fx: u16, fy: u16) -> Option<(u16, u16)> {
        match self.origin {
            ViewOrigin::Native => {
                if fx >= WIDTH || fy >= HEIGHT {
                    None
                } else {
                    Some((fx, fy))
                }
            }
            ViewOrigin::Hold(rotation) => match gray4_touch_framebuffer(fx, fy, rotation) {
                Some((hx, hy)) => framebuffer_to_page(hx, hy, rotation),
                None => None,
            },
        }
    }

    /// [`FramebufferPoint`] → logical [`PagePoint`].
    ///
    /// A [`crate::coords::GlassPoint`] will not compile here.
    #[inline]
    #[must_use]
    pub const fn map_touch_point(self, point: FramebufferPoint) -> Option<PagePoint> {
        match self.map_touch_framebuffer(point.x, point.y) {
            Some((x, y)) => Some(PagePoint { x, y }),
            None => None,
        }
    }

    /// Raw GT911 sample → logical [`PagePoint`].
    #[inline]
    #[must_use]
    pub fn map_touch_sample(self, sample: DigitizerSample) -> Option<PagePoint> {
        self.map_touch_raw(sample.x, sample.y)
            .map(|(x, y)| PagePoint { x, y })
    }

    /// True when a raw sample lands in `rect`.
    ///
    /// `rect` is page pixels. Native treats that as the 800×480 canvas.
    #[inline]
    #[must_use]
    pub fn hit_raw(self, sample: DigitizerSample, rect: HitRect) -> bool {
        match self.map_touch_sample(sample) {
            Some(page) => rect.contains(page),
            None => false,
        }
    }

    /// True when a framebuffer tap lands in `rect`.
    ///
    /// Pass [`FramebufferPoint`], not UART `p0=`.
    #[inline]
    #[must_use]
    pub const fn hit_framebuffer(self, point: FramebufferPoint, rect: HitRect) -> bool {
        match self.map_touch_point(point) {
            Some(page) => rect.contains(page),
            None => false,
        }
    }

    /// Landmark in this view’s logical pixels.
    ///
    /// Enclosure points are defined on the USB-down portrait page, then
    /// mapped through panel RAM into the current origin.
    #[inline]
    #[must_use]
    pub const fn landmark(self, mark: Landmark) -> Option<(u16, u16)> {
        let (px, py) = enclosure_portrait0(mark);
        let Some((fx, fy)) = page_to_framebuffer(px, py, PageRotation::Portrait0) else {
            return None;
        };
        match self.origin {
            ViewOrigin::Native => Some((fx, fy)),
            ViewOrigin::Hold(rotation) => framebuffer_to_page(fx, fy, rotation),
        }
    }

    /// True when an in-plane hold change should repaint.
    ///
    /// `None` is FaceUp / FaceDown / unknown: keep the last page, no redraw
    /// from this helper. Matches [`crate::imu::Orientation::page_rotation`].
    #[inline]
    #[must_use]
    pub const fn needs_redraw(from: Option<PageRotation>, to: Option<PageRotation>) -> bool {
        match (from, to) {
            (Some(a), Some(b)) => !holds_equal(a, b),
            _ => false,
        }
    }
}

impl PanelView for View {
    type Hold = PageRotation;
    type Landmark = Landmark;

    #[inline]
    fn logical_size(&self) -> (u16, u16) {
        (*self).logical_size()
    }

    #[inline]
    fn map_draw(&self, x: u16, y: u16) -> Option<(u16, u16)> {
        (*self).map_draw(x, y)
    }

    #[inline]
    fn map_touch_framebuffer(&self, fx: u16, fy: u16) -> Option<(u16, u16)> {
        (*self).map_touch_framebuffer(fx, fy)
    }

    #[inline]
    fn map_touch_raw(&self, cx: u32, cy: u32) -> Option<(u16, u16)> {
        (*self).map_touch_raw(cx, cy)
    }

    #[inline]
    fn landmark(&self, mark: Landmark) -> Option<(u16, u16)> {
        (*self).landmark(mark)
    }

    #[inline]
    fn current_hold(&self) -> Option<PageRotation> {
        (*self).current_hold()
    }

    #[inline]
    fn set_hold(&mut self, hold: PageRotation) {
        View::set_hold(self, hold);
    }

    #[inline]
    fn is_native(&self) -> bool {
        (*self).is_native()
    }
}

/// USB-down portrait page point for a landmark (diagram fractions).
const fn enclosure_portrait0(mark: Landmark) -> (u16, u16) {
    match mark {
        Landmark::UsbC => (PAGE_WIDTH * 4 / 5, PAGE_HEIGHT - 1),
        Landmark::Reset => (PAGE_WIDTH / 6, PAGE_HEIGHT - 1),
        Landmark::Microphone => (PAGE_WIDTH * 2 / 6, PAGE_HEIGHT - 1),
        Landmark::ButtonAi => (PAGE_WIDTH - 1, PAGE_HEIGHT / 6),
        Landmark::ButtonUp => (PAGE_WIDTH - 1, PAGE_HEIGHT / 3),
        Landmark::ButtonDown => (PAGE_WIDTH - 1, PAGE_HEIGHT / 2),
        Landmark::SdSlot => (0, PAGE_HEIGHT * 3 / 4),
    }
}

const fn holds_equal(a: PageRotation, b: PageRotation) -> bool {
    matches!(
        (a, b),
        (PageRotation::Portrait0, PageRotation::Portrait0)
            | (PageRotation::Portrait180, PageRotation::Portrait180)
            | (PageRotation::Landscape0, PageRotation::Landscape0)
            | (PageRotation::Landscape180, PageRotation::Landscape180)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{framebuffer_to_page, gray4_touch_framebuffer, page_to_framebuffer};

    const HOLDS: [PageRotation; 4] = [
        PageRotation::Portrait0,
        PageRotation::Portrait180,
        PageRotation::Landscape0,
        PageRotation::Landscape180,
    ];

    #[test]
    fn native_is_the_default_and_is_identity() {
        let view = View::builder().build();
        assert!(view.is_native());
        assert_eq!(view, View::native());
        assert_eq!(view.logical_size(), (WIDTH, HEIGHT));
        assert_eq!(view.current_hold(), None);
        assert_eq!(view.map_draw(0, 0), Some((0, 0)));
        assert_eq!(view.map_draw(WIDTH - 1, HEIGHT - 1), Some((799, 479)));
        assert_eq!(view.map_draw(WIDTH, 0), None);
        assert_eq!(view.map_touch_framebuffer(120, 290), Some((120, 290)));
        assert_eq!(view.map_touch_framebuffer(WIDTH, 0), None);
    }

    #[test]
    fn hold_map_draw_matches_page_to_framebuffer() {
        for rotation in HOLDS {
            let view = View::from_hold(rotation);
            assert_eq!(view.current_hold(), Some(rotation));
            assert_eq!(view.logical_size(), rotation.page_size());
            let (w, h) = rotation.page_size();
            for (x, y) in [
                (0, 0),
                (w - 1, 0),
                (0, h - 1),
                (w - 1, h - 1),
                (w / 2, h / 2),
            ] {
                assert_eq!(
                    view.map_draw(x, y),
                    page_to_framebuffer(x, y, rotation),
                    "{rotation:?} draw ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn hold_touch_uses_gray4_then_page() {
        for rotation in HOLDS {
            let view = View::from_hold(rotation);
            for (fx, fy) in [(0, 0), (799, 0), (0, 479), (799, 479), (120, 290)] {
                let expected = gray4_touch_framebuffer(fx, fy, rotation)
                    .and_then(|(hx, hy)| framebuffer_to_page(hx, hy, rotation));
                assert_eq!(
                    view.map_touch_framebuffer(fx, fy),
                    expected,
                    "{rotation:?} touch ({fx},{fy})"
                );
            }
        }
    }

    #[test]
    fn landscape_touch_is_only_the_otp_180() {
        let (x, y, w, h) = (
            80u16,
            HEIGHT.saturating_sub(100),
            WIDTH.saturating_sub(160),
            72u16,
        );
        let cx = x + w / 2;
        let cy = y + h / 2;
        for rotation in [PageRotation::Landscape0, PageRotation::Landscape180] {
            let view = View::from_hold(rotation);
            let (fx, fy) = page_to_framebuffer(cx, cy, rotation).expect("canvas");
            let sx = WIDTH - 1 - fx;
            let sy = HEIGHT - 1 - fy;
            let visible = view.map_touch_framebuffer(sx, sy).expect("visible");
            assert_eq!(
                visible,
                (cx, cy),
                "{rotation:?} OTP 180 must land on the ink button"
            );
            let opposite = view.map_touch_framebuffer(fx, fy).expect("opposite");
            assert_ne!(
                opposite,
                (cx, cy),
                "{rotation:?} canvas path must not be the ink"
            );
        }
    }

    #[test]
    fn portrait0_landmarks_sit_on_the_enclosure_edges() {
        let view = View::from_hold(PageRotation::Portrait0);
        let (uw, uh) = view.logical_size();
        let (ux, uy) = view.landmark(Landmark::UsbC).expect("usb");
        assert_eq!(uy, uh - 1, "USB-C on the bottom short edge");
        assert!(ux > uw / 2, "USB-C toward the right of that edge");
        let (rx, ry) = view.landmark(Landmark::Reset).expect("reset");
        assert_eq!(ry, uh - 1);
        assert!(rx < ux);
        let (kx, ky) = view.landmark(Landmark::ButtonAi).expect("ai");
        assert_eq!(kx, uw - 1, "keys on the right long edge");
        assert!(ky < uh / 2);
        let (sx, _sy) = view.landmark(Landmark::SdSlot).expect("sd");
        assert_eq!(sx, 0, "SD on the left long edge");
    }

    #[test]
    fn landmarks_follow_the_hold() {
        let usb_l0 = View::from_hold(PageRotation::Landscape0)
            .landmark(Landmark::UsbC)
            .expect("l0");
        assert_eq!(usb_l0.0, WIDTH - 1, "USB-C on the right in Landscape0");
        let usb_l180 = View::from_hold(PageRotation::Landscape180)
            .landmark(Landmark::UsbC)
            .expect("l180");
        assert_eq!(usb_l180.0, 0, "USB-C on the left in Landscape180");
        let usb_p180 = View::from_hold(PageRotation::Portrait180)
            .landmark(Landmark::UsbC)
            .expect("p180");
        assert_eq!(usb_p180.1, 0, "USB-C on the top in Portrait180");

        let native = View::native().landmark(Landmark::UsbC).expect("native");
        let p0 = enclosure_portrait0(Landmark::UsbC);
        assert_eq!(
            native,
            page_to_framebuffer(p0.0, p0.1, PageRotation::Portrait0).expect("fb")
        );
    }

    #[test]
    fn needs_redraw_ignores_flat_and_unknown() {
        assert!(View::needs_redraw(
            Some(PageRotation::Portrait0),
            Some(PageRotation::Landscape0)
        ));
        assert!(!View::needs_redraw(
            Some(PageRotation::Portrait0),
            Some(PageRotation::Portrait0)
        ));
        assert!(!View::needs_redraw(Some(PageRotation::Portrait0), None));
        assert!(!View::needs_redraw(None, Some(PageRotation::Portrait0)));
        assert!(!View::needs_redraw(None, None));
    }

    fn draw_via_trait<V: PanelView>(view: &V, x: u16, y: u16) -> Option<(u16, u16)> {
        view.map_draw(x, y)
    }

    #[test]
    fn sticky_view_answers_the_shared_trait() {
        let view = View::native();
        assert_eq!(draw_via_trait(&view, 0, 0), Some((0, 0)));
        assert_eq!(
            PanelView::map_touch_framebuffer(&view, 120, 290),
            Some((120, 290))
        );
        assert!(PanelView::is_native(&view));
        assert!(!View::hold_changed(
            Some(PageRotation::Portrait0),
            Some(PageRotation::Portrait0)
        ));
        assert!(View::hold_changed(
            Some(PageRotation::Portrait0),
            Some(PageRotation::Landscape0)
        ));
    }

    #[test]
    fn with_hold_opts_into_cost() {
        let mut view = View::native();
        view.set_hold(PageRotation::Portrait0);
        assert_eq!(view, View::from_hold(PageRotation::Portrait0));
        assert!(!view.is_native());
    }

    /// UART `p0=679,189` while `imu=Portrait0` is glass, not START.
    ///
    /// Treating those digits as a framebuffer tap misses the strip.
    /// [`GlassPoint::to_framebuffer`] lands on page ≈ `(189, 679)`.
    #[test]
    fn glass_as_framebuffer_misses_portrait0_start() {
        use crate::coords::{FramebufferPoint, GlassPoint, HitRect};

        let view = View::from_hold(PageRotation::Portrait0);
        let start = HitRect {
            x: 50,
            y: PAGE_HEIGHT.saturating_sub(150),
            w: PAGE_WIDTH.saturating_sub(100),
            h: 90,
            slop: 10,
        };
        let glass = GlassPoint { x: 679, y: 189 };
        let wrong = FramebufferPoint {
            x: glass.x,
            y: glass.y,
        };
        assert!(
            !view.hit_framebuffer(wrong, start),
            "glass digits used as canvas must miss START"
        );
        let fb = glass.to_framebuffer().expect("on panel");
        assert_eq!(fb, FramebufferPoint { x: 120, y: 290 });
        assert!(view.hit_framebuffer(fb, start), "undo 180° must hit START");
        let page = view.map_touch_point(fb).expect("in page");
        assert!((50..430).contains(&page.x));
        assert!((650..740).contains(&page.y));
    }
}
