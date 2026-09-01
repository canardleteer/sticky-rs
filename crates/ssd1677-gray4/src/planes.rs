//! Four-gray plane building and framebuffer transforms.
//!
//! These are pure functions over byte slices: no bus, no pins, no allocation.
//! They are the part of a four-gray driver most likely to be subtly wrong, so
//! they are also the part with exact expected byte counts in tests.
//!
//! # How grayscale works here
//!
//! The SSD1677 was designed for black/white/red. It holds **two** RAM planes
//! (`Write RAM (Black White)` 0x24 and `Write RAM (RED)` 0x26, each 960×680
//! bits — SSD1677 Rev 1.0 section `6.5 RAM`). For each pixel the two bits
//! select one of four waveform slots (LUT0..LUT3), per `Table 6-4 : RAM bit
//! and LUT mapping for 3-color display`:
//!
//! | RED RAM bit | Black/White RAM bit | Waveform slot |
//! | --- | --- | --- |
//! | 0 | 0 | LUT0 |
//! | 0 | 1 | LUT1 |
//! | 1 | 0 | LUT2 |
//! | 1 | 1 | LUT3 |
//!
//! On black-and-white film there is no red ink, so those four slots become
//! four *waveforms* — which is how four gray levels appear. The panel does not
//! have a grayscale mode; factory OTP (one-time programmable memory on the
//! glass) or a microcontroller-written 105-byte table gives it one.
//!
//! The plane mapping and the waveform are **one design**, not two. On the
//! Seeed Sticky the confirmed path is factory OTP with
//! [`PlaneMapping::SEEED_OTP`]. [`PlaneMapping::LUT_INDEX_ORDER`] maps gray
//! level `g` to slot `g` for a microcontroller table whose LUT0..LUT3 are
//! ordered black through white; if your table orders phases differently,
//! supply a matching [`PlaneMapping`] instead of reordering your image data.

/// A four-gray pixel value, as packed in a 2 bits-per-pixel framebuffer.
///
/// Values follow the board contract: `0` is black through `3` is white.
pub mod gray {
    /// Black.
    pub const BLACK: u8 = 0;
    /// Dark gray.
    pub const DARK_GRAY: u8 = 1;
    /// Light gray.
    pub const LIGHT_GRAY: u8 = 2;
    /// White.
    pub const WHITE: u8 = 3;
}

/// Errors from the packing helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackError {
    /// Width is not a multiple of 8, so plane rows would not be byte aligned.
    WidthNotByteAligned {
        /// The offending width in pixels.
        width: usize,
    },
    /// A buffer length did not match the geometry.
    BufferLength {
        /// Bytes the geometry requires.
        expected: usize,
        /// Bytes the caller supplied.
        actual: usize,
    },
}

/// Which RAM bits represent each gray level.
///
/// Index by gray level (0..=3); each entry is `(black_white_bit, red_bit)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneMapping {
    bits: [(bool, bool); 4],
}

impl PlaneMapping {
    /// Maps gray level `g` onto LUT index `g`, using the datasheet's
    /// `(R RAM, B/W RAM)` to LUT-index relationship (Table 6-4):
    /// `index = (red << 1) | black_white`.
    ///
    /// Pair this with a waveform table whose LUT0..LUT3 are ordered black,
    /// dark gray, light gray, white.
    pub const LUT_INDEX_ORDER: Self = Self {
        bits: [
            (false, false), // gray 0 -> LUT0
            (true, false),  // gray 1 -> LUT1
            (false, true),  // gray 2 -> LUT2
            (true, true),   // gray 3 -> LUT3
        ],
    };

    /// Seeed `seeed_epaper` OTP gray4 mapping (stock `reterminal_template`).
    ///
    /// The vendor plane builder takes 2bpp MSB-first pixels, sends bit1 to
    /// DTM1 (command 0x24) and bit0 to DTM2 (command 0x26), then **inverts**
    /// each bit because their OTP RAM polarity is 1 = white. Pair this with
    /// the OTP gray4 refresh ([`crate::sequence::SEEED_GRAY4_TEMPERATURE`],
    /// then [`crate::sequence::UpdateSequence::SEEED_GRAY4`]), not
    /// with an MCU 0x32 table.
    ///
    /// | gray | 2bpp | after invert (BW, RED) | LUT index |
    /// | --- | --- | --- | --- |
    /// | 0 black | 00 | (1, 1) | 3 |
    /// | 1 | 01 | (1, 0) | 1 |
    /// | 2 | 10 | (0, 1) | 2 |
    /// | 3 white | 11 | (0, 0) | 0 |
    pub const SEEED_OTP: Self = Self {
        bits: [
            (true, true),   // gray 0 -> LUT3
            (true, false),  // gray 1 -> LUT1
            (false, true),  // gray 2 -> LUT2
            (false, false), // gray 3 -> LUT0
        ],
    };

    /// Builds a custom mapping. Entries are indexed by gray level and hold
    /// `(black_white_bit, red_bit)`.
    #[inline]
    #[must_use]
    pub const fn new(bits: [(bool, bool); 4]) -> Self {
        Self { bits }
    }

    /// The `(black_white_bit, red_bit)` pair for a gray level.
    ///
    /// Levels above 3 cannot occur in 2bpp data; they saturate to white.
    #[inline]
    #[must_use]
    pub const fn bits_for(&self, gray: u8) -> (bool, bool) {
        self.bits[(gray & 0b11) as usize]
    }
}

/// Bytes per row of a 2bpp four-gray framebuffer.
#[inline]
#[must_use]
pub const fn gray4_stride(width: usize) -> usize {
    width / 4
}

/// Bytes per row of a 1bpp plane.
#[inline]
#[must_use]
pub const fn plane_stride(width: usize) -> usize {
    width / 8
}

/// Splits a 2bpp four-gray framebuffer into the two 1bpp controller planes.
///
/// `source` is MSB-first, 4 pixels per byte. `black_white` receives the plane
/// written with command 0x24 and `red` the plane written with 0x26, both
/// MSB-first, 8 pixels per byte.
///
/// For the Sticky's 800x480 canvas that is 96,000 bytes in and 48,000 bytes
/// into each plane.
pub fn split_gray4(
    source: &[u8],
    width: usize,
    height: usize,
    mapping: &PlaneMapping,
    black_white: &mut [u8],
    red: &mut [u8],
) -> Result<(), PackError> {
    if width % 8 != 0 {
        return Err(PackError::WidthNotByteAligned { width });
    }

    let expected_source = gray4_stride(width) * height;
    if source.len() != expected_source {
        return Err(PackError::BufferLength {
            expected: expected_source,
            actual: source.len(),
        });
    }

    let expected_plane = plane_stride(width) * height;
    for plane in [&*black_white, &*red] {
        if plane.len() != expected_plane {
            return Err(PackError::BufferLength {
                expected: expected_plane,
                actual: plane.len(),
            });
        }
    }

    black_white.fill(0);
    red.fill(0);

    for y in 0..height {
        for x in 0..width {
            let gray = read_gray4(source, width, x, y);
            let (bw_bit, red_bit) = mapping.bits_for(gray);

            let plane_index = y * plane_stride(width) + x / 8;
            let plane_mask = 0x80u8 >> (x % 8);

            if bw_bit {
                black_white[plane_index] |= plane_mask;
            }
            if red_bit {
                red[plane_index] |= plane_mask;
            }
        }
    }

    Ok(())
}

/// Reads one 2bpp pixel. MSB-first: pixel 0 occupies bits 7-6.
#[inline]
#[must_use]
pub fn read_gray4(source: &[u8], width: usize, x: usize, y: usize) -> u8 {
    let byte = source[y * gray4_stride(width) + x / 4];
    let shift = 6 - 2 * (x % 4);
    (byte >> shift) & 0b11
}

/// Writes one 2bpp pixel in place.
#[inline]
pub fn write_gray4(target: &mut [u8], width: usize, x: usize, y: usize, gray: u8) {
    let index = y * gray4_stride(width) + x / 4;
    let shift = 6 - 2 * (x % 4);
    target[index] = (target[index] & !(0b11 << shift)) | ((gray & 0b11) << shift);
}

/// Reduces four-gray to 1bpp: levels >= 2 become white, 0 and 1 become black.
///
/// Matches the board contract's mono conversion, for pages that do not need
/// grayscale and can use a cheaper refresh.
pub fn gray4_to_mono(
    source: &[u8],
    width: usize,
    height: usize,
    mono: &mut [u8],
) -> Result<(), PackError> {
    if width % 8 != 0 {
        return Err(PackError::WidthNotByteAligned { width });
    }

    let expected_source = gray4_stride(width) * height;
    if source.len() != expected_source {
        return Err(PackError::BufferLength {
            expected: expected_source,
            actual: source.len(),
        });
    }

    let expected_mono = plane_stride(width) * height;
    if mono.len() != expected_mono {
        return Err(PackError::BufferLength {
            expected: expected_mono,
            actual: mono.len(),
        });
    }

    mono.fill(0);
    for y in 0..height {
        for x in 0..width {
            if read_gray4(source, width, x, y) >= gray::LIGHT_GRAY {
                mono[y * plane_stride(width) + x / 8] |= 0x80u8 >> (x % 8);
            }
        }
    }

    Ok(())
}

/// Reads one 1bpp pixel. MSB-first: pixel 0 occupies bit 7.
#[inline]
#[must_use]
pub fn read_mono(source: &[u8], width: usize, x: usize, y: usize) -> bool {
    let byte = source[y * plane_stride(width) + x / 8];
    byte & (0x80u8 >> (x % 8)) != 0
}

/// Writes one 1bpp pixel in place. `true` is bit 1 (white in [`gray4_to_mono`]).
#[inline]
pub fn write_mono(target: &mut [u8], width: usize, x: usize, y: usize, white: bool) {
    let index = y * plane_stride(width) + x / 8;
    let mask = 0x80u8 >> (x % 8);
    if white {
        target[index] |= mask;
    } else {
        target[index] &= !mask;
    }
}

/// Rotates a packed 1bpp plane by 180 degrees into `target`.
///
/// Pair with [`mirror_x_plane`] for the Sticky's on-unit stack (packed 180°
/// then Seeed software `mirror_x`). Do not also send Lotus `0x21`.
pub fn rotate180_mono(
    source: &[u8],
    width: usize,
    height: usize,
    target: &mut [u8],
) -> Result<(), PackError> {
    if width % 8 != 0 {
        return Err(PackError::WidthNotByteAligned { width });
    }

    let expected = plane_stride(width) * height;
    if source.len() != expected {
        return Err(PackError::BufferLength {
            expected,
            actual: source.len(),
        });
    }
    if target.len() != expected {
        return Err(PackError::BufferLength {
            expected,
            actual: target.len(),
        });
    }

    for y in 0..height {
        for x in 0..width {
            let white = read_mono(source, width, x, y);
            write_mono(target, width, width - 1 - x, height - 1 - y, white);
        }
    }

    Ok(())
}

/// Seeed software `mirror_x`: reverse the bit order of every plane byte.
///
/// Seeed's open driver bit-reverses the 1bpp rows instead of sending
/// `Display Update Control 1` (`0x21`). Lotus writes `0x21`; this crate does
/// not send that opcode on the Sticky OTP path.
pub fn mirror_x_plane(plane: &mut [u8]) {
    for byte in plane.iter_mut() {
        *byte = byte.reverse_bits();
    }
}

/// Rotates a packed 2bpp framebuffer by 180 degrees into `target`.
///
/// The Sticky needs this plus the controller's horizontal mirror to match
/// glass. Rotating packed data means reversing byte order *and* reversing the
/// four pixels inside each byte, which is exactly the step that is easy to get
/// half right.
pub fn rotate180_gray4(
    source: &[u8],
    width: usize,
    height: usize,
    target: &mut [u8],
) -> Result<(), PackError> {
    let expected = gray4_stride(width) * height;
    if source.len() != expected {
        return Err(PackError::BufferLength {
            expected,
            actual: source.len(),
        });
    }
    if target.len() != expected {
        return Err(PackError::BufferLength {
            expected,
            actual: target.len(),
        });
    }

    for y in 0..height {
        for x in 0..width {
            let gray = read_gray4(source, width, x, y);
            write_gray4(target, width, width - 1 - x, height - 1 - y, gray);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 8;
    const H: usize = 2;

    fn canvas() -> [u8; gray4_stride(W) * H] {
        [0; gray4_stride(W) * H]
    }

    #[test]
    fn sticky_canvas_sizes_match_the_board_contract() {
        assert_eq!(gray4_stride(800), 200);
        assert_eq!(gray4_stride(800) * 480, 96_000);
        assert_eq!(plane_stride(800), 100);
        assert_eq!(plane_stride(800) * 480, 48_000);
    }

    #[test]
    fn pixels_are_msb_first_within_a_byte() {
        let mut buf = canvas();
        write_gray4(&mut buf, W, 0, 0, gray::WHITE);
        assert_eq!(buf[0], 0b1100_0000);

        write_gray4(&mut buf, W, 3, 0, gray::DARK_GRAY);
        assert_eq!(buf[0], 0b1100_0001);

        assert_eq!(read_gray4(&buf, W, 0, 0), gray::WHITE);
        assert_eq!(read_gray4(&buf, W, 3, 0), gray::DARK_GRAY);
    }

    #[test]
    fn split_maps_gray_levels_onto_lut_index_bits() {
        let mut buf = canvas();
        // One pixel of each level, left to right.
        for (x, level) in [gray::BLACK, gray::DARK_GRAY, gray::LIGHT_GRAY, gray::WHITE]
            .into_iter()
            .enumerate()
        {
            write_gray4(&mut buf, W, x, 0, level);
        }

        let mut bw = [0u8; plane_stride(W) * H];
        let mut red = [0u8; plane_stride(W) * H];
        split_gray4(
            &buf,
            W,
            H,
            &PlaneMapping::LUT_INDEX_ORDER,
            &mut bw,
            &mut red,
        )
        .unwrap();

        // index = (red << 1) | bw, so gray 1 and 3 set the B/W bit and
        // gray 2 and 3 set the red bit.
        assert_eq!(bw[0], 0b0101_0000);
        assert_eq!(red[0], 0b0011_0000);
    }

    #[test]
    fn seeed_otp_mapping_inverts_gray_bits_onto_otp_polarity() {
        let mut buf = canvas();
        // Fill is black (0). OTP maps black onto both plane bits, so a zeroed
        // 8-pixel row would set the rest of the plane byte. White is neither
        // bit, matching the four-pixel check below.
        buf.fill(0b1111_1111);
        for (x, level) in [gray::BLACK, gray::DARK_GRAY, gray::LIGHT_GRAY, gray::WHITE]
            .into_iter()
            .enumerate()
        {
            write_gray4(&mut buf, W, x, 0, level);
        }

        let mut bw = [0u8; plane_stride(W) * H];
        let mut red = [0u8; plane_stride(W) * H];
        split_gray4(&buf, W, H, &PlaneMapping::SEEED_OTP, &mut bw, &mut red).unwrap();

        // black -> both planes, dark -> BW only, light -> red only, white -> neither
        assert_eq!(bw[0], 0b1100_0000);
        assert_eq!(red[0], 0b1010_0000);
    }

    #[test]
    fn split_honours_a_custom_mapping() {
        // Background is dark gray, which this mapping sends to LUT0 (no bits
        // set), so only the one black pixel should appear in a plane.
        let mut buf = canvas();
        buf.fill(0b0101_0101);
        write_gray4(&mut buf, W, 0, 0, gray::BLACK);

        // Swap the roles of black and dark gray relative to the default order.
        let mapping =
            PlaneMapping::new([(true, false), (false, false), (false, true), (true, true)]);

        let mut bw = [0u8; plane_stride(W) * H];
        let mut red = [0u8; plane_stride(W) * H];
        split_gray4(&buf, W, H, &mapping, &mut bw, &mut red).unwrap();

        assert_eq!(bw[0], 0b1000_0000);
        assert_eq!(red[0], 0);
    }

    #[test]
    fn split_rejects_mismatched_buffers() {
        let buf = canvas();
        let mut bw = [0u8; 1];
        let mut red = [0u8; plane_stride(W) * H];

        assert_eq!(
            split_gray4(
                &buf,
                W,
                H,
                &PlaneMapping::LUT_INDEX_ORDER,
                &mut bw,
                &mut red
            ),
            Err(PackError::BufferLength {
                expected: plane_stride(W) * H,
                actual: 1
            })
        );
    }

    #[test]
    fn split_rejects_widths_that_are_not_byte_aligned() {
        let buf = [0u8; 1];
        let mut bw = [0u8; 1];
        let mut red = [0u8; 1];
        assert_eq!(
            split_gray4(
                &buf,
                4,
                1,
                &PlaneMapping::LUT_INDEX_ORDER,
                &mut bw,
                &mut red
            ),
            Err(PackError::WidthNotByteAligned { width: 4 })
        );
    }

    #[test]
    fn mono_threshold_is_light_gray_and_above() {
        let mut buf = canvas();
        for (x, level) in [gray::BLACK, gray::DARK_GRAY, gray::LIGHT_GRAY, gray::WHITE]
            .into_iter()
            .enumerate()
        {
            write_gray4(&mut buf, W, x, 0, level);
        }

        let mut mono = [0u8; plane_stride(W) * H];
        gray4_to_mono(&buf, W, H, &mut mono).unwrap();
        assert_eq!(mono[0], 0b0011_0000);
    }

    #[test]
    fn rotate180_reverses_bytes_and_pixels_within_bytes() {
        let mut buf = canvas();
        write_gray4(&mut buf, W, 0, 0, gray::WHITE);
        write_gray4(&mut buf, W, 1, 0, gray::LIGHT_GRAY);

        let mut rotated = canvas();
        rotate180_gray4(&buf, W, H, &mut rotated).unwrap();

        // Top-left pixel lands bottom-right; the neighbour lands next to it.
        assert_eq!(read_gray4(&rotated, W, W - 1, H - 1), gray::WHITE);
        assert_eq!(read_gray4(&rotated, W, W - 2, H - 1), gray::LIGHT_GRAY);
        assert_eq!(read_gray4(&rotated, W, 0, 0), gray::BLACK);
    }

    #[test]
    fn rotate180_twice_is_the_identity() {
        let mut buf = canvas();
        for x in 0..W {
            write_gray4(&mut buf, W, x, 0, (x % 4) as u8);
            write_gray4(&mut buf, W, x, 1, (3 - x % 4) as u8);
        }

        let mut once = canvas();
        let mut twice = canvas();
        rotate180_gray4(&buf, W, H, &mut once).unwrap();
        rotate180_gray4(&once, W, H, &mut twice).unwrap();

        assert_eq!(buf, twice);
    }

    #[test]
    fn mono_pixels_are_msb_first_within_a_byte() {
        let mut buf = [0u8; plane_stride(W) * H];
        write_mono(&mut buf, W, 0, 0, true);
        write_mono(&mut buf, W, 1, 0, true);
        assert_eq!(buf[0], 0b1100_0000);
        assert!(read_mono(&buf, W, 0, 0));
        assert!(!read_mono(&buf, W, 2, 0));
    }

    #[test]
    fn rotate180_mono_moves_the_top_left_pixel() {
        let mut buf = [0u8; plane_stride(W) * H];
        write_mono(&mut buf, W, 0, 0, true);

        let mut rotated = [0u8; plane_stride(W) * H];
        rotate180_mono(&buf, W, H, &mut rotated).unwrap();

        assert!(read_mono(&rotated, W, W - 1, H - 1));
        assert!(!read_mono(&rotated, W, 0, 0));
    }

    #[test]
    fn rotate180_mono_twice_is_the_identity() {
        let mut buf = [0u8; plane_stride(W) * H];
        write_mono(&mut buf, W, 0, 0, true);
        write_mono(&mut buf, W, 3, 1, true);

        let mut once = [0u8; plane_stride(W) * H];
        let mut twice = [0u8; plane_stride(W) * H];
        rotate180_mono(&buf, W, H, &mut once).unwrap();
        rotate180_mono(&once, W, H, &mut twice).unwrap();
        assert_eq!(buf, twice);
    }

    #[test]
    fn mirror_x_reverses_bits_in_each_byte() {
        let mut plane = [0b1000_0001u8, 0b1111_0000];
        mirror_x_plane(&mut plane);
        assert_eq!(plane, [0b1000_0001, 0b0000_1111]);
    }
}
