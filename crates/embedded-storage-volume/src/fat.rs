//! FAT32 byte-stream adapter with partition bounds and explicit synchronization.
use crate::partition::{Partition, Volume, SECTOR_BYTES};
use crate::{BlockDevice, BlockDeviceMut, BlockIndex, Error};
use fatfs::IoBase;
/// Native byte-stream traits used by the adopted FAT filesystem.
pub use fatfs::{Read, Seek, SeekFrom, Write};

/// Cursor over one partition. Partial-sector writes use read/modify/write and
/// never cross its end. The owner chooses application buffering and flushes.
pub struct FatStream<'a, D> {
    partition: Partition<'a, D>,
    position: u64,
    failed: bool,
}
impl<'a, D: BlockDeviceMut> FatStream<'a, D> {
    /// Borrow a FAT32 volume. Geometry and bounds are checked before I/O.
    pub fn new(device: &'a mut D, volume: Volume) -> Result<Self, Error> {
        if volume.filesystem != crate::FilesystemKind::Fat32 {
            return Err(Error::Geometry);
        }
        Ok(Self {
            partition: Partition::new(device, volume)?,
            position: 0,
            failed: false,
        })
    }
    /// Synchronize before returning the exclusive device borrow.
    pub fn shutdown(mut self) -> Result<&'a mut D, Self> {
        if self.flush().is_err() {
            return Err(self);
        }
        Ok(self.partition.release())
    }
    /// Return the device without implicit I/O for recovery.
    pub fn release(self) -> &'a mut D {
        self.partition.release()
    }
    fn remaining(&self) -> usize {
        usize::try_from(self.partition.volume().sectors * SECTOR_BYTES as u64 - self.position)
            .unwrap_or(usize::MAX)
    }
}
impl<D> IoBase for FatStream<'_, D> {
    type Error = Error;
}
impl fatfs::IoError for Error {
    fn is_interrupted(&self) -> bool {
        false
    }
    fn new_unexpected_eof_error() -> Self {
        Self::Bounds
    }
    fn new_write_zero_error() -> Self {
        Self::Bounds
    }
}
impl<D: BlockDeviceMut> Read for FatStream<'_, D> {
    fn read(&mut self, out: &mut [u8]) -> Result<usize, Error> {
        if self.failed {
            return Err(Error::Media);
        }
        let count = out.len().min(self.remaining());
        let mut done = 0;
        let mut sector = [0; SECTOR_BYTES];
        while done < count {
            let index = self.position / SECTOR_BYTES as u64;
            let within = (self.position % SECTOR_BYTES as u64) as usize;
            let take = (SECTOR_BYTES - within).min(count - done);
            if self
                .partition
                .read_blocks(BlockIndex(index), &mut sector)
                .is_err()
            {
                self.failed = true;
                return Err(Error::Media);
            }
            out[done..done + take].copy_from_slice(&sector[within..within + take]);
            self.position += take as u64;
            done += take;
        }
        Ok(done)
    }
}
impl<D: BlockDeviceMut> Write for FatStream<'_, D> {
    fn write(&mut self, input: &[u8]) -> Result<usize, Error> {
        if self.failed {
            return Err(Error::Media);
        }
        // Refuse the whole out-of-bounds request before modifying any sector.
        if input.len() > self.remaining() {
            return Err(Error::Bounds);
        }
        let mut done = 0;
        let mut sector = [0; SECTOR_BYTES];
        while done < input.len() {
            let index = self.position / SECTOR_BYTES as u64;
            let within = (self.position % SECTOR_BYTES as u64) as usize;
            let take = (SECTOR_BYTES - within).min(input.len() - done);
            if (within != 0 || take != SECTOR_BYTES)
                && self
                    .partition
                    .read_blocks(BlockIndex(index), &mut sector)
                    .is_err()
            {
                self.failed = true;
                return Err(Error::Media);
            }
            sector[within..within + take].copy_from_slice(&input[done..done + take]);
            if self
                .partition
                .write_blocks(BlockIndex(index), &sector)
                .is_err()
            {
                self.failed = true;
                return Err(Error::Media);
            }
            self.position += take as u64;
            done += take;
        }
        Ok(done)
    }
    fn flush(&mut self) -> Result<(), Error> {
        if self.failed {
            return Err(Error::Media);
        }
        if self.partition.flush().is_err() {
            self.failed = true;
            return Err(Error::Media);
        }
        Ok(())
    }
}
impl<D: BlockDeviceMut> Seek for FatStream<'_, D> {
    fn seek(&mut self, from: SeekFrom) -> Result<u64, Error> {
        let end = self.partition.volume().sectors * SECTOR_BYTES as u64;
        let next = match from {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => end.checked_add_signed(offset),
        }
        .filter(|position| *position <= end)
        .ok_or(Error::Bounds)?;
        self.position = next;
        Ok(next)
    }
}
/// Format FAT32 through the adopted formatter and flush its metadata.
pub fn format<D: BlockDeviceMut>(
    stream: &mut FatStream<'_, D>,
    confirmed: bool,
) -> Result<(), Error> {
    if !confirmed {
        return Err(Error::Unconfirmed);
    }
    fatfs::format_volume(
        &mut *stream,
        fatfs::FormatVolumeOptions::new().fat_type(fatfs::FatType::Fat32),
    )
    .map_err(|_| Error::Filesystem)?;
    stream.flush()
}
/// Re-export the adopted filesystem API so applications can compose their own
/// file operations without a second virtual filesystem or hidden bus locks.
pub use fatfs::{FileSystem, FsOptions};
