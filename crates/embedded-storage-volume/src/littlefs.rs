//! littlefs adapter for rewriteable, sector-addressed managed media.
//!
//! Erase is a logical no-op, matching upstream littlefs's file block device.
//! Every program callback flushes the lower device because littlefs2's C sync
//! callback does no I/O. Closing or syncing each file still matters.
use crate::partition::{Partition, Volume, SECTOR_BYTES};
use crate::{BlockDevice, BlockDeviceMut, BlockIndex, Error};
use littlefs2::driver::Storage;
use littlefs2::fs::Filesystem;
use littlefs2::io;

/// Default 64 MiB volume, using 4 KiB allocation blocks.
pub const DEFAULT_BLOCKS: usize = 16384;
/// Logical allocation block size, independent of the card's unknown FTL geometry.
pub const BLOCK_BYTES: usize = 4096;

/// A compiled littlefs profile over one exclusively borrowed partition.
///
/// `BLOCKS` and `CYCLES` are build-time choices. Applications can dispatch among
/// these profile types at runtime. Failed writes poison this instance so its
/// mount cannot continue issuing writes after an uncertain media completion.
pub struct Littlefs<'a, D, const BLOCKS: usize = DEFAULT_BLOCKS, const CYCLES: isize = 500> {
    partition: Partition<'a, D>,
    failed: bool,
}
impl<'a, D: BlockDeviceMut, const N: usize, const C: isize> Littlefs<'a, D, N, C> {
    /// Require the exact compiled geometry before borrowing the volume.
    pub fn new(device: &'a mut D, volume: Volume) -> Result<Self, Error> {
        if volume.filesystem != crate::FilesystemKind::Littlefs
            || N < 2
            || (C != -1 && C <= 0)
            || N.checked_mul(BLOCK_BYTES).is_none_or(|bytes| {
                Some(bytes as u64) != volume.sectors.checked_mul(SECTOR_BYTES as u64)
            })
        {
            return Err(Error::Geometry);
        }
        Ok(Self {
            partition: Partition::new(device, volume)?,
            failed: false,
        })
    }
    /// Explicitly format this partition. Never used as a mount-error fallback.
    pub fn format(&mut self, confirmed: bool) -> Result<(), Error> {
        if !confirmed {
            return Err(Error::Unconfirmed);
        }
        Filesystem::format(self).map_err(|_| Error::Filesystem)
    }
    /// Mount for one closure, then release its caller-backed filesystem state.
    /// File handles cannot outlive the closure. The upstream closure file APIs
    /// propagate close errors; this scope's end performs no implicit media I/O.
    /// Callback errors remain errors and never cause formatting.
    pub fn with_fs<R>(
        &mut self,
        callback: impl FnOnce(&Filesystem<'_, Self>) -> io::Result<R>,
    ) -> Result<R, Error> {
        Filesystem::mount_and_then(self, callback).map_err(|_| Error::Filesystem)
    }
    /// Flush and return the exclusive device borrow. A flush failure retains
    /// this adapter so the caller can inspect or deliberately abandon it.
    pub fn shutdown(mut self) -> Result<&'a mut D, Self> {
        if self.failed || self.partition.flush().is_err() {
            self.failed = true;
            return Err(self);
        }
        Ok(self.partition.release())
    }
    /// Return the device without I/O for recovery after a failed mount/write.
    pub fn release(self) -> &'a mut D {
        self.partition.release()
    }
    fn check(&self, offset: usize, length: usize, unit: usize) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::IO);
        }
        if !offset.is_multiple_of(unit)
            || !length.is_multiple_of(unit)
            || offset
                .checked_add(length)
                .is_none_or(|end| end > N * BLOCK_BYTES)
        {
            return Err(io::Error::INVALID);
        }
        Ok(())
    }
}
impl<D: BlockDeviceMut, const N: usize, const C: isize> Storage for Littlefs<'_, D, N, C> {
    const READ_SIZE: usize = SECTOR_BYTES;
    const WRITE_SIZE: usize = SECTOR_BYTES;
    const BLOCK_SIZE: usize = BLOCK_BYTES;
    const BLOCK_COUNT: usize = N;
    const BLOCK_CYCLES: isize = C;
    type CACHE_SIZE = littlefs2::consts::U512;
    // LOOKAHEAD_SIZE counts u64 words, so 16 words occupy 128 bytes.
    type LOOKAHEAD_SIZE = littlefs2::consts::U16;
    fn read(&mut self, offset: usize, buffer: &mut [u8]) -> io::Result<usize> {
        self.check(offset, buffer.len(), SECTOR_BYTES)?;
        if buffer.is_empty() {
            return Ok(0);
        }
        if self
            .partition
            .read_blocks(BlockIndex((offset / SECTOR_BYTES) as u64), buffer)
            .is_err()
        {
            self.failed = true;
            return Err(io::Error::IO);
        }
        Ok(buffer.len())
    }
    fn write(&mut self, offset: usize, buffer: &[u8]) -> io::Result<usize> {
        self.check(offset, buffer.len(), SECTOR_BYTES)?;
        if buffer.is_empty() {
            return Ok(0);
        }
        if self
            .partition
            .write_blocks(BlockIndex((offset / SECTOR_BYTES) as u64), buffer)
            .and_then(|()| self.partition.flush())
            .is_err()
        {
            self.failed = true;
            return Err(io::Error::IO);
        }
        Ok(buffer.len())
    }
    fn erase(&mut self, offset: usize, length: usize) -> io::Result<usize> {
        self.check(offset, length, BLOCK_BYTES)?;
        Ok(length)
    }
}
