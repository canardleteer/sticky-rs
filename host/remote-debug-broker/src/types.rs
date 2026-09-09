//! Owner spawn and serve options. Not clap.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use crate::control::ConnectRequest;
use crate::Error;

/// Called after a successful pair when [`ConnectRequest::remember`] is true.
pub type RememberHook = Arc<dyn Fn(&ConnectRequest) -> Result<(), Error> + Send + Sync>;

/// Options for [`crate::serve_with`].
#[derive(Clone)]
pub struct ServeOpts<'a> {
    /// Directory for the endpoint file and pid file.
    pub dir: &'a Path,
    /// Loopback bind address. `None` is `127.0.0.1:0`.
    pub listen: Option<SocketAddr>,
    /// Install a SIGINT handler that shuts the owner down.
    pub install_ctrlc: bool,
    /// Log `connected` / `snapshot` / errors on stderr (never a MAC).
    pub log: bool,
    /// After a successful pair with [`ConnectRequest::remember`].
    pub on_remember: Option<RememberHook>,
}

/// How [`crate::ensure_broker`] starts a detached serve.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    /// Binary to spawn (the caller’s serve CLI).
    pub exe: std::path::PathBuf,
    /// Full argv after the executable. No implicit `remote-debug serve`.
    pub args: Vec<String>,
}
