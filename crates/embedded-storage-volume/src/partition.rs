//! Aligned partition policy and bounds-checked views over caller-owned media.
use crate::{BlockDevice, BlockDeviceMut, BlockGeometry, BlockIndex, Error};
use gpt_disk_types::{Chs, MasterBootRecord, MbrPartitionRecord, U32Le};

/// SD logical sector length; the SD transport itself is supplied by a driver.
pub const SECTOR_BYTES: usize = 512;
/// One MiB alignment in 512-byte sectors.
pub const ALIGN_SECTORS: u64 = 2048;
/// Default littlefs partition size in sectors (64 MiB).
pub const LITTLEFS_SECTORS: u64 = 131072;
/// MBR type for non-FAT private data; littlefs superblocks identify the format.
pub const PRIVATE_MBR_TYPE: u8 = 0xda;
/// FAT32 with LBA addressing.
pub const FAT32_MBR_TYPE: u8 = 0x0c;

/// Filesystem selected by application configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesystemKind {
    /// Copy-on-write metadata with bounded compiled geometry.
    Littlefs,
    /// FAT32 bulk storage; durability differs from littlefs.
    Fat32,
}
/// One configured volume, expressed in logical sectors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volume {
    /// First sector on the card.
    pub start: u64,
    /// Number of sectors in the volume.
    pub sectors: u64,
    /// Filesystem to mount or explicitly format.
    pub filesystem: FilesystemKind,
}
/// Three application volumes: state, update staging, and bulk data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Indexed in application order, independent of filesystem choice.
    pub volumes: [Volume; 3],
}
impl Layout {
    /// Read the supported GPT profile using caller-supplied 16 KiB scratch.
    /// Validate both header and entry CRCs before exposing any partition.
    /// A damaged primary table falls back to its independently checked backup.
    pub fn read_gpt<D: BlockDeviceMut>(device: &mut D, scratch: &mut [u8]) -> Result<Self, Error> {
        require_sector_geometry(device)?;
        let sectors = device.geometry().block_count.0;
        if sectors < 68 {
            return Err(Error::Geometry);
        }
        let mut disk = gpt_disk_io::Disk::new(DiskIo(device)).map_err(|_| Error::Partition)?;
        for location in [1, sectors.checked_sub(1).ok_or(Error::Geometry)?] {
            let result = (|| {
                let header = disk
                    .read_gpt_header(gpt_disk_types::Lba(location), &mut [0; 512])
                    .map_err(|_| Error::Media)?;
                let disk_guid = header.disk_guid;
                if !header.is_signature_valid()
                    || header.revision != gpt_disk_types::GptHeaderRevision::VERSION_1_0
                    || header.header_size.to_u32() != 92
                    || header.reserved.to_u32() != 0
                    || disk_guid == gpt_disk_types::Guid::ZERO
                    || header.my_lba.to_u64() != location
                    || header.alternate_lba.to_u64() != if location == 1 { sectors - 1 } else { 1 }
                    || header.header_crc32 != header.calculate_header_crc32()
                    || header.first_usable_lba.to_u64() < 34
                    || header.last_usable_lba.to_u64() > sectors - 34
                    || header.first_usable_lba.to_u64() > header.last_usable_lba.to_u64()
                    || header.number_of_partition_entries.to_u32() != 128
                    || header.size_of_partition_entry.to_u32() != 128
                    || header.partition_entry_lba.to_u64()
                        != if location == 1 { 2 } else { sectors - 33 }
                {
                    return Err(Error::Partition);
                }
                let entries = disk
                    .read_gpt_partition_entry_array(
                        header
                            .get_partition_entry_array_layout()
                            .map_err(|_| Error::Partition)?,
                        scratch,
                    )
                    .map_err(|_| Error::Partition)?;
                if entries.calculate_crc32() != header.partition_entry_array_crc32 {
                    return Err(Error::Partition);
                }
                let mut volumes = [Volume {
                    start: 0,
                    sectors: 0,
                    filesystem: FilesystemKind::Littlefs,
                }; 3];
                let mut unique = [gpt_disk_types::Guid::ZERO; 3];
                for (index, volume) in volumes.iter_mut().enumerate() {
                    let entry = entries
                        .get_partition_entry(index as u32)
                        .ok_or(Error::Partition)?;
                    let partition_guid = entry.unique_partition_guid;
                    if partition_guid == gpt_disk_types::Guid::ZERO
                        || unique[..index].contains(&partition_guid)
                        || partition_guid == disk_guid
                    {
                        return Err(Error::Partition);
                    }
                    unique[index] = partition_guid;
                    volume.filesystem = match entry.partition_type_guid {
                        PRIVATE_GPT_TYPE => FilesystemKind::Littlefs,
                        gpt_disk_types::GptPartitionType::BASIC_DATA => FilesystemKind::Fat32,
                        _ => return Err(Error::Partition),
                    };
                    volume.start = entry.starting_lba.to_u64();
                    let end = entry.ending_lba.to_u64();
                    volume.sectors = end
                        .checked_sub(volume.start)
                        .and_then(|n| n.checked_add(1))
                        .ok_or(Error::Partition)?;
                    if volume.start < header.first_usable_lba.to_u64()
                        || end > header.last_usable_lba.to_u64()
                    {
                        return Err(Error::Partition);
                    }
                }
                if (3..128).any(|index| {
                    entries
                        .get_partition_entry(index)
                        .is_none_or(|entry| entry.is_used())
                }) {
                    return Err(Error::Partition);
                }
                let layout = Self { volumes };
                layout.validate(sectors)?;
                Ok(layout)
            })();
            if result.is_ok() {
                return result;
            }
        }
        Err(Error::Partition)
    }
    /// Default aligned layout. Requires room for at least 64 MiB of FAT32 bulk
    /// storage, avoiding a formatter-dependent minimum-volume fallback.
    pub fn default_for(sectors: u64) -> Result<Self, Error> {
        let bulk_start = ALIGN_SECTORS + 2 * LITTLEFS_SECTORS;
        let end = sectors / ALIGN_SECTORS * ALIGN_SECTORS;
        if end < bulk_start + LITTLEFS_SECTORS || end > u64::from(u32::MAX) {
            return Err(Error::Geometry);
        }
        Ok(Self {
            volumes: [
                Volume {
                    start: ALIGN_SECTORS,
                    sectors: LITTLEFS_SECTORS,
                    filesystem: FilesystemKind::Littlefs,
                },
                Volume {
                    start: ALIGN_SECTORS + LITTLEFS_SECTORS,
                    sectors: LITTLEFS_SECTORS,
                    filesystem: FilesystemKind::Littlefs,
                },
                Volume {
                    start: bulk_start,
                    sectors: end - bulk_start,
                    filesystem: FilesystemKind::Fat32,
                },
            ],
        })
    }
    /// Validate all ranges before mounting or writing any partition metadata.
    pub fn validate(&self, sectors: u64) -> Result<(), Error> {
        for (i, volume) in self.volumes.iter().enumerate() {
            let end = volume
                .start
                .checked_add(volume.sectors)
                .ok_or(Error::Bounds)?;
            if volume.start < ALIGN_SECTORS
                || volume.sectors == 0
                || end > sectors
                || volume.start % ALIGN_SECTORS != 0
                || volume.sectors % ALIGN_SECTORS != 0
            {
                return Err(Error::Partition);
            }
            for other in &self.volumes[..i] {
                let other_end = other
                    .start
                    .checked_add(other.sectors)
                    .ok_or(Error::Bounds)?;
                if volume.start < other_end && other.start < end {
                    return Err(Error::Partition);
                }
            }
        }
        Ok(())
    }
    /// Produce reusable UEFI MBR structures. Serialization stays in upstream
    /// `gpt_disk_io`; no unaligned casts or application boot code are needed.
    pub fn mbr(&self) -> Result<MasterBootRecord, Error> {
        let mut mbr = MasterBootRecord {
            signature: [0x55, 0xaa],
            unknown: [0; 2],
            ..MasterBootRecord::default()
        };
        for (record, volume) in mbr.partitions.iter_mut().zip(self.volumes) {
            *record = MbrPartitionRecord {
                start_chs: Chs([0xff; 3]),
                end_chs: Chs([0xff; 3]),
                os_indicator: match volume.filesystem {
                    FilesystemKind::Littlefs => PRIVATE_MBR_TYPE,
                    FilesystemKind::Fat32 => FAT32_MBR_TYPE,
                },
                starting_lba: U32Le::from_u32(
                    u32::try_from(volume.start).map_err(|_| Error::Bounds)?,
                ),
                size_in_lba: U32Le::from_u32(
                    u32::try_from(volume.sectors).map_err(|_| Error::Bounds)?,
                ),
                ..MbrPartitionRecord::default()
            };
        }
        Ok(mbr)
    }
    /// Read the supported three-volume MBR. Extended and protective MBRs are
    /// rejected here; GPT callers use `Layout::read_gpt` instead.
    pub fn read_mbr<D: BlockDevice>(device: &mut D) -> Result<Self, Error> {
        require_sector_geometry(device)?;
        let mut bytes = [0; SECTOR_BYTES];
        device
            .read_blocks(BlockIndex(0), &mut bytes)
            .map_err(|_| Error::Media)?;
        if bytes[510..] != [0x55, 0xaa] {
            return Err(Error::Partition);
        }
        let mut volumes = [Volume {
            start: 0,
            sectors: 0,
            filesystem: FilesystemKind::Littlefs,
        }; 3];
        for (i, volume) in volumes.iter_mut().enumerate() {
            let row = &bytes[446 + i * 16..462 + i * 16];
            if !matches!(row[0], 0 | 0x80) {
                return Err(Error::Partition);
            }
            volume.filesystem = match row[4] {
                PRIVATE_MBR_TYPE => FilesystemKind::Littlefs,
                FAT32_MBR_TYPE => FilesystemKind::Fat32,
                _ => return Err(Error::Partition),
            };
            volume.start = u64::from(u32::from_le_bytes(
                row[8..12].try_into().map_err(|_| Error::Partition)?,
            ));
            volume.sectors = u64::from(u32::from_le_bytes(
                row[12..16].try_into().map_err(|_| Error::Partition)?,
            ));
        }
        if bytes[494..510].iter().any(|b| *b != 0) {
            return Err(Error::Partition);
        }
        let layout = Self { volumes };
        layout.validate(device.geometry().block_count.0)?;
        Ok(layout)
    }
}

/// Project-specific GPT type for private littlefs data. It is not a boot type.
pub const PRIVATE_GPT_TYPE: gpt_disk_types::GptPartitionType = gpt_disk_types::GptPartitionType(
    gpt_disk_types::guid!("c27c678b-ff75-4c26-a8ce-2f0d7cd32f51"),
);
/// Required GPT entry-array scratch length for the supported 128-entry profile.
pub const GPT_SCRATCH_BYTES: usize = 16384;

/// Publish backup GPT entries/header before primary entries/header and the
/// protective MBR. The caller supplies fresh disk/partition GUIDs and formats
/// volumes first. A power loss can leave a valid old or new table; provisioning
/// is destructive and is never a mount-recovery action.
pub fn write_gpt<D: BlockDeviceMut>(
    device: &mut D,
    layout: &Layout,
    ids: [gpt_disk_types::Guid; 4],
    scratch: &mut [u8],
    confirmed: bool,
) -> Result<(), Error> {
    use gpt_disk_types::{
        BlockSize, GptHeader, GptPartitionEntry, GptPartitionEntryArray,
        GptPartitionEntryArrayLayout, GptPartitionEntrySize, GptPartitionType, Lba, LbaLe, U32Le,
    };
    if !confirmed {
        return Err(Error::Unconfirmed);
    }
    require_sector_geometry(device)?;
    let sectors = device.geometry().block_count.0;
    layout.validate(sectors)?;
    if sectors < 68
        || ids.contains(&gpt_disk_types::Guid::ZERO)
        || ids.iter().enumerate().any(|(i, id)| ids[..i].contains(id))
        || layout
            .volumes
            .iter()
            .any(|v| v.start + v.sectors > sectors - 33)
    {
        return Err(Error::Partition);
    }
    let mut entries = GptPartitionEntryArray::new(
        GptPartitionEntryArrayLayout {
            start_lba: Lba(sectors - 33),
            entry_size: GptPartitionEntrySize::new(128).map_err(|_| Error::Geometry)?,
            num_entries: 128,
        },
        BlockSize::BS_512,
        scratch,
    )
    .map_err(|_| Error::Geometry)?;
    entries.storage_mut().fill(0);
    for (index, volume) in layout.volumes.iter().enumerate() {
        *entries
            .get_partition_entry_mut(index as u32)
            .ok_or(Error::Partition)? = GptPartitionEntry {
            partition_type_guid: match volume.filesystem {
                FilesystemKind::Littlefs => PRIVATE_GPT_TYPE,
                FilesystemKind::Fat32 => GptPartitionType::BASIC_DATA,
            },
            unique_partition_guid: ids[index + 1],
            starting_lba: LbaLe::from_u64(volume.start),
            ending_lba: LbaLe::from_u64(volume.start + volume.sectors - 1),
            name: ["state", "updates", "bulk"][index]
                .parse()
                .map_err(|_| Error::Partition)?,
            ..Default::default()
        };
    }
    let mut header = GptHeader {
        my_lba: LbaLe::from_u64(sectors - 1),
        alternate_lba: LbaLe::from_u64(1),
        first_usable_lba: LbaLe::from_u64(34),
        last_usable_lba: LbaLe::from_u64(sectors - 34),
        disk_guid: ids[0],
        partition_entry_lba: LbaLe::from_u64(sectors - 33),
        number_of_partition_entries: U32Le::from_u32(128),
        size_of_partition_entry: U32Le::from_u32(128),
        partition_entry_array_crc32: entries.calculate_crc32(),
        ..Default::default()
    };
    header.update_header_crc32();
    let mut disk = gpt_disk_io::Disk::new(DiskIo(device)).map_err(|_| Error::Partition)?;
    disk.write_gpt_partition_entry_array(&entries)
        .map_err(|_| Error::Media)?;
    disk.write_secondary_gpt_header(&header, &mut [0; 512])
        .map_err(|_| Error::Media)?;
    entries.set_start_lba(Lba(2));
    header.my_lba = LbaLe::from_u64(1);
    header.alternate_lba = LbaLe::from_u64(sectors - 1);
    header.partition_entry_lba = LbaLe::from_u64(2);
    header.update_header_crc32();
    disk.write_gpt_partition_entry_array(&entries)
        .map_err(|_| Error::Media)?;
    disk.write_primary_gpt_header(&header, &mut [0; 512])
        .map_err(|_| Error::Media)?;
    disk.write_protective_mbr(&mut [0; 512])
        .map_err(|_| Error::Media)
}
/// Reject incompatible logical geometry before constructing any adapter.
pub fn require_sector_geometry<D: BlockDevice>(device: &D) -> Result<(), Error> {
    if device.geometry().logical_block_size.get() as usize != SECTOR_BYTES {
        return Err(Error::Geometry);
    }
    Ok(())
}
/// A bounded volume that exclusively borrows a caller-owned block device.
/// Drop performs no I/O. Call `flush` before `release` when data is pending.
pub struct Partition<'a, D> {
    device: &'a mut D,
    volume: Volume,
}
impl<'a, D: BlockDevice> Partition<'a, D> {
    /// Borrow a nonempty in-bounds volume. Layout-wide overlap checks belong to
    /// `Layout::validate`, since this view only sees one volume.
    pub fn new(device: &'a mut D, volume: Volume) -> Result<Self, Error> {
        require_sector_geometry(device)?;
        if volume.sectors == 0
            || volume.sectors.checked_mul(SECTOR_BYTES as u64).is_none()
            || volume
                .start
                .checked_add(volume.sectors)
                .is_none_or(|end| end > device.geometry().block_count.0)
        {
            return Err(Error::Bounds);
        }
        Ok(Self { device, volume })
    }
    /// Return the exclusive device borrow without implicit I/O.
    pub fn release(self) -> &'a mut D {
        self.device
    }
    /// Descriptor supplied at construction.
    pub const fn volume(&self) -> Volume {
        self.volume
    }
    fn absolute(
        &self,
        start: BlockIndex,
        len: usize,
    ) -> hadris_storage::Result<BlockIndex, D::Error> {
        if len == 0 || !len.is_multiple_of(SECTOR_BYTES) {
            return Err(hadris_storage::Error::InvalidBufferLength {
                length: len,
                block_size: SECTOR_BYTES as u32,
            });
        }
        let count = (len / SECTOR_BYTES) as u64;
        if start
            .0
            .checked_add(count)
            .is_none_or(|end| end > self.volume.sectors)
        {
            return Err(hadris_storage::Error::OutOfBounds {
                start: start.0,
                count,
                device_blocks: self.volume.sectors,
            });
        }
        self.volume
            .start
            .checked_add(start.0)
            .map(BlockIndex)
            .ok_or(hadris_storage::Error::AddressOverflow)
    }
}
impl<D: BlockDevice> BlockDevice for Partition<'_, D> {
    type Error = D::Error;
    fn geometry(&self) -> BlockGeometry {
        let mut geometry = self.device.geometry();
        geometry.block_count.0 = self.volume.sectors;
        geometry
    }
    fn read_blocks(
        &mut self,
        start: BlockIndex,
        buffer: &mut [u8],
    ) -> hadris_storage::Result<(), Self::Error> {
        self.device
            .read_blocks(self.absolute(start, buffer.len())?, buffer)
    }
}
impl<D: BlockDeviceMut> BlockDeviceMut for Partition<'_, D> {
    fn write_blocks(
        &mut self,
        start: BlockIndex,
        buffer: &[u8],
    ) -> hadris_storage::Result<(), Self::Error> {
        self.device
            .write_blocks(self.absolute(start, buffer.len())?, buffer)
    }
    fn flush(&mut self) -> hadris_storage::Result<(), Self::Error> {
        self.device.flush()
    }
}
/// Bridge the common block contract to the upstream GPT/MBR implementation.
/// Native errors remain available through the underlying driver; GPT uses a
/// compact classified error suitable for no_std transports.
/// Writes flush immediately. Its `BlockIo::flush` is a no-op, keeping the
/// upstream `Disk` destructor free of device I/O.
pub struct DiskIo<'a, D>(pub &'a mut D);
impl<D: BlockDeviceMut> gpt_disk_io::BlockIo for DiskIo<'_, D> {
    type Error = Error;
    fn block_size(&self) -> gpt_disk_types::BlockSize {
        gpt_disk_types::BlockSize::BS_512
    }
    fn num_blocks(&mut self) -> Result<u64, Error> {
        require_sector_geometry(self.0)?;
        Ok(self.0.geometry().block_count.0)
    }
    fn read_blocks(&mut self, start: gpt_disk_types::Lba, buf: &mut [u8]) -> Result<(), Error> {
        require_sector_geometry(self.0)?;
        self.0
            .read_blocks(BlockIndex(start.0), buf)
            .map_err(|_| Error::Media)
    }
    fn write_blocks(&mut self, start: gpt_disk_types::Lba, buf: &[u8]) -> Result<(), Error> {
        require_sector_geometry(self.0)?;
        self.0
            .write_blocks(BlockIndex(start.0), buf)
            .and_then(|()| self.0.flush())
            .map_err(|_| Error::Media)
    }
    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
/// Write an explicitly confirmed MBR and flush it. Provisioners should first
/// invalidate sector zero, format volumes, then publish this table last.
pub fn write_mbr<D: BlockDeviceMut>(
    device: &mut D,
    layout: &Layout,
    confirmed: bool,
) -> Result<(), Error> {
    if !confirmed {
        return Err(Error::Unconfirmed);
    }
    require_sector_geometry(device)?;
    layout.validate(device.geometry().block_count.0)?;
    let mut disk = gpt_disk_io::Disk::new(DiskIo(device)).map_err(|_| Error::Partition)?;
    disk.write_mbr(&layout.mbr()?, &mut [0; SECTOR_BYTES])
        .map_err(|_| Error::Media)?;
    disk.flush().map_err(|_| Error::Media)
}
