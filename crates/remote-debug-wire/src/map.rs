//! Map proto injects onto [`panel_view`] types and product GPIO numbers.

use panel_view::{ExpectedFrame, FrameKind, TouchSample, TouchSource};

use crate::v1::{self, ProductKey};

/// Sticky AI Voice / OK (GPIO4).
pub const PRODUCT_KEY_OK_GPIO: u8 = 4;

/// Sticky Page Up (GPIO5).
pub const PRODUCT_KEY_PAGE_UP_GPIO: u8 = 5;

/// Sticky Page Down (GPIO6).
pub const PRODUCT_KEY_PAGE_DOWN_GPIO: u8 = 6;

/// Why an inject could not be mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    /// `x` / `y` does not fit in a framebuffer `u16`.
    Coord,
    /// `slot` is outside 0..=4.
    Slot,
    /// `source` is not SYNTHETIC (injects are never physical).
    Source,
    /// `key` is unspecified.
    Key,
    /// `kind` is unspecified or unknown.
    Kind,
    /// Width / height / plane lengths fail [`ExpectedFrame::is_consistent`].
    Frame,
}

/// `InjectTouch` → [`TouchSample`] in pre-rotation framebuffer pixels.
///
/// # Errors
///
/// [`MapError`] when the coordinates, slot, or source are not a
/// synthetic framebuffer tap.
pub fn inject_touch_sample(msg: &v1::InjectTouch) -> Result<TouchSample, MapError> {
    if msg.x > u32::from(u16::MAX) || msg.y > u32::from(u16::MAX) {
        return Err(MapError::Coord);
    }
    let slot = match msg.slot {
        None => None,
        Some(s) if s <= 4 => Some(s as u8),
        Some(_) => return Err(MapError::Slot),
    };
    let source = match msg.source.as_known() {
        Some(v1::TouchSource::TOUCH_SOURCE_SYNTHETIC) => TouchSource::Synthetic,
        _ => return Err(MapError::Source),
    };
    Ok(TouchSample {
        source,
        x: msg.x as u16,
        y: msg.y as u16,
        slot,
    })
}

/// Product key → Sticky GPIO. Unspecified is [`MapError::Key`].
///
/// # Errors
///
/// [`MapError::Key`] when the enum is unspecified or unknown.
pub fn inject_button_gpio(key: &buffa::EnumValue<ProductKey>) -> Result<u8, MapError> {
    match key.as_known() {
        Some(ProductKey::PRODUCT_KEY_OK) => Ok(PRODUCT_KEY_OK_GPIO),
        Some(ProductKey::PRODUCT_KEY_PAGE_UP) => Ok(PRODUCT_KEY_PAGE_UP_GPIO),
        Some(ProductKey::PRODUCT_KEY_PAGE_DOWN) => Ok(PRODUCT_KEY_PAGE_DOWN_GPIO),
        _ => Err(MapError::Key),
    }
}

/// GPIO 4/5/6 → product key. Other pads are [`None`].
#[must_use]
pub fn product_key_from_gpio(gpio: u8) -> Option<ProductKey> {
    match gpio {
        PRODUCT_KEY_OK_GPIO => Some(ProductKey::PRODUCT_KEY_OK),
        PRODUCT_KEY_PAGE_UP_GPIO => Some(ProductKey::PRODUCT_KEY_PAGE_UP),
        PRODUCT_KEY_PAGE_DOWN_GPIO => Some(ProductKey::PRODUCT_KEY_PAGE_DOWN),
        _ => None,
    }
}

/// Pass-through for a product hold token (`optional uint32`).
#[inline]
#[must_use]
pub const fn hold_to_u32(hold: u32) -> u32 {
    hold
}

/// Pass-through for a product hold token.
#[inline]
#[must_use]
pub const fn hold_from_u32(hold: u32) -> u32 {
    hold
}

/// [`panel_view::FrameKind`] → wire enum.
#[must_use]
pub const fn frame_kind_to_wire(kind: FrameKind) -> v1::FrameKind {
    match kind {
        FrameKind::Mono => v1::FrameKind::FRAME_KIND_MONO,
        FrameKind::Gray4 => v1::FrameKind::FRAME_KIND_GRAY4,
    }
}

/// Wire enum → [`panel_view::FrameKind`].
///
/// # Errors
///
/// [`MapError::Kind`] when the value is unspecified or unknown.
pub fn frame_kind_from_wire(kind: &buffa::EnumValue<v1::FrameKind>) -> Result<FrameKind, MapError> {
    match kind.as_known() {
        Some(v1::FrameKind::FRAME_KIND_MONO) => Ok(FrameKind::Mono),
        Some(v1::FrameKind::FRAME_KIND_GRAY4) => Ok(FrameKind::Gray4),
        _ => Err(MapError::Kind),
    }
}

/// Borrowed [`ExpectedFrame`] from a decoded [`v1::Snapshot`].
///
/// Empty `red` is `None` (mono). Host tests use tiny planes; do not
/// call this on device to clone 48 KiB.
///
/// # Errors
///
/// [`MapError::Kind`] or [`MapError::Frame`] when the meta is not a
/// consistent packed 1-bit canvas.
pub fn snapshot_expected(msg: &v1::Snapshot) -> Result<ExpectedFrame<'_, u32>, MapError> {
    if msg.width > u32::from(u16::MAX) || msg.height > u32::from(u16::MAX) {
        return Err(MapError::Frame);
    }
    let kind = frame_kind_from_wire(&msg.kind)?;
    let red = match kind {
        FrameKind::Mono => {
            if !msg.red.is_empty() {
                return Err(MapError::Frame);
            }
            None
        }
        FrameKind::Gray4 => Some(msg.red.as_slice()),
    };
    let frame = ExpectedFrame {
        width: msg.width as u16,
        height: msg.height as u16,
        kind,
        hold: msg.hold,
        bw: msg.bw.as_slice(),
        red,
    };
    if frame.is_consistent() {
        Ok(frame)
    } else {
        Err(MapError::Frame)
    }
}
