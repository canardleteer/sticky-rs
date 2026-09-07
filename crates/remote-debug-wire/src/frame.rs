//! Length-delimited [`crate::v1::Envelope`] on a byte stream.

use alloc::vec::Vec;

use buffa::Message;

use crate::v1::{
    envelope::Body, Envelope, FrameKind, Snapshot, SnapshotAck, SnapshotBusy, SnapshotClear,
};

/// `Envelope.version` this crate writes and accepts.
pub const ENVELOPE_VERSION: u32 = 1;

/// Framing or version error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// Fewer than 4 bytes, or length prefix longer than the rest.
    Truncated,
    /// `version` is not [`ENVELOPE_VERSION`].
    Version,
    /// buffa could not decode an Envelope.
    Decode,
}

/// Prepend a little-endian u32 length to `payload`.
#[must_use]
pub fn framed(payload: &[u8]) -> Vec<u8> {
    let n = payload.len() as u32;
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Strip the u32 LE length. Returns the payload slice.
///
/// # Errors
///
/// [`FrameError::Truncated`] when the prefix is missing or the length
/// does not match the remaining bytes.
pub fn unframe(bytes: &[u8]) -> Result<&[u8], FrameError> {
    if bytes.len() < 4 {
        return Err(FrameError::Truncated);
    }
    let n = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let rest = &bytes[4..];
    if rest.len() != n {
        return Err(FrameError::Truncated);
    }
    Ok(rest)
}

/// Encode `envelope` and frame it.
#[must_use]
pub fn encode_envelope(envelope: &Envelope) -> Vec<u8> {
    framed(&envelope.encode_to_vec())
}

/// Unframe and decode one [`Envelope`].
///
/// # Errors
///
/// Truncated frame, unknown version, or a decode failure.
pub fn decode_envelope(bytes: &[u8]) -> Result<Envelope, FrameError> {
    let payload = unframe(bytes)?;
    let env = Envelope::decode_from_slice(payload).map_err(|_| FrameError::Decode)?;
    if env.version != ENVELOPE_VERSION {
        return Err(FrameError::Version);
    }
    Ok(env)
}

/// Build a framed [`Snapshot`] from borrowed planes (host / small tests).
///
/// Device transports with a 48 KiB plane should encode from the frozen
/// slot without calling this (it copies `bw` / `red` into the generated
/// `Vec`).
#[must_use]
pub fn encode_snapshot_envelope(
    nonce: u64,
    width: u16,
    height: u16,
    kind: FrameKind,
    hold: Option<u32>,
    bw: &[u8],
    red: Option<&[u8]>,
) -> Vec<u8> {
    let snap = Snapshot {
        nonce,
        width: u32::from(width),
        height: u32::from(height),
        kind: kind.into(),
        hold,
        bw: bw.to_vec(),
        red: red.map(<[u8]>::to_vec).unwrap_or_default(),
        ..Snapshot::default()
    };
    let env = Envelope {
        version: ENVELOPE_VERSION,
        body: Some(Body::from(snap)),
        ..Envelope::default()
    };
    encode_envelope(&env)
}

/// Wrap a body in a versioned envelope and frame it.
#[must_use]
pub fn encode_body(body: impl Into<Body>) -> Vec<u8> {
    let env = Envelope {
        version: ENVELOPE_VERSION,
        body: Some(body.into()),
        ..Envelope::default()
    };
    encode_envelope(&env)
}

/// Convenience constructors for the empty / ack / busy control messages.
#[must_use]
pub fn encode_snapshot_ack(nonce: u64) -> Vec<u8> {
    encode_body(SnapshotAck {
        nonce,
        ..SnapshotAck::default()
    })
}

/// Operator clear (no nonce).
#[must_use]
pub fn encode_snapshot_clear() -> Vec<u8> {
    encode_body(SnapshotClear::default())
}

/// Busy reply echoing the armed nonce (0 if none).
#[must_use]
pub fn encode_snapshot_busy(armed: u64) -> Vec<u8> {
    encode_body(SnapshotBusy {
        nonce: armed,
        ..SnapshotBusy::default()
    })
}
