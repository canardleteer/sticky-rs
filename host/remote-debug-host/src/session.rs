//! Framed Envelope session on top of a [`crate::Transport`].

use std::collections::VecDeque;
use std::time::Duration;

use panel_view::{FrameKind, TouchSample};
use remote_debug_wire::v1::envelope::Body;
use remote_debug_wire::v1::{
    GetSnapshot, InjectButton, InjectTouch, ProductKey, SnapshotAck, TouchPhase,
    TouchSource as WireTouch, TouchSpace,
};
use remote_debug_wire::{
    decode_envelope, encode_body, encode_reboot, encode_snapshot_clear, inject_button_gpio,
    snapshot_expected, ControlLayout, FrameAssembler, GATT_RX_UUID, GATT_SERVICE_UUID,
    GATT_TX_UUID,
};

use crate::{Error, Transport};

/// How many Target / Scene `LogLine` copies the host keeps.
///
/// Firmware notifies each line; this ring drops the oldest so a
/// targets walk can still show `target loop` after `target show id=0`
/// overwrote `last_log`. Never a MAC.
pub const LOG_RING: usize = 16;

/// One GATT `LogLine` kept in [`LOG_RING`] (never a MAC).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Device milliseconds since boot when the line was formatted.
    pub t_ms: u32,
    /// UART text. Never a MAC.
    pub text: String,
}

/// In-tree Sticky advertise name ([`remote_debug_wire::StickyLayout`]).
///
/// A second device passes its own `ConnectRequest.target` / `--name`.
pub const DEFAULT_ADV_NAME: &str = remote_debug_wire::StickyLayout::ADV_NAME;

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
    /// `Scene::persist_byte`.
    pub scene: Option<u32>,
    /// Targets walk id when on that card.
    pub target_step: Option<u32>,
    /// `dot` / `slide_x` / `slide_y`, or unset.
    pub target_kind: Option<u32>,
    /// Expected page X.
    pub target_expect_x: Option<u32>,
    /// Expected page Y.
    pub target_expect_y: Option<u32>,
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
    logs: VecDeque<LogEntry>,
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
            logs: VecDeque::new(),
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

    /// Newest Target / Scene `LogLine` text (never a MAC).
    #[must_use]
    pub fn last_log(&self) -> Option<&str> {
        self.logs.back().map(|entry| entry.text.as_str())
    }

    /// Oldest-first ring of Target / Scene lines (never a MAC).
    pub fn recent_logs(&self) -> impl Iterator<Item = &LogEntry> {
        self.logs.iter()
    }

    /// Synthetic framebuffer tap.
    ///
    /// # Errors
    ///
    /// Write failure.
    pub fn inject_touch(&mut self, sample: TouchSample) -> Result<(), Error> {
        self.inject_touch_ex(sample, TouchPhase::TOUCH_PHASE_DOWN, false)
    }

    /// Synthetic tap with phase and optional page space.
    ///
    /// # Errors
    ///
    /// Write failure.
    pub fn inject_touch_ex(
        &mut self,
        sample: TouchSample,
        phase: TouchPhase,
        page: bool,
    ) -> Result<(), Error> {
        let space = if page {
            TouchSpace::TOUCH_SPACE_PAGE
        } else {
            TouchSpace::TOUCH_SPACE_FRAMEBUFFER
        };
        let msg = InjectTouch {
            x: u32::from(sample.x),
            y: u32::from(sample.y),
            slot: sample.slot.map(u32::from),
            source: WireTouch::TOUCH_SOURCE_SYNTHETIC.into(),
            phase: phase.into(),
            space: space.into(),
            ..InjectTouch::default()
        };
        self.transport.write_frame(&encode_body(msg))?;
        self.drain_logs();
        Ok(())
    }

    /// Consume queued GATT `LogLine` fragments (no wait).
    pub fn drain_logs(&mut self) {
        while let Some(chunk) = self.transport.try_read_chunk() {
            if let Ok(Some(frame)) = self.assembler.push(&chunk) {
                if let Ok(env) = decode_envelope(&frame) {
                    if let Some(Body::LogLine(line)) = env.body {
                        self.push_log(line.t_ms, line.text);
                    }
                }
            }
        }
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
        self.drain_logs();
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
    /// Writes `Reboot`, requires `RebootAck`, then
    /// forgets the BlueZ bond. RAM keys on the device do not survive.
    ///
    /// # Errors
    ///
    /// Write, framing, timeout or storage-barrier failure. Refusal preserves
    /// the session and bond for recovery.
    pub fn reboot(&mut self) -> Result<(), Error> {
        self.reboot_with_force(false)
    }

    /// Reset the MCU with an explicit recovery policy. Forced reset skips the
    /// firmware's storage drain and can interrupt an outstanding write.
    pub fn reboot_with_force(&mut self, force: bool) -> Result<(), Error> {
        let frame = if force {
            encode_body(remote_debug_wire::v1::Reboot {
                force,
                ..Default::default()
            })
        } else {
            encode_reboot()
        };
        self.transport.write_frame(&frame)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if std::time::Instant::now() >= deadline {
                self.assembler.clear();
                return Err(Error::Io("reboot acknowledgement timeout".into()));
            }
            let chunk = match self.transport.read_chunk() {
                Ok(chunk) => chunk,
                Err(error) => {
                    self.assembler.clear();
                    return Err(error);
                }
            };
            let frame = match self.assembler.push(&chunk) {
                Ok(frame) => frame,
                Err(error) => {
                    self.assembler.clear();
                    return Err(error.into());
                }
            };
            if let Some(frame) = frame {
                match decode_envelope(&frame)?.body {
                    Some(Body::RebootAck(ack)) => {
                        if ack.storage_failed {
                            return Err(Error::Io(
                                "storage drain failed; use reboot --force".into(),
                            ));
                        }
                        break;
                    }
                    Some(Body::LogLine(line)) => self.push_log(line.t_ms, line.text),
                    _ => {}
                }
            }
        }
        let _ = self.transport.disconnect(false);
        Ok(())
    }

    /// Submit one bounded storage request and await its matching reply.
    /// Long device jobs acknowledge acceptance; callers poll STATUS while the
    /// same connection remains available for snapshots and forced reboot.
    pub fn storage(
        &mut self,
        request: remote_debug_wire::v1::StorageRequest,
    ) -> Result<remote_debug_wire::v1::StorageReply, Error> {
        if request.id == 0 || request.data.len() > 512 {
            return Err(Error::Io("invalid storage request".into()));
        }
        let id = request.id;
        self.transport.write_frame(&encode_body(request))?;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if std::time::Instant::now() >= deadline {
                self.assembler.clear();
                return Err(Error::Io("storage reply timeout".into()));
            }
            let chunk = match self.transport.read_chunk() {
                Ok(chunk) => chunk,
                Err(error) => {
                    self.assembler.clear();
                    return Err(error);
                }
            };
            let frame = match self.assembler.push(&chunk) {
                Ok(frame) => frame,
                Err(error) => {
                    self.assembler.clear();
                    return Err(error.into());
                }
            };
            if let Some(frame) = frame {
                let envelope = match decode_envelope(&frame) {
                    Ok(envelope) => envelope,
                    Err(error) => {
                        self.assembler.clear();
                        return Err(error.into());
                    }
                };
                match envelope.body {
                    Some(Body::StorageReply(reply)) if reply.id == id => return Ok(*reply),
                    Some(Body::LogLine(line)) => self.push_log(line.t_ms, line.text),
                    _ => {}
                }
            }
        }
    }

    fn push_log(&mut self, t_ms: u32, text: String) {
        if self.logs.len() == LOG_RING {
            self.logs.pop_front();
        }
        self.logs.push_back(LogEntry { t_ms, text });
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
                        let kind_num = snap.target_kind.as_known().map(|k| k as u32);
                        return Ok(SnapshotPlanes {
                            nonce: snap.nonce,
                            width: expected.width,
                            height: expected.height,
                            kind: expected.kind,
                            hold: expected.hold,
                            scene: snap.scene,
                            target_step: snap.target_step,
                            target_kind: kind_num.filter(|&k| k != 0),
                            target_expect_x: snap.target_expect_x,
                            target_expect_y: snap.target_expect_y,
                            bw: expected.bw.to_vec(),
                            red: expected.red.map(<[u8]>::to_vec),
                        });
                    }
                    Some(Body::SnapshotBusy(busy)) => {
                        return Err(Error::SnapshotBusy { armed: busy.nonce });
                    }
                    Some(Body::LogLine(line)) => {
                        self.push_log(line.t_ms, line.text);
                    }
                    _ => {}
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Session, LOG_RING};
    use crate::FakeTransport;

    /// Script notifications independently of requests, including partial frames.
    struct StorageTransport {
        chunks: std::collections::VecDeque<Vec<u8>>,
        writes: usize,
        disconnects: usize,
    }
    impl crate::Transport for StorageTransport {
        fn write_frame(&mut self, _: &[u8]) -> Result<(), crate::Error> {
            self.writes += 1;
            Ok(())
        }
        fn read_chunk(&mut self) -> Result<Vec<u8>, crate::Error> {
            self.chunks
                .pop_front()
                .ok_or(crate::Error::Io("scripted timeout".into()))
        }
        fn try_read_chunk(&mut self) -> Option<Vec<u8>> {
            self.chunks.pop_front()
        }
        fn disconnect(&mut self, _: bool) -> Result<(), crate::Error> {
            self.disconnects += 1;
            Ok(())
        }
    }

    #[test]
    fn storage_ignores_other_ids_and_preserves_logs_between_att_chunks() {
        use remote_debug_wire::{
            encode_body, encode_log_line,
            v1::{StorageReply, StorageRequest},
        };
        let reply = encode_body(StorageReply {
            id: 42,
            ok: true,
            job_id: 42,
            job_complete: true,
            job_ok: false,
            ..Default::default()
        });
        let mut chunks = std::collections::VecDeque::from([
            encode_body(StorageReply {
                id: 41,
                ok: true,
                ..Default::default()
            }),
            encode_log_line(12, "storage verification complete"),
        ]);
        chunks.extend(reply.chunks(3).map(<[u8]>::to_vec));
        let mut session = Session::new(
            StorageTransport {
                chunks,
                writes: 0,
                disconnects: 0,
            },
            false,
        );
        let result = session
            .storage(StorageRequest {
                id: 42,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(result.id, 42);
        assert!(result.job_complete);
        assert!(!result.job_ok);
        assert_eq!(session.last_log(), Some("storage verification complete"));
        assert_eq!(session.transport.writes, 1);
    }

    #[test]
    fn storage_timeout_discards_partial_frame_before_next_request() {
        use remote_debug_wire::{
            encode_body,
            v1::{StorageReply, StorageRequest},
        };
        let old = encode_body(StorageReply {
            id: 1,
            ..Default::default()
        });
        let mut session = Session::new(
            StorageTransport {
                chunks: [old[..6].to_vec()].into(),
                writes: 0,
                disconnects: 0,
            },
            false,
        );
        assert!(session
            .storage(StorageRequest {
                id: 1,
                ..Default::default()
            })
            .is_err());
        session
            .transport
            .chunks
            .push_back(encode_body(StorageReply {
                id: 2,
                ok: true,
                ..Default::default()
            }));
        assert!(
            session
                .storage(StorageRequest {
                    id: 2,
                    ..Default::default()
                })
                .unwrap()
                .ok
        );
        assert!(session
            .storage(StorageRequest {
                id: 0,
                ..Default::default()
            })
            .is_err());
        assert!(session
            .storage(StorageRequest {
                id: 3,
                data: vec![0; 513],
                ..Default::default()
            })
            .is_err());
        assert_eq!(session.transport.writes, 2);
    }

    #[test]
    fn reboot_refusal_and_missing_ack_preserve_session_for_recovery() {
        use remote_debug_wire::{
            encode_body,
            v1::{RebootAck, StorageReply, StorageRequest},
        };
        for chunks in [
            vec![],
            vec![encode_body(RebootAck {
                storage_failed: true,
                ..Default::default()
            })],
        ] {
            let mut session = Session::new(
                StorageTransport {
                    chunks: chunks.into(),
                    writes: 0,
                    disconnects: 0,
                },
                true,
            );
            assert!(session.reboot().is_err());
            assert_eq!(session.transport.disconnects, 0);
            session
                .transport
                .chunks
                .push_back(encode_body(StorageReply {
                    id: 7,
                    ok: true,
                    ..Default::default()
                }));
            assert!(
                session
                    .storage(StorageRequest {
                        id: 7,
                        ..Default::default()
                    })
                    .unwrap()
                    .ok
            );
        }
    }

    #[test]
    fn reboot_acks_then_forgets_the_bluez_bond() {
        let mut session = Session::new(FakeTransport::tiny(), true);
        session.reboot().expect("reboot");
        assert_eq!(session.transport.disconnects, 1);
        assert_eq!(session.transport.last_keep_bond, Some(false));
    }

    #[test]
    fn log_ring_keeps_newest_and_drops_oldest() {
        let mut session = Session::new(FakeTransport::tiny(), false);
        for i in 0..(LOG_RING + 2) {
            session.push_log(i as u32, format!("line-{i}"));
        }
        assert_eq!(session.last_log(), Some("line-17"));
        let texts: Vec<_> = session.recent_logs().map(|e| e.text.as_str()).collect();
        assert_eq!(texts.len(), LOG_RING);
        assert_eq!(texts[0], "line-2");
        assert_eq!(texts[LOG_RING - 1], "line-17");
    }

    #[test]
    fn drain_logs_reads_enqueued_notify() {
        let mut transport = FakeTransport::tiny();
        transport.enqueue_log(42, "target show id=0");
        let mut session = Session::new(transport, false);
        session.drain_logs();
        assert_eq!(session.last_log(), Some("target show id=0"));
        let entry = session.recent_logs().next().expect("one line");
        assert_eq!(entry.t_ms, 42);
    }
}
