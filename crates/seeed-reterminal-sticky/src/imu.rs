//! Enclosure orientation from the LSM6DS3TR-C accelerometer.
//!
//! Labels follow the enclosure diagram (glass facing you, USB-C on the
//! bottom short edge is portrait), then the gravity axis seen on a unit.
//! Face-up and face-down are **not** aliases for portrait and landscape:
//! a flat device has no meaningful in-plane reading. Keep the last
//! in-plane page (default USB-down portrait) instead of inventing one.

/// Accelerometer sensitivity at +/-2 g, in g per LSB.
pub const SENSITIVITY_G_PER_LSB: f32 = 0.000_061;

/// Threshold on the dominant axis, in g, that classified placement reliably.
pub const DOMINANT_AXIS_THRESHOLD_G: f32 = 0.70;

/// The same threshold in raw LSB, so classification needs no floating point.
pub const DOMINANT_AXIS_THRESHOLD_LSB: i32 =
    (DOMINANT_AXIS_THRESHOLD_G / SENSITIVITY_G_PER_LSB) as i32;

/// Enclosure orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// USB-C on the bottom short edge. Gravity dominant on −Y.
    Portrait0,
    /// USB-C on the top short edge. Gravity dominant on +Y.
    Portrait180,
    /// USB-C on the right short edge. Gravity dominant on −X.
    Landscape0,
    /// USB-C on the left short edge. Gravity dominant on +X.
    Landscape180,
    /// Gravity dominant on +Z: lying face up.
    FaceUp,
    /// Gravity dominant on −Z: lying face down.
    FaceDown,
}

impl Orientation {
    /// In-plane page for this pose. `None` when the unit is flat.
    #[must_use]
    pub const fn page_rotation(self) -> Option<crate::display::PageRotation> {
        match self {
            Self::Portrait0 => Some(crate::display::PageRotation::Portrait0),
            Self::Portrait180 => Some(crate::display::PageRotation::Portrait180),
            Self::Landscape0 => Some(crate::display::PageRotation::Landscape0),
            Self::Landscape180 => Some(crate::display::PageRotation::Landscape180),
            Self::FaceUp | Self::FaceDown => None,
        }
    }
}

/// Classifies orientation from a raw accelerometer sample.
///
/// Returns `None` when no axis passes [`DOMINANT_AXIS_THRESHOLD_LSB`] — the
/// device is tilted, in motion, or in free fall. Callers should keep the last
/// known orientation rather than inventing one.
#[must_use]
pub fn classify(x: i16, y: i16, z: i16) -> Option<Orientation> {
    let (x, y, z) = (i32::from(x), i32::from(y), i32::from(z));

    let dominant = [x.abs(), y.abs(), z.abs()]
        .into_iter()
        .max()
        .unwrap_or_default();
    if dominant < DOMINANT_AXIS_THRESHOLD_LSB {
        return None;
    }

    Some(if x.abs() == dominant {
        if x < 0 {
            Orientation::Landscape0
        } else {
            Orientation::Landscape180
        }
    } else if y.abs() == dominant {
        if y < 0 {
            Orientation::Portrait0
        } else {
            Orientation::Portrait180
        }
    } else if z > 0 {
        Orientation::FaceUp
    } else {
        Orientation::FaceDown
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One g in raw LSB at +/-2 g.
    const ONE_G: i16 = 16_384;

    #[test]
    fn threshold_matches_the_calibrated_figure() {
        // 0.70 g at 0.000061 g/LSB.
        assert_eq!(DOMINANT_AXIS_THRESHOLD_LSB, 11_475);
    }

    #[test]
    fn each_axis_maps_to_the_enclosure_pose() {
        // USB-C down / up are the Y axis; USB-C right / left are X.
        assert_eq!(classify(0, -ONE_G, 0), Some(Orientation::Portrait0));
        assert_eq!(classify(0, ONE_G, 0), Some(Orientation::Portrait180));
        assert_eq!(classify(-ONE_G, 0, 0), Some(Orientation::Landscape0));
        assert_eq!(classify(ONE_G, 0, 0), Some(Orientation::Landscape180));
        assert_eq!(classify(0, 0, ONE_G), Some(Orientation::FaceUp));
        assert_eq!(classify(0, 0, -ONE_G), Some(Orientation::FaceDown));
    }

    #[test]
    fn an_ambiguous_sample_is_reported_as_unknown() {
        // Tilted 45 degrees on two axes: nothing dominates.
        let component = (0.6 * f32::from(ONE_G)) as i16;
        assert_eq!(classify(component, component, 0), None);
        assert_eq!(classify(0, 0, 0), None);
    }

    #[test]
    fn face_up_is_not_confused_with_portrait() {
        // Flat on a desk with a little X noise still reads face up.
        let noise = 1_000;
        assert_eq!(classify(noise, noise, ONE_G), Some(Orientation::FaceUp));
    }

    #[test]
    fn only_in_plane_poses_have_a_page_rotation() {
        assert_eq!(
            Orientation::Portrait0.page_rotation(),
            Some(crate::display::PageRotation::Portrait0)
        );
        assert_eq!(
            Orientation::Landscape0.page_rotation(),
            Some(crate::display::PageRotation::Landscape0)
        );
        assert_eq!(Orientation::FaceUp.page_rotation(), None);
        assert_eq!(Orientation::FaceDown.page_rotation(), None);
    }
}
