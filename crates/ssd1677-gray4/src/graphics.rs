//! `embedded-graphics` support for a four-gray canvas.
//!
//! [`Gray4Canvas`] borrows a caller-owned 2bpp framebuffer, so the allocation
//! strategy stays with the application — on this board that buffer is 96,000
//! bytes for an 800x480 canvas and generally lives in PSRAM.
//!
//! `embedded-graphics`' [`Gray2`] is exactly four levels, so no lossy
//! conversion happens between drawing and the controller planes.

use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::{Gray2, GrayColor};
use embedded_graphics_core::Pixel;

use crate::planes::{gray4_stride, write_gray4, PackError};

/// A four-gray drawing surface over a borrowed 2bpp framebuffer.
#[derive(Debug)]
pub struct Gray4Canvas<'buf> {
    buffer: &'buf mut [u8],
    width: usize,
    height: usize,
}

impl<'buf> Gray4Canvas<'buf> {
    /// Wraps a framebuffer.
    ///
    /// # Errors
    ///
    /// Fails if the width is not a multiple of 8 (plane rows would not be byte
    /// aligned) or the buffer length does not match `width * height / 4`.
    pub fn new(buffer: &'buf mut [u8], width: usize, height: usize) -> Result<Self, PackError> {
        if width % 8 != 0 {
            return Err(PackError::WidthNotByteAligned { width });
        }

        let expected = gray4_stride(width) * height;
        if buffer.len() != expected {
            return Err(PackError::BufferLength {
                expected,
                actual: buffer.len(),
            });
        }

        Ok(Self {
            buffer,
            width,
            height,
        })
    }

    /// Fills the canvas with one gray level (0 black to 3 white).
    pub fn fill(&mut self, gray: u8) {
        let level = gray & 0b11;
        let byte = level | (level << 2) | (level << 4) | (level << 6);
        self.buffer.fill(byte);
    }

    /// The packed 2bpp framebuffer, ready for [`crate::planes::split_gray4`].
    #[inline]
    #[must_use]
    pub fn buffer(&self) -> &[u8] {
        self.buffer
    }

    /// Canvas width in pixels.
    #[inline]
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Canvas height in pixels.
    #[inline]
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }
}

impl OriginDimensions for Gray4Canvas<'_> {
    fn size(&self) -> Size {
        Size::new(self.width as u32, self.height as u32)
    }
}

impl DrawTarget for Gray4Canvas<'_> {
    type Color = Gray2;
    /// Drawing cannot fail: out-of-bounds pixels are dropped, as
    /// `embedded-graphics` expects of a framebuffer target.
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            let (Ok(x), Ok(y)) = (usize::try_from(point.x), usize::try_from(point.y)) else {
                continue;
            };
            if x >= self.width || y >= self.height {
                continue;
            }
            write_gray4(self.buffer, self.width, x, y, color.luma());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use embedded_graphics::prelude::*;
    use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};

    use super::*;
    use crate::planes::{gray, plane_stride, read_gray4, split_gray4, PlaneMapping};

    const W: usize = 16;
    const H: usize = 4;

    #[test]
    fn rejects_a_mismatched_buffer() {
        let mut buffer = [0u8; 4];
        assert_eq!(
            Gray4Canvas::new(&mut buffer, W, H).unwrap_err(),
            PackError::BufferLength {
                expected: gray4_stride(W) * H,
                actual: 4
            }
        );
    }

    #[test]
    fn fill_writes_every_pixel_of_the_level() {
        let mut buffer = [0u8; gray4_stride(W) * H];
        let mut canvas = Gray4Canvas::new(&mut buffer, W, H).unwrap();
        canvas.fill(gray::WHITE);
        assert!(canvas.buffer().iter().all(|byte| *byte == 0xff));

        canvas.fill(gray::LIGHT_GRAY);
        assert!(canvas.buffer().iter().all(|byte| *byte == 0b1010_1010));
    }

    #[test]
    fn drawing_lands_where_embedded_graphics_says_it_should() {
        let mut buffer = [0u8; gray4_stride(W) * H];
        let mut canvas = Gray4Canvas::new(&mut buffer, W, H).unwrap();
        canvas.fill(gray::WHITE);

        Rectangle::new(Point::new(2, 1), Size::new(3, 2))
            .into_styled(PrimitiveStyle::with_fill(Gray2::new(gray::BLACK)))
            .draw(&mut canvas)
            .unwrap();

        assert_eq!(read_gray4(canvas.buffer(), W, 1, 1), gray::WHITE);
        assert_eq!(read_gray4(canvas.buffer(), W, 2, 1), gray::BLACK);
        assert_eq!(read_gray4(canvas.buffer(), W, 4, 2), gray::BLACK);
        assert_eq!(read_gray4(canvas.buffer(), W, 5, 2), gray::WHITE);
        assert_eq!(read_gray4(canvas.buffer(), W, 2, 3), gray::WHITE);
    }

    #[test]
    fn out_of_bounds_pixels_are_dropped_not_wrapped() {
        let mut buffer = [0u8; gray4_stride(W) * H];
        let mut canvas = Gray4Canvas::new(&mut buffer, W, H).unwrap();
        canvas.fill(gray::WHITE);

        Pixel(Point::new(-1, 0), Gray2::new(gray::BLACK))
            .draw(&mut canvas)
            .unwrap();
        Pixel(Point::new(0, H as i32), Gray2::new(gray::BLACK))
            .draw(&mut canvas)
            .unwrap();

        assert!(canvas.buffer().iter().all(|byte| *byte == 0xff));
    }

    #[test]
    fn a_drawn_canvas_splits_into_controller_planes() {
        let mut buffer = [0u8; gray4_stride(W) * H];
        let mut canvas = Gray4Canvas::new(&mut buffer, W, H).unwrap();
        canvas.fill(gray::BLACK);
        Pixel(Point::new(0, 0), Gray2::new(gray::WHITE))
            .draw(&mut canvas)
            .unwrap();

        let mut bw = [0u8; plane_stride(W) * H];
        let mut second = [0u8; plane_stride(W) * H];
        split_gray4(
            canvas.buffer(),
            W,
            H,
            &PlaneMapping::LUT_INDEX_ORDER,
            &mut bw,
            &mut second,
        )
        .unwrap();

        // White is LUT3, so both planes carry the top bit of row 0.
        assert_eq!(bw[0], 0x80);
        assert_eq!(second[0], 0x80);
        assert_eq!(bw[1], 0x00);
    }
}
