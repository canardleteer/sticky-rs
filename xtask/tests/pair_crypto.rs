//! Host fixture for the firmware's controller-seeded pairing crypto.
//! No BLE adapter, UART, or attached board participates in these tests.

#[path = "../../firmware/embassy-debug/src/pair_crypto.rs"]
mod pair_crypto;

use std::cell::{Cell, RefCell};
use std::future::poll_fn;
use std::task::Poll;

use bt_hci::cmd::controller_baseband::Reset;
use bt_hci::cmd::le::LeRand;
use bt_hci::cmd::{Cmd, Error, Opcode};
use bt_hci::controller::{Controller, ControllerCmdSync};
use bt_hci::data::{AclPacket, IsoPacket, SyncPacket};
use bt_hci::ControllerToHostPacket;
use embassy_debug::PairFailWhy;
use rand_chacha::rand_core::{Rng as _, SeedableRng as _};
use rand_chacha::ChaCha12Rng;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Fault {
    #[default]
    None,
    Reset,
    SecondRandom,
    Allocation,
    Read,
}

#[derive(Default)]
struct ControllerFixture {
    fault: Fault,
    commands: RefCell<Vec<Opcode>>,
    completed: Cell<usize>,
    random_calls: Cell<u8>,
}

impl ControllerFixture {
    async fn complete(&self, opcode: Opcode) -> Result<(), Error<embedded_io::ErrorKind>> {
        self.commands.borrow_mut().push(opcode);
        let request = self.commands.borrow().len();
        poll_fn(|cx| {
            if self.completed.get() >= request {
                Poll::Ready(())
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await;
        Ok(())
    }
}

impl embedded_io::ErrorType for ControllerFixture {
    type Error = embedded_io::ErrorKind;
}

impl Controller for ControllerFixture {
    type Buffer<'a> = ();

    fn alloc_buf(&self) -> Result<(), Self::Error> {
        if self.fault == Fault::Allocation {
            Err(embedded_io::ErrorKind::Other)
        } else {
            Ok(())
        }
    }

    async fn write_acl_data(&self, _: &AclPacket<'_>) -> Result<(), Self::Error> {
        panic!("entropy startup must not send ACL data")
    }

    async fn write_sync_data(&self, _: &SyncPacket<'_>) -> Result<(), Self::Error> {
        panic!("entropy startup must not send synchronous data")
    }

    async fn write_iso_data(&self, _: &IsoPacket<'_>) -> Result<(), Self::Error> {
        panic!("entropy startup must not send isochronous data")
    }

    async fn read<'a>(
        &self,
        _: &'a mut Self::Buffer<'_>,
    ) -> Result<ControllerToHostPacket<'a>, Self::Error> {
        if self.fault == Fault::Read {
            return Err(embedded_io::ErrorKind::Other);
        }
        // Like ExternalController, a command completes only while reads are
        // polled. Keeping this pending also checks cancellation of the reader
        // when the last entropy reply arrives.
        poll_fn(|cx| {
            self.completed.set(self.commands.borrow().len());
            cx.waker().wake_by_ref();
            Poll::Pending
        })
        .await
    }
}

impl ControllerCmdSync<Reset> for ControllerFixture {
    async fn exec(&self, _: &Reset) -> Result<(), Error<Self::Error>> {
        if self.fault == Fault::Reset {
            return Err(Error::Io(embedded_io::ErrorKind::Other));
        }
        self.complete(Reset::OPCODE).await
    }
}

impl ControllerCmdSync<LeRand> for ControllerFixture {
    async fn exec(&self, _: &LeRand) -> Result<[u8; 8], Error<Self::Error>> {
        self.random_calls.set(self.random_calls.get() + 1);
        if self.fault == Fault::SecondRandom && self.random_calls.get() == 2 {
            return Err(Error::Io(embedded_io::ErrorKind::Other));
        }
        self.complete(LeRand::OPCODE).await?;
        Ok([self.random_calls.get(); 8])
    }
}

// A single test keeps the global provider unseeded until all failure cases
// have run; parallel tests must never share or reseed this singleton.
#[tokio::test]
async fn entropy_errors_fail_closed_and_controller_bytes_seed_the_crypto_provider() {
    let unseeded =
        || std::panic::catch_unwind(|| embassy_crypto::rng_fill_bytes(&mut [0; 16])).is_err();
    assert!(unseeded());
    for fault in [
        Fault::Reset,
        Fault::SecondRandom,
        Fault::Allocation,
        Fault::Read,
    ] {
        let controller = ControllerFixture {
            fault,
            ..Default::default()
        };
        assert_eq!(
            pair_crypto::seed_from_controller(&controller).await,
            Err(PairFailWhy::BleStart)
        );
        assert!(
            unseeded(),
            "failed startup must not install a fallback seed"
        );
    }

    let controller = ControllerFixture::default();
    pair_crypto::seed_from_controller(&controller)
        .await
        .unwrap();
    assert_eq!(
        *controller.commands.borrow(),
        [
            Reset::OPCODE,
            LeRand::OPCODE,
            LeRand::OPCODE,
            LeRand::OPCODE,
            LeRand::OPCODE
        ]
    );
    let seed = std::array::from_fn(|i| (i / 8 + 1) as u8);
    let mut expected = ChaCha12Rng::from_seed(seed);
    for _ in 0..2 {
        let mut want = [0; 64];
        let mut got = [0; 64];
        expected.fill_bytes(&mut want);
        embassy_crypto::rng_fill_bytes(&mut got);
        assert_eq!(
            got, want,
            "controller seed must drive the continuing stream"
        );
    }
}
