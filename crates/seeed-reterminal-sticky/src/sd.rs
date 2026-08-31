//! MicroSD on the shared SPI bus: clocks, detect polarity, read-only identify.
//!
//! The slot is on the left long edge. Pins live in [`crate::pins`]. The rail
//! is [`crate::rails::SdRail`]. This module never builds a write, erase, or
//! program command. A FAT mount is firmware plus `embedded-sdmmc`, not here.
//!
//! Identify is SPI mode: init at [`INIT_HZ`], then the caller may raise the
//! clock and use [`send_status`]. Product serial in CID is parsed only to
//! skip it — it is not stored on [`Identity`].

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiBus;

use crate::display;

/// SPI clock for card init (SD spec: ≤ 400 kHz).
pub const INIT_HZ: u32 = 400_000;

/// Dummy `0xFF` bytes with CS high before `CMD0` (80 clocks).
pub const INIT_DUMMY_BYTES: usize = 10;

/// CID register length.
pub const CID_LEN: usize = 16;

/// Product name bytes in CID (not the product serial).
pub const NAME_LEN: usize = 5;

/// How many `0xFF` clocks to wait for an R1.
const R1_ATTEMPTS: u8 = 16;

/// How many `0xFF` clocks to wait for a data token.
const TOKEN_ATTEMPTS: u8 = 64;

/// `ACMD41` retries (1 ms apart).
const OP_COND_ATTEMPTS: u16 = 2000;

/// OCR CCS bit: 1 = SDHC / SDXC.
const OCR_CCS: u32 = 1 << 30;

/// HCS bit in `ACMD41` (host supports high capacity).
const ACMD41_HCS: u32 = 1 << 30;

/// `CMD8` VHS 2.7–3.6 V plus check pattern.
const IF_COND_ARG: u32 = 0x0000_01AA;

/// Read-only SPI commands this module will emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    /// `CMD0` `GO_IDLE_STATE`.
    GoIdleState = 0,
    /// `CMD8` `SEND_IF_COND`.
    SendIfCond = 8,
    /// `CMD9` `SEND_CSD` (reserved for a later reader; unused by identify).
    SendCsd = 9,
    /// `CMD10` `SEND_CID`.
    SendCid = 10,
    /// `CMD13` `SEND_STATUS`.
    SendStatus = 13,
    /// `ACMD41` `SD_SEND_OP_COND` (after [`Command::AppCmd`]).
    SdSendOpCond = 41,
    /// `CMD55` `APP_CMD`.
    AppCmd = 55,
    /// `CMD58` `READ_OCR`.
    ReadOcr = 58,
}

/// Card family from OCR CCS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    /// SDSC (CCS clear).
    Sdsc,
    /// SDHC or SDXC (CCS set).
    Sdhc,
}

impl CardKind {
    /// UART token (`sdsc` / `sdhc`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sdsc => "sdsc",
            Self::Sdhc => "sdhc",
        }
    }

    /// CCS set means SDHC / SDXC.
    #[must_use]
    pub const fn from_ocr(ocr: u32) -> Self {
        if ocr & OCR_CCS != 0 {
            Self::Sdhc
        } else {
            Self::Sdsc
        }
    }
}

/// Read-only card identity. No product serial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// SDSC vs SDHC from OCR.
    pub kind: CardKind,
    /// CID manufacturer id.
    pub mid: u8,
    /// CID product name (five bytes, not sanitized).
    pub name: [u8; NAME_LEN],
}

/// Why identify or [`send_status`] failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifyError {
    /// No R1 or data token in time.
    Timeout,
    /// R1 was present but not the idle/ready value we needed.
    Unexpected(u8),
    /// SPI or CS pin error.
    Bus,
}

/// Insert pulls [`crate::pins::SD_CARD_DETECT`] low.
#[must_use]
pub const fn card_inserted(detect_is_high: bool) -> bool {
    !detect_is_high
}

/// CRC-7 over the first five command bytes. The wire byte is this `| 1`.
#[must_use]
pub fn crc7(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            if crc & 0x80 != 0 {
                crc = (crc << 1) ^ 0x12;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Six-byte SPI command frame for a read-only [`Command`].
#[must_use]
pub fn command(cmd: Command, arg: u32) -> [u8; 6] {
    let mut frame = [
        0x40 | (cmd as u8),
        (arg >> 24) as u8,
        (arg >> 16) as u8,
        (arg >> 8) as u8,
        arg as u8,
        0,
    ];
    frame[5] = crc7(&frame[..5]) | 1;
    frame
}

/// Manufacturer id and product name. CID bytes 9–12 (PSN) are ignored.
#[must_use]
pub fn identity_from_cid(kind: CardKind, cid: &[u8; CID_LEN]) -> Identity {
    let mut name = [0u8; NAME_LEN];
    name.copy_from_slice(&cid[3..8]);
    Identity {
        kind,
        mid: cid[0],
        name,
    }
}

/// `CMD0` / `CMD8` / `ACMD41` / `CMD58` / `CMD10`. No writes.
pub fn identify<SPI, CS, D>(
    spi: &mut SPI,
    cs: &mut CS,
    delay: &mut D,
) -> Result<Identity, IdentifyError>
where
    SPI: SpiBus,
    CS: OutputPin,
    D: DelayNs,
{
    idle_clocks(spi, cs)?;
    let r1 = r1_cmd(spi, cs, Command::GoIdleState, 0)?;
    if r1 != 0x01 {
        return Err(IdentifyError::Unexpected(r1));
    }

    let (if_r1, _) = r3_cmd(spi, cs, Command::SendIfCond, IF_COND_ARG)?;
    let hcs = if if_r1 == 0x05 { 0 } else { ACMD41_HCS };

    let mut ready = false;
    for _ in 0..OP_COND_ATTEMPTS {
        let app = r1_cmd(spi, cs, Command::AppCmd, 0)?;
        if app & 0x7e != 0 {
            return Err(IdentifyError::Unexpected(app));
        }
        let op = r1_cmd(spi, cs, Command::SdSendOpCond, hcs)?;
        if op == 0x00 {
            ready = true;
            break;
        }
        if op != 0x01 {
            return Err(IdentifyError::Unexpected(op));
        }
        delay.delay_ms(1);
    }
    if !ready {
        return Err(IdentifyError::Timeout);
    }

    let (ocr_r1, ocr) = r3_cmd(spi, cs, Command::ReadOcr, 0)?;
    if ocr_r1 != 0x00 {
        return Err(IdentifyError::Unexpected(ocr_r1));
    }
    let kind = CardKind::from_ocr(ocr);
    let cid = read_cid(spi, cs)?;
    Ok(identity_from_cid(kind, &cid))
}

/// `CMD13` after init. Used when the caller raises the SPI clock.
pub fn send_status<SPI, CS>(spi: &mut SPI, cs: &mut CS) -> Result<(), IdentifyError>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    select(cs)?;
    spi.write(&command(Command::SendStatus, 0))
        .map_err(|_| IdentifyError::Bus)?;
    let r1 = wait_r1(spi)?;
    let _r2 = read_byte(spi)?;
    release(spi, cs)?;
    if r1 != 0x00 {
        return Err(IdentifyError::Unexpected(r1));
    }
    Ok(())
}

fn idle_clocks<SPI, CS>(spi: &mut SPI, cs: &mut CS) -> Result<(), IdentifyError>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    cs.set_high().map_err(|_| IdentifyError::Bus)?;
    spi.write(&[0xFF; INIT_DUMMY_BYTES])
        .map_err(|_| IdentifyError::Bus)
}

fn r1_cmd<SPI, CS>(spi: &mut SPI, cs: &mut CS, cmd: Command, arg: u32) -> Result<u8, IdentifyError>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    select(cs)?;
    spi.write(&command(cmd, arg))
        .map_err(|_| IdentifyError::Bus)?;
    let r1 = wait_r1(spi)?;
    release(spi, cs)?;
    Ok(r1)
}

fn r3_cmd<SPI, CS>(
    spi: &mut SPI,
    cs: &mut CS,
    cmd: Command,
    arg: u32,
) -> Result<(u8, u32), IdentifyError>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    select(cs)?;
    spi.write(&command(cmd, arg))
        .map_err(|_| IdentifyError::Bus)?;
    let r1 = wait_r1(spi)?;
    let mut payload = [0xFF; 4];
    if r1 & 0x80 == 0 && r1 != 0x05 {
        spi.transfer_in_place(&mut payload)
            .map_err(|_| IdentifyError::Bus)?;
    }
    release(spi, cs)?;
    let ocr = u32::from_be_bytes(payload);
    Ok((r1, ocr))
}

fn read_cid<SPI, CS>(spi: &mut SPI, cs: &mut CS) -> Result<[u8; CID_LEN], IdentifyError>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    select(cs)?;
    spi.write(&command(Command::SendCid, 0))
        .map_err(|_| IdentifyError::Bus)?;
    let r1 = wait_r1(spi)?;
    if r1 != 0x00 {
        release(spi, cs)?;
        return Err(IdentifyError::Unexpected(r1));
    }
    wait_token(spi)?;
    let mut cid = [0xFFu8; CID_LEN];
    spi.transfer_in_place(&mut cid)
        .map_err(|_| IdentifyError::Bus)?;
    let mut crc = [0xFFu8; 2];
    spi.transfer_in_place(&mut crc)
        .map_err(|_| IdentifyError::Bus)?;
    release(spi, cs)?;
    Ok(cid)
}

fn wait_r1<SPI: SpiBus>(spi: &mut SPI) -> Result<u8, IdentifyError> {
    for _ in 0..R1_ATTEMPTS {
        let b = read_byte(spi)?;
        if b & 0x80 == 0 {
            return Ok(b);
        }
    }
    Err(IdentifyError::Timeout)
}

fn wait_token<SPI: SpiBus>(spi: &mut SPI) -> Result<(), IdentifyError> {
    for _ in 0..TOKEN_ATTEMPTS {
        if read_byte(spi)? == 0xFE {
            return Ok(());
        }
    }
    Err(IdentifyError::Timeout)
}

fn read_byte<SPI: SpiBus>(spi: &mut SPI) -> Result<u8, IdentifyError> {
    let mut byte = [0xFFu8];
    spi.transfer_in_place(&mut byte)
        .map_err(|_| IdentifyError::Bus)?;
    Ok(byte[0])
}

fn select<CS: OutputPin>(cs: &mut CS) -> Result<(), IdentifyError> {
    cs.set_low().map_err(|_| IdentifyError::Bus)
}

fn release<SPI, CS>(spi: &mut SPI, cs: &mut CS) -> Result<(), IdentifyError>
where
    SPI: SpiBus,
    CS: OutputPin,
{
    cs.set_high().map_err(|_| IdentifyError::Bus)?;
    spi.write(&[0xFF]).map_err(|_| IdentifyError::Bus)
}

const _: () = assert!(INIT_HZ <= display::SPI_MAX_HZ);
const _: () = assert!(Command::GoIdleState as u8 != 24);
const _: () = assert!(Command::SendCid as u8 != 24);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd0_and_cmd8_crc_match_the_sd_spec() {
        assert_eq!(command(Command::GoIdleState, 0), [0x40, 0, 0, 0, 0, 0x95]);
        assert_eq!(
            command(Command::SendIfCond, IF_COND_ARG),
            [0x48, 0, 0, 0x01, 0xAA, 0x87]
        );
    }

    #[test]
    fn detect_low_is_inserted() {
        assert!(card_inserted(false));
        assert!(!card_inserted(true));
    }

    #[test]
    fn ocr_ccs_selects_sdhc() {
        assert_eq!(CardKind::from_ocr(0), CardKind::Sdsc);
        assert_eq!(CardKind::from_ocr(OCR_CCS), CardKind::Sdhc);
        assert_eq!(CardKind::Sdhc.as_str(), "sdhc");
    }

    #[test]
    fn identity_skips_the_product_serial() {
        let mut cid = [0u8; CID_LEN];
        cid[0] = 0x03;
        cid[3..8].copy_from_slice(b"SC16G");
        cid[9..13].copy_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
        let id = identity_from_cid(CardKind::Sdhc, &cid);
        assert_eq!(id.mid, 0x03);
        assert_eq!(&id.name, b"SC16G");
        assert_ne!(&id.name, b"DEADB");
    }

    #[test]
    fn public_commands_are_not_block_writes() {
        for cmd in [
            Command::GoIdleState,
            Command::SendIfCond,
            Command::SendCsd,
            Command::SendCid,
            Command::SendStatus,
            Command::SdSendOpCond,
            Command::AppCmd,
            Command::ReadOcr,
        ] {
            let n = cmd as u8;
            assert_ne!(n, 24, "CMD24 WRITE_BLOCK");
            assert_ne!(n, 25, "CMD25 WRITE_MULTIPLE");
            assert_ne!(n, 27, "CMD27 PROGRAM_CSD");
            assert!(!(32..=38).contains(&n), "erase family {n}");
        }
    }

    #[test]
    fn init_clock_stays_inside_the_panel_default() {
        assert_eq!(INIT_HZ, 400_000);
        assert!(INIT_HZ <= display::SPI_MAX_HZ);
    }
}
