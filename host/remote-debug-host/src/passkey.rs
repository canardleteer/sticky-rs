//! Six-digit DisplayOnly passkey. The caller owns UART / glass / `--pin`.

use crate::Error;

/// Supplies the six digits the peripheral showed (`pair pin=` / glass).
///
/// Invoked from the BlueZ KeyboardOnly agent. Implementations must not
/// block the D-Bus loop for long; the Linux transport runs this on a
/// blocking thread.
pub trait PasskeySource: Send + Sync {
    /// Six digits `0..=999_999`.
    ///
    /// # Errors
    ///
    /// [`Error::Passkey`] when no PIN is available in time.
    fn request_passkey(&self) -> Result<u32, Error>;
}

/// Fixed PIN from `--pin` (no UART).
pub struct FixedPasskey {
    pin: u32,
}

impl FixedPasskey {
    /// `pin` is already `0..=999_999`.
    #[must_use]
    pub fn new(pin: u32) -> Self {
        Self {
            pin: pin % 1_000_000,
        }
    }
}

impl PasskeySource for FixedPasskey {
    fn request_passkey(&self) -> Result<u32, Error> {
        Ok(self.pin)
    }
}

/// One-shot channel. UART scrape (in `sticky-host`) sends the digits here.
pub struct ChannelPasskey {
    rx: std::sync::Mutex<std::sync::mpsc::Receiver<u32>>,
    timeout: std::time::Duration,
}

impl ChannelPasskey {
    /// Wait up to `timeout` for one PIN.
    #[must_use]
    pub fn new(rx: std::sync::mpsc::Receiver<u32>, timeout: std::time::Duration) -> Self {
        Self {
            rx: std::sync::Mutex::new(rx),
            timeout,
        }
    }
}

impl PasskeySource for ChannelPasskey {
    fn request_passkey(&self) -> Result<u32, Error> {
        let rx = self
            .rx
            .lock()
            .map_err(|_| Error::Passkey("passkey lock".into()))?;
        rx.recv_timeout(self.timeout)
            .map_err(|_| Error::Passkey("no pair pin= before timeout".into()))
    }
}
