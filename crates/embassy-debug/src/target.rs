//! Touch-validation marks and UART tokens.
//!
//! Five dots plus two midline slides, in **page** pixels for the current
//! IMU hold. Completing the last mark wraps to the first (`target loop`);
//! there is no white end card. Firmware paints and hit-tests; this
//! module owns the numbers so a host test can share them.

use crate::{FormatError, LOG_PREFIX};

/// Corner / slide-end inset from the page edge, in page pixels.
///
/// Same 80 px gutter as the PaperMono targets walk on a 480×800
/// portrait page. Landscape uses the same inset on 800×480.
pub const TARGET_INSET_PX: u16 = 80;

/// Drawn disk radius, in page pixels. Smaller than [`TARGET_SLOP_PX`].
pub const TARGET_RADIUS_PX: u16 = 48;

/// Euclidean hit slop around a dot, and on-line slop for a slide.
pub const TARGET_SLOP_PX: u16 = 100;

/// How close a slide must come to each end (page pixels).
pub const TARGET_SLIDE_END_INSET: u16 = 80;

/// First slide (`slide_x`). Dots are `0..=4`.
pub const TARGET_SLIDE_X_ID: u8 = 5;

/// Last mark (`slide_y`). After this, the walk wraps to `0`.
pub const TARGET_SLIDE_Y_ID: u8 = 6;

/// Inclusive last mark id.
pub const TARGET_LAST_ID: u8 = TARGET_SLIDE_Y_ID;

/// What the glass is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// Circular tap.
    Dot,
    /// Horizontal midline swipe, near edge to near edge.
    SlideX,
    /// Vertical midline swipe, near edge to near edge.
    SlideY,
}

impl TargetKind {
    /// UART `kind=` token.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dot => "dot",
            Self::SlideX => "slide_x",
            Self::SlideY => "slide_y",
        }
    }
}

/// One mark in **page** pixels for the current hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetMark {
    /// Walk index (`0..=6`).
    pub id: u8,
    /// Dot or slide axis.
    pub kind: TargetKind,
    /// Dot centre, or the slide's constant axis (midline).
    pub x: u16,
    /// Dot centre, or the slide's constant axis (midline).
    pub y: u16,
    /// Drawn radius (dots) or half-width hint (slides).
    pub r: u16,
}

/// Verb on a `target` UART line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetVerb {
    /// Card painted this mark (`target show`).
    Show,
    /// Tap / swipe scored (`target hit`).
    Hit,
    /// Tap missed the slop (`target miss`). Slides do not miss.
    Miss,
    /// Last mark scored; walk restarted at id 0 (`target loop`).
    Loop,
}

impl TargetVerb {
    /// UART verb token after `target `.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Loop => "loop",
        }
    }
}

/// Next mark id, wrapping after [`TARGET_LAST_ID`].
#[inline]
#[must_use]
pub const fn next_target_id(id: u8) -> u8 {
    if id >= TARGET_LAST_ID {
        0
    } else {
        id.saturating_add(1)
    }
}

/// Page-space mark for `id` on a page of `(page_w, page_h)`.
///
/// Portrait 480×800 and landscape 800×480 share the same fractions:
/// centre, then four inset corners, then the two midlines. Do not
/// reuse a 480×800 table on a landscape page.
#[must_use]
pub const fn target_mark(id: u8, page_w: u16, page_h: u16) -> TargetMark {
    let mid_x = page_w / 2;
    let mid_y = page_h / 2;
    let x_hi = page_w.saturating_sub(TARGET_INSET_PX);
    let y_hi = page_h.saturating_sub(TARGET_INSET_PX);
    match id {
        TARGET_SLIDE_X_ID => TargetMark {
            id,
            kind: TargetKind::SlideX,
            x: mid_x,
            y: mid_y,
            r: TARGET_SLOP_PX,
        },
        TARGET_SLIDE_Y_ID => TargetMark {
            id,
            kind: TargetKind::SlideY,
            x: mid_x,
            y: mid_y,
            r: TARGET_SLOP_PX,
        },
        1 => TargetMark {
            id,
            kind: TargetKind::Dot,
            x: TARGET_INSET_PX,
            y: TARGET_INSET_PX,
            r: TARGET_RADIUS_PX,
        },
        2 => TargetMark {
            id,
            kind: TargetKind::Dot,
            x: x_hi,
            y: TARGET_INSET_PX,
            r: TARGET_RADIUS_PX,
        },
        3 => TargetMark {
            id,
            kind: TargetKind::Dot,
            x: TARGET_INSET_PX,
            y: y_hi,
            r: TARGET_RADIUS_PX,
        },
        4 => TargetMark {
            id,
            kind: TargetKind::Dot,
            x: x_hi,
            y: y_hi,
            r: TARGET_RADIUS_PX,
        },
        _ => TargetMark {
            id: 0,
            kind: TargetKind::Dot,
            x: mid_x,
            y: mid_y,
            r: TARGET_RADIUS_PX,
        },
    }
}

/// Integer Euclidean distance in page pixels.
#[must_use]
pub fn dist_px(ax: u16, ay: u16, bx: u16, by: u16) -> u16 {
    let dx = i32::from(ax) - i32::from(bx);
    let dy = i32::from(ay) - i32::from(by);
    isqrt_u32(
        dx.unsigned_abs()
            .saturating_mul(dx.unsigned_abs())
            .saturating_add(dy.unsigned_abs().saturating_mul(dy.unsigned_abs())),
    )
}

/// True when a page tap is inside [`TARGET_SLOP_PX`] of a dot.
#[must_use]
pub fn dot_hit(px: u16, py: u16, mark: TargetMark) -> Option<u16> {
    if !matches!(mark.kind, TargetKind::Dot) {
        return None;
    }
    let d = dist_px(px, py, mark.x, mark.y);
    (d <= TARGET_SLOP_PX).then_some(d)
}

/// True when a page sample is on a slide line (slop on the constant axis).
#[must_use]
pub fn slide_on_line(px: u16, py: u16, mark: TargetMark) -> bool {
    match mark.kind {
        TargetKind::SlideX => py.abs_diff(mark.y) <= TARGET_SLOP_PX,
        TargetKind::SlideY => px.abs_diff(mark.x) <= TARGET_SLOP_PX,
        TargetKind::Dot => false,
    }
}

/// Axis value to accumulate for a slide (`x` for [`TargetKind::SlideX`]).
#[must_use]
pub fn slide_axis_value(px: u16, py: u16, mark: TargetMark) -> Option<u16> {
    match mark.kind {
        TargetKind::SlideX => Some(px),
        TargetKind::SlideY => Some(py),
        TargetKind::Dot => None,
    }
}

/// Page length along a slide (width for X, height for Y).
#[must_use]
pub const fn slide_page_len(mark: TargetMark, page_w: u16, page_h: u16) -> u16 {
    match mark.kind {
        TargetKind::SlideX => page_w,
        TargetKind::SlideY => page_h,
        TargetKind::Dot => 0,
    }
}

/// True when a slide span has reached both inset ends.
#[must_use]
pub const fn slide_complete(min_v: u16, max_v: u16, page_len: u16) -> bool {
    let hi = page_len.saturating_sub(TARGET_SLIDE_END_INSET);
    min_v <= TARGET_SLIDE_END_INSET && max_v >= hi
}

/// Fields for one `target` UART line (same layout as [`crate::Event::Target`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetLine {
    /// Milliseconds since boot.
    pub t_ms: u32,
    /// `show` / `hit` / `miss` / `loop`.
    pub verb: TargetVerb,
    /// Walk index (`0..=6`). Unused on `loop`.
    pub id: u8,
    /// Dot or slide axis.
    pub kind: TargetKind,
    /// Tap (or last slide sample) in page pixels.
    pub page_x: u16,
    /// Tap (or last slide sample) in page pixels.
    pub page_y: u16,
    /// Painted mark centre.
    pub expect_x: u16,
    /// Painted mark centre.
    pub expect_y: u16,
    /// `r=` on show, `d=` on a dot, `span=` on a slide.
    pub metric: u16,
}

/// `target show|hit|miss` plus page / expect / metric, or `target loop`.
pub fn format_target(line: TargetLine, buf: &mut [u8]) -> Result<&str, FormatError> {
    let TargetLine {
        t_ms,
        verb,
        id,
        kind,
        page_x,
        page_y,
        expect_x,
        expect_y,
        metric,
    } = line;
    match verb {
        TargetVerb::Loop => crate::write_into(
            buf,
            format_args!("{LOG_PREFIX}: t={t_ms} target loop"),
        ),
        TargetVerb::Show => crate::write_into(
            buf,
            format_args!(
                "{LOG_PREFIX}: t={t_ms} target show id={id} kind={} page={page_x},{page_y} r={metric}",
                kind.as_str()
            ),
        ),
        TargetVerb::Hit if matches!(kind, TargetKind::SlideX | TargetKind::SlideY) => {
            crate::write_into(
                buf,
                format_args!(
                    "{LOG_PREFIX}: t={t_ms} target hit id={id} kind={} page={page_x},{page_y} expect={expect_x},{expect_y} span={metric}",
                    kind.as_str()
                ),
            )
        }
        TargetVerb::Hit | TargetVerb::Miss => crate::write_into(
            buf,
            format_args!(
                "{LOG_PREFIX}: t={t_ms} target {} id={id} kind={} page={page_x},{page_y} expect={expect_x},{expect_y} d={metric}",
                verb.as_str(),
                kind.as_str()
            ),
        ),
    }
}

fn isqrt_u32(n: u32) -> u16 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x.min(u32::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use seeed_reterminal_sticky::display::{PageRotation, HEIGHT, PAGE_HEIGHT, PAGE_WIDTH, WIDTH};
    use std::string::String;

    fn line(
        verb: TargetVerb,
        id: u8,
        kind: TargetKind,
        page: (u16, u16),
        expect: (u16, u16),
        metric: u16,
    ) -> String {
        let mut buf = [0u8; crate::LINE_CAPACITY];
        String::from(
            format_target(
                TargetLine {
                    t_ms: 9,
                    verb,
                    id,
                    kind,
                    page_x: page.0,
                    page_y: page.1,
                    expect_x: expect.0,
                    expect_y: expect.1,
                    metric,
                },
                &mut buf,
            )
            .unwrap(),
        )
    }

    #[test]
    fn portrait_marks_match_the_papermono_gutter() {
        let (w, h) = PageRotation::Portrait0.page_size();
        assert_eq!((w, h), (PAGE_WIDTH, PAGE_HEIGHT));
        let c = target_mark(0, w, h);
        assert_eq!((c.x, c.y), (240, 400));
        assert_eq!(target_mark(1, w, h).x, 80);
        assert_eq!(target_mark(2, w, h).x, 400);
        assert_eq!(target_mark(3, w, h).y, 720);
        assert_eq!(target_mark(4, w, h).y, 720);
    }

    #[test]
    fn landscape_marks_use_the_800x480_page() {
        let (w, h) = PageRotation::Landscape0.page_size();
        assert_eq!((w, h), (WIDTH, HEIGHT));
        let c = target_mark(0, w, h);
        assert_eq!((c.x, c.y), (400, 240));
        assert_eq!(
            target_mark(1, w, h),
            TargetMark {
                id: 1,
                kind: TargetKind::Dot,
                x: 80,
                y: 80,
                r: TARGET_RADIUS_PX,
            }
        );
        assert_eq!(target_mark(2, w, h).x, 720);
        assert_eq!(target_mark(3, w, h).y, 400);
        let sx = target_mark(TARGET_SLIDE_X_ID, w, h);
        assert_eq!(sx.kind, TargetKind::SlideX);
        assert_eq!((sx.x, sx.y), (400, 240));
    }

    #[test]
    fn four_holds_share_page_fractions_not_a_fixed_table() {
        use seeed_reterminal_sticky::view::View;
        use seeed_reterminal_sticky::PagePoint;

        for rotation in [
            PageRotation::Portrait0,
            PageRotation::Portrait180,
            PageRotation::Landscape0,
            PageRotation::Landscape180,
        ] {
            let (w, h) = rotation.page_size();
            let c = target_mark(0, w, h);
            assert_eq!(c.x, w / 2, "{rotation:?}");
            assert_eq!(c.y, h / 2, "{rotation:?}");
            assert!(dot_hit(c.x, c.y, c).is_some());
            let view = View::from_hold(rotation);
            let fb = view
                .map_draw_point(PagePoint { x: c.x, y: c.y })
                .expect("centre on canvas");
            let page = view.map_touch_point(fb).expect("page");
            assert!(
                dot_hit(page.x, page.y, c).is_some(),
                "{rotation:?} draw/touch round-trip missed the disk"
            );
        }
    }

    #[test]
    fn walk_wraps_instead_of_leaving() {
        assert_eq!(next_target_id(0), 1);
        assert_eq!(next_target_id(TARGET_LAST_ID), 0);
    }

    #[test]
    fn slide_needs_both_inset_ends() {
        assert!(!slide_complete(200, 400, 800));
        assert!(slide_complete(80, 720, 800));
        assert!(slide_complete(0, 799, 800));
    }

    #[test]
    fn target_lines_match_the_agreed_shape() {
        assert_eq!(
            line(TargetVerb::Show, 0, TargetKind::Dot, (240, 400), (0, 0), 48),
            "embassy-debug: t=9 target show id=0 kind=dot page=240,400 r=48"
        );
        assert_eq!(
            line(
                TargetVerb::Hit,
                0,
                TargetKind::Dot,
                (238, 402),
                (240, 400),
                3
            ),
            "embassy-debug: t=9 target hit id=0 kind=dot page=238,402 expect=240,400 d=3"
        );
        assert_eq!(
            line(
                TargetVerb::Miss,
                0,
                TargetKind::Dot,
                (100, 100),
                (240, 400),
                280
            ),
            "embassy-debug: t=9 target miss id=0 kind=dot page=100,100 expect=240,400 d=280"
        );
        assert_eq!(
            line(
                TargetVerb::Hit,
                5,
                TargetKind::SlideX,
                (400, 240),
                (400, 240),
                640
            ),
            "embassy-debug: t=9 target hit id=5 kind=slide_x page=400,240 expect=400,240 span=640"
        );
        assert_eq!(
            line(TargetVerb::Loop, 0, TargetKind::Dot, (0, 0), (0, 0), 0),
            "embassy-debug: t=9 target loop"
        );
    }
}
