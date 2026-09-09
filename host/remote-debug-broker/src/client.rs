//! Blocking ConnectRPC client of a running owner.

use std::path::Path;
use std::time::Duration;

use connectrpc::client::{ClientConfig, HttpClient};

use crate::connect_svc::RemoteDebugControlServiceClient;
use crate::control::{
    ConnectRequest, ConnectResponse, DisconnectRequest, DisconnectResponse, GetSnapshotRequest,
    GetSnapshotResponse, InjectButtonRequest, InjectButtonResponse, InjectTouchRequest,
    InjectTouchResponse, ListTargetsRequest, ListTargetsResponse, RebootRequest, RebootResponse,
    ShutdownRequest, ShutdownResponse, SnapshotAckRequest, SnapshotAckResponse,
    SnapshotClearRequest, SnapshotClearResponse, StatusRequest, StatusResponse,
};
use crate::{endpoint_path, Error, NO_BROKER};

/// Printed when the endpoint file is missing or the peer is gone.
fn no_owner_at(path: &Path) -> Error {
    Error::message(format!("{NO_BROKER} ({})", path.display()))
}

/// One blocking client of `RemoteDebugControlService`.
pub struct ControlClient {
    inner: RemoteDebugControlServiceClient<HttpClient>,
}

impl ControlClient {
    /// Read the endpoint file and open a plaintext HTTP client.
    ///
    /// # Errors
    ///
    /// Missing owner, bad URI, or client setup.
    pub fn open(dir: &Path) -> Result<Self, Error> {
        let path = endpoint_path(dir);
        let uri = std::fs::read_to_string(&path).map_err(|_| no_owner_at(&path))?;
        let uri = uri.trim();
        if uri.is_empty() {
            return Err(no_owner_at(&path));
        }
        let parsed = uri
            .parse()
            .map_err(|error| Error::message(format!("owner endpoint URI: {error}")))?;
        let http = HttpClient::plaintext();
        let config = ClientConfig::new(parsed).with_default_timeout(CLIENT_TIMEOUT);
        Ok(Self {
            inner: RemoteDebugControlServiceClient::new(http, config),
        })
    }

    /// Start pair on a worker. Returns `pairing` immediately.
    ///
    /// Poll [`Self::status`] until `connected` or `pair failed`.
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn connect(&self, request: ConnectRequest) -> Result<ConnectResponse, Error> {
        Ok(block_on(self.inner.connect(request))?.into_owned())
    }

    /// Session meter: `pairing` / `connected` / `disconnected` / `pair failed`.
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn status(&self, request: StatusRequest) -> Result<StatusResponse, Error> {
        Ok(block_on(self.inner.status(request))?.into_owned())
    }

    /// Advertise names the owner currently tracks (never a MAC).
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn list_targets(&self, request: ListTargetsRequest) -> Result<ListTargetsResponse, Error> {
        Ok(block_on(self.inner.list_targets(request))?.into_owned())
    }

    /// Synthetic tap or slide (framebuffer or page space).
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn inject_touch(&self, request: InjectTouchRequest) -> Result<InjectTouchResponse, Error> {
        Ok(block_on(self.inner.inject_touch(request))?.into_owned())
    }

    /// Product-key edge (`ok` / `page-up` / `page-down`).
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn inject_button(
        &self,
        request: InjectButtonRequest,
    ) -> Result<InjectButtonResponse, Error> {
        Ok(block_on(self.inner.inject_button(request))?.into_owned())
    }

    /// Arm LAST DRAW. Reply includes plane bytes; the caller writes files.
    ///
    /// # Errors
    ///
    /// Transport or RPC failure (including snapshot busy).
    pub fn get_snapshot(&self, request: GetSnapshotRequest) -> Result<GetSnapshotResponse, Error> {
        Ok(block_on(self.inner.get_snapshot(request))?.into_owned())
    }

    /// Release the armed snapshot nonce.
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn snapshot_ack(&self, request: SnapshotAckRequest) -> Result<SnapshotAckResponse, Error> {
        Ok(block_on(self.inner.snapshot_ack(request))?.into_owned())
    }

    /// Operator abort (no nonce). Use after a failed get.
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn snapshot_clear(
        &self,
        request: SnapshotClearRequest,
    ) -> Result<SnapshotClearResponse, Error> {
        Ok(block_on(self.inner.snapshot_clear(request))?.into_owned())
    }

    /// Software-reset the embedded MCU (not this host).
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn reboot(&self, request: RebootRequest) -> Result<RebootResponse, Error> {
        Ok(block_on(self.inner.reboot(request))?.into_owned())
    }

    /// Drop one GATT session. Empty map also [`Self::shutdown`]s.
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn disconnect(&self, request: DisconnectRequest) -> Result<DisconnectResponse, Error> {
        Ok(block_on(self.inner.disconnect(request))?.into_owned())
    }

    /// Stop the owner and unbind the loopback listener.
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn shutdown(&self, request: ShutdownRequest) -> Result<ShutdownResponse, Error> {
        Ok(block_on(self.inner.shutdown(request))?.into_owned())
    }
}

fn block_on<F, T, E>(fut: F) -> Result<T, Error>
where
    F: std::future::Future<Output = Result<T, E>> + Send,
    T: Send,
    Error: From<E>,
{
    // clap-mcp `run` is on a Tokio worker. A nested
    // `Builder::block_on` panics (`Cannot start a runtime from within a
    // runtime`) and poisons the stdio mutex (`session lock`).
    match std::thread::scope(|scope| {
        scope
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(fut)
                    .map_err(Error::from)
            })
            .join()
    }) {
        Ok(result) => result,
        Err(_) => Err(Error::message("client worker panicked")),
    }
}

/// How long a client waits for one RPC.
///
/// `connect` returns `pairing` without waiting for BlueZ. Snapshot
/// notify budget is 30s.
pub(crate) const CLIENT_TIMEOUT: Duration = Duration::from_secs(120);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_on_from_inside_tokio() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let value = block_on(async { Ok::<_, Error>(7_u8) }).expect("off-runtime");
            assert_eq!(value, 7);
        });
    }
}
