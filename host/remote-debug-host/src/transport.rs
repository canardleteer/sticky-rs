//! Byte transport: write one framed envelope, read ATT fragments.

use crate::Error;

/// Host→device write and device→host notify fragments.
///
/// Implementations must not print a MAC. Tests use [`crate::FakeTransport`].
pub trait Transport {
    /// Write one framed envelope, chunked to the ATT MTU if needed.
    ///
    /// # Errors
    ///
    /// Link or GATT write failure.
    fn write_frame(&mut self, framed: &[u8]) -> Result<(), Error>;

    /// Next notify payload (one ATT fragment, not necessarily a frame).
    ///
    /// # Errors
    ///
    /// Timeout or disconnect.
    fn read_chunk(&mut self) -> Result<Vec<u8>, Error>;

    /// Drop the GATT link. `keep_bond` is the remember-me flag.
    ///
    /// # Errors
    ///
    /// Disconnect or unbond failure (unknown units should be removed).
    fn disconnect(&mut self, keep_bond: bool) -> Result<(), Error>;
}
