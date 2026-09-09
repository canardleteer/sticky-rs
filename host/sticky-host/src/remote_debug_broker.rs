//! Sticky live owner for the generic Unix-socket broker.
//!
//! RPC types, `serve_with`, and [`SpawnSpec`] live in
//! `remote-debug-broker`. This module keeps UART `pair pin=` scrape,
//! remember-me, the Sticky flock runtime dir, and the xtask
//! `remote-debug serve` argv.

use std::path::{Path, PathBuf};
use std::time::Duration;

use remote_debug_wire::v1::ProductKey;

use crate::original::Layout;
use crate::uart_lock::default_lock_dir;
use crate::Error;

pub use remote_debug_broker::{
    broker_log_path, broker_pid_path, broker_socket_path, serve_with, BrokerPhase, BrokerReply,
    BrokerRequest, ConnectReq, RebootReq, ServeOpts, SnapshotWire, SpawnSpec, NO_BROKER,
};

/// Runtime dir for the broker socket (same family as the UART flock).
#[must_use]
pub fn broker_runtime_dir() -> PathBuf {
    default_lock_dir()
}

/// On-disk `target/debug/xtask` when present; otherwise `current_exe`.
///
/// A stdio MCP process that outlives `cargo build -p xtask` has a
/// deleted inode; spawning that path is `ENOENT`.
#[must_use]
pub fn resolve_broker_exe(repo_root: &Path, current_exe: &Path) -> PathBuf {
    let on_disk = repo_root.join("target").join("debug").join("xtask");
    if on_disk.is_file() {
        on_disk
    } else {
        current_exe.to_path_buf()
    }
}

/// Product key from `ok` / `page-up` / `page-down`.
///
/// # Errors
///
/// Unknown token.
pub fn parse_product_key(raw: &str) -> Result<ProductKey, Error> {
    remote_debug_broker::parse_product_key(raw).map_err(Error::from)
}

/// Send one request and wait for one reply.
///
/// # Errors
///
/// Missing broker, I/O, or JSON.
pub fn rpc(dir: &Path, name: &str, request: &BrokerRequest) -> Result<BrokerReply, Error> {
    remote_debug_broker::rpc(dir, name, request).map_err(Error::from)
}

/// Poll until the pid file is live or `budget` elapses.
///
/// # Errors
///
/// Timeout.
pub fn wait_for_broker(dir: &Path, name: &str, budget: Duration) -> Result<(), Error> {
    remote_debug_broker::wait_for_broker(dir, name, budget).map_err(Error::from)
}

/// Unlink a leftover socket when the peer pid is dead, then spawn
/// `remote-debug serve` if nothing is listening.
///
/// `exe` is the same xtask binary (`remote-debug serve --name …`).
///
/// # Errors
///
/// Spawn failure or the child never bound.
pub fn ensure_broker(dir: &Path, name: &str, exe: &Path) -> Result<(), Error> {
    remote_debug_broker::ensure_broker(
        dir,
        name,
        &SpawnSpec {
            exe: exe.to_path_buf(),
            args: vec![
                "remote-debug".into(),
                "serve".into(),
                "--name".into(),
                name.into(),
                "--socket-dir".into(),
                dir.to_string_lossy().into_owned(),
            ],
        },
    )
    .map_err(Error::from)
}

/// Foreground live owner (Linux BlueZ). Blocks until disconnect or Ctrl-C.
///
/// # Errors
///
/// Bind failure, BlueZ, or UART scrape.
pub fn serve_live(layout: &Layout, name: &str, socket_dir: Option<&Path>) -> Result<(), Error> {
    let default = broker_runtime_dir();
    let dir = socket_dir.unwrap_or(&default);
    std::fs::create_dir_all(dir)?;
    serve_live_inner(layout, dir, name)
}

fn serve_live_inner(layout: &Layout, dir: &Path, name: &str) -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    {
        use std::sync::Arc;

        use remote_debug_broker::{RememberHook, ServeOpts};

        let layout = layout.clone();
        let on_remember: RememberHook = Arc::new(move |req| {
            remember_from_req(&layout, req)
                .map_err(|error| remote_debug_broker::Error::message(error.to_string()))
        });
        remote_debug_broker::serve_with(
            ServeOpts {
                dir,
                name,
                install_ctrlc: true,
                log: true,
                on_remember: Some(on_remember),
            },
            |req| open_live_session(req).map_err(map_open),
        )
        .map_err(Error::from)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (layout, dir, name);
        Err(Error::RemoteDebug(
            "remote-debug serve needs Linux BlueZ".into(),
        ))
    }
}

fn map_open(error: Error) -> remote_debug_broker::Error {
    remote_debug_broker::Error::message(error.to_string())
}

/// Pair and open a [`remote_debug_host::BluerTransport`] session.
///
/// UART lock is held only while scraping a new `pair pin=`, then released.
///
/// # Errors
///
/// Port, lock, PIN timeout, or BlueZ.
#[cfg(target_os = "linux")]
pub fn open_live_session(
    req: &ConnectReq,
) -> Result<remote_debug_host::Session<remote_debug_host::BluerTransport>, Error> {
    use std::sync::mpsc;

    use remote_debug_host::{connect, connect_with, ChannelPasskey, FixedPasskey, PasskeySource};

    use crate::detect;
    use crate::uart_lock::try_acquire;
    use crate::wait_new_pair_pin;

    let window = Duration::from_secs(remote_debug_host::PAIR_WINDOW_SECS);
    let transport = if let Some(pin) = req.pin {
        let passkey: std::sync::Arc<dyn PasskeySource> =
            std::sync::Arc::new(FixedPasskey::new(pin));
        connect(&req.name, passkey).map_err(map_ble)?
    } else {
        let port = req.port.clone();
        let (tx, rx) = mpsc::channel();
        let (start_tx, start_rx) = mpsc::channel();
        std::thread::spawn(move || {
            if start_rx.recv_timeout(window).is_err() {
                return;
            }
            let opened = match detect::resolve_sticky_port(port.clone()) {
                Ok(resolved) => try_acquire(&resolved, "remote-debug").and_then(|uart| {
                    crate::cdc_listen::CdcListen::open(&resolved).map(|cdc| (cdc, uart))
                }),
                // `cdc-acm` unbound after a listen skipped Drop: usbfs still
                // sees the unique QinHeng. Do not require a replug for auto-PIN.
                Err(Error::MissingStickyUart) if port.is_none() => {
                    crate::cdc_listen::CdcListen::open_unique_locked(|path| {
                        try_acquire(path, "remote-debug")
                    })
                }
                Err(_) => return,
            };
            let Ok((mut cdc, uart)) = opened else {
                return;
            };
            let _uart = uart;
            let mut seen = Vec::new();
            if let Ok(pin) = wait_new_pair_pin(&mut cdc, &mut seen, window) {
                let _ = tx.send(pin);
            }
        });
        let passkey: std::sync::Arc<dyn PasskeySource> =
            std::sync::Arc::new(ChannelPasskey::new(rx, window));
        connect_with(&req.name, passkey, move || {
            let _ = start_tx.send(());
        })
        .map_err(map_ble)?
    };
    Ok(remote_debug_host::Session::new(transport, req.remember))
}

fn remember_from_req(layout: &Layout, req: &ConnectReq) -> Result<(), Error> {
    use crate::detect;
    use crate::usb_serial_from_port;

    let usb = req
        .port
        .as_deref()
        .and_then(usb_serial_from_port)
        .or_else(|| {
            detect::resolve_sticky_port(req.port.clone())
                .ok()
                .and_then(|port| usb_serial_from_port(&port))
        });
    crate::remember_unit(layout, None, usb.as_deref())
}

fn map_ble(error: remote_debug_host::Error) -> Error {
    Error::RemoteDebug(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_broker_exe_prefers_on_disk_xtask() {
        let tmp = tempfile::tempdir().unwrap();
        let on_disk = tmp.path().join("target").join("debug").join("xtask");
        fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        fs::write(&on_disk, b"").unwrap();
        let current = tmp.path().join("deleted-inode");
        assert_eq!(resolve_broker_exe(tmp.path(), &current), on_disk);
    }

    #[test]
    fn resolve_broker_exe_falls_back_to_current() {
        let tmp = tempfile::tempdir().unwrap();
        let current = tmp.path().join("current");
        fs::write(&current, b"").unwrap();
        assert_eq!(resolve_broker_exe(tmp.path(), &current), current);
    }
}
