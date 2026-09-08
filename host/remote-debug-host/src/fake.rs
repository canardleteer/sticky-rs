//! In-memory transport for host tests (no adapter).

use std::collections::VecDeque;

use remote_debug_wire::v1::envelope::Body;
use remote_debug_wire::{
    decode_envelope, encode_snapshot_busy, encode_snapshot_envelope, FrameAssembler, GetOutcome,
    SnapshotSlot,
};

use crate::{Error, Transport};

/// Scripted peripheral: one snapshot slot + tiny planes.
pub struct FakeTransport {
    inbound: VecDeque<Vec<u8>>,
    slot: SnapshotSlot,
    bw: Vec<u8>,
    width: u16,
    height: u16,
    connected: bool,
    /// How many times [`Transport::disconnect`] ran.
    pub disconnects: u32,
    /// Last `keep_bond` passed to disconnect.
    pub last_keep_bond: Option<bool>,
}

impl FakeTransport {
    /// 8×4 mono fixture.
    #[must_use]
    pub fn tiny() -> Self {
        Self {
            inbound: VecDeque::new(),
            slot: SnapshotSlot::new(),
            bw: vec![0xA0, 0x50, 0x0F, 0xF0],
            width: 8,
            height: 4,
            connected: true,
            disconnects: 0,
            last_keep_bond: None,
        }
    }
}

impl Transport for FakeTransport {
    fn write_frame(&mut self, framed: &[u8]) -> Result<(), Error> {
        if !self.connected {
            return Err(Error::NotConnected);
        }
        let env = decode_envelope(framed)?;
        match env.body {
            Some(Body::GetSnapshot(get)) => {
                let outcome = self.slot.on_get(get.nonce, true);
                match outcome {
                    GetOutcome::Armed | GetOutcome::Retry => {
                        self.inbound.push_back(encode_snapshot_envelope(
                            get.nonce,
                            self.width,
                            self.height,
                            remote_debug_wire::v1::FrameKind::FRAME_KIND_MONO,
                            Some(2),
                            &self.bw,
                            None,
                        ));
                    }
                    GetOutcome::Busy { armed } => {
                        self.inbound.push_back(encode_snapshot_busy(armed));
                    }
                    GetOutcome::Empty | GetOutcome::Zero => {
                        self.inbound.push_back(encode_snapshot_busy(0));
                    }
                }
            }
            Some(Body::SnapshotAck(ack)) => {
                let _ = self.slot.on_ack(ack.nonce);
            }
            Some(Body::SnapshotClear(_)) => {
                let _ = self.slot.on_clear();
            }
            Some(Body::InjectTouch(_) | Body::InjectButton(_)) | None => {}
            Some(_) => {}
        }
        Ok(())
    }

    fn read_chunk(&mut self) -> Result<Vec<u8>, Error> {
        self.inbound
            .pop_front()
            .ok_or(Error::Io("no notify".into()))
    }

    fn disconnect(&mut self, keep_bond: bool) -> Result<(), Error> {
        self.connected = false;
        self.disconnects += 1;
        self.last_keep_bond = Some(keep_bond);
        Ok(())
    }
}

/// Split a framed envelope into ATT-sized pieces (reassembly tests).
#[must_use]
pub fn shatter(framed: &[u8], piece: usize) -> Vec<Vec<u8>> {
    framed.chunks(piece.max(1)).map(<[u8]>::to_vec).collect()
}

/// Reassemble `shatter` output with [`FrameAssembler`].
///
/// # Errors
///
/// Frame cap or decode failure.
pub fn reassemble_pieces(pieces: &[Vec<u8>]) -> Result<Vec<u8>, Error> {
    let mut asm = FrameAssembler::host_rx();
    for piece in pieces {
        if let Some(frame) = asm.push(piece)? {
            return Ok(frame);
        }
    }
    Err(Error::Io("incomplete shatter".into()))
}
