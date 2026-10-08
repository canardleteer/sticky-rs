//! Bounded package validation before an update may be marked ready.
use crate::Error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Maximum length-prefixed JSON metadata accepted from a remote sender.
pub const MANIFEST_MAX: usize = 1024;
/// Factory app0 size; staging does not write flash or OTA metadata.
pub const APP_MAX: u32 = 0x600000;
/// Metadata version understood by this implementation.
pub const FORMAT_VERSION: u32 = 1;
/// Owned strings are unnecessary; serde borrows metadata from the bounded buffer.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest<'a> {
    /// Package metadata format version.
    pub format: u32,
    /// Board target, currently `seeed-reterminal-sticky`.
    pub board: &'a str,
    /// MCU target, currently `esp32s3`.
    pub chip: &'a str,
    /// Payload kind, currently `app0`.
    pub kind: &'a str,
    /// Human firmware version; activation and downgrade policy are deferred.
    pub version: &'a str,
    /// Exact binary length, excluding the manifest and prefix.
    pub length: u32,
    /// SHA-256 bytes serialized as a JSON array.
    pub sha256: [u8; 32],
}
impl<'a> Manifest<'a> {
    /// Parse bounded JSON and validate target, version, and length constraints.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.is_empty() || bytes.len() > MANIFEST_MAX {
            return Err(Error::Package);
        }
        let (manifest, used): (Self, usize) =
            serde_json_core::from_slice(bytes).map_err(|_| Error::Package)?;
        if bytes[used..].iter().any(|byte| !byte.is_ascii_whitespace())
            || manifest.format != FORMAT_VERSION
            || manifest.board != "seeed-reterminal-sticky"
            || manifest.chip != "esp32s3"
            || manifest.kind != "app0"
            || manifest.version.is_empty()
            || manifest.version.len() > 64
            || manifest.length < 24
            || manifest.length > APP_MAX
        {
            return Err(Error::Package);
        }
        Ok(manifest)
    }
}
/// Streaming length and digest verification. This provides corruption detection;
/// verify the package publisher before any future flash activation.
pub struct Verifier {
    expected_length: u32,
    expected_digest: [u8; 32],
    received: u32,
    hash: Sha256,
    valid_magic: bool,
    header: [u8; 24],
}
impl Verifier {
    /// Begin validation of the application bytes after parsing metadata.
    pub fn new(manifest: &Manifest<'_>) -> Self {
        Self {
            expected_length: manifest.length,
            expected_digest: manifest.sha256,
            received: 0,
            hash: Sha256::new(),
            valid_magic: false,
            header: [0; 24],
        }
    }
    /// Consume the next ordered payload chunk. Refuse excess bytes before hashing.
    pub fn update(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let total = self
            .received
            .checked_add(u32::try_from(bytes.len()).map_err(|_| Error::Package)?)
            .ok_or(Error::Package)?;
        if total > self.expected_length {
            return Err(Error::Package);
        }
        if self.received == 0 && !bytes.is_empty() {
            self.valid_magic = bytes[0] == 0xe9;
        }
        if self.received < 24 {
            let start = self.received as usize;
            let take = bytes.len().min(24 - start);
            self.header[start..start + take].copy_from_slice(&bytes[..take]);
        }
        self.hash.update(bytes);
        self.received = total;
        Ok(())
    }
    /// Verify exact length and SHA-256 before publishing readiness.
    pub fn finish(self) -> Result<(), Error> {
        let digest: [u8; 32] = self.hash.finalize().into();
        if !self.valid_magic
            || !(1..=16).contains(&self.header[1])
            || u16::from_le_bytes([self.header[12], self.header[13]]) != 9
            || self.header[23] > 1
            || self.received != self.expected_length
            || digest != self.expected_digest
        {
            return Err(Error::Package);
        }
        Ok(())
    }
}

/// Validate the next chunk against this boot's acknowledged pending length.
/// The caller advances to the returned offset only after successful close;
/// on failure it abandons the transfer. An absent length never resumes stale
/// pending data. This performs no I/O and borrows no file or device.
pub fn next_offset(acknowledged: Option<u64>, offset: u64, bytes: usize) -> Result<u64, Error> {
    let end = offset.checked_add(bytes as u64).ok_or(Error::Package)?;
    if acknowledged != Some(offset)
        || !(1..=512).contains(&bytes)
        || end > 4 + MANIFEST_MAX as u64 + u64::from(APP_MAX)
    {
        return Err(Error::Package);
    }
    Ok(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_requires_begin_and_exact_acknowledged_order() {
        assert!(next_offset(None, 0, 512).is_err());
        let acknowledged = next_offset(Some(0), 0, 512).unwrap();
        assert_eq!(acknowledged, 512);
        assert!(next_offset(Some(acknowledged), 0, 512).is_err()); // duplicate
        assert!(next_offset(Some(acknowledged), 1024, 512).is_err()); // missing
        assert!(next_offset(Some(acknowledged), 512, 0).is_err());
        assert!(next_offset(Some(acknowledged), 512, 513).is_err());
        assert!(next_offset(Some(u64::MAX), u64::MAX, 1).is_err());
        assert!(next_offset(
            Some(4 + MANIFEST_MAX as u64 + u64::from(APP_MAX)),
            4 + MANIFEST_MAX as u64 + u64::from(APP_MAX),
            1
        )
        .is_err());
        // Reset/interruption clears the caller's acknowledged length.
        assert!(next_offset(None, acknowledged, 1).is_err());
    }
}
