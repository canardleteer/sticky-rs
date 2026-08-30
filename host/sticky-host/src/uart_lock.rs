//! Exclusive UART session lock so one command cannot DTR/RTS-reset another.
//!
//! Commands that reset the chip (backup, restore, `--probe`, etc.) pulse
//! EN/IO0 or load the flasher stub. Long listeners (`monitor`, `learn-uart`,
//! etc.) do not reset while watching, but they still hold this lock so a
//! second xtask cannot pulse DTR mid-session.
//! A second reset mid-dump or mid-write-bin interrupts the stub. Inventory
//! without `--probe` does not take this lock.
//!
//! This is the **one** shared session for xtask **and** any in-repo subprocess
//! that might pulse DTR (`esptool`, `espflash`, `cargo espflash`, nested
//! `cargo xtask`). Run those children with [`UartSession::status`] /
//! [`UartSession::output`] so the flock stays held until they exit. Those
//! helpers also set [`UART_LOCK_ENV`] so a cooperating child can join the
//! same session instead of failing [`try_acquire`].
//!
//! The lock is an advisory `flock` on a sidecar file keyed by the UART inode
//! (so a by-id node and its ACM target share one session). Dropping a
//! session that **owns** the flock (process exit included) releases it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

use fs4::fs_std::FileExt;
use sha2::{Digest, Sha256};

use crate::Error;

/// Environment variable [`UartSession::prepare_command`] sets on children.
///
/// Value is the absolute lock-file path. A cooperating child that would
/// reset the same UART calls [`try_acquire`] and joins this session instead
/// of pulsing DTR while the parent still holds the flock.
pub const UART_LOCK_ENV: &str = "STICKY_UART_LOCK";

/// Held exclusive flock (or a child joined to a parent's flock).
///
/// Keep this in scope for the whole UART-touching command, including any
/// subprocess wait. Dropping an owning session releases the lock.
#[derive(Debug)]
#[must_use = "dropping the UART session releases the lock and lets another command pulse DTR"]
pub struct UartSession {
    path: PathBuf,
    /// `Some` when this process took the flock. `None` when joined via
    /// [`UART_LOCK_ENV`] (parent still owns it).
    _file: Option<File>,
}

/// Acquire an exclusive UART session for `port`, or refuse without waiting.
///
/// `command` is recorded so a contending xtask can say who holds the port.
/// If [`UART_LOCK_ENV`] points at this port's lock and the holder is this
/// process or an ancestor, joins that session (nested `cargo xtask` / wrapper).
pub fn try_acquire(port: &str, command: &str) -> Result<UartSession, Error> {
    try_acquire_in(&std::env::temp_dir(), port, command)
}

/// Testable [`try_acquire`] with an explicit lock directory.
pub fn try_acquire_in(lock_dir: &Path, port: &str, command: &str) -> Result<UartSession, Error> {
    acquire(lock_dir, port, command, inherit_from_env().as_deref())
}

fn inherit_from_env() -> Option<PathBuf> {
    std::env::var_os(UART_LOCK_ENV).map(PathBuf::from)
}

fn acquire(
    lock_dir: &Path,
    port: &str,
    command: &str,
    inherit: Option<&Path>,
) -> Result<UartSession, Error> {
    fs::create_dir_all(lock_dir)?;
    let path = lock_path(lock_dir, port);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    match file.try_lock_exclusive() {
        Ok(true) => {
            write_holder(&file, command)?;
            Ok(UartSession {
                path,
                _file: Some(file),
            })
        }
        Ok(false) if can_join(&path, inherit) => Ok(UartSession { path, _file: None }),
        Ok(false) => Err(busy_from(&path)),
        Err(error) => Err(Error::from(error)),
    }
}

impl UartSession {
    /// Lock file this session uses (also the value of [`UART_LOCK_ENV`]).
    #[must_use]
    pub fn lock_path(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    fn owns_flock(&self) -> bool {
        self._file.is_some()
    }

    /// Point a child at this session. Required before spawning anything that
    /// might pulse DTR and that knows [`try_acquire`].
    pub fn prepare_command(&self, cmd: &mut Command) {
        cmd.env(UART_LOCK_ENV, &self.path);
    }

    /// Run `cmd` and wait. The flock stays held until the child exits.
    ///
    /// Use this (or [`Self::output`]) for `esptool`, `espflash`,
    /// `cargo espflash`, nested `cargo xtask`, and any other UART-touching
    /// subprocess. Do not `Command::spawn` and drop this session first.
    pub fn status(&self, cmd: &mut Command) -> Result<ExitStatus, Error> {
        self.prepare_command(cmd);
        cmd.status().map_err(Error::from)
    }

    /// Like [`Self::status`], but captures stdout/stderr.
    pub fn output(&self, cmd: &mut Command) -> Result<Output, Error> {
        self.prepare_command(cmd);
        cmd.output().map_err(Error::from)
    }
}

fn lock_path(lock_dir: &Path, port: &str) -> PathBuf {
    lock_dir.join(format!("sticky-rs-xtask-uart-{}.lock", lock_stem(port)))
}

fn lock_stem(port: &str) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = fs::metadata(port) {
            return format!("{}-{}", meta.dev(), meta.ino());
        }
    }
    let digest = Sha256::digest(port.as_bytes());
    format!("{digest:x}")
}

fn write_holder(file: &File, command: &str) -> io::Result<()> {
    file.set_len(0)?;
    let mut file = file;
    writeln!(file, "pid={}", std::process::id())?;
    writeln!(file, "command={}", command.trim())?;
    file.sync_all()
}

fn busy_from(path: &Path) -> Error {
    let text = fs::read_to_string(path).unwrap_or_default();
    let (pid, command) = parse_holder(&text);
    Error::UartBusy { pid, command }
}

fn can_join(lock_path: &Path, inherit: Option<&Path>) -> bool {
    let Some(inherit) = inherit else {
        return false;
    };
    if inherit != lock_path {
        return false;
    }
    let text = fs::read_to_string(lock_path).unwrap_or_default();
    let Some(pid) = parse_holder(&text).0 else {
        return false;
    };
    is_self_or_ancestor(pid)
}

fn is_self_or_ancestor(pid: u32) -> bool {
    let mut current = std::process::id();
    for _ in 0..64 {
        if current == pid {
            return true;
        }
        match ppid_of(current) {
            Some(0 | 1) | None => return false,
            Some(next) if next == current => return false,
            Some(next) => current = next,
        }
    }
    false
}

fn ppid_of(pid: u32) -> Option<u32> {
    let text = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

fn parse_holder(text: &str) -> (Option<u32>, Option<String>) {
    let mut pid = None;
    let mut command = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("pid=") {
            pid = rest.trim().parse().ok();
        }
        if let Some(rest) = line.strip_prefix("command=") {
            let value = rest.trim();
            if !value.is_empty() {
                command = Some(value.to_string());
            }
        }
    }
    (pid, command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn port_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"").unwrap();
        path
    }

    #[test]
    fn second_acquire_on_the_same_inode_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = dir.path().join("locks");
        let port = port_file(dir.path(), "uart");
        let port = port.to_str().unwrap();
        let first = try_acquire_in(&lock_dir, port, "restore-factory-firmware").unwrap();
        let err = try_acquire_in(&lock_dir, port, "detect-connected --probe").unwrap_err();
        match err {
            Error::UartBusy {
                pid: Some(pid),
                command: Some(command),
            } => {
                assert_eq!(pid, std::process::id());
                assert_eq!(command, "restore-factory-firmware");
            }
            other => panic!("expected UartBusy with holder, got {other:?}"),
        }
        drop(first);
        let _released = try_acquire_in(&lock_dir, port, "confirm-factory-firmware").unwrap();
    }

    #[test]
    fn symlink_and_target_share_a_session() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = dir.path().join("locks");
        let target = port_file(dir.path(), "tty");
        let link = dir.path().join("by-id");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let _first = try_acquire_in(
            &lock_dir,
            target.to_str().unwrap(),
            "backup-factory-firmware",
        )
        .unwrap();
        let err = try_acquire_in(
            &lock_dir,
            link.to_str().unwrap(),
            "restore-factory-firmware",
        )
        .unwrap_err();
        assert!(matches!(err, Error::UartBusy { .. }));
    }

    #[test]
    fn distinct_inodes_do_not_contend() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = dir.path().join("locks");
        let a = port_file(dir.path(), "a");
        let b = port_file(dir.path(), "b");
        let _first =
            try_acquire_in(&lock_dir, a.to_str().unwrap(), "backup-factory-firmware").unwrap();
        let _second =
            try_acquire_in(&lock_dir, b.to_str().unwrap(), "backup-factory-firmware").unwrap();
    }

    #[test]
    fn parse_holder_reads_pid_and_command() {
        assert_eq!(
            parse_holder("pid=42\ncommand=restore-factory-firmware\n"),
            (Some(42), Some("restore-factory-firmware".into()))
        );
        assert_eq!(parse_holder(""), (None, None));
    }

    #[test]
    fn nested_acquire_joins_when_inherit_path_matches() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = dir.path().join("locks");
        let port = port_file(dir.path(), "uart");
        let port = port.to_str().unwrap();
        let parent = try_acquire_in(&lock_dir, port, "backup-factory-firmware").unwrap();
        let child = acquire(
            &lock_dir,
            port,
            "detect-connected --probe",
            Some(parent.lock_path()),
        )
        .unwrap();
        assert!(!child.owns_flock(), "child joins; parent keeps the flock");
        drop(child);
        let err = try_acquire_in(&lock_dir, port, "restore-factory-firmware").unwrap_err();
        assert!(matches!(err, Error::UartBusy { .. }));
    }

    #[test]
    fn lock_stays_held_while_a_child_process_runs() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = dir.path().join("locks");
        let port = port_file(dir.path(), "uart");
        let port_str = port.to_str().unwrap().to_string();
        let session = try_acquire_in(&lock_dir, &port_str, "backup-factory-firmware").unwrap();
        let (started, wait_started) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut cmd = Command::new("sleep");
            cmd.arg("0.4");
            started.send(()).unwrap();
            session.status(&mut cmd).unwrap()
        });
        wait_started.recv().unwrap();
        thread::sleep(Duration::from_millis(50));
        let err = try_acquire_in(&lock_dir, &port_str, "detect-connected --probe").unwrap_err();
        assert!(matches!(err, Error::UartBusy { .. }));
        assert!(handle.join().unwrap().success());
        let _released = try_acquire_in(&lock_dir, &port_str, "confirm-factory-firmware").unwrap();
    }
}
