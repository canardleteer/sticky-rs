//! Device-side Envelope dispatch for remote-debug.
//!
//! [`handle_envelope`] is transport-agnostic. The optional `gatt`
//! feature adds the Trouble RX/TX service and ATT notify helpers.
//! Implementing firmware owns LAST planes, the radio, and pairing
//! glass. Do not persist SMP keys to factory NVS. Never a MAC.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use panel_view::TouchSample;
use remote_debug_wire::v1::envelope::Body;
use remote_debug_wire::v1::{TouchPhase, TouchSpace};
use remote_debug_wire::{
    decode_envelope, inject_button_id, inject_touch_phase, inject_touch_sample, inject_touch_space,
    AckOutcome, ClearOutcome, ControlLayout, FrameError, GetOutcome, SnapshotSlot,
};

#[cfg(feature = "gatt")]
#[allow(missing_docs)]
mod gatt;

#[cfg(feature = "gatt")]
pub use gatt::{
    notify_armed_snapshot, notify_bytes, notify_payload_max, PairTokenService, RemoteDebugService,
};

/// What the transport should send after [`handle_envelope`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeOutcome {
    /// No device→host envelope (inject, ack, clear, ignore).
    None,
    /// Stream the frozen LAST planes as a framed `Snapshot`.
    Snapshot,
    /// `SnapshotBusy` echoing `armed` (0 when the get was empty/zero).
    Busy {
        /// Nonce already holding the slot, or 0.
        armed: u64,
    },
    /// Host asked to software-reset the **embedded MCU** (not the host).
    Reboot,
}

/// Image callbacks. LAST buffers stay with the implementor.
pub trait Device {
    /// Synthetic tap (framebuffer pixels unless `space` is page).
    fn on_inject_touch(&mut self, sample: TouchSample, phase: TouchPhase, space: TouchSpace);

    /// Product-key edge. `key_id` is [`ControlLayout::product_key_id`].
    fn on_inject_button(&mut self, key_id: u8, down: bool);

    /// True when LAST is a consistent compose.
    fn last_ready(&self) -> bool;

    /// [`SnapshotSlot::on_get`].
    fn slot_get(&mut self, nonce: u64, last_ready: bool) -> GetOutcome;

    /// [`SnapshotSlot::on_ack`].
    fn slot_ack(&mut self, nonce: u64) -> AckOutcome;

    /// [`SnapshotSlot::on_clear`].
    fn slot_clear(&mut self) -> ClearOutcome;

    /// Host `Reboot`. ACK on the wire, then reset the MCU.
    fn on_reboot(&mut self);
}

/// [`Device`] that holds a [`SnapshotSlot`] and forwards the rest.
pub struct SlotDevice<D> {
    /// Image callbacks (inject / LAST ready / reboot).
    pub inner: D,
    /// Capacity-1 pull.
    pub slot: SnapshotSlot,
}

impl<D> SlotDevice<D> {
    /// Empty slot.
    #[must_use]
    pub const fn new(inner: D) -> Self {
        Self {
            inner,
            slot: SnapshotSlot::new(),
        }
    }
}

impl<D: LastSource> Device for SlotDevice<D> {
    fn on_inject_touch(&mut self, sample: TouchSample, phase: TouchPhase, space: TouchSpace) {
        self.inner.on_inject_touch(sample, phase, space);
    }

    fn on_inject_button(&mut self, key_id: u8, down: bool) {
        self.inner.on_inject_button(key_id, down);
    }

    fn last_ready(&self) -> bool {
        self.inner.last_ready()
    }

    fn slot_get(&mut self, nonce: u64, last_ready: bool) -> GetOutcome {
        self.slot.on_get(nonce, last_ready)
    }

    fn slot_ack(&mut self, nonce: u64) -> AckOutcome {
        self.slot.on_ack(nonce)
    }

    fn slot_clear(&mut self) -> ClearOutcome {
        self.slot.on_clear()
    }

    fn on_reboot(&mut self) {
        self.inner.on_reboot();
    }
}

/// Injects + LAST ready + reboot (slot owned by [`SlotDevice`]).
pub trait LastSource {
    /// See [`Device::on_inject_touch`].
    fn on_inject_touch(&mut self, sample: TouchSample, phase: TouchPhase, space: TouchSpace);

    /// See [`Device::on_inject_button`].
    fn on_inject_button(&mut self, key_id: u8, down: bool);

    /// See [`Device::last_ready`].
    fn last_ready(&self) -> bool;

    /// See [`Device::on_reboot`].
    fn on_reboot(&mut self);
}

/// Decode one framed Envelope and apply it through [`Device`].
///
/// `L` maps product keys. Page-space taps stay framebuffer until
/// the implementor remaps them.
///
/// # Errors
///
/// [`FrameError`] when the bytes are not one version-1 envelope.
pub fn handle_envelope<D, L>(device: &mut D, bytes: &[u8]) -> Result<EnvelopeOutcome, FrameError>
where
    D: Device,
    L: ControlLayout,
{
    let env = decode_envelope(bytes)?;
    let outcome = match env.body {
        Some(Body::InjectTouch(msg)) => {
            if let Ok(sample) = inject_touch_sample(&msg) {
                device.on_inject_touch(sample, inject_touch_phase(&msg), inject_touch_space(&msg));
            }
            EnvelopeOutcome::None
        }
        Some(Body::InjectButton(msg)) => {
            if let Ok(key_id) = inject_button_id::<L>(&msg.key) {
                device.on_inject_button(key_id, msg.down);
            }
            EnvelopeOutcome::None
        }
        Some(Body::GetSnapshot(msg)) => match device.slot_get(msg.nonce, device.last_ready()) {
            GetOutcome::Armed | GetOutcome::Retry => EnvelopeOutcome::Snapshot,
            GetOutcome::Busy { armed } => EnvelopeOutcome::Busy { armed },
            GetOutcome::Empty | GetOutcome::Zero => EnvelopeOutcome::Busy { armed: 0 },
        },
        Some(Body::SnapshotAck(msg)) => {
            let _ = device.slot_ack(msg.nonce);
            EnvelopeOutcome::None
        }
        Some(Body::SnapshotClear(_)) => {
            let _ = device.slot_clear();
            EnvelopeOutcome::None
        }
        Some(Body::Reboot(_)) => {
            device.on_reboot();
            EnvelopeOutcome::Reboot
        }
        Some(Body::Snapshot(_) | Body::SnapshotBusy(_) | Body::LogLine(_) | Body::RebootAck(_))
        | None => EnvelopeOutcome::None,
    };
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    extern crate std;

    use panel_view::{TouchSample, TouchSource};
    use remote_debug_wire::v1::{InjectButton, InjectTouch, ProductKey, TouchSource as WireTouch};
    use remote_debug_wire::{encode_body, encode_reboot, encode_snapshot_clear, StickyLayout};

    use super::*;

    #[derive(Default)]
    struct Rec {
        touches: alloc::vec::Vec<(u16, u16)>,
        buttons: alloc::vec::Vec<(u8, bool)>,
        ready: bool,
        reboots: u8,
    }

    impl LastSource for Rec {
        fn on_inject_touch(&mut self, sample: TouchSample, _: TouchPhase, _: TouchSpace) {
            self.touches.push((sample.x, sample.y));
        }

        fn on_inject_button(&mut self, key_id: u8, down: bool) {
            self.buttons.push((key_id, down));
        }

        fn last_ready(&self) -> bool {
            self.ready
        }

        fn on_reboot(&mut self) {
            self.reboots += 1;
        }
    }

    #[test]
    fn inject_and_slot_and_reboot() {
        let mut dev = SlotDevice::new(Rec {
            ready: true,
            ..Rec::default()
        });
        let touch = encode_body(InjectTouch {
            x: 10,
            y: 20,
            source: WireTouch::TOUCH_SOURCE_SYNTHETIC.into(),
            ..InjectTouch::default()
        });
        assert_eq!(
            handle_envelope::<_, StickyLayout>(&mut dev, &touch).unwrap(),
            EnvelopeOutcome::None
        );
        assert_eq!(dev.inner.touches, alloc::vec![(10, 20)]);

        let btn = encode_body(InjectButton {
            key: ProductKey::PRODUCT_KEY_PAGE_DOWN.into(),
            down: true,
            ..InjectButton::default()
        });
        handle_envelope::<_, StickyLayout>(&mut dev, &btn).unwrap();
        assert_eq!(dev.inner.buttons, alloc::vec![(6, true)]);

        let get = encode_body(remote_debug_wire::v1::GetSnapshot {
            nonce: 0x11,
            ..remote_debug_wire::v1::GetSnapshot::default()
        });
        assert_eq!(
            handle_envelope::<_, StickyLayout>(&mut dev, &get).unwrap(),
            EnvelopeOutcome::Snapshot
        );
        assert_eq!(
            handle_envelope::<_, StickyLayout>(&mut dev, &get).unwrap(),
            EnvelopeOutcome::Snapshot
        );
        let other = encode_body(remote_debug_wire::v1::GetSnapshot {
            nonce: 0x22,
            ..remote_debug_wire::v1::GetSnapshot::default()
        });
        assert_eq!(
            handle_envelope::<_, StickyLayout>(&mut dev, &other).unwrap(),
            EnvelopeOutcome::Busy { armed: 0x11 }
        );
        handle_envelope::<_, StickyLayout>(&mut dev, &encode_snapshot_clear()).unwrap();
        assert!(!dev.slot.is_armed());

        assert_eq!(
            handle_envelope::<_, StickyLayout>(&mut dev, &encode_reboot()).unwrap(),
            EnvelopeOutcome::Reboot
        );
        assert_eq!(dev.inner.reboots, 1);
        let _ = TouchSource::Synthetic;
    }
}
