//! Transport-agnostic protobuf codec for remote panel debug.
//!
//! [`Envelope`] is one message. On the wire it is a **u32 little-endian
//! length** then the encoded bytes. SoftAP, BLE, or a later UART
//! side-channel pick the byte stream; this crate does not.
//!
//! Snapshot capacity is **one frozen slot** ([`SnapshotSlot`]). Planes
//! stay with the implementor; encode them from borrowed slices.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

/// Generated `sticky.remote.v1` types ([`buffa`](https://docs.rs/buffa)).
#[allow(missing_docs)]
pub mod proto {
    include!("gen/_include.rs");
}

/// `sticky.remote.v1` messages and enums.
pub use proto::sticky::remote::v1;

mod frame;
mod gatt;
mod map;
mod reassemble;
mod slot;

pub use frame::{
    bytes_field_header_to_slice, decode_envelope, encode_body, encode_envelope,
    encode_snapshot_ack, encode_snapshot_busy, encode_snapshot_clear, encode_snapshot_envelope,
    framed, snapshot_body_len, snapshot_envelope_payload_len, snapshot_preamble_to_slice, unframe,
    write_bytes_field_header, write_framed_snapshot, write_snapshot_preamble, FrameError,
    SnapshotMeta, ENVELOPE_VERSION,
};
pub use gatt::{GATT_RX_UUID, GATT_SERVICE_UUID, GATT_TX_UUID};
pub use map::{
    frame_kind_from_wire, frame_kind_to_wire, hold_from_u32, hold_to_u32, inject_button_gpio,
    inject_touch_sample, product_key_from_gpio, snapshot_expected, MapError, PRODUCT_KEY_OK_GPIO,
    PRODUCT_KEY_PAGE_DOWN_GPIO, PRODUCT_KEY_PAGE_UP_GPIO,
};
pub use reassemble::{FrameAssembler, DEVICE_RX_MAX, HOST_RX_MAX};
pub use slot::{AckOutcome, ClearOutcome, GetOutcome, SnapshotSlot};

#[cfg(test)]
mod tests {
    extern crate std;

    use panel_view::{ExpectedFrame, FrameKind, TouchSample, TouchSource};

    use super::*;
    use crate::v1::{
        envelope::Body, GetSnapshot, InjectButton, InjectTouch, ProductKey, Snapshot,
        TouchSource as WireTouch,
    };

    #[test]
    fn inject_touch_round_trips_to_a_synthetic_sample() {
        let msg = InjectTouch {
            x: 100,
            y: 200,
            slot: Some(2),
            source: WireTouch::TOUCH_SOURCE_SYNTHETIC.into(),
            ..InjectTouch::default()
        };
        let bytes = encode_body(msg.clone());
        let env = decode_envelope(&bytes).expect("framed");
        let Body::InjectTouch(got) = env.body.expect("body") else {
            panic!("expected InjectTouch");
        };
        assert_eq!(
            inject_touch_sample(&got),
            Ok(TouchSample {
                source: TouchSource::Synthetic,
                x: 100,
                y: 200,
                slot: Some(2),
            })
        );
        assert_eq!(
            inject_touch_sample(&msg).unwrap().source,
            TouchSource::Synthetic
        );
    }

    #[test]
    fn inject_touch_rejects_physical_and_oob() {
        let phys = InjectTouch {
            x: 1,
            y: 1,
            source: WireTouch::TOUCH_SOURCE_PHYSICAL.into(),
            ..InjectTouch::default()
        };
        assert_eq!(inject_touch_sample(&phys), Err(MapError::Source));
        let oob = InjectTouch {
            x: u32::from(u16::MAX) + 1,
            y: 0,
            source: WireTouch::TOUCH_SOURCE_SYNTHETIC.into(),
            ..InjectTouch::default()
        };
        assert_eq!(inject_touch_sample(&oob), Err(MapError::Coord));
        let slot = InjectTouch {
            x: 0,
            y: 0,
            slot: Some(5),
            source: WireTouch::TOUCH_SOURCE_SYNTHETIC.into(),
            ..InjectTouch::default()
        };
        assert_eq!(inject_touch_sample(&slot), Err(MapError::Slot));
    }

    #[test]
    fn inject_button_maps_product_keys() {
        assert_eq!(
            inject_button_gpio(&ProductKey::PRODUCT_KEY_OK.into()),
            Ok(PRODUCT_KEY_OK_GPIO)
        );
        assert_eq!(
            inject_button_gpio(&ProductKey::PRODUCT_KEY_PAGE_UP.into()),
            Ok(PRODUCT_KEY_PAGE_UP_GPIO)
        );
        assert_eq!(
            inject_button_gpio(&ProductKey::PRODUCT_KEY_PAGE_DOWN.into()),
            Ok(PRODUCT_KEY_PAGE_DOWN_GPIO)
        );
        assert_eq!(
            inject_button_gpio(&ProductKey::PRODUCT_KEY_UNSPECIFIED.into()),
            Err(MapError::Key)
        );
        let btn = InjectButton {
            key: ProductKey::PRODUCT_KEY_PAGE_DOWN.into(),
            down: true,
            ..InjectButton::default()
        };
        let env = decode_envelope(&encode_body(btn)).expect("framed");
        let Body::InjectButton(got) = env.body.expect("body") else {
            panic!("expected InjectButton");
        };
        assert_eq!(inject_button_gpio(&got.key), Ok(6));
        assert!(got.down);
    }

    #[test]
    fn snapshot_slot_arm_ack_clear_and_busy() {
        let mut slot = SnapshotSlot::new();
        assert_eq!(slot.on_get(0, true), GetOutcome::Zero);
        assert!(!slot.is_armed());
        assert_eq!(slot.on_get(0x11, false), GetOutcome::Empty);
        assert_eq!(slot.on_get(0x11, true), GetOutcome::Armed);
        assert_eq!(slot.armed_nonce(), Some(0x11));
        assert_eq!(slot.on_get(0x11, true), GetOutcome::Retry);
        assert_eq!(slot.on_get(0x22, true), GetOutcome::Busy { armed: 0x11 });
        assert_eq!(slot.on_ack(0), AckOutcome::Zero);
        assert_eq!(slot.on_ack(0x22), AckOutcome::Miss { armed: 0x11 });
        assert_eq!(slot.armed_nonce(), Some(0x11));
        assert_eq!(slot.on_ack(0x11), AckOutcome::Released);
        assert!(!slot.is_armed());
        assert_eq!(slot.on_ack(0x11), AckOutcome::Stale);
        assert_eq!(slot.on_get(0x33, true), GetOutcome::Armed);
        assert_eq!(slot.on_clear(), ClearOutcome::Cleared);
        assert!(!slot.is_armed());
        assert_eq!(slot.on_ack(0x33), AckOutcome::Stale);
        assert_eq!(slot.on_clear(), ClearOutcome::Cleared);
    }

    #[test]
    fn envelope_framing_rejects_truncation_and_bad_version() {
        let get = GetSnapshot {
            nonce: 0xabc,
            ..GetSnapshot::default()
        };
        let bytes = encode_body(get);
        assert_eq!(unframe(&bytes[..3]), Err(FrameError::Truncated));
        let mut short = bytes.clone();
        short.truncate(bytes.len() - 1);
        assert_eq!(unframe(&short), Err(FrameError::Truncated));
        let mut bad_ver = decode_envelope(&bytes).expect("ok");
        bad_ver.version = 99;
        assert_eq!(
            decode_envelope(&encode_envelope(&bad_ver)),
            Err(FrameError::Version)
        );
        assert_eq!(
            decode_envelope(&encode_snapshot_ack(0xabc))
                .expect("ack")
                .body
                .map(|b| matches!(b, Body::SnapshotAck(_))),
            Some(true)
        );
        assert!(matches!(
            decode_envelope(&encode_snapshot_clear())
                .expect("clear")
                .body,
            Some(Body::SnapshotClear(_))
        ));
        assert!(matches!(
            decode_envelope(&encode_snapshot_busy(0x11))
                .expect("busy")
                .body,
            Some(Body::SnapshotBusy(_))
        ));
    }

    #[test]
    fn tiny_snapshot_round_trips_and_rejects_inconsistent() {
        let bw = [0xA0, 0x50, 0x0F, 0xF0];
        let bytes = encode_snapshot_envelope(
            0x42,
            8,
            4,
            v1::FrameKind::FRAME_KIND_MONO,
            Some(hold_to_u32(2)),
            &bw,
            None,
        );
        let env = decode_envelope(&bytes).expect("snap");
        let Body::Snapshot(snap) = env.body.expect("body") else {
            panic!("expected Snapshot");
        };
        let frame = snapshot_expected(&snap).expect("consistent");
        assert_eq!(frame.width, 8);
        assert_eq!(frame.height, 4);
        assert_eq!(frame.kind, FrameKind::Mono);
        assert_eq!(frame.hold, Some(2));
        assert_eq!(frame.bw, &bw);
        assert!(frame.red.is_none());
        assert_eq!(hold_from_u32(2), 2);

        let bad = Snapshot {
            nonce: 1,
            width: 8,
            height: 4,
            kind: v1::FrameKind::FRAME_KIND_MONO.into(),
            bw: bw.to_vec(),
            red: alloc::vec![0; 4],
            ..Snapshot::default()
        };
        assert_eq!(snapshot_expected(&bad), Err(MapError::Frame));
        let _ = ExpectedFrame::<u32>::plane_bytes(8, 4);
    }

    #[test]
    fn get_ack_round_trip() {
        let bytes = encode_body(GetSnapshot {
            nonce: 0xdead_beef_cafe,
            ..GetSnapshot::default()
        });
        let env = decode_envelope(&bytes).expect("get");
        let Body::GetSnapshot(get) = env.body.expect("body") else {
            panic!("expected GetSnapshot");
        };
        assert_eq!(get.nonce, 0xdead_beef_cafe);
        let ack = decode_envelope(&encode_snapshot_ack(get.nonce)).expect("ack");
        let Body::SnapshotAck(ack) = ack.body.expect("body") else {
            panic!("expected SnapshotAck");
        };
        assert_eq!(ack.nonce, 0xdead_beef_cafe);
    }

    #[test]
    fn att_reassembly_yields_one_framed_envelope() {
        let framed = encode_body(GetSnapshot {
            nonce: 7,
            ..GetSnapshot::default()
        });
        let mut asm = FrameAssembler::device_rx();
        assert_eq!(asm.push(&framed[..1]).unwrap(), None);
        assert_eq!(asm.push(&framed[1..3]).unwrap(), None);
        let got = asm.push(&framed[3..]).unwrap().expect("complete");
        assert_eq!(got, framed);
        let env = decode_envelope(&got).expect("decode");
        let Body::GetSnapshot(get) = env.body.expect("body") else {
            panic!("expected GetSnapshot");
        };
        assert_eq!(get.nonce, 7);
    }

    #[test]
    fn att_reassembly_rejects_oversize_prefix() {
        let mut asm = FrameAssembler::new(8);
        let mut huge = (1000u32).to_le_bytes().to_vec();
        huge.extend_from_slice(&[0; 4]);
        assert_eq!(asm.push(&huge), Err(FrameError::TooLarge));
    }

    #[test]
    fn write_framed_snapshot_matches_decode() {
        let bw = [0x11, 0x22, 0x33, 0x44];
        let mut streamed = alloc::vec::Vec::new();
        write_framed_snapshot(
            SnapshotMeta {
                nonce: 0x99,
                width: 8,
                height: 4,
                kind: v1::FrameKind::FRAME_KIND_MONO,
                hold: Some(2),
            },
            &bw,
            None,
            &mut streamed,
        );
        let env = decode_envelope(&streamed).expect("stream");
        let Body::Snapshot(snap) = env.body.expect("body") else {
            panic!("expected Snapshot");
        };
        assert_eq!(snap.nonce, 0x99);
        assert_eq!(snap.bw, bw);
        assert_eq!(snap.hold, Some(2));
    }
}
