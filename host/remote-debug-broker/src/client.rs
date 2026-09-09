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

    /// [`RemoteDebugControlServiceClient::connect`].
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn connect(&self, request: ConnectRequest) -> Result<ConnectResponse, Error> {
        Ok(block_on(self.inner.connect(request))?.into_owned())
    }

    /// [`RemoteDebugControlServiceClient::status`].
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn status(&self, request: StatusRequest) -> Result<StatusResponse, Error> {
        Ok(block_on(self.inner.status(request))?.into_owned())
    }

    /// [`RemoteDebugControlServiceClient::list_targets`].
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn list_targets(&self, request: ListTargetsRequest) -> Result<ListTargetsResponse, Error> {
        Ok(block_on(self.inner.list_targets(request))?.into_owned())
    }

    /// [`RemoteDebugControlServiceClient::inject_touch`].
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn inject_touch(&self, request: InjectTouchRequest) -> Result<InjectTouchResponse, Error> {
        Ok(block_on(self.inner.inject_touch(request))?.into_owned())
    }

    /// [`RemoteDebugControlServiceClient::inject_button`].
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

    /// [`RemoteDebugControlServiceClient::get_snapshot`].
    ///
    /// # Errors
    ///
    /// Transport or RPC failure (including snapshot busy).
    pub fn get_snapshot(&self, request: GetSnapshotRequest) -> Result<GetSnapshotResponse, Error> {
        Ok(block_on(self.inner.get_snapshot(request))?.into_owned())
    }

    /// [`RemoteDebugControlServiceClient::snapshot_ack`].
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn snapshot_ack(&self, request: SnapshotAckRequest) -> Result<SnapshotAckResponse, Error> {
        Ok(block_on(self.inner.snapshot_ack(request))?.into_owned())
    }

    /// [`RemoteDebugControlServiceClient::snapshot_clear`].
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

    /// [`RemoteDebugControlServiceClient::reboot`].
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn reboot(&self, request: RebootRequest) -> Result<RebootResponse, Error> {
        Ok(block_on(self.inner.reboot(request))?.into_owned())
    }

    /// [`RemoteDebugControlServiceClient::disconnect`].
    ///
    /// # Errors
    ///
    /// Transport or RPC failure.
    pub fn disconnect(&self, request: DisconnectRequest) -> Result<DisconnectResponse, Error> {
        Ok(block_on(self.inner.disconnect(request))?.into_owned())
    }

    /// [`RemoteDebugControlServiceClient::shutdown`].
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
    F: std::future::Future<Output = Result<T, E>>,
    Error: From<E>,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(fut)
        .map_err(Error::from)
}

/// How long a client waits for one RPC.
///
/// `connect` returns `pairing` without waiting for BlueZ. Snapshot
/// notify budget is 30s.
pub(crate) const CLIENT_TIMEOUT: Duration = Duration::from_secs(120);
