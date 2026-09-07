//! Snapshot and tagged-touch companions for [`crate::PanelView`].
//!
//! [`crate::PanelView`] remaps coordinates only. It does not own pixel
//! buffers. A firmware or host debugger that wants the last composed
//! frame implements [`PanelCapture`]. Physical glass and a later
//! radio injector share [`PanelTouchAlign`] so both sources hit-test
//! the same framebuffer point.

use crate::PanelView;

/// Who produced a tap in the panel framebuffer.
///
/// [`Self::Physical`] is the digitizer (GT911 on the Sticky).
/// [`Self::Synthetic`] is a debugger inject. Both must use the same
/// pre-rotation framebuffer space as [`ExpectedFrame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchSource {
    /// Contact from the product digitizer.
    Physical,
    /// Injected sample (desk debug). Not a finger on the glass.
    Synthetic,
}

impl TouchSource {
    /// UART token when a firmware feature prints `src=`.
    #[inline]
    #[must_use]
    pub const fn as_uart_token(self) -> &'static str {
        match self {
            Self::Physical => "phys",
            Self::Synthetic => "syn",
        }
    }
}

/// One tap in **pre-rotation panel framebuffer** pixels.
///
/// `x` / `y` match [`ExpectedFrame`] (native RAM), not UART glass
/// `p0=` / screen space, and not a raw GT911 480×800 sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchSample {
    /// Physical glass or a debugger inject.
    pub source: TouchSource,
    /// Framebuffer X.
    pub x: u16,
    /// Framebuffer Y.
    pub y: u16,
    /// Optional contact slot (0..=4 on a five-point GT911).
    pub slot: Option<u8>,
}

impl TouchSample {
    /// Framebuffer tap with no slot.
    #[inline]
    #[must_use]
    pub const fn new(source: TouchSource, x: u16, y: u16) -> Self {
        Self {
            source,
            x,
            y,
            slot: None,
        }
    }
}

/// How many 1-bit planes the last compose used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// One plane (OTP 1-bit full / partial).
    Mono,
    /// Black/white plus red/gray plane (OTP gray4).
    Gray4,
}

/// Last composed native panel RAM (what firmware expects to send).
///
/// `width` × `height` is the **pre-rotation** canvas (800×480 on the
/// Sticky). `hold` is the product pose used to compose, or `None` on
/// a native identity canvas. This is not a panel readout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedFrame<'a, Hold> {
    /// Canvas width in pixels.
    pub width: u16,
    /// Canvas height in pixels.
    pub height: u16,
    /// One plane or the gray4 pair.
    pub kind: FrameKind,
    /// In-plane hold at compose time, if the implementor used one.
    pub hold: Option<Hold>,
    /// Black/white 1-bit plane, packed MSB-first, `width/8` stride.
    pub bw: &'a [u8],
    /// Second gray4 plane, or `None` on [`FrameKind::Mono`].
    pub red: Option<&'a [u8]>,
}

impl<Hold> ExpectedFrame<'_, Hold> {
    /// Packed bytes for one 1-bit plane of `width` × `height`.
    #[inline]
    #[must_use]
    pub const fn plane_bytes(width: u16, height: u16) -> usize {
        (width as usize / 8).saturating_mul(height as usize)
    }

    /// `bw` / `red` lengths match [`Self::kind`] and the canvas size.
    #[inline]
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        if self.width == 0 || self.height == 0 || self.width % 8 != 0 {
            return false;
        }
        let n = Self::plane_bytes(self.width, self.height);
        if self.bw.len() != n {
            return false;
        }
        match self.kind {
            FrameKind::Mono => self.red.is_none(),
            FrameKind::Gray4 => self.red.is_some_and(|red| red.len() == n),
        }
    }
}

/// Last expected compose. The implementor owns the plane bytes.
///
/// [`crate::PanelView`] (and Sticky `View`) do **not** implement this.
/// Firmware publishes a cell after draw; a host stub holds a tiny buffer.
pub trait PanelCapture {
    /// Same hold type as the product [`PanelView::Hold`].
    type Hold: Copy + Eq;

    /// Native RAM last composed, or `None` before the first paint.
    fn expected_frame(&self) -> Option<ExpectedFrame<'_, Self::Hold>>;
}

/// Map a tagged framebuffer tap through [`PanelView::map_touch_framebuffer`].
///
/// [`TouchSource`] does not change the algebra. Physical and Synthetic
/// at the same `(x, y)` must return the same logical page.
pub trait PanelTouchAlign: PanelView {
    /// Framebuffer sample → logical page. Source is ignored.
    #[inline]
    #[must_use]
    fn align_touch(&self, sample: TouchSample) -> Option<(u16, u16)> {
        self.map_touch_framebuffer(sample.x, sample.y)
    }
}

impl<T: PanelView> PanelTouchAlign for T {}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use crate::PanelView;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Hold {
        A,
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

    struct StubCapture {
        bw: [u8; 4],
        red: [u8; 4],
        kind: FrameKind,
    }

    impl PanelCapture for StubCapture {
        type Hold = Hold;

        fn expected_frame(&self) -> Option<ExpectedFrame<'_, Hold>> {
            Some(ExpectedFrame {
                width: 8,
                height: 4,
                kind: self.kind,
                hold: Some(Hold::A),
                bw: &self.bw,
                red: match self.kind {
                    FrameKind::Mono => None,
                    FrameKind::Gray4 => Some(&self.red),
                },
            })
        }
    }

    #[test]
    fn physical_and_synthetic_share_the_framebuffer_map() {
        let view = Native;
        let phys = TouchSample::new(TouchSource::Physical, 3, 1);
        let syn = TouchSample::new(TouchSource::Synthetic, 3, 1);
        assert_eq!(view.align_touch(phys), view.align_touch(syn));
        assert_eq!(view.align_touch(phys), Some((3, 1)));
        assert_eq!(
            view.align_touch(TouchSample::new(TouchSource::Synthetic, 9, 0)),
            None
        );
    }

    #[test]
    fn gray4_frame_lengths_match_kind() {
        let cap = StubCapture {
            bw: [0; 4],
            red: [0xff; 4],
            kind: FrameKind::Gray4,
        };
        let frame = cap.expected_frame().expect("published");
        assert!(frame.is_consistent());
        assert_eq!(ExpectedFrame::<Hold>::plane_bytes(8, 4), 4);
    }

    #[test]
    fn mono_frame_rejects_a_red_plane() {
        let frame = ExpectedFrame::<Hold> {
            width: 8,
            height: 4,
            kind: FrameKind::Mono,
            hold: None,
            bw: &[0; 4],
            red: Some(&[0; 4]),
        };
        assert!(!frame.is_consistent());
    }

    #[test]
    fn uart_tokens_are_short() {
        assert_eq!(TouchSource::Physical.as_uart_token(), "phys");
        assert_eq!(TouchSource::Synthetic.as_uart_token(), "syn");
    }
}
