//! Pairing crypto entropy, held in RAM for this boot only.
//!
//! The pinned TrouBLE ESP example registers a ChaCha12 provider for Embassy
//! crypto. Here its 256-bit seed comes from the BLE controller's `LeRand`,
//! preserving the previous host's entropy source. No NVS, gauge, or flash
//! writes occur. See TrouBLE revision `866ee711`, `examples/esp32/src/csprng.rs`.

use core::cell::RefCell;

use bt_hci::cmd::controller_baseband::Reset;
use bt_hci::cmd::le::LeRand;
use bt_hci::cmd::SyncCmd;
use bt_hci::controller::{Controller, ControllerCmdSync};
use embassy_debug::PairFailWhy;
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{with_timeout, Duration};
use rand_chacha::rand_core::{Rng as _, SeedableRng as _};
use rand_chacha::ChaCha12Rng;

/// Maximum boot-time controller entropy wait before the pair task fails.
const ENTROPY_TIMEOUT: Duration = Duration::from_secs(3);

/// CSPRNG shared by synchronous crypto callbacks on the Embassy executor.
///
/// A critical section protects the short in-RAM byte generation, as described
/// in *The Embedded Rust Book*: Concurrency. Never hold it over an `.await`.
static RNG: Mutex<CriticalSectionRawMutex, RefCell<Option<ChaCha12Rng>>> =
    Mutex::new(RefCell::new(None));

/// Embassy crypto's single global random-byte provider for this image.
struct Driver;

impl embassy_crypto::driver::Rng for Driver {
    /// Fill crypto input from the seeded CSPRNG without touching a bus.
    ///
    /// Panics if startup violated the seed-before-stack invariant. There is no
    /// deterministic fallback that could silently produce predictable keys.
    fn fill_bytes(buf: &mut [u8]) {
        RNG.lock(|rng| {
            rng.borrow_mut()
                .as_mut()
                .expect("pair crypto used before controller entropy")
                .fill_bytes(buf);
        });
    }
}

embassy_crypto::rng_impl!(Driver);

/// Seed pairing from controller entropy before constructing the TrouBLE stack.
///
/// `ExternalController::read` must run while HCI commands wait for completion.
/// This temporary read loop owns only a controller buffer; it is cancelled
/// before the normal BLE runner takes over (*The Embassy Book*: cooperative
/// tasks). Reset and four `LeRand` commands reproduce the old runner's order.
/// The normal runner may reset the controller again; its entropy is already
/// copied into the in-RAM CSPRNG. Never print or persist the seed.
///
/// # Errors
///
/// Returns `BleStart` on transport, command, allocation, or timeout failure.
/// The caller must stop startup on error, before creating any security keys.
pub async fn seed_from_controller<C>(controller: &C) -> Result<(), PairFailWhy>
where
    C: Controller + ControllerCmdSync<Reset> + ControllerCmdSync<LeRand>,
{
    let entropy = async {
        Reset::new()
            .exec(controller)
            .await
            .map_err(|_| PairFailWhy::BleStart)?;
        let mut seed = [0u8; 32];
        for chunk in seed.chunks_mut(8) {
            let bytes = LeRand::new()
                .exec(controller)
                .await
                .map_err(|_| PairFailWhy::BleStart)?;
            chunk.copy_from_slice(&bytes);
        }
        Ok(seed)
    };
    let read = async {
        let mut buffer = controller.alloc_buf().map_err(|_| PairFailWhy::BleStart)?;
        loop {
            // Command completions are consumed inside ExternalController::read.
            // No connection exists yet, so other startup events can be ignored.
            let _ = controller
                .read(&mut buffer)
                .await
                .map_err(|_| PairFailWhy::BleStart)?;
        }
    };
    let result = with_timeout(ENTROPY_TIMEOUT, select(entropy, read))
        .await
        .map_err(|_| PairFailWhy::BleStart)?;
    let seed = match result {
        Either::First(seed) => seed?,
        Either::Second(error) => return error,
    };
    RNG.lock(|rng| rng.replace(Some(ChaCha12Rng::from_seed(seed))));
    Ok(())
}
