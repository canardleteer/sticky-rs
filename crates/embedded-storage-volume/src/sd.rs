//! Adapter for embedded-sdmmc's synchronous single-sector completion contract.
//!
//! The driver waits for card busy release and checks CMD13 after a single
//! sector write. This adapter always uses that path; no extra SD opcodes are
//! implemented here. Card-internal power-loss guarantees remain media-specific.
use crate::{BlockCount, BlockDevice, BlockDeviceMut, BlockGeometry, BlockIndex, BlockSize, Error};
use embedded_hal::{delay::DelayNs, spi::SpiDevice};
use embedded_sdmmc::{Block, BlockDevice as SdBlockDevice, BlockIdx};

/// Caller-owned card with cached geometry and a latched failed-transport state.
pub struct SdMedia<S: SpiDevice, T: DelayNs> {
    card: embedded_sdmmc::SdCard<S, T>,
    sectors: u64,
    failed: bool,
}
impl<S: SpiDevice, T: DelayNs> SdMedia<S, T> {
    /// Initialize through the upstream driver; caller must have enabled power,
    /// settled the rail, deselected the panel, and selected a <=400 kHz clock.
    pub fn new(card: embedded_sdmmc::SdCard<S, T>) -> Result<Self, Error> {
        let sectors = u64::from(card.num_blocks().map_err(|_| Error::Media)?.0);
        Ok(Self {
            card,
            sectors,
            failed: false,
        })
    }
    /// Return the owned transport without implicit I/O for rail-reset recovery.
    pub fn release(self) -> embedded_sdmmc::SdCard<S, T> {
        self.card
    }
    /// Borrow the driver to change its caller-supplied SPI configuration.
    pub fn card(&self) -> &embedded_sdmmc::SdCard<S, T> {
        &self.card
    }
    /// Latch failure after a caller-controlled power or bus interruption.
    ///
    /// This performs no I/O. Reads, writes and flushes then fail until the caller
    /// releases this transport and constructs a newly initialized adapter.
    pub fn invalidate(&mut self) {
        self.failed = true;
    }
    fn range(&self, start: BlockIndex, len: usize) -> hadris_storage::Result<(), Error> {
        let count = (len / 512) as u64;
        if len == 0 || !len.is_multiple_of(512) {
            return Err(hadris_storage::Error::InvalidBufferLength {
                length: len,
                block_size: 512,
            });
        }
        if start
            .0
            .checked_add(count)
            .is_none_or(|end| end > self.sectors)
        {
            return Err(hadris_storage::Error::OutOfBounds {
                start: start.0,
                count,
                device_blocks: self.sectors,
            });
        }
        Ok(())
    }
}
/// Wrap a card failure without pretending that an incomplete write succeeded.
fn media_error() -> hadris_storage::Error<Error> {
    hadris_storage::Error::Io(hadris_io::Error::from_source(Error::Media))
}
impl<S: SpiDevice, T: DelayNs> BlockDevice for SdMedia<S, T> {
    type Error = Error;
    fn geometry(&self) -> BlockGeometry {
        BlockGeometry::new(
            BlockSize::new(512).expect("constant nonzero sector size"),
            BlockCount(self.sectors),
        )
    }
    fn read_blocks(
        &mut self,
        start: BlockIndex,
        out: &mut [u8],
    ) -> hadris_storage::Result<(), Error> {
        self.range(start, out.len())?;
        if self.failed {
            return Err(media_error());
        }
        let mut sector = Block::new();
        for (i, buf) in out.as_chunks_mut::<512>().0.iter_mut().enumerate() {
            if self
                .card
                .read(
                    core::slice::from_mut(&mut sector),
                    BlockIdx((start.0 + i as u64) as u32),
                )
                .is_err()
            {
                self.failed = true;
                return Err(media_error());
            }
            buf.copy_from_slice(&sector.contents);
        }
        Ok(())
    }
}
impl<S: SpiDevice, T: DelayNs> BlockDeviceMut for SdMedia<S, T> {
    fn write_blocks(
        &mut self,
        start: BlockIndex,
        input: &[u8],
    ) -> hadris_storage::Result<(), Error> {
        self.range(start, input.len())?;
        if self.failed {
            return Err(media_error());
        }
        let mut sector = Block::new();
        for (i, buf) in input.as_chunks::<512>().0.iter().enumerate() {
            sector.contents.copy_from_slice(buf);
            if self
                .card
                .write(
                    core::slice::from_ref(&sector),
                    BlockIdx((start.0 + i as u64) as u32),
                )
                .is_err()
            {
                self.failed = true;
                return Err(media_error());
            }
        }
        Ok(())
    }
    fn flush(&mut self) -> hadris_storage::Result<(), Error> {
        // Every single-sector write completed its busy/status handshake already.
        // The driver's API provides no stronger controller-cache flush command.
        if self.failed {
            Err(media_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoIo;
    impl embedded_hal::spi::ErrorType for NoIo {
        type Error = core::convert::Infallible;
    }
    impl SpiDevice for NoIo {
        fn transaction(
            &mut self,
            _: &mut [embedded_hal::spi::Operation<'_, u8>],
        ) -> Result<(), Self::Error> {
            panic!("device I/O after externally interrupted power");
        }
    }
    impl DelayNs for NoIo {
        fn delay_ns(&mut self, _: u32) {
            panic!("driver activity after externally interrupted power");
        }
    }

    #[test]
    fn externally_interrupted_power_refuses_io_flush_and_drop() {
        // Supply cached geometry directly so this regression exercises an
        // already-owned transport without issuing initialization traffic.
        let mut media = SdMedia {
            card: embedded_sdmmc::SdCard::new(NoIo, NoIo),
            sectors: 8,
            failed: false,
        };
        media.invalidate();
        assert!(media.read_blocks(BlockIndex(0), &mut [0; 512]).is_err());
        assert!(media.write_blocks(BlockIndex(0), &[0; 512]).is_err());
        assert!(media.flush().is_err());
        // Scope exit must also leave the disconnected device untouched.
    }
}
