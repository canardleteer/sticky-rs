//! Reassemble one u32-LE framed [`crate::v1::Envelope`] from ATT chunks.

use alloc::vec::Vec;

use crate::frame::FrameError;

/// Default cap for inbound host→device frames (inject / get / ack / clear).
///
/// Those messages are tens of bytes. A 1 KiB cap rejects a runaway
/// writer without needing a 48 KiB device heap slot.
pub const DEVICE_RX_MAX: usize = 1024;

/// Default cap for device→host frames (a gray4 pair plus protobuf).
///
/// Two 48 KiB planes plus envelope tags stay under 100 KiB. Host may
/// `Vec` this; device must not.
pub const HOST_RX_MAX: usize = 96 * 1024 + 256;

/// Accumulate ATT write/notify fragments until one framed envelope.
///
/// Push chunks in order. When the u32 LE length and that many payload
/// bytes are present, [`Self::push`] returns the complete framed
/// buffer (length prefix included) for [`crate::decode_envelope`].
pub struct FrameAssembler {
    buf: Vec<u8>,
    max: usize,
}

impl FrameAssembler {
    /// Empty assembler that refuses a frame larger than `max`.
    #[must_use]
    pub fn new(max: usize) -> Self {
        Self {
            buf: Vec::new(),
            max,
        }
    }

    /// Device inbound (small control messages).
    #[must_use]
    pub fn device_rx() -> Self {
        Self::new(DEVICE_RX_MAX)
    }

    /// Host inbound (may include a snapshot).
    #[must_use]
    pub fn host_rx() -> Self {
        Self::new(HOST_RX_MAX)
    }

    /// Bytes held that are not yet one complete frame.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.buf.len()
    }

    /// Drop a partial frame (disconnect / overflow recovery).
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Append `chunk`. `Some` is one complete framed envelope.
    ///
    /// Extra bytes after that frame stay in the assembler (a well-behaved
    /// peer sends one frame at a time; leftovers are still framed).
    ///
    /// # Errors
    ///
    /// [`FrameError::TooLarge`] when the length prefix or the growing
    /// buffer exceeds `max`. [`FrameError::Truncated`] is not used
    /// here — short input just waits for more chunks.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Option<Vec<u8>>, FrameError> {
        if chunk.is_empty() {
            return Ok(None);
        }
        if self.buf.len().saturating_add(chunk.len()) > self.max {
            self.buf.clear();
            return Err(FrameError::TooLarge);
        }
        self.buf.extend_from_slice(chunk);
        self.take_frame()
    }

    fn take_frame(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let n = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        let need = 4usize.saturating_add(n);
        if need > self.max || n > self.max {
            self.buf.clear();
            return Err(FrameError::TooLarge);
        }
        if self.buf.len() < need {
            return Ok(None);
        }
        let rest = self.buf.split_off(need);
        let frame = core::mem::replace(&mut self.buf, rest);
        Ok(Some(frame))
    }
}
