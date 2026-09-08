//! Framed Envelope session on top of a [`crate::Transport`].

use std::time::Duration;

use panel_view::{FrameKind, TouchSample};
use remote_debug_wire::v1::envelope::Body;
use remote_debug_wire::v1::{
    GetSnapshot, InjectButton, InjectTouch, ProductKey, SnapshotAck, TouchSource as WireTouch,
};
use remote_debug_wire::{
    decode_envelope, encode_body, encode_reboot, encode_snapshot_clear, inject_button_gpio,
    snapshot_expected, FrameAssembler, GATT_RX_UUID, GATT_SERVICE_UUID, GATT_TX_UUID,
};

use crate::{Error, Transport};

/// Default advertise name (embassy-debug pair card).
pub const DEFAULT_ADV_NAME: &str = "sticky-rs";

/// Documented GATT UUIDs (same strings as `remote-debug-wire`).
#[must_use]
pub fn service_uuid() -> &'static str {
    GATT_SERVICE_UUID
}

/// Host → device write characteristic.
#[must_use]
pub fn rx_uuid() -> &'static str {
    GATT_RX_UUID
}

/// Device → host notify characteristic.
#[must_use]
pub fn tx_uuid() -> &'static str {
    GATT_TX_UUID
}

/// Last-compose planes pulled from a `Snapshot` envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPlanes {
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
    /// Black/white plane.
    pub bw: Vec<u8>,
    /// Red/gray plane when gray4.
    pub red: Option<Vec<u8>>,
}

/// Protocol state on an already-open transport.
pub struct Session<T: Transport> {
    transport: T,
    assembler: FrameAssembler,
    last_nonce: Option<u64>,
    keep_bond: bool,
}

impl<T: Transport> Session<T> {
    /// Wrap a connected transport. `keep_bond` is the remember-me flag.
    #[must_use]
    pub fn new(transport: T, keep_bond: bool) -> Self {
        Self {
            transport,
            assembler: FrameAssembler::host_rx(),
            last_nonce: None,
            keep_bond,
        }
    }

    /// Whether disconnect should leave the BlueZ bond.
    #[must_use]
    pub fn keep_bond(&self) -> bool {
        self.keep_bond
    }

    /// Last GetSnapshot nonce, if any.
    #[must_use]
    pub fn last_nonce(&self) -> Option<u64> {
        self.last_nonce
    }

    /// Synthetic framebuffer tap.
    ///
    /// # Errors
    ///
    /// Write failure.
    pub fn inject_touch(&mut self, sample: TouchSample) -> Result<(), Error> {
        let msg = InjectTouch {
            x: u32::from(sample.x),
            y: u32::from(sample.y),
            slot: sample.slot.map(u32::from),
            source: WireTouch::TOUCH_SOURCE_SYNTHETIC.into(),
            ..InjectTouch::default()
        };
        self.transport.write_frame(&encode_body(msg))
    }

    /// Product-key short press (`ok` / `page-up` / `page-down`).
    ///
    /// # Errors
    ///
    /// Unknown key or write failure.
    pub fn inject_button(&mut self, key: ProductKey, down: bool) -> Result<(), Error> {
        let _ = inject_button_gpio(&key.into()).map_err(Error::Map)?;
        let msg = InjectButton {
            key: key.into(),
            down,
            ..InjectButton::default()
        };
        self.transport.write_frame(&encode_body(msg))
    }

    /// Arm a snapshot and wait for `Snapshot` or `SnapshotBusy`.
    ///
    /// # Errors
    ///
    /// Zero nonce, busy slot, timeout, or a bad frame.
    pub fn get_snapshot(&mut self, nonce: u64) -> Result<SnapshotPlanes, Error> {
        if nonce == 0 {
            return Err(Error::Io("snapshot nonce must be non-zero".into()));
        }
        self.last_nonce = Some(nonce);
        let msg = GetSnapshot {
            nonce,
            ..GetSnapshot::default()
        };
        self.transport.write_frame(&encode_body(msg))?;
        self.recv_snapshot(Duration::from_secs(30))
    }

    /// Matching Ack. Mismatched nonce is sent anyway (device logs, no release).
    ///
    /// # Errors
    ///
    /// Write failure.
    pub fn snapshot_ack(&mut self, nonce: u64) -> Result<(), Error> {
        self.transport.write_frame(&encode_body(SnapshotAck {
            nonce,
            ..SnapshotAck::default()
        }))
    }

    /// Nonce-free clear.
    ///
    /// # Errors
    ///
    /// Write failure.
    pub fn snapshot_clear(&mut self) -> Result<(), Error> {
        self.transport.write_frame(&encode_snapshot_clear())
    }

    /// Software-reset the **embedded MCU** (not this host).
    ///
    /// Writes `Reboot`, waits for `RebootAck` or a link drop, then
    /// forgets the BlueZ bond. RAM keys on the device do not survive.
    ///
    /// # Errors
    ///
    /// Write failure before the reset starts.
    pub fn reboot(&mut self) -> Result<(), Error> {
        self.transport.write_frame(&encode_reboot())?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if std::time::Instant::now() > deadline {
                break;
            }
            match self.transport.read_chunk() {
                Ok(chunk) => match self.assembler.push(&chunk) {
                    Ok(None) => continue,
                    Ok(Some(frame)) => {
                        if matches!(decode_envelope(&frame)?.body, Some(Body::RebootAck(_))) {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Err(_) => break,
            }
        }
        let _ = self.transport.disconnect(false);
        Ok(())
    }

    /// Drop the link. Unknown units lose the BlueZ bond when `keep_bond` is false.
    ///
    /// # Errors
    ///
    /// Transport disconnect failure.
    pub fn disconnect(&mut self) -> Result<(), Error> {
        self.transport.disconnect(self.keep_bond)
    }

    fn recv_snapshot(&mut self, budget: Duration) -> Result<SnapshotPlanes, Error> {
        let deadline = std::time::Instant::now() + budget;
        loop {
            if std::time::Instant::now() > deadline {
                return Err(Error::Io("snapshot notify timeout".into()));
            }
            let chunk = self.transport.read_chunk()?;
            match self.assembler.push(&chunk)? {
                None => continue,
                Some(frame) => match decode_envelope(&frame)?.body {
                    Some(Body::Snapshot(snap)) => {
                        let expected = snapshot_expected(&snap).map_err(Error::Map)?;
                        return Ok(SnapshotPlanes {
                            nonce: snap.nonce,
                            width: expected.width,
                            height: expected.height,
                            kind: expected.kind,
                            hold: expected.hold,
                            bw: expected.bw.to_vec(),
                            red: expected.red.map(<[u8]>::to_vec),
                        });
                    }
                    Some(Body::SnapshotBusy(busy)) => {
                        return Err(Error::SnapshotBusy { armed: busy.nonce });
                    }
                    _ => {}
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Session;
    use crate::FakeTransport;

    #[test]
    fn reboot_acks_then_forgets_the_bluez_bond() {
        let mut session = Session::new(FakeTransport::tiny(), true);
        session.reboot().expect("reboot");
        assert_eq!(session.transport.disconnects, 1);
        assert_eq!(session.transport.last_keep_bond, Some(false));
    }
}
