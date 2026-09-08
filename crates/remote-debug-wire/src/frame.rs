//! Length-delimited [`crate::v1::Envelope`] on a byte stream.

use alloc::vec::Vec;

use buffa::Message;

use buffa::EncodeSink;

use crate::v1::{
    envelope::Body, Envelope, FrameKind, Reboot, RebootAck, SnapshotAck, SnapshotBusy,
    SnapshotClear,
};

/// `Envelope.version` this crate writes and accepts.
pub const ENVELOPE_VERSION: u32 = 1;

/// Snapshot scalars. Planes stay borrowed at the call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotMeta {
    /// Host nonce that armed the slot.
    pub nonce: u64,
    /// Packed width.
    pub width: u16,
    /// Packed height.
    pub height: u16,
    /// Mono or gray4.
    pub kind: FrameKind,
    /// Product hold token.
    pub hold: Option<u32>,
}

/// Protobuf field number for `Envelope.snapshot`.
const ENVELOPE_SNAPSHOT_FIELD: u32 = 13;

/// Framing or version error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// Fewer than 4 bytes, or length prefix longer than the rest.
    Truncated,
    /// `version` is not [`ENVELOPE_VERSION`].
    Version,
    /// buffa could not decode an Envelope.
    Decode,
    /// Length prefix or assembler buffer exceeded the configured cap.
    TooLarge,
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
    let mut out = Vec::new();
    write_framed_snapshot(
        SnapshotMeta {
            nonce,
            width,
            height,
            kind,
            hold,
        },
        bw,
        red,
        &mut out,
    );
    out
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

/// Host → device: software-reset the **embedded MCU** (not the host).
#[must_use]
pub fn encode_reboot() -> Vec<u8> {
    encode_body(Reboot::default())
}

/// Device → host: ACK before the MCU reset so ATT can flush.
#[must_use]
pub fn encode_reboot_ack() -> Vec<u8> {
    encode_body(RebootAck::default())
}

/// Encoded size of a `Snapshot` body (no envelope wrapper).
///
/// Used so a device can write the u32 LE length and the length-delimited
/// snapshot header without cloning `bw` / `red`.
#[must_use]
pub fn snapshot_body_len(
    nonce: u64,
    width: u16,
    height: u16,
    kind: FrameKind,
    hold: Option<u32>,
    bw_len: usize,
    red_len: usize,
) -> u32 {
    let mut size = 0u64;
    if nonce != 0 {
        size += 1 + u64::from(buffa::types::FIXED64_ENCODED_LEN as u32);
    }
    if width != 0 {
        size += 1 + buffa::types::uint32_encoded_len(u32::from(width)) as u64;
    }
    if height != 0 {
        size += 1 + buffa::types::uint32_encoded_len(u32::from(height)) as u64;
    }
    let kind_i = buffa::Enumeration::to_i32(&kind);
    if kind_i != 0 {
        size += 1 + buffa::types::int32_encoded_len(kind_i) as u64;
    }
    if let Some(v) = hold {
        size += 1 + buffa::types::uint32_encoded_len(v) as u64;
    }
    if bw_len != 0 {
        size += 1 + buffa::encoding::varint_len(bw_len as u64) as u64 + bw_len as u64;
    }
    if red_len != 0 {
        size += 1 + buffa::encoding::varint_len(red_len as u64) as u64 + red_len as u64;
    }
    buffa::saturate_size(size)
}

/// Encoded size of a versioned envelope whose body is one `Snapshot`.
#[must_use]
pub fn snapshot_envelope_payload_len(inner: u32) -> u32 {
    let mut size = 0u64;
    size += 1 + buffa::types::uint32_encoded_len(ENVELOPE_VERSION) as u64;
    size += 1 + buffa::encoding::varint_len(u64::from(inner)) as u64 + u64::from(inner);
    buffa::saturate_size(size)
}

/// Write a framed snapshot envelope from borrowed planes.
///
/// Writes `u32` LE length, then `Envelope.version` and a length-delimited
/// `Snapshot`. Device transports implement [`EncodeSink`] so they can
/// flush ATT notify chunks without a second 48 KiB `Vec`.
pub fn write_framed_snapshot<S: EncodeSink>(
    meta: SnapshotMeta,
    bw: &[u8],
    red: Option<&[u8]>,
    sink: &mut S,
) {
    let red = red.unwrap_or(&[]);
    let inner = snapshot_body_len(
        meta.nonce,
        meta.width,
        meta.height,
        meta.kind,
        meta.hold,
        bw.len(),
        red.len(),
    );
    let payload = snapshot_envelope_payload_len(inner);
    sink.put_slice(&payload.to_le_bytes());
    buffa::types::put_uint32_field(1, ENVELOPE_VERSION, sink);
    buffa::types::put_len_delimited_header(ENVELOPE_SNAPSHOT_FIELD, u64::from(inner), sink);
    write_snapshot_body(meta, bw, red, sink);
}

/// Write `Snapshot` fields (no envelope) from borrowed slices.
fn write_snapshot_body<S: EncodeSink>(meta: SnapshotMeta, bw: &[u8], red: &[u8], sink: &mut S) {
    write_snapshot_scalars(meta, sink);
    if !bw.is_empty() {
        buffa::types::put_shared_bytes_field(6, &bw, sink);
    }
    if !red.is_empty() {
        buffa::types::put_shared_bytes_field(7, &red, sink);
    }
}

/// Envelope + snapshot tags and scalars, stopping before plane payloads.
///
/// Device notifies this (tens of bytes), then the `bw` / `red` slices in
/// ATT-sized chunks, then [`write_bytes_field_header`] for a trailing
/// red plane. Does not copy the planes.
pub fn write_snapshot_preamble<S: EncodeSink>(
    meta: SnapshotMeta,
    bw_len: usize,
    red_len: usize,
    sink: &mut S,
) {
    let inner = snapshot_body_len(
        meta.nonce,
        meta.width,
        meta.height,
        meta.kind,
        meta.hold,
        bw_len,
        red_len,
    );
    let payload = snapshot_envelope_payload_len(inner);
    sink.put_slice(&payload.to_le_bytes());
    buffa::types::put_uint32_field(1, ENVELOPE_VERSION, sink);
    buffa::types::put_len_delimited_header(ENVELOPE_SNAPSHOT_FIELD, u64::from(inner), sink);
    write_snapshot_scalars(meta, sink);
    if bw_len != 0 {
        write_bytes_field_header(6, bw_len, sink);
    }
}

/// Tag + length varint for a proto `bytes` field (payload follows).
pub fn write_bytes_field_header<S: EncodeSink>(field: u32, len: usize, sink: &mut S) {
    buffa::types::put_len_delimited_header(field, len as u64, sink);
}

/// Write [`write_snapshot_preamble`] into `buf`. Returns bytes used.
///
/// Device stack helper: the preamble is tens of bytes, not a plane.
///
/// # Errors
///
/// [`FrameError::TooLarge`] when `buf` cannot hold the preamble.
pub fn snapshot_preamble_to_slice(
    meta: SnapshotMeta,
    bw_len: usize,
    red_len: usize,
    buf: &mut [u8],
) -> Result<usize, FrameError> {
    let mut sink = SliceSink { buf, pos: 0 };
    write_snapshot_preamble(meta, bw_len, red_len, &mut sink);
    Ok(sink.pos)
}

/// Write [`write_bytes_field_header`] into `buf`. Returns bytes used.
///
/// # Errors
///
/// [`FrameError::TooLarge`] when `buf` cannot hold the header.
pub fn bytes_field_header_to_slice(
    field: u32,
    len: usize,
    buf: &mut [u8],
) -> Result<usize, FrameError> {
    let mut sink = SliceSink { buf, pos: 0 };
    write_bytes_field_header(field, len, &mut sink);
    Ok(sink.pos)
}

/// Sequential write into a caller slice (device stack / host tests).
struct SliceSink<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl SliceSink<'_> {
    fn put(&mut self, src: &[u8]) {
        let end = self.pos.saturating_add(src.len());
        if end > self.buf.len() {
            // Saturate; caller sized the buffer. Truncation is a bug.
            return;
        }
        self.buf[self.pos..end].copy_from_slice(src);
        self.pos = end;
    }
}

impl EncodeSink for SliceSink<'_> {
    fn put_u8(&mut self, value: u8) {
        self.put(&[value]);
    }

    fn put_slice(&mut self, src: &[u8]) {
        self.put(src);
    }

    fn put_u32_le(&mut self, value: u32) {
        self.put(&value.to_le_bytes());
    }

    fn put_u64_le(&mut self, value: u64) {
        self.put(&value.to_le_bytes());
    }

    fn put_shared(&mut self, bytes: buffa::bytes::Bytes) {
        self.put(&bytes);
    }
}

fn write_snapshot_scalars<S: EncodeSink>(meta: SnapshotMeta, sink: &mut S) {
    if meta.nonce != 0 {
        buffa::types::put_fixed64_field(1, meta.nonce, sink);
    }
    if meta.width != 0 {
        buffa::types::put_uint32_field(2, u32::from(meta.width), sink);
    }
    if meta.height != 0 {
        buffa::types::put_uint32_field(3, u32::from(meta.height), sink);
    }
    let kind_i = buffa::Enumeration::to_i32(&meta.kind);
    if kind_i != 0 {
        buffa::types::put_int32_field(4, kind_i, sink);
    }
    if let Some(v) = meta.hold {
        buffa::types::put_uint32_field(5, v, sink);
    }
}
