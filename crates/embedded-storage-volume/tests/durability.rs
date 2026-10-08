//! Host durability checks use sparse media with injectable torn-sector writes.
use embedded_storage_volume::{fat, littlefs::Littlefs, partition};
use embedded_storage_volume::{
    BlockCount, BlockDevice, BlockDeviceMut, BlockGeometry, BlockIndex, BlockSize, Error,
    FilesystemKind, Layout, Volume, WritePolicy,
};
use littlefs2::path;
use std::collections::BTreeMap;

#[derive(Clone)]
struct Media {
    sectors: u64,
    data: BTreeMap<u64, [u8; 512]>,
    writes: usize,
    flushes: usize,
    fail_write: Option<usize>,
    fail_flush: bool,
    fail_read: bool,
    torn: bool,
}
impl Media {
    fn new(sectors: u64) -> Self {
        Self {
            sectors,
            data: BTreeMap::new(),
            writes: 0,
            flushes: 0,
            fail_write: None,
            fail_flush: false,
            fail_read: false,
            torn: false,
        }
    }
    fn check(&self, start: BlockIndex, len: usize) -> hadris_storage::Result<(), Error> {
        if len == 0 || !len.is_multiple_of(512) {
            return Err(hadris_storage::Error::InvalidBufferLength {
                length: len,
                block_size: 512,
            });
        }
        if start
            .0
            .checked_add((len / 512) as u64)
            .is_none_or(|end| end > self.sectors)
        {
            return Err(hadris_storage::Error::AddressOverflow);
        }
        Ok(())
    }
}
impl BlockDevice for Media {
    type Error = Error;
    fn geometry(&self) -> BlockGeometry {
        BlockGeometry::new(BlockSize::new(512).unwrap(), BlockCount(self.sectors))
    }
    fn read_blocks(
        &mut self,
        start: BlockIndex,
        buf: &mut [u8],
    ) -> hadris_storage::Result<(), Error> {
        self.check(start, buf.len())?;
        if self.fail_read {
            return Err(hadris_storage::Error::Io(hadris_io::Error::from_source(
                Error::Media,
            )));
        }
        for (index, out) in buf.as_chunks_mut::<512>().0.iter_mut().enumerate() {
            out.copy_from_slice(
                self.data
                    .get(&(start.0 + index as u64))
                    .unwrap_or(&[0; 512]),
            );
        }
        Ok(())
    }
}
impl BlockDeviceMut for Media {
    fn write_blocks(&mut self, start: BlockIndex, buf: &[u8]) -> hadris_storage::Result<(), Error> {
        self.check(start, buf.len())?;
        self.writes += 1;
        if self.fail_write == Some(self.writes) {
            if self.torn {
                self.data.entry(start.0).or_insert([0; 512])[..256].copy_from_slice(&buf[..256]);
            }
            return Err(hadris_storage::Error::Io(hadris_io::Error::from_source(
                Error::Media,
            )));
        }
        for (index, input) in buf.as_chunks::<512>().0.iter().enumerate() {
            self.data.insert(start.0 + index as u64, *input);
        }
        Ok(())
    }
    fn flush(&mut self) -> hadris_storage::Result<(), Error> {
        self.flushes += 1;
        if self.fail_flush {
            return Err(hadris_storage::Error::Io(hadris_io::Error::from_source(
                Error::Media,
            )));
        }
        Ok(())
    }
}
fn state_volume() -> Volume {
    Volume {
        start: 2048,
        sectors: 128,
        filesystem: FilesystemKind::Littlefs,
    }
}

#[test]
fn replacement_survives_fifty_cut_positions_with_and_without_torn_sectors() {
    let mut original = Media::new(4096);
    let volume = state_volume();
    let mut fs = Littlefs::<_, 16>::new(&mut original, volume).unwrap();
    fs.format(true).unwrap();
    fs.with_fs(|fs| fs.write(path!("record"), b"previous committed value"))
        .unwrap();
    let original = fs.shutdown().ok().unwrap().clone();
    let next = [0xa5; 24576];
    let mut injected = 0;
    for torn in [false, true] {
        for position in 1..=50 {
            let mut media = original.clone();
            media.writes = 0;
            media.fail_write = Some(position);
            media.torn = torn;
            let mut adapter = Littlefs::<_, 16>::new(&mut media, volume).unwrap();
            let result = adapter.with_fs(|fs| {
                fs.write(path!("pending"), &next)?;
                fs.rename(path!("pending"), path!("record"))
            });
            let media = adapter.release();
            if result.is_err() {
                injected += 1;
            }
            media.fail_write = None;
            // Simulated reboot constructs a fresh adapter; a failed instance
            // cannot continue writing after uncertain completion.
            let recovered = Littlefs::<_, 16>::new(media, volume)
                .unwrap()
                .with_fs(|fs| fs.read::<24576>(path!("record")))
                .unwrap();
            assert!(
                recovered.as_slice() == b"previous committed value" || recovered.as_slice() == next,
                "cut={position}, torn={torn}"
            );
        }
    }
    assert_eq!(injected, 100, "every cut must actually interrupt a write");
}

#[test]
fn failed_flush_poisoning_and_drop_do_not_issue_more_io() {
    use littlefs2::driver::Storage;
    let mut media = Media::new(4096);
    media.fail_flush = true;
    let mut adapter = Littlefs::<_, 16>::new(&mut media, state_volume()).unwrap();
    assert!(adapter.write(0, &[7; 512]).is_err());
    assert!(adapter.write(512, &[8; 512]).is_err());
    let media = adapter.release();
    assert_eq!(media.writes, 1);
    assert_eq!(media.flushes, 1);
    let before = media.flushes;
    {
        let _adapter = Littlefs::<_, 16>::new(media, state_volume()).unwrap();
    }
    assert_eq!(media.flushes, before);
}

#[test]
fn fat_failure_blocks_cleanup_io_and_requires_a_fresh_adapter() {
    use fat::{Seek, SeekFrom, Write};
    let mut media = Media::new(4096);
    let volume = Volume {
        filesystem: FilesystemKind::Fat32,
        ..state_volume()
    };
    media.fail_write = Some(1);
    let mut stream = fat::FatStream::new(&mut media, volume).unwrap();
    assert_eq!(stream.write(&[1; 513]), Err(Error::Media));
    assert_eq!(stream.write(&[2; 512]), Err(Error::Media));
    assert_eq!(stream.flush(), Err(Error::Media));
    let media = stream.release();
    assert_eq!(media.writes, 1);
    assert_eq!(media.flushes, 0);
    media.fail_write = None;
    media.fail_flush = true;
    let mut stream = fat::FatStream::new(media, volume).unwrap();
    stream.write_all(&[3; 3]).unwrap();
    assert_eq!(stream.flush(), Err(Error::Media));
    assert_eq!(stream.flush(), Err(Error::Media));
    assert_eq!(stream.write(&[4]), Err(Error::Media));
    let media = stream.release();
    assert_eq!(media.flushes, 1);
    media.fail_flush = false;
    let before = (media.writes, media.flushes);
    {
        let mut stream = fat::FatStream::new(media, volume).unwrap();
        assert!(stream.seek(SeekFrom::End(1)).is_err());
    }
    assert_eq!((media.writes, media.flushes), before);
}

#[test]
fn invalid_mbr_rows_are_rejected_without_writes() {
    let mut original = Media::new(1048576);
    let layout = Layout::default_for(original.sectors).unwrap();
    partition::write_mbr(&mut original, &layout, true).unwrap();
    for offset in [510, 446, 450, 494, 498] {
        let mut media = original.clone();
        media.data.get_mut(&0).unwrap()[offset] ^= 0x33;
        assert_eq!(
            Layout::read_mbr(&mut media),
            Err(Error::Partition),
            "byte {offset}"
        );
        assert_eq!(media.writes, original.writes);
    }
    assert!(Layout::default_for(2048 + 3 * 131072 - 1).is_err());
    assert!(partition::Partition::new(
        &mut original,
        Volume {
            start: u64::MAX,
            sectors: 1,
            ..state_volume()
        }
    )
    .is_err());
}

#[test]
fn pending_close_and_rename_failures_preserve_published_package() {
    let mut original = Media::new(4096);
    let volume = state_volume();
    Littlefs::<_, 16>::new(&mut original, volume)
        .unwrap()
        .format(true)
        .unwrap();
    Littlefs::<_, 16>::new(&mut original, volume)
        .unwrap()
        .with_fs(|fs| fs.write(path!("ready"), &[0x55; 1024]))
        .unwrap();
    let mut probe = original.clone();
    let before = probe.writes;
    Littlefs::<_, 16>::new(&mut probe, volume)
        .unwrap()
        .with_fs(|fs| {
            fs.write(path!("pending"), &[0xaa; 1024])?;
            fs.rename(path!("pending"), path!("ready"))
        })
        .unwrap();
    let count = probe.writes - before;
    assert!(count > 1);
    for cut in 1..=count {
        for torn in [false, true] {
            let mut media = original.clone();
            media.fail_write = Some(media.writes + cut);
            media.torn = torn;
            let result = Littlefs::<_, 16>::new(&mut media, volume)
                .unwrap()
                .with_fs(|fs| {
                    fs.write(path!("pending"), &[0xaa; 1024])?;
                    fs.rename(path!("pending"), path!("ready"))
                });
            assert!(result.is_err(), "write {cut} must actually fail");
            media.fail_write = None;
            let ready = Littlefs::<_, 16>::new(&mut media, volume)
                .unwrap()
                .with_fs(|fs| fs.read::<1024>(path!("ready")))
                .unwrap();
            assert!(ready.as_slice() == [0x55; 1024] || ready.as_slice() == [0xaa; 1024]);
        }
    }
}

#[test]
fn mbr_roundtrip_and_partition_boundaries() {
    let mut media = Media::new(1048576);
    let layout = Layout::default_for(media.sectors).unwrap();
    assert_eq!(
        partition::write_mbr(&mut media, &layout, false),
        Err(Error::Unconfirmed)
    );
    assert_eq!(media.writes, 0);
    partition::write_mbr(&mut media, &layout, true).unwrap();
    assert_eq!(Layout::read_mbr(&mut media).unwrap(), layout);
    let before = media.writes;
    let mut view = partition::Partition::new(&mut media, layout.volumes[0]).unwrap();
    assert!(view
        .write_blocks(BlockIndex(layout.volumes[0].sectors), &[1; 512])
        .is_err());
    assert!(view.write_blocks(BlockIndex(u64::MAX), &[1; 512]).is_err());
    let media = view.release();
    assert_eq!(media.writes, before);
    let mut invalid = layout;
    invalid.volumes[1].start = invalid.volumes[0].start;
    assert_eq!(invalid.validate(media.sectors), Err(Error::Partition));
    assert!(Littlefs::<_, 16>::new(
        media,
        Volume {
            sectors: u64::MAX,
            ..state_volume()
        }
    )
    .is_err());
}

#[test]
fn gpt_crc_validation_and_backup_recovery() {
    use gpt_disk_types::guid;
    let mut media = Media::new(1048576);
    let layout = Layout::default_for(media.sectors - 34).unwrap();
    let ids = [
        guid!("7cb7e010-fc74-4cf0-9800-11d2250e6110"),
        guid!("7cb7e011-fc74-4cf0-9800-11d2250e6110"),
        guid!("7cb7e012-fc74-4cf0-9800-11d2250e6110"),
        guid!("7cb7e013-fc74-4cf0-9800-11d2250e6110"),
    ];
    let mut scratch = [0; partition::GPT_SCRATCH_BYTES];
    partition::write_gpt(&mut media, &layout, ids, &mut scratch, true).unwrap();
    assert_eq!(Layout::read_gpt(&mut media, &mut scratch).unwrap(), layout);
    media.data.get_mut(&1).unwrap()[16] ^= 1;
    assert_eq!(Layout::read_gpt(&mut media, &mut scratch).unwrap(), layout);
    media.data.get_mut(&(media.sectors - 33)).unwrap()[100] ^= 1;
    assert_eq!(
        Layout::read_gpt(&mut media, &mut scratch),
        Err(Error::Partition)
    );
}

#[test]
fn fat32_partial_sector_writes_and_remount() {
    use fatfs::{Read, Write};
    let mut media = Media::new(140000);
    let volume = Volume {
        start: 2048,
        sectors: 131072,
        filesystem: FilesystemKind::Fat32,
    };
    fat::format(&mut fat::FatStream::new(&mut media, volume).unwrap(), true).unwrap();
    {
        let fs = fat::FileSystem::new(
            fat::FatStream::new(&mut media, volume).unwrap(),
            fat::FsOptions::new(),
        )
        .unwrap();
        let mut file = fs.root_dir().create_file("bulk.dat").unwrap();
        file.write_all(&[0x9a; 1001]).unwrap();
        file.flush().unwrap();
        drop(file);
        fs.unmount().unwrap();
    }
    let fs = fat::FileSystem::new(
        fat::FatStream::new(&mut media, volume).unwrap(),
        fat::FsOptions::new(),
    )
    .unwrap();
    let mut file = fs.root_dir().open_file("bulk.dat").unwrap();
    let mut actual = [0; 1001];
    file.read_exact(&mut actual).unwrap();
    assert_eq!(actual, [0x9a; 1001]);
    drop(file);
    fs.unmount().unwrap();
}

#[test]
fn buffering_deadlines_and_manifest_corruption() {
    use embedded_storage_volume::package::{Manifest, Verifier};
    use sha2::{Digest, Sha256};
    assert!(!WritePolicy::LOG.due(1, 999, 0));
    assert!(WritePolicy::LOG.due(4096, 0, 0));
    assert!(WritePolicy::LOG.due(1, 1000, 0));
    assert!(WritePolicy::LOG.due(1, 0, 100));
    assert!(!WritePolicy::Immediate.due(0, 1000, 0));
    let mut image = [1; 100];
    image[0] = 0xe9;
    image[12] = 9;
    image[13] = 0;
    let manifest = Manifest {
        format: 1,
        board: "seeed-reterminal-sticky",
        chip: "esp32s3",
        kind: "app0",
        version: "test",
        length: 100,
        sha256: Sha256::digest(image).into(),
    };
    let mut buffer = [0; 1024];
    let size = serde_json_core::to_slice(&manifest, &mut buffer).unwrap();
    let parsed = Manifest::parse(&buffer[..size]).unwrap();
    let mut verifier = Verifier::new(&parsed);
    for chunk in image.chunks(13) {
        verifier.update(chunk).unwrap();
    }
    verifier.finish().unwrap();
    let mut verifier = Verifier::new(&parsed);
    image[45] ^= 1;
    verifier.update(&image).unwrap();
    assert_eq!(verifier.finish(), Err(Error::Package));
    let mut verifier = Verifier::new(&parsed);
    verifier.update(&image[..99]).unwrap();
    assert_eq!(verifier.finish(), Err(Error::Package));
    assert!(Manifest::parse(&[0; 1025]).is_err());
}

#[test]
fn read_and_shutdown_errors_poison_littlefs_until_abandoned() {
    use littlefs2::driver::Storage;
    for read_failure in [false, true] {
        let mut media = Media::new(4096);
        media.fail_read = read_failure;
        media.fail_flush = !read_failure;
        let mut adapter = Littlefs::<_, 16>::new(&mut media, state_volume()).unwrap();
        if read_failure {
            assert!(adapter.read(0, &mut [0; 512]).is_err());
        } else {
            adapter = adapter.shutdown().err().unwrap();
        }
        assert!(adapter.write(0, &[1; 512]).is_err());
        let media = adapter.release();
        assert_eq!(media.writes, 0);
        assert_eq!(media.flushes, usize::from(!read_failure));
    }
}

#[test]
fn gpt_rejects_invalid_guids_and_missing_end_reservation_before_writing() {
    use gpt_disk_types::{guid, Guid};
    let mut media = Media::new(1048576);
    let mut scratch = [0; partition::GPT_SCRATCH_BYTES];
    let layout = Layout::default_for(media.sectors - 34).unwrap();
    let ids = [
        guid!("7cb7e010-fc74-4cf0-9800-11d2250e6110"),
        guid!("7cb7e011-fc74-4cf0-9800-11d2250e6110"),
        guid!("7cb7e012-fc74-4cf0-9800-11d2250e6110"),
        guid!("7cb7e013-fc74-4cf0-9800-11d2250e6110"),
    ];
    let mut zero = ids;
    zero[1] = Guid::ZERO;
    let mut duplicate = ids;
    duplicate[2] = duplicate[1];
    for bad in [zero, duplicate] {
        assert!(partition::write_gpt(&mut media, &layout, bad, &mut scratch, true).is_err());
        assert_eq!(media.writes, 0);
        assert_eq!(media.flushes, 0);
    }
    let no_reservation = Layout::default_for(media.sectors).unwrap();
    assert!(partition::write_gpt(&mut media, &no_reservation, ids, &mut scratch, true).is_err());
    assert_eq!(media.writes, 0);
}

#[test]
fn manifest_targets_and_image_header_failures_do_not_pass_with_a_matching_hash() {
    use embedded_storage_volume::package::{Manifest, Verifier, APP_MAX};
    use sha2::{Digest, Sha256};
    let mut image = [0; 100];
    image[0] = 0xe9;
    image[1] = 1;
    image[12] = 9;
    for (board, chip, kind, format, version, length) in [
        ("other", "esp32s3", "app0", 1, "test", 100),
        ("seeed-reterminal-sticky", "esp32", "app0", 1, "test", 100),
        ("seeed-reterminal-sticky", "esp32s3", "app1", 1, "test", 100),
        ("seeed-reterminal-sticky", "esp32s3", "app0", 0, "test", 100),
        ("seeed-reterminal-sticky", "esp32s3", "app0", 1, "", 100),
        ("seeed-reterminal-sticky", "esp32s3", "app0", 1, "test", 23),
        (
            "seeed-reterminal-sticky",
            "esp32s3",
            "app0",
            1,
            "test",
            APP_MAX + 1,
        ),
    ] {
        let manifest = Manifest {
            board,
            chip,
            kind,
            format,
            version,
            length,
            sha256: Sha256::digest(image).into(),
        };
        let mut buffer = [0; 1024];
        let n = serde_json_core::to_slice(&manifest, &mut buffer).unwrap();
        assert!(Manifest::parse(&buffer[..n]).is_err());
    }
    for (offset, value) in [(0, 0), (1, 0), (12, 0), (23, 2)] {
        let mut bad = image;
        bad[offset] = value;
        let manifest = Manifest {
            board: "seeed-reterminal-sticky",
            chip: "esp32s3",
            kind: "app0",
            format: 1,
            version: "test",
            length: 100,
            sha256: Sha256::digest(bad).into(),
        };
        let mut verifier = Verifier::new(&manifest);
        verifier.update(&bad).unwrap();
        assert!(verifier.finish().is_err());
    }
}
