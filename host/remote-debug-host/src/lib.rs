//! Generic host session for remote-debug framed envelopes.
//!
//! No clap, no UART, no Sticky pins. Linux uses BlueZ [`bluer`]
//! (Connect, not Pair). Tests use [`FakeTransport`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod fake;
mod passkey;
mod session;
mod transport;

#[cfg(target_os = "linux")]
mod linux;

pub use error::Error;
pub use fake::{reassemble_pieces, shatter, FakeTransport};
pub use passkey::{ChannelPasskey, FixedPasskey, PasskeySource};
pub use session::{rx_uuid, service_uuid, tx_uuid, Session, SnapshotPlanes, DEFAULT_ADV_NAME};
pub use transport::Transport;

#[cfg(target_os = "linux")]
pub use linux::{
    connect, connect_with, BluerTransport, DISCOVER_SECS, GATT_READY_SECS, PAIR_WINDOW_SECS,
};

#[cfg(test)]
mod tests {
    use panel_view::{TouchSample, TouchSource};
    use remote_debug_wire::v1::ProductKey;
    use remote_debug_wire::{decode_envelope, encode_body, FrameAssembler};

    use super::*;

    #[test]
    fn fake_session_get_ack_and_busy() {
        let mut session = Session::new(FakeTransport::tiny(), false);
        let snap = session.get_snapshot(0x11).expect("first get");
        assert_eq!(snap.nonce, 0x11);
        assert_eq!(snap.width, 8);
        assert_eq!(snap.bw.len(), 4);
        session.snapshot_ack(0x11).expect("ack");
        let snap2 = session.get_snapshot(0x22).expect("second get");
        assert_eq!(snap2.nonce, 0x22);
        let mut busy = Session::new(FakeTransport::tiny(), true);
        busy.get_snapshot(0x11).expect("arm");
        match busy.get_snapshot(0x99) {
            Err(Error::SnapshotBusy { armed: 0x11 }) => {}
            other => panic!("expected busy, got {other:?}"),
        }
        busy.disconnect().expect("drop");
        assert!(busy.keep_bond());
    }

    #[test]
    fn fake_inject_and_clear() {
        let mut session = Session::new(FakeTransport::tiny(), false);
        session
            .inject_touch(TouchSample {
                source: TouchSource::Synthetic,
                x: 10,
                y: 20,
                slot: None,
            })
            .expect("touch");
        session
            .inject_button(ProductKey::PRODUCT_KEY_OK, true)
            .expect("btn");
        session.get_snapshot(1).expect("get");
        session.snapshot_clear().expect("clear");
        session.get_snapshot(2).expect("after clear");
    }

    #[test]
    fn shatter_then_reassemble() {
        let framed = encode_body(remote_debug_wire::v1::GetSnapshot {
            nonce: 9,
            ..remote_debug_wire::v1::GetSnapshot::default()
        });
        let pieces = shatter(&framed, 3);
        assert!(pieces.len() > 1);
        let joined = reassemble_pieces(&pieces).expect("join");
        let env = decode_envelope(&joined).expect("decode");
        assert!(matches!(
            env.body,
            Some(remote_debug_wire::v1::envelope::Body::GetSnapshot(_))
        ));
        let mut asm = FrameAssembler::device_rx();
        assert!(asm.push(&framed).expect("one shot").is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pair_window_covers_gatt_and_retry() {
        assert_eq!(
            PAIR_WINDOW_SECS,
            GATT_READY_SECS + DISCOVER_SECS + GATT_READY_SECS
        );
    }

    #[test]
    fn gatt_uuids_are_local_and_not_the_pair_token() {
        assert_eq!(service_uuid(), "c81e1000-5c8a-4f0e-9c3a-2e7b1a0d4f11");
        assert_ne!(service_uuid(), "6b1d0001-5c8a-4f0e-9c3a-2e7b1a0d4f11");
        assert_eq!(DEFAULT_ADV_NAME, "sticky-rs");
    }
}
