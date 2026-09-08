//! Documented GATT UUIDs for an encrypted remote-debug link.
//!
//! These are **local 128-bit** assignments, not Bluetooth SIG 16-bit
//! numbers and not the embassy-debug pair-card token service
//! (`6b1d0001-5c8a-4f0e-9c3a-2e7b1a0d4f11`). A second codebase can
//! implement the same peripheral after its own pairing policy.
//!
//! ATT writes and notifies are not self-delimiting. Both sides
//! concatenate fragments and reassemble **one** u32-LE framed
//! [`crate::v1::Envelope`] ([`crate::FrameAssembler`]). Chunk size is
//! transport, not a proto `SnapshotChunk`.

/// Remote-debug GATT service.
///
/// Host discovers this after DisplayOnly SMP. Characteristics require
/// an encrypted link (`permissions(encrypted)` on the peripheral).
pub const GATT_SERVICE_UUID: &str = "c81e1000-5c8a-4f0e-9c3a-2e7b1a0d4f11";

/// Host → device: write of framed [`crate::v1::Envelope`] bytes.
///
/// Encrypted write. One ATT write may be a fragment; append until a
/// full frame is present, then [`crate::decode_envelope`].
pub const GATT_RX_UUID: &str = "c81e1001-5c8a-4f0e-9c3a-2e7b1a0d4f11";

/// Device → host: notify of framed [`crate::v1::Envelope`] bytes.
///
/// Encrypted notify. A 48 KiB snapshot is many ATT payloads. Reassemble
/// with [`crate::FrameAssembler`] before decode.
pub const GATT_TX_UUID: &str = "c81e1002-5c8a-4f0e-9c3a-2e7b1a0d4f11";
