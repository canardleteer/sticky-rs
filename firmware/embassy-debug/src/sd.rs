//! Read-only MicroSD identify and FAT list (`--features sd`).
//!
//! Rail on, EPD CS high, init at [`seeed_reterminal_sticky::sd::INIT_HZ`],
//! then [`sd::send_status`] at 10 MHz and 20 MHz. Root list and one
//! `Mode::ReadOnly` read via `embedded-sdmmc`. No writes, no CID product
//! serial, no file contents on UART.
//! Do not combine with `mic`, `radio`, or `wifi` (package envelope).

#[cfg(feature = "mic")]
compile_error!("do not combine sd with mic");
#[cfg(feature = "radio")]
compile_error!("do not combine sd with radio");
#[cfg(feature = "wifi")]
compile_error!("do not combine sd with wifi");

use core::ops::ControlFlow;

use embassy_debug::{
    format_sd_ack, format_sd_cd, format_sd_dir, format_sd_ent, format_sd_id, format_sd_none,
    format_sd_read, format_sd_vol, sanitize_radio_label, LINE_CAPACITY, RADIO_LABEL_MAX,
};
use embassy_time::Delay;
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::{Operation, SpiDevice};
use embedded_sdmmc::{
    Mode, SdCard, ShortFileName, TimeSource, Timestamp, VolumeIdx, VolumeManager,
};
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::peripherals::{GPIO10, GPIO11, GPIO8};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::Rate;
use esp_hal::Blocking;
use esp_println::println;
use seeed_reterminal_sticky::display;
use seeed_reterminal_sticky::power::Latched;
use seeed_reterminal_sticky::rails::{Disabled, Rail, SdRail};
use seeed_reterminal_sticky::sd::{self, IdentifyError};

/// Clocks to prove after init.
const RAISE_HZ: [u32; 2] = [display::SPI_MAX_HZ, 20_000_000];

/// How many root entries to print.
const MAX_ENTS: u8 = 8;

/// Bytes to pull from the first regular file (not printed).
const READ_BUF: usize = 64;

/// Pins the display task needs when `--features sd`.
pub struct SdParts {
    /// Card CS. Idle high except during identify / mount.
    pub cs: GPIO8<'static>,
    /// Card detect. Insert = low.
    pub cd: GPIO11<'static>,
    /// Rail starts disabled.
    pub rail: SdRail<Output<'static>, Disabled>,
}

/// Disabled `SdRail` for the display task. Caller must not park GPIO10.
pub fn park_rail(sd_en: GPIO10<'static>, latch: &Latched) -> SdRail<Output<'static>, Disabled> {
    Rail::new(
        Output::new(sd_en, Level::Low, OutputConfig::default()),
        latch,
    )
    .expect("driving the SD rail cannot fail")
}

/// Identify, ACK clocks, then list root. Leaves CS high and the rail off.
pub fn run<D: DelayNs>(spi: &mut Spi<'static, Blocking>, parts: SdParts, delay: &mut D) {
    let mut cs = Output::new(parts.cs, Level::High, OutputConfig::default());
    let cd = Input::new(parts.cd, InputConfig::default().with_pull(Pull::Up));
    let inserted = sd::card_inserted(cd.is_high());
    print_line(|buf| format_sd_cd(inserted, buf));
    if !inserted {
        print_line(|buf| format_sd_none("empty", buf));
        return;
    }

    let rail = parts
        .rail
        .enable(delay)
        .expect("driving the SD rail cannot fail");

    match set_hz(spi, sd::INIT_HZ).and_then(|()| sd::identify(spi, &mut cs, delay)) {
        Ok(id) => {
            let mut name = [0u8; RADIO_LABEL_MAX];
            let name = sanitize_radio_label(&id.name, &mut name);
            print_line(|buf| format_sd_id(sd::INIT_HZ, id.kind.as_str(), id.mid, name, buf));
            for hz in RAISE_HZ {
                let ok = set_hz(spi, hz)
                    .and_then(|()| sd::send_status(spi, &mut cs))
                    .is_ok();
                print_line(|buf| format_sd_ack(hz, ok, buf));
            }
            if set_hz(spi, display::SPI_MAX_HZ).is_ok() {
                list_fat(spi, &mut cs);
            } else {
                print_line(|buf| format_sd_none("fat", buf));
            }
        }
        Err(IdentifyError::Timeout) => print_line(|buf| format_sd_none("timeout", buf)),
        Err(IdentifyError::Unexpected(_)) | Err(IdentifyError::Bus) => {
            print_line(|buf| format_sd_none("nak", buf));
        }
    }

    let _ = rail.disable();
    cs.set_high();
}

/// Borrow shared SPI2 with EPD deselected and inspect FAT volume zero.
/// Enumerate at most `MAX_ENTS` visible short names, then read at most
/// `READ_BUF` bytes from the first regular file without printing its contents.
/// Mount, directory and read errors produce bounded UART diagnostics; this
/// diagnostic uses `Mode::ReadOnly` and never provisions a volume.
fn list_fat(spi: &mut Spi<'static, Blocking>, cs: &mut Output<'static>) {
    let bus = CsSpi { spi, cs };
    let card = SdCard::new(bus, Delay);
    let volumes = VolumeManager::new(card, NoClock);
    let Ok(volume) = volumes.open_volume(VolumeIdx(0)) else {
        print_line(|buf| format_sd_none("fat", buf));
        return;
    };
    print_line(|buf| format_sd_vol(0, buf));
    let Ok(root) = volume.open_root_dir() else {
        print_line(|buf| format_sd_none("fat", buf));
        return;
    };

    let mut shown = 0u8;
    let mut first_file: Option<ShortFileName> = None;
    let walk = root.iterate_dir(|ent| {
        if ent.attributes.is_volume() || is_dot(&ent.name) {
            return ControlFlow::Continue(());
        }
        let mut raw = [0u8; 12];
        let raw = sfn_bytes(&ent.name, &mut raw);
        let mut label = [0u8; RADIO_LABEL_MAX];
        let name = sanitize_radio_label(raw, &mut label);
        let bytes = if ent.attributes.is_directory() {
            None
        } else {
            if first_file.is_none() {
                first_file = Some(ent.name);
            }
            Some(ent.size)
        };
        print_line(|buf| format_sd_ent(name, bytes, buf));
        shown = shown.saturating_add(1);
        if shown >= MAX_ENTS {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    if walk.is_err() {
        print_line(|buf| format_sd_none("fat", buf));
        return;
    }
    print_line(|buf| format_sd_dir(shown, buf));

    let Some(file_name) = first_file else {
        return;
    };
    let Ok(file) = root.open_file_in_dir(file_name, Mode::ReadOnly) else {
        print_line(|buf| format_sd_none("read", buf));
        return;
    };
    let mut scratch = [0u8; READ_BUF];
    match file.read(&mut scratch) {
        Ok(n) => {
            let mut raw = [0u8; 12];
            let raw = sfn_bytes(&file_name, &mut raw);
            let mut label = [0u8; RADIO_LABEL_MAX];
            let name = sanitize_radio_label(raw, &mut label);
            print_line(|buf| format_sd_read(name, n as u32, buf));
        }
        Err(_) => print_line(|buf| format_sd_none("read", buf)),
    }
}

/// Recognize FAT's current/parent directory entries from their short base name.
/// This pure filter prevents those navigation entries from filling the report.
fn is_dot(name: &ShortFileName) -> bool {
    let base = name.base_name();
    base == b"." || base == b".."
}

/// Copy an 8.3 name into caller-owned storage, inserting the extension dot.
/// Return only initialized bytes, bounded by the twelve-byte destination.
/// No allocation, filesystem access or Unicode conversion occurs here.
fn sfn_bytes<'a>(name: &ShortFileName, out: &'a mut [u8; 12]) -> &'a [u8] {
    let base = name.base_name();
    let ext = name.extension();
    let mut n = 0;
    for &byte in base {
        if n < 12 {
            out[n] = byte;
            n += 1;
        }
    }
    if !ext.is_empty() && n < 12 {
        out[n] = b'.';
        n += 1;
        for &byte in ext {
            if n < 12 {
                out[n] = byte;
                n += 1;
            }
        }
    }
    &out[..n]
}

/// Configure shared SPI2 for mode zero at the caller's selected frequency.
/// Both devices must be deselected before changing clock configuration. An
/// unsupported HAL configuration maps to `IdentifyError::Bus`; this function
/// does not assert CS or send an SD command.
fn set_hz(spi: &mut Spi<'static, Blocking>, hz: u32) -> Result<(), IdentifyError> {
    spi.apply_config(
        &SpiConfig::default()
            .with_frequency(Rate::from_hz(hz))
            .with_mode(SpiMode::_0),
    )
    .map_err(|_| IdentifyError::Bus)
}

/// Format one UART diagnostic in a fixed stack buffer and print it if it fits.
/// Formatting errors skip the row; callers retain responsibility for selecting
/// public labels and excluding file contents or card identity fields.
fn print_line(format: impl FnOnce(&mut [u8]) -> Result<&str, embassy_debug::FormatError>) {
    let mut buf = [0u8; LINE_CAPACITY];
    if let Ok(line) = format(&mut buf) {
        println!("{line}");
    }
}

/// Clock is unused; we never write directory timestamps.
struct NoClock;

impl TimeSource for NoClock {
    /// Return a placeholder timestamp for the read-only filesystem adapter.
    /// No RTC transaction occurs and no directory timestamp is written.
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 0,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

/// `SpiDevice` over the shared bus with the card CS. EPD CS stays high.
struct CsSpi<'a> {
    spi: &'a mut Spi<'static, Blocking>,
    cs: &'a mut Output<'static>,
}

/// Coarse SPI transport failure used by the read-only SD diagnostic.
/// The HAL detail is intentionally collapsed; CS is still released on failure.
#[derive(Debug)]
struct SdSpiError;

impl embedded_hal::spi::Error for SdSpiError {
    /// Classify every collapsed HAL transfer failure as the portable Other kind.
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

impl embedded_hal::spi::ErrorType for CsSpi<'_> {
    /// Transport error shared by all SD operations on this borrowed device.
    type Error = SdSpiError;
}

impl SpiDevice for CsSpi<'_> {
    /// Assert GPIO8 for this operation group, then deselect on success or error.
    /// Read clocks transmit idle-high bytes; writes/transfers borrow the shared
    /// SPI2 controller synchronously. Delay operations retain CS. The caller
    /// exclusively owns the bus and keeps panel GPIO15 high throughout.
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        self.cs.set_low();
        // Keep fallible transfers inside the closure. Its ? exits return here,
        // so both success and bus failure reach the final CS-high write.
        let result = (|| {
            for op in operations.iter_mut() {
                match op {
                    Operation::Read(buf) => {
                        // SD SPI reads need outgoing idle-one bits to generate
                        // the clock; the same borrowed bytes receive the reply.
                        buf.fill(0xFF);
                        self.spi.transfer(buf).map_err(|_| SdSpiError)?;
                    }
                    Operation::Write(buf) => self.spi.write(buf).map_err(|_| SdSpiError)?,
                    Operation::Transfer(read, write) => {
                        let n = read.len().min(write.len());
                        read[..n].copy_from_slice(&write[..n]);
                        read[n..].fill(0xFF);
                        self.spi.transfer(read).map_err(|_| SdSpiError)?;
                    }
                    Operation::TransferInPlace(buf) => {
                        self.spi.transfer(buf).map_err(|_| SdSpiError)?;
                    }
                    Operation::DelayNs(ns) => Delay.delay_ns(*ns),
                }
            }
            Ok(())
        })();
        self.cs.set_high();
        result
    }
}
