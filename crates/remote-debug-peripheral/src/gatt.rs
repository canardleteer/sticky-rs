//! Trouble GATT service and ATT notify helpers.
//!
//! The image builds the radio `Stack` and `#[gatt_server]`. This
//! module supplies the remote-debug characteristics and streaming
//! notify so a second firmware does not copy embassy-debug.

use remote_debug_wire::{
    bytes_field_header_to_slice, encode_snapshot_busy, snapshot_preamble_to_slice, SnapshotMeta,
};
use trouble_host::prelude::*;

/// Encrypted remote-debug GATT (local UUIDs; not SIG).
///
/// `rx` is host→device framed envelopes. `tx` notifies device→host
/// frames. The stored value is a dummy byte; writes and
/// `notify_raw(..., store=false)` carry the ATT payload.
#[gatt_service(uuid = "c81e1000-5c8a-4f0e-9c3a-2e7b1a0d4f11")]
pub struct RemoteDebugService {
    /// Host → device write (ATT chunks).
    #[characteristic(
        uuid = "c81e1001-5c8a-4f0e-9c3a-2e7b1a0d4f11",
        write,
        write_without_response,
        permissions(encrypted)
    )]
    pub rx: u8,
    /// Device → host notify (ATT chunks).
    #[characteristic(
        uuid = "c81e1002-5c8a-4f0e-9c3a-2e7b1a0d4f11",
        notify,
        permissions(encrypted)
    )]
    pub tx: u8,
}

/// Pair-card token so Settings has a GATT target (optional extra service).
///
/// Local UUIDs, not SIG. Encrypted read. A foreign image may omit this
/// and only include [`RemoteDebugService`].
#[gatt_service(uuid = "6b1d0001-5c8a-4f0e-9c3a-2e7b1a0d4f11")]
pub struct PairTokenService {
    /// Dummy encrypted-read byte.
    #[characteristic(
        uuid = "6b1d0002-5c8a-4f0e-9c3a-2e7b1a0d4f11",
        read,
        value = 1,
        permissions(encrypted)
    )]
    pub token: u8,
}

/// ATT notify payload size for this link (opcode + handle eat 3 bytes).
#[must_use]
pub fn notify_payload_max<P: PacketPool>(gatt: &GattConnection<'_, '_, P>) -> usize {
    (gatt.raw().att_mtu() as usize)
        .saturating_sub(3)
        .clamp(20, 244)
}

/// Notify `bytes` in ATT-sized slices. `store` is false.
pub async fn notify_bytes<P: PacketPool>(
    gatt: &GattConnection<'_, '_, P>,
    tx: &Characteristic<u8>,
    bytes: &[u8],
) {
    let max = notify_payload_max(gatt);
    for part in bytes.chunks(max) {
        let _ = tx.notify_raw(gatt, part, false).await;
    }
}

/// Stream LAST as a framed `Snapshot` without cloning 48 KiB.
///
/// `meta` returns `(SnapshotMeta, bw_len, red_len)` or `None` (busy 0).
/// `copy_plane` copies `len` bytes of `bw` (`true`) or `red` starting
/// at `off` into `dst`. Return 0 to abort.
pub async fn notify_armed_snapshot<P, Meta, CopyPlane>(
    gatt: &GattConnection<'_, '_, P>,
    tx: &Characteristic<u8>,
    meta: Meta,
    mut copy_plane: CopyPlane,
) where
    P: PacketPool,
    Meta: FnOnce() -> Option<(SnapshotMeta, usize, usize)>,
    CopyPlane: FnMut(bool, usize, usize, &mut [u8]) -> usize,
{
    let Some((snap_meta, bw_len, red_len)) = meta() else {
        notify_bytes(gatt, tx, &encode_snapshot_busy(0)).await;
        return;
    };
    let mut preamble = [0u8; 96];
    if let Ok(n) = snapshot_preamble_to_slice(snap_meta, bw_len, red_len, &mut preamble) {
        notify_bytes(gatt, tx, &preamble[..n]).await;
    }
    notify_plane_chunks(gatt, tx, true, bw_len, &mut copy_plane).await;
    if red_len != 0 {
        let mut hdr = [0u8; 8];
        if let Ok(n) = bytes_field_header_to_slice(7, red_len, &mut hdr) {
            notify_bytes(gatt, tx, &hdr[..n]).await;
        }
        notify_plane_chunks(gatt, tx, false, red_len, &mut copy_plane).await;
    }
}

async fn notify_plane_chunks<P, CopyPlane>(
    gatt: &GattConnection<'_, '_, P>,
    tx: &Characteristic<u8>,
    bw: bool,
    len: usize,
    copy_plane: &mut CopyPlane,
) where
    P: PacketPool,
    CopyPlane: FnMut(bool, usize, usize, &mut [u8]) -> usize,
{
    let max = notify_payload_max(gatt);
    let mut off = 0;
    while off < len {
        let n = (len - off).min(max);
        let mut chunk = [0u8; 244];
        let copied = copy_plane(bw, off, n, &mut chunk);
        if copied == 0 {
            return;
        }
        notify_bytes(gatt, tx, &chunk[..copied]).await;
        off += copied;
    }
}
