//! Persistent SD volumes on the display's Core 1 SPI owner.
//!
//! *The Embassy Book* separates task scheduling from synchronous peripheral
//! transfers. Core 0 submits bounded jobs; this module performs SD callbacks
//! on Core 1 and yields between stress rounds. No critical section spans SPI.
//! *The Embedded Rust Book* ownership model keeps GPIO8 and GPIO10 with their
//! owner; the latch witness gates construction before any SD power is enabled.
//! *The Rust on ESP Book* distinguishes internal RAM, PSRAM, and the two CPU
//! executors. Only small requests cross that boundary here; filesystem handles
//! and borrowed SPI resources never move to the radio executor.
//!
//! References: Embedded Rust Book, "Concurrency"; Rust on ESP Book, "Memory
//! allocation"; Embassy Book, "Executor". Board wiring and software sequencing
//! are recorded in the hardware skill's "microSD" and "Deep-sleep rails".
extern crate alloc;
#[cfg(any(feature = "sd", feature = "charge", feature = "mic", feature = "radio"))]
compile_error!(
    "storage shares the default pair/wifi image; do not combine exclusive diagnostic features"
);
use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use embassy_sync::{
    blocking_mutex::{raw::CriticalSectionRawMutex, Mutex},
    channel::Channel,
    signal::Signal,
};
use embassy_time::{Delay, Duration, Timer};
use embedded_hal::{
    delay::DelayNs,
    digital::OutputPin,
    spi::{ErrorType, Operation, SpiDevice},
};
use embedded_hal_bus::spi::RefCellDevice;
use embedded_storage_volume::{
    fat,
    littlefs::Littlefs,
    package::{Manifest, Verifier, APP_MAX, MANIFEST_MAX},
    partition,
    sd::SdMedia,
};
use embedded_storage_volume::{BlockDevice, BlockDeviceMut, BlockIndex, Error, Layout};
use esp_hal::{
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    peripherals::{GPIO10, GPIO11, GPIO8},
    spi::master::{Config, Spi},
    time::Rate,
    Blocking,
};
use esp_println::println;
use littlefs2::{io::OpenSeekFrom, path};
use remote_debug_wire::v1::{StorageOperation as Op, StorageReply, StorageRequest};
use seeed_reterminal_sticky::{
    power::Latched,
    rails::{Disabled, Enabled, Rail, SdRail},
    sd,
};
use static_cell::StaticCell;

/// State/update geometry: 64 MiB divided into 4 KiB logical littlefs blocks.
const LITTLEFS_BLOCKS: usize = 16384;
/// One physical SD sector and the largest acknowledged transfer payload.
const CHUNK_BYTES: usize = partition::SECTOR_BYTES;
/// Confirmed stress is bounded so a malformed request cannot run forever.
#[cfg(feature = "remote-debug")]
const STRESS_ROUNDS_MAX: u32 = 10000;
/// Test-only delays are at most one minute, encoded in milliseconds on wire.
#[cfg(feature = "storage-test")]
const TEST_DELAY_MAX_MS: u32 = 60000;
/// Recovery waits this long with GPIO10 disabled before identifying again.
/// This is software settling time, not a measured rail discharge deadline.
const RECOVERY_SETTLE_MS: u32 = 100;
/// Core 0 retains power/session when Core 1 cannot complete this barrier.
const QUIESCE_TIMEOUT_SECS: u64 = 5;

/// Shared controller, exclusively used by Core 1 tasks.
pub type SharedBus = RefCell<Spi<'static, Blocking>>;
/// The SPI object cannot move while either device holds a borrowed bus handle.
static SPI_BUS: StaticCell<SharedBus> = StaticCell::new();
/// SD CS remains owned even when driver initialization fails and must retry.
static SD_CS: StaticCell<RefCell<Output<'static>>> = StaticCell::new();
/// Cross-core requests contain at most one 512-byte payload per slot.
static REQUESTS: Channel<CriticalSectionRawMutex, StorageRequest, 2> = Channel::new();
/// FIFO replies are consumed by the one GATT notify owner on Core 0.
pub static REPLIES: Channel<CriticalSectionRawMutex, StorageReply, 4> = Channel::new();
/// Successful shutdown barrier; a false result preserves power for recovery.
static QUIESCED: Signal<CriticalSectionRawMutex, (u32, bool)> = Signal::new();
/// Generation of the pending internal barrier; zero cancels queued work.
static BARRIER: AtomicU32 = AtomicU32::new(0);
/// Monotonic barrier identifiers prevent a late reply completing another wait.
static NEXT_BARRIER: AtomicU32 = AtomicU32::new(1);
/// Long operation active; prevents concurrent format and staging requests.
static BUSY: AtomicBool = AtomicBool::new(false);
/// Stop accepting writes before normal reset or sleep.
static STOPPING: AtomicBool = AtomicBool::new(false);
/// All configured filesystems mounted successfully.
static MOUNTED: AtomicBool = AtomicBool::new(false);
/// Validated update package exists.
static READY: AtomicBool = AtomicBool::new(false);
/// Sector count is bounded by the upstream SD driver's u32 addressing.
static SECTORS: AtomicU32 = AtomicU32::new(0);
/// Persisted record sequence recovered on mount and advanced after readback.
static VERIFIED: AtomicU32 = AtomicU32::new(0);
/// Last job failed; retained until another admitted operation completes.
static FAILED: AtomicBool = AtomicBool::new(false);
/// Four completion receipts; the short lock only copies data, never accesses SPI.
static JOBS: Mutex<CriticalSectionRawMutex, RefCell<embedded_storage_volume::jobs::Jobs>> =
    Mutex::new(RefCell::new(embedded_storage_volume::jobs::Jobs::new()));
/// Confirmed test fault consumed by the Core 0 latch/reset owner.
#[cfg(feature = "storage-test")]
pub static FAULT: Signal<CriticalSectionRawMutex, (Op, u32)> = Signal::new();
/// Test-only interruption at the next completed pending-file write boundary.
#[cfg(feature = "storage-test")]
pub static CUT_SD: AtomicBool = AtomicBool::new(false);

/// GPIO9's last digital external-power sample: unknown=0, absent=1, present=2.
/// Core 0 owns the input; replies on either core read only this atomic cache.
/// The voltage divider is read-only here, with no gauge or charger operation.
static EXTERNAL_POWER: AtomicU8 = AtomicU8::new(EXTERNAL_UNKNOWN);
/// No GPIO9 sample has been published since this boot.
const EXTERNAL_UNKNOWN: u8 = 0;
/// GPIO9 reads below the digital high threshold; this is not a zero-volt proof.
const EXTERNAL_ABSENT: u8 = 1;
/// GPIO9 reads high on the board's external-power divider.
const EXTERNAL_PRESENT: u8 = 2;
/// Core 0 sample interval; software scheduling choice, not a measured deadline.
const EXTERNAL_SAMPLE_MS: u64 = 100;

/// Observe GPIO9 PWR_IN_VOLT on Core 0 without driving the divider net.
/// This samples the digital indication every 100 ms and yields between reads.
/// This distinguishes the divider's indication from USB enumeration state;
/// a low indication is not an analog zero-volt or battery state measurement.
#[embassy_executor::task]
pub async fn external_power_task(pin: Input<'static>) {
    loop {
        EXTERNAL_POWER.store(
            if pin.is_high() {
                EXTERNAL_PRESENT
            } else {
                EXTERNAL_ABSENT
            },
            Ordering::Release,
        );
        Timer::after(Duration::from_millis(EXTERNAL_SAMPLE_MS)).await;
    }
}

/// Raw board resources already gated by the acquired power latch.
pub struct StorageParts {
    /// GPIO8, SD chip select; idle-high while the card is powered.
    pub cs: GPIO8<'static>,
    /// GPIO11, active-low card detect with an external pull-up.
    pub detect: GPIO11<'static>,
    /// GPIO10 rail, initially disabled.
    pub rail: SdRail<SdEnablePin, Disabled>,
}
/// GPIO10 enable output whose disabled level survives MCU deep sleep.
///
/// The typed rail controls voltage transitions and settle delays. This adapter
/// couples a low output to the HAL's pad hold, then releases that hold before
/// enable. It owns the pin and performs no I/O on drop. Core 0 constructs it
/// after the latch; StorageParts moves ownership to Core 1.
pub struct SdEnablePin(Output<'static>);
impl embedded_hal::digital::ErrorType for SdEnablePin {
    /// GPIO10 level/hold writes are infallible in this HAL adapter.
    type Error = core::convert::Infallible;
}
impl OutputPin for SdEnablePin {
    /// Drive SD_EN low, then retain that disabled level through deep sleep.
    /// The ESP HAL's pad hold preserves configuration without a rail pulse.
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.0.set_low();
        self.0.set_pad_hold(true);
        Ok(())
    }
    /// Release the disabled pad hold before driving SD_EN high.
    /// Rail::enable supplies the subsequent settle delay; GPIO writes cannot
    /// fail on this HAL, and no SPI transaction starts from this method.
    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.0.set_pad_hold(false);
        self.0.set_high();
        Ok(())
    }
}
/// Construct GPIO10 disabled after the latch; no bus traffic occurs here.
/// The output starts low to avoid an unintended card-power pulse. The latch
/// witness proves board acquisition; this infallible GPIO cannot fail to park.
pub fn park_rail(pin: GPIO10<'static>, latch: &Latched) -> SdRail<SdEnablePin, Disabled> {
    Rail::new(
        SdEnablePin(Output::new(pin, Level::Low, OutputConfig::default())),
        latch,
    )
    .expect("infallible SD enable GPIO")
}
/// Move the shared controller into stable storage on Core 1.
/// Call exactly once before constructing either device. StaticCell prevents a
/// second owner; a repeated call panics rather than aliasing the peripheral.
pub fn share(bus: Spi<'static, Blocking>) -> &'static SharedBus {
    SPI_BUS.init(RefCell::new(bus))
}
/// GPIO CS handle with application-controlled sharing on one core.
#[derive(Clone, Copy)]
pub struct CsPin(&'static RefCell<Output<'static>>);
impl embedded_hal::digital::ErrorType for CsPin {
    /// Core 1 GPIO chip-select writes cannot fail in this adapter.
    type Error = core::convert::Infallible;
}
impl OutputPin for CsPin {
    /// Assert the caller's CS (GPIO8 SD or GPIO15 panel) on Core 1 only.
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().set_low();
        Ok(())
    }
    /// Deselect that caller's CS even when the transfer reports an error.
    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().set_high();
        Ok(())
    }
}
/// SPI device that selects its clock before each transaction.
/// The bus borrow ends before RefCellDevice asserts CS, so clock changes and
/// complete transactions remain synchronous and cannot interleave on Core 1.
pub struct ConfiguredDevice {
    /// SPI2 clock configuration is borrowed only before CS assertion.
    bus: &'static SharedBus,
    /// RefCellDevice brackets operations with CS and releases the bus borrow.
    device: RefCellDevice<'static, Spi<'static, Blocking>, CsPin, Delay>,
    /// This device's selected clock; initialization starts at INIT_HZ.
    hz: u32,
}
impl ConfiguredDevice {
    /// Build a device from caller-owned stable CS storage and a shared bus.
    /// Both handles must stay on Core 1. Construction deselects the infallible
    /// GPIO; frequency validation occurs when the first transaction runs.
    pub fn new(bus: &'static SharedBus, cs: &'static RefCell<Output<'static>>, hz: u32) -> Self {
        Self {
            bus,
            device: RefCellDevice::new(bus, CsPin(cs), Delay).expect("infallible CS GPIO"),
            hz,
        }
    }
    /// Change this device's next-transaction frequency after SD initialization.
    fn set_hz(&mut self, hz: u32) {
        self.hz = hz;
    }
}
impl ErrorType for ConfiguredDevice {
    /// Preserve clock-selection and transfer failures as distinct device errors.
    type Error = ConfiguredError;
}
/// Separate clock-configuration failures from complete SPI transaction failures.
#[derive(Debug)]
pub enum ConfiguredError {
    /// Requested frequency cannot be configured by the HAL.
    Clock,
    /// Transfer or chip-select error from the shared-device adapter.
    Transfer(embedded_hal_bus::spi::DeviceError<esp_hal::spi::Error, core::convert::Infallible>),
}
impl embedded_hal::spi::Error for ConfiguredError {
    /// Preserve the HAL error category; clock setup has no specific bus kind.
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        match self {
            Self::Clock => embedded_hal::spi::ErrorKind::Other,
            Self::Transfer(error) => error.kind(),
        }
    }
}
impl SpiDevice for ConfiguredDevice {
    /// Select SPI2 frequency while both clients are deselected, then execute
    /// the complete synchronous transfer. A configuration failure asserts no
    /// CS; RefCellDevice deselects on a transfer failure. No await can retain
    /// either RefCell borrow or interleave panel/card operations.
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        self.bus
            .borrow_mut()
            .apply_config(&Config::default().with_frequency(Rate::from_hz(self.hz)))
            .map_err(|_| ConfiguredError::Clock)?;
        self.device
            .transaction(operations)
            .map_err(ConfiguredError::Transfer)
    }
}
/// Stable panel CS storage; both device handles use the same arbitration rules.
static PANEL_CS: StaticCell<RefCell<Output<'static>>> = StaticCell::new();
/// Construct the panel device; every SD transaction has ended before it runs.
/// GPIO15 is owned by the display caller. The same synchronous transaction
/// adapter applies its clock before CS, including after a low-speed SD retry.
pub fn panel_device(bus: &'static SharedBus, pin: Output<'static>, hz: u32) -> ConfiguredDevice {
    ConfiguredDevice::new(bus, PANEL_CS.init(RefCell::new(pin)), hz)
}
/// Runtime owner of card transport, capacity, partition descriptors, and rail.
/// Filesystems mount for bounded operations instead of keeping self-referential
/// handles. Failed mounts never trigger provisioning.
pub struct Runtime {
    /// Core 1 controller; no filesystem handle escapes this owner.
    bus: &'static SharedBus,
    /// Stable GPIO8 handle, idle high while powered.
    cs: &'static RefCell<Output<'static>>,
    /// GPIO11 is sampled with the board external pull-up.
    detect: Input<'static>,
    /// Enabled GPIO10 owner, moved into disabled after a successful cut.
    rail: Option<SdRail<SdEnablePin, Enabled>>,
    /// Disabled GPIO10 owner, retained across failed identification.
    disabled: Option<SdRail<SdEnablePin, Disabled>>,
    /// SD callback adapter; a failed one is abandoned before reidentification.
    media: Option<SdMedia<ConfiguredDevice, Delay>>,
    /// Validated MBR descriptors; absent after mount failure or rail release.
    layout: Option<Layout>,
    /// Length of this boot's active pending transfer; reset never resumes it.
    staging_length: Option<u64>,
}
impl Runtime {
    /// Settle SD power and initialize before the panel starts using SPI2.
    /// A card error leaves the GPIO owner available for remote rail recovery.
    /// GPIO8 starts high and GPIO11 uses its external pull-up. Startup keeps
    /// BUSY set until the storage task checks ready data; construction never
    /// formats an absent or incompatible card.
    pub fn new(bus: &'static SharedBus, parts: StorageParts) -> Self {
        BUSY.store(true, Ordering::Release);
        let cs = SD_CS.init(RefCell::new(Output::new(
            parts.cs,
            Level::High,
            OutputConfig::default(),
        )));
        let mut runtime = Self {
            bus,
            cs,
            detect: Input::new(parts.detect, InputConfig::default().with_pull(Pull::None)),
            rail: None,
            disabled: Some(parts.rail),
            media: None,
            layout: None,
            staging_length: None,
        };
        if runtime.initialize().is_err() {
            FAILED.store(true, Ordering::Release);
        }
        runtime
    }
    /// Initialize at <=400 kHz, then raise this card's transactions to 10 MHz.
    /// GPIO10's typed rail enables and settles before idle clocks. Card detect,
    /// identify, driver initialization, or mount errors propagate and retain
    /// resources for explicit recovery; no filesystem is reformatted here.
    fn initialize(&mut self) -> Result<(), Error> {
        if !sd::card_inserted(self.detect.is_high()) {
            return Err(Error::Media);
        }
        self.cs.borrow_mut().set_pad_hold(false);
        self.cs.borrow_mut().set_high();
        if let Some(rail) = self.disabled.take() {
            self.rail = Some(rail.enable(&mut Delay).expect("infallible SD rail"));
        }
        // Board identify emits the required idle clocks with both CS lines high.
        let mut spi = self.bus.borrow_mut();
        spi.apply_config(&Config::default().with_frequency(Rate::from_hz(sd::INIT_HZ)))
            .map_err(|_| Error::Media)?;
        sd::identify(&mut *spi, &mut *self.cs.borrow_mut(), &mut Delay)
            .map_err(|_| Error::Media)?;
        drop(spi);
        let card = embedded_sdmmc::SdCard::new(
            ConfiguredDevice::new(self.bus, self.cs, sd::INIT_HZ),
            Delay,
        );
        let media = SdMedia::new(card)?;
        media
            .card()
            .spi(|device| device.set_hz(seeed_reterminal_sticky::display::SPI_MAX_HZ));
        SECTORS.store(media.geometry().block_count.0 as u32, Ordering::Release);
        self.media = Some(media);
        self.mount()
    }
    /// Probe every volume and verify state without changing card contents.
    /// Clear published mount/ready state first so a failed retry cannot expose
    /// a stale layout. This synchronous step validates filesystems and the
    /// state digest; storage_task separately performs yielding ready readback.
    fn mount(&mut self) -> Result<(), Error> {
        MOUNTED.store(false, Ordering::Release);
        READY.store(false, Ordering::Release);
        self.layout = None;
        self.staging_length = None;
        let media = self.media.as_mut().ok_or(Error::Media)?;
        let layout = Layout::read_mbr(media)?;
        for volume in &layout.volumes {
            match volume.filesystem {
                partition::FilesystemKind::Littlefs => {
                    Littlefs::<_, LITTLEFS_BLOCKS>::new(media, *volume)?.with_fs(|_| Ok(()))?
                }
                partition::FilesystemKind::Fat32 => {
                    let stream = fat::FatStream::new(media, *volume)?;
                    fat::FileSystem::new(stream, fat::FsOptions::new())
                        .map_err(|_| Error::Filesystem)?
                        .unmount()
                        .map_err(|_| Error::Filesystem)?;
                }
            }
        }
        self.layout = Some(layout);
        self.verify_record()?;
        MOUNTED.store(true, Ordering::Release);
        Ok(())
    }
    /// Recover and validate the published counter before accepting new writes.
    /// Missing state is valid on a newly formatted card; corrupt state fails.
    fn verify_record(&mut self) -> Result<(), Error> {
        let volume = self.layout.ok_or(Error::Partition)?.volumes[0];
        let media = self.media.as_mut().ok_or(Error::Media)?;
        let sequence = Littlefs::<_, LITTLEFS_BLOCKS>::new(media, volume)?.with_fs(|fs| {
            match fs.metadata(path!("record.bin")) {
                Err(littlefs2::io::Error::NO_SUCH_ENTRY) => return Ok(0),
                Err(error) => return Err(error),
                Ok(_) => {}
            }
            let record = fs.read::<CHUNK_BYTES>(path!("record.bin"))?;
            embedded_storage_volume::record::decode(&record)
                .map_err(|_| littlefs2::io::Error::CORRUPTION)
        })?;
        VERIFIED.store(sequence, Ordering::Release);
        Ok(())
    }
    /// Invalidate MBR before formatting, and publish the validated table last.
    /// Admission already requires confirmation. Every phase propagates errors;
    /// interruption leaves an invalid table rather than a partially formatted
    /// layout advertised as usable. Formatting blocks Core 1 until it returns.
    fn provision(&mut self) -> Result<(), Error> {
        MOUNTED.store(false, Ordering::Release);
        READY.store(false, Ordering::Release);
        self.layout = None;
        self.staging_length = None;
        let media = self.media.as_mut().ok_or(Error::Media)?;
        let layout = Layout::default_for(media.geometry().block_count.0)?;
        media
            .write_blocks(BlockIndex(0), &[0; 512])
            .map_err(|_| Error::Media)?;
        media.flush().map_err(|_| Error::Media)?;
        for volume in layout.volumes {
            match volume.filesystem {
                partition::FilesystemKind::Littlefs => {
                    Littlefs::<_, LITTLEFS_BLOCKS>::new(media, volume)?.format(true)?
                }
                partition::FilesystemKind::Fat32 => {
                    fat::format(&mut fat::FatStream::new(media, volume)?, true)?
                }
            }
        }
        partition::write_mbr(media, &layout, true)?;
        self.mount()
    }
    /// One committed state replacement and readback, using a separate pending
    /// file so a failure can preserve the previous named record.
    /// A 512-byte stack record contains sequence and SHA-256. State publication
    /// precedes replaceable FAT work; a FAT failure may therefore leave a valid
    /// newer state. The caller yields between rounds, never inside SPI.
    fn round(&mut self) -> Result<(), Error> {
        let volume = self.layout.ok_or(Error::Partition)?.volumes[0];
        let media = self.media.as_mut().ok_or(Error::Media)?;
        let sequence = VERIFIED.load(Ordering::Acquire).wrapping_add(1);
        let record = embedded_storage_volume::record::encode(sequence);
        #[cfg(feature = "storage-test")]
        let cut = &mut self.rail;
        #[cfg(feature = "storage-test")]
        let disabled = &mut self.disabled;
        let result = Littlefs::<_, LITTLEFS_BLOCKS>::new(media, volume)?.with_fs(|fs| {
            fs.write(path!("record.pending"), &record)?;
            #[cfg(feature = "storage-test")]
            if CUT_SD.swap(false, Ordering::AcqRel) {
                // No transfer is active here. This controlled cut tests file
                // publication; MCU/latch faults exercise arbitrary callbacks.
                self.cs.borrow_mut().set_high();
                self.bus
                    .borrow_mut()
                    .write(&[0])
                    .map_err(|_| littlefs2::io::Error::IO)?;
                if let Some(rail) = cut.take() {
                    *disabled = Some(rail.disable().expect("infallible SD rail"));
                }
                self.cs.borrow_mut().set_low();
                return Err(littlefs2::io::Error::IO);
            }
            fs.rename(path!("record.pending"), path!("record.bin"))?;
            let read = fs.read::<CHUNK_BYTES>(path!("record.bin"))?;
            if read.as_slice() != record {
                return Err(littlefs2::io::Error::CORRUPTION);
            }
            Ok(())
        });
        #[cfg(feature = "storage-test")]
        if self.rail.is_none() {
            // The controlled cut occurs between callbacks, so no transport
            // error has marked the driver yet. Poison it after the filesystem
            // borrow ends: later shutdown must refuse this stale powered-off
            // driver, and power_cycle must reconstruct it without cleanup I/O.
            media.invalidate();
        }
        result?;
        VERIFIED.store(sequence, Ordering::Release);
        // FAT is bulk storage. Its flushes do not make directory updates atomic;
        // only the littlefs record is the recovery acceptance criterion.
        let volume = self.layout.ok_or(Error::Partition)?.volumes[2];
        let fs = fat::FileSystem::new(fat::FatStream::new(media, volume)?, fat::FsOptions::new())
            .map_err(|_| Error::Filesystem)?;
        {
            use fat::{Read, Seek, SeekFrom, Write};
            let mut file = fs
                .root_dir()
                .create_file("bulk.dat")
                .map_err(|_| Error::Filesystem)?;
            file.truncate().map_err(|_| Error::Filesystem)?;
            file.write_all(&record).map_err(|_| Error::Filesystem)?;
            file.flush().map_err(|_| Error::Filesystem)?;
            file.seek(SeekFrom::Start(0))
                .map_err(|_| Error::Filesystem)?;
            let mut read = [0; CHUNK_BYTES];
            file.read_exact(&mut read).map_err(|_| Error::Filesystem)?;
            if read != record {
                return Err(Error::Filesystem);
            }
        }
        fs.unmount().map_err(|_| Error::Filesystem)?;
        Ok(())
    }
    /// Validate one package with bounded metadata and payload buffers. Mount
    /// only for a 512-byte chunk, then yield so Core 1 can repaint the panel.
    /// Missing ready data is valid; incomplete pending data is always rejected.
    /// A bounded manifest and 512-byte payload chunk occupy task storage, not a
    /// package-sized heap allocation. Mounts close before each await. Exact
    /// length, digest, header, media, or STOPPING errors prevent publication.
    async fn validate_package(
        &mut self,
        path: &littlefs2::path::Path,
        optional: bool,
    ) -> Result<bool, Error> {
        let volume = self.layout.ok_or(Error::Partition)?.volumes[1];
        let media = self.media.as_mut().ok_or(Error::Media)?;
        let header = Littlefs::<_, LITTLEFS_BLOCKS>::new(media, volume)?.with_fs(|fs| {
            match fs.metadata(path) {
                Err(littlefs2::io::Error::NO_SUCH_ENTRY) if optional => return Ok(None),
                Err(error) => return Err(error),
                Ok(_) => {}
            }
            let (bytes, length) =
                fs.read_chunk::<{ MANIFEST_MAX + 4 }>(path, OpenSeekFrom::Start(0))?;
            if bytes.len() < 4 {
                return Err(littlefs2::io::Error::INVALID);
            }
            let size = u32::from_le_bytes(
                bytes[..4]
                    .try_into()
                    .map_err(|_| littlefs2::io::Error::INVALID)?,
            ) as usize;
            if size == 0 || size > MANIFEST_MAX || bytes.len() < size + 4 {
                return Err(littlefs2::io::Error::INVALID);
            }
            let manifest =
                Manifest::parse(&bytes[4..4 + size]).map_err(|_| littlefs2::io::Error::INVALID)?;
            if length != 4 + size + manifest.length as usize {
                return Err(littlefs2::io::Error::INVALID);
            }
            Ok(Some((Verifier::new(&manifest), 4 + size, length)))
        })?;
        let Some((mut verifier, mut offset, length)) = header else {
            return Ok(false);
        };
        while offset < length {
            if STOPPING.load(Ordering::Acquire) {
                return Err(Error::Media);
            }
            let (bytes, actual_length) = Littlefs::<_, LITTLEFS_BLOCKS>::new(media, volume)?
                .with_fs(|fs| {
                    fs.read_chunk::<CHUNK_BYTES>(path, OpenSeekFrom::Start(offset as u32))
                })?;
            if bytes.is_empty() || actual_length != length {
                return Err(Error::Package);
            }
            verifier.update(&bytes)?;
            offset += bytes.len();
            Timer::after(Duration::from_millis(1)).await;
        }
        verifier.finish()?;
        Ok(true)
    }
    /// Write an ordered package chunk and close/sync before its acknowledgement.
    /// staging_length admits only this boot's uninterrupted transfer. Any
    /// offset/size or filesystem error cancels it; begin replaces only pending
    /// data. Finish is called after full readback and renames to ready last.
    fn stage(&mut self, request: &StorageRequest, operation: Op) -> Result<(), Error> {
        // Validate sequencing before opening a file. A stale pending file from
        // a previous boot never grants permission to append or publish it.
        match operation {
            Op::STORAGE_OPERATION_STAGE_BEGIN => self.staging_length = None,
            Op::STORAGE_OPERATION_STAGE_CHUNK => {
                if embedded_storage_volume::package::next_offset(
                    self.staging_length,
                    request.offset,
                    request.data.len(),
                )
                .is_err()
                {
                    self.staging_length = None;
                    return Err(Error::Package);
                }
            }
            Op::STORAGE_OPERATION_STAGE_FINISH => {
                if self.staging_length.take() != Some(request.offset) {
                    return Err(Error::Package);
                }
            }
            _ => return Err(Error::Package),
        }
        let volume = self.layout.ok_or(Error::Partition)?.volumes[1];
        let media = self.media.as_mut().ok_or(Error::Media)?;
        let result =
            Littlefs::<_, LITTLEFS_BLOCKS>::new(media, volume)?.with_fs(|fs| match operation {
                Op::STORAGE_OPERATION_STAGE_BEGIN => fs.write(path!("pending.pkg"), &[]),
                Op::STORAGE_OPERATION_STAGE_CHUNK => {
                    if request.data.is_empty() || request.data.len() > CHUNK_BYTES {
                        return Err(littlefs2::io::Error::INVALID);
                    }
                    let length = fs.metadata(path!("pending.pkg"))?.len();
                    if request.offset != length as u64
                        || length + request.data.len() > 4 + MANIFEST_MAX + APP_MAX as usize
                    {
                        return Err(littlefs2::io::Error::INVALID);
                    }
                    fs.write_chunk(path!("pending.pkg"), &request.data, OpenSeekFrom::End(0))
                }
                Op::STORAGE_OPERATION_STAGE_FINISH => {
                    fs.rename(path!("pending.pkg"), path!("ready.pkg"))
                }
                _ => Err(littlefs2::io::Error::INVALID),
            });
        if let Err(error) = result {
            self.staging_length = None;
            return Err(error);
        }
        if operation == Op::STORAGE_OPERATION_STAGE_BEGIN {
            self.staging_length = Some(0);
        } else if operation == Op::STORAGE_OPERATION_STAGE_CHUNK {
            self.staging_length = Some(request.offset + request.data.len() as u64);
        }
        if operation == Op::STORAGE_OPERATION_STAGE_FINISH {
            READY.store(true, Ordering::Release);
        }
        Ok(())
    }
    /// Stop clocks with both devices deselected, then cut the SD supply.
    /// Firmware sequencing does not establish that the rail reaches zero volts.
    /// Flush failure returns before cutting power. Sending zero parks MOSI low
    /// with SCK idle low; GPIO8 is held low only after GPIO10 is disabled to
    /// avoid sourcing the unpowered card through CS. Handles are then released.
    fn disable(&mut self) -> Result<(), Error> {
        if let Some(media) = self.media.as_mut() {
            media.flush().map_err(|_| Error::Media)?;
        }
        self.cs.borrow_mut().set_high();
        self.bus
            .borrow_mut()
            .write(&[0])
            .map_err(|_| Error::Media)?;
        if let Some(rail) = self.rail.take() {
            self.disabled = Some(rail.disable().expect("infallible SD rail"));
        }
        self.cs.borrow_mut().set_low();
        // Keep CS low beside held-low SD_EN through deep sleep. Initialization
        // explicitly releases this hold before idle clocks or card commands.
        self.cs.borrow_mut().set_pad_hold(true);
        self.media = None;
        self.layout = None;
        self.staging_length = None;
        MOUNTED.store(false, Ordering::Release);
        Ok(())
    }
    /// Explicit rail recovery, always returning through low-speed card init.
    /// A poisoned transport can fail shutdown; recovery deliberately abandons
    /// it, repeats bus parking, and waits 100 ms before reidentification. This
    /// is the sole path that resumes writes after a successful quiesce.
    fn power_cycle(&mut self) -> Result<(), Error> {
        self.staging_length = None;
        // Recovery also permits abandoning a poisoned transport's failed flush.
        let _ = self.disable();
        self.media = None;
        self.cs.borrow_mut().set_pad_hold(false);
        self.cs.borrow_mut().set_high();
        self.bus
            .borrow_mut()
            .write(&[0])
            .map_err(|_| Error::Media)?;
        if let Some(rail) = self.rail.take() {
            self.disabled = Some(rail.disable().expect("infallible SD rail"));
        }
        self.cs.borrow_mut().set_low();
        Delay.delay_ms(RECOVERY_SETTLE_MS);
        self.cs.borrow_mut().set_pad_hold(false);
        self.cs.borrow_mut().set_high();
        self.initialize()
    }
}
/// Start writes before a test-only reset or power interruption is scheduled.
/// Return false when admission fails, so no fault is armed without active work.
#[cfg(feature = "storage-test")]
pub fn begin_fault_stress() -> bool {
    /// Distinguish repeated test jobs from retained completion receipts.
    static NEXT_FAULT: AtomicU32 = AtomicU32::new(0);
    let id = u64::MAX - 2 - u64::from(NEXT_FAULT.fetch_add(1, Ordering::Relaxed));
    submit(StorageRequest {
        id,
        operation: Op::STORAGE_OPERATION_STRESS.into(),
        count: STRESS_ROUNDS_MAX,
        ..Default::default()
    })
    .is_some_and(|reply| reply.ok && reply.job_id == id)
}
/// Produce cached status without waiting for the Core 1 executor or SPI.
/// Atomic fields are individual observations, not a transactional filesystem
/// snapshot. Completion is established by job_status's receipt, never BUSY.
pub fn status(id: u64, ok: bool, message: &str) -> StorageReply {
    StorageReply {
        id,
        ok,
        message: message.into(),
        sectors: u64::from(SECTORS.load(Ordering::Acquire)),
        mounted: MOUNTED.load(Ordering::Acquire),
        busy: BUSY.load(Ordering::Acquire),
        verified: VERIFIED.load(Ordering::Acquire),
        ready: READY.load(Ordering::Acquire),
        external_power: match EXTERNAL_POWER.load(Ordering::Acquire) {
            EXTERNAL_ABSENT => Some(false),
            EXTERNAL_PRESENT => Some(true),
            _ => None,
        },
        // RTC reset-cause bits are read-only. Retaining this observation in
        // every reply distinguishes reset/sleep/power trials after UART boot
        // lines have passed; ChipPowerOn also covers some analog reset causes.
        reset_reason: alloc::format!(
            "{:?}",
            esp_hal::rtc_cntl::reset_reason(esp_hal::system::Cpu::ProCpu)
        ),
        ..Default::default()
    }
}
/// Query a retained job receipt. Eviction or reboot cannot masquerade as success.
#[cfg(feature = "remote-debug")]
fn job_status(id: u64, job_id: u64) -> StorageReply {
    let receipt = JOBS.lock(|jobs| jobs.borrow().get(job_id));
    let Some(receipt) = receipt else {
        return status(id, false, "job-not-found");
    };
    let mut reply = status(
        id,
        !receipt.complete || receipt.ok,
        if !receipt.complete {
            "working"
        } else if receipt.ok {
            "complete"
        } else {
            "failed"
        },
    );
    reply.job_id = receipt.id;
    reply.job_complete = receipt.complete;
    reply.job_ok = receipt.ok;
    reply
}
/// Queue one bounded request. An immediate reply acknowledges long jobs; short
/// operations reply after completed I/O through the FIFO REPLIES channel.
/// Core 0 validates size, confirmation, and admission without touching SPI.
/// BUSY serializes operations. Queue failure releases admission and records a
/// failed receipt; status remains available during formatting and recovery.
#[cfg(feature = "remote-debug")]
pub fn submit(request: StorageRequest) -> Option<StorageReply> {
    let id = request.id;
    let operation = request.operation.as_known();
    if id == 0 || request.data.len() > CHUNK_BYTES || operation.is_none() {
        return Some(status(id, false, "invalid"));
    }
    let operation = operation.expect("validated enum");
    if (operation == Op::STORAGE_OPERATION_STRESS
        && !(1..=STRESS_ROUNDS_MAX).contains(&request.count))
        || (operation == Op::STORAGE_OPERATION_STAGE_BEGIN
            && (request.offset != 0 || !request.data.is_empty()))
        || (operation == Op::STORAGE_OPERATION_STAGE_FINISH && !request.data.is_empty())
    {
        return Some(status(id, false, "invalid"));
    }
    if operation == Op::STORAGE_OPERATION_STATUS {
        if request.offset != 0 {
            return Some(job_status(id, request.offset));
        }
        return Some(status(
            id,
            !FAILED.load(Ordering::Acquire),
            if BUSY.load(Ordering::Acquire) {
                "working"
            } else {
                "idle"
            },
        ));
    }
    if STOPPING.load(Ordering::Acquire) && operation != Op::STORAGE_OPERATION_POWER_CYCLE {
        return Some(status(id, false, "stopping"));
    }
    if !MOUNTED.load(Ordering::Acquire)
        && !matches!(
            operation,
            Op::STORAGE_OPERATION_PROVISION
                | Op::STORAGE_OPERATION_VERIFY
                | Op::STORAGE_OPERATION_POWER_CYCLE
                | Op::STORAGE_OPERATION_QUIESCE
        )
    {
        return Some(status(id, false, "not-mounted"));
    }
    if BUSY.swap(true, Ordering::AcqRel) {
        return Some(status(id, false, "busy"));
    }
    if operation == Op::STORAGE_OPERATION_PROVISION && !request.confirmed {
        BUSY.store(false, Ordering::Release);
        return Some(status(id, false, "confirmation-required"));
    }
    if matches!(
        operation,
        Op::STORAGE_OPERATION_FAULT_RESET
            | Op::STORAGE_OPERATION_FAULT_SD
            | Op::STORAGE_OPERATION_FAULT_POWER
            | Op::STORAGE_OPERATION_TIMED_SLEEP
    ) {
        BUSY.store(false, Ordering::Release);
        #[cfg(feature = "storage-test")]
        if request.confirmed && (1..=TEST_DELAY_MAX_MS).contains(&request.count) {
            if operation == Op::STORAGE_OPERATION_FAULT_POWER
                && EXTERNAL_POWER.load(Ordering::Acquire) != EXTERNAL_ABSENT
            {
                // Confirmed latch tests still require the divider's low
                // indication. USB enumeration loss is insufficient evidence.
                return Some(status(id, false, "external-power-present-or-unknown"));
            }
            if operation != Op::STORAGE_OPERATION_TIMED_SLEEP {
                if !MOUNTED.load(Ordering::Acquire) {
                    return Some(status(id, false, "not-mounted"));
                }
                if !begin_fault_stress() {
                    return Some(status(id, false, "fault-stress-refused"));
                }
            }
            FAULT.signal((operation, request.count));
            return Some(status(id, true, "fault-armed"));
        }
        return Some(status(id, false, "test-controls-unavailable"));
    }
    let background = matches!(
        operation,
        Op::STORAGE_OPERATION_PROVISION
            | Op::STORAGE_OPERATION_VERIFY
            | Op::STORAGE_OPERATION_STRESS
            | Op::STORAGE_OPERATION_STAGE_FINISH
    );
    if background && JOBS.lock(|jobs| jobs.borrow_mut().begin(id)).is_err() {
        BUSY.store(false, Ordering::Release);
        return Some(status(id, false, "duplicate-job"));
    }
    if operation == Op::STORAGE_OPERATION_QUIESCE {
        STOPPING.store(true, Ordering::Release);
    }
    if REQUESTS.try_send(request).is_err() {
        if operation == Op::STORAGE_OPERATION_QUIESCE {
            STOPPING.store(false, Ordering::Release);
        }
        if background {
            let _ = JOBS.lock(|jobs| jobs.borrow_mut().finish(id, false));
        }
        BUSY.store(false, Ordering::Release);
        return Some(status(id, false, "queue-full"));
    }
    FAILED.store(false, Ordering::Release);
    if background {
        Some(job_status(id, id))
    } else {
        None
    }
}
/// Drain and park storage before normal reboot, deep sleep, or latch release.
/// A five-second timeout preserves power and leaves forced reboot available.
/// Core 0 stops admission before enqueueing the barrier. A generation token
/// cancels stale queued work on timeout; an already executing synchronous SD
/// callback cannot be preempted. Failure restores admission and keeps BLE live.
pub async fn quiesce() -> Result<(), Error> {
    let generation = NEXT_BARRIER.fetch_add(1, Ordering::Relaxed).max(1);
    if BARRIER
        .compare_exchange(0, generation, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(Error::Media);
    }
    QUIESCED.reset();
    STOPPING.store(true, Ordering::Release);
    let request = StorageRequest {
        id: u64::MAX,
        operation: Op::STORAGE_OPERATION_QUIESCE.into(),
        count: generation,
        ..Default::default()
    };
    let outcome = embassy_time::with_timeout(Duration::from_secs(QUIESCE_TIMEOUT_SECS), async {
        REQUESTS.send(request).await;
        loop {
            let (reply_generation, ok) = QUIESCED.wait().await;
            if reply_generation == generation {
                break ok;
            }
        }
    })
    .await;
    BARRIER.store(0, Ordering::Release);
    if outcome == Ok(true) {
        Ok(())
    } else {
        // Cancel a queued barrier before allowing the session to recover. An
        // already running synchronous flush cannot be preempted by Core 0.
        STOPPING.store(false, Ordering::Release);
        Err(Error::Media)
    }
}
/// Core 1 job owner; each filesystem operation owns the shared controller only
/// during synchronous transactions, leaving panel BUSY waits usable by SD.
/// One Runtime receives requests in order. Finish receipts are published before
/// BUSY is released. Short replies use a bounded FIFO to the Core 0 GATT owner;
/// boot ready validation and stress/package loops yield between media calls.
#[embassy_executor::task]
pub async fn storage_task(mut runtime: Runtime) {
    if MOUNTED.load(Ordering::Acquire) {
        match runtime.validate_package(path!("ready.pkg"), true).await {
            Ok(ready) => READY.store(ready, Ordering::Release),
            Err(_) => FAILED.store(true, Ordering::Release),
        }
    }
    BUSY.store(false, Ordering::Release);
    println!(
        "storage init sectors={} mounted={}",
        SECTORS.load(Ordering::Acquire),
        MOUNTED.load(Ordering::Acquire)
    );
    loop {
        let request = REQUESTS.receive().await;
        let Some(operation) = request.operation.as_known() else {
            continue;
        };
        if operation == Op::STORAGE_OPERATION_QUIESCE
            && request.id == u64::MAX
            && BARRIER.load(Ordering::Acquire) != request.count
        {
            // The caller timed out while this request was queued.
            continue;
        }
        let result = match operation {
            Op::STORAGE_OPERATION_PROVISION => runtime.provision(),
            Op::STORAGE_OPERATION_VERIFY => match runtime.mount() {
                Err(error) => Err(error),
                Ok(()) => runtime
                    .validate_package(path!("ready.pkg"), true)
                    .await
                    .map(|ready| READY.store(ready, Ordering::Release)),
            },
            Op::STORAGE_OPERATION_STRESS => {
                let mut result = Ok(());
                for _ in 0..request.count {
                    if STOPPING.load(Ordering::Acquire) {
                        result = Err(Error::Media);
                        break;
                    }
                    result = runtime.round();
                    if result.is_err() {
                        break;
                    }
                    Timer::after(Duration::from_millis(1)).await;
                }
                result
            }
            Op::STORAGE_OPERATION_POWER_CYCLE => {
                let result = runtime.power_cycle();
                if result.is_ok() {
                    STOPPING.store(false, Ordering::Release);
                    match runtime.validate_package(path!("ready.pkg"), true).await {
                        Ok(ready) => READY.store(ready, Ordering::Release),
                        Err(_) => {
                            FAILED.store(true, Ordering::Release);
                            BUSY.store(false, Ordering::Release);
                            REPLIES
                                .send(status(request.id, false, "package-verification-failed"))
                                .await;
                            continue;
                        }
                    }
                }
                result
            }
            Op::STORAGE_OPERATION_STAGE_BEGIN | Op::STORAGE_OPERATION_STAGE_CHUNK => {
                runtime.stage(&request, operation)
            }
            Op::STORAGE_OPERATION_STAGE_FINISH => {
                if runtime.staging_length != Some(request.offset) {
                    runtime.staging_length = None;
                    Err(Error::Package)
                } else {
                    match runtime.validate_package(path!("pending.pkg"), false).await {
                        Ok(true) => runtime.stage(&request, operation),
                        _ => {
                            runtime.staging_length = None;
                            Err(Error::Package)
                        }
                    }
                }
            }
            Op::STORAGE_OPERATION_QUIESCE => runtime.disable(),
            _ => Err(Error::Geometry),
        };
        FAILED.store(result.is_err(), Ordering::Release);
        // Complete the receipt before releasing BUSY; later requests cannot
        // overwrite this result while a host polls its correlation identifier.
        let _ = JOBS.lock(|jobs| jobs.borrow_mut().finish(request.id, result.is_ok()));
        BUSY.store(false, Ordering::Release);
        println!(
            "storage job op={} ok={} verified={}",
            operation as i32,
            result.is_ok(),
            VERIFIED.load(Ordering::Acquire)
        );
        if operation == Op::STORAGE_OPERATION_QUIESCE {
            if result.is_err() {
                STOPPING.store(false, Ordering::Release);
            }
            if request.id == u64::MAX {
                QUIESCED.signal((request.count, result.is_ok()));
                continue;
            }
        }
        if !matches!(
            operation,
            Op::STORAGE_OPERATION_PROVISION
                | Op::STORAGE_OPERATION_VERIFY
                | Op::STORAGE_OPERATION_STRESS
                | Op::STORAGE_OPERATION_STAGE_FINISH
        ) {
            REPLIES
                .send(status(
                    request.id,
                    result.is_ok(),
                    if result.is_ok() { "complete" } else { "failed" },
                ))
                .await;
        }
    }
}
