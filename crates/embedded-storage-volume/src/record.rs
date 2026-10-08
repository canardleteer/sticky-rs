//! Deterministic test records whose digest covers the persisted sequence.
use crate::Error;
use sha2::{Digest, Sha256};
/// One sector of state with a sequence prefix and SHA-256 trailer.
pub const RECORD_BYTES: usize = 512;
/// Encode one sequence, filling its payload deterministically for readback.
pub fn encode(sequence: u32) -> [u8; RECORD_BYTES] {
    let mut record = [0; RECORD_BYTES];
    record[..4].copy_from_slice(&sequence.to_le_bytes());
    for (index, byte) in record[4..480].iter_mut().enumerate() {
        *byte = (index as u8).wrapping_add(sequence as u8);
    }
    let digest: [u8; 32] = Sha256::digest(&record[..480]).into();
    record[480..].copy_from_slice(&digest);
    record
}
/// Validate exact length, digest, and payload before returning the sequence.
pub fn decode(bytes: &[u8]) -> Result<u32, Error> {
    if bytes.len() != RECORD_BYTES {
        return Err(Error::Filesystem);
    }
    let sequence = u32::from_le_bytes(bytes[..4].try_into().map_err(|_| Error::Filesystem)?);
    if bytes != encode(sequence) {
        return Err(Error::Filesystem);
    }
    Ok(sequence)
}
