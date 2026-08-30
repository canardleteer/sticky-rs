//! Operator TTY input. Waits use cbreak so `s` skips without Enter.

use std::io::{self, IsTerminal};
use std::os::fd::AsFd;

use rustix::termios::{
    tcgetattr, tcsetattr, LocalModes, OptionalActions, SpecialCodeIndex, Termios,
};

use crate::learn_uart_impl::steps::{parse_timeout_reply, TimeoutReply};
use crate::Error;

/// `s` or `S`.
#[must_use]
pub fn is_skip_key(b: u8) -> bool {
    b == b's' || b == b'S'
}

struct Cbreak {
    saved: Termios,
}

impl Cbreak {
    fn enter(vmin: u8) -> Option<Self> {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            return None;
        }
        let fd = stdin.as_fd();
        let saved = tcgetattr(fd).ok()?;
        let mut now = saved.clone();
        now.local_modes.remove(LocalModes::ICANON);
        now.special_codes[SpecialCodeIndex::VMIN] = vmin;
        now.special_codes[SpecialCodeIndex::VTIME] = 0;
        tcsetattr(fd, OptionalActions::Now, &now).ok()?;
        Some(Self { saved })
    }

    fn enter_poll() -> Option<Self> {
        Self::enter(0)
    }

    fn enter_key() -> Option<Self> {
        Self::enter(1)
    }
}

impl Drop for Cbreak {
    fn drop(&mut self) {
        let stdin = io::stdin();
        let fd = stdin.as_fd();
        if let Ok(mut poll) = tcgetattr(fd) {
            poll.local_modes.remove(LocalModes::ICANON);
            poll.special_codes[SpecialCodeIndex::VMIN] = 0;
            poll.special_codes[SpecialCodeIndex::VTIME] = 0;
            let _ = tcsetattr(fd, OptionalActions::Now, &poll);
            let mut buf = [0u8; 64];
            loop {
                match rustix::io::read(fd, &mut buf) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }
        let _ = tcsetattr(fd, OptionalActions::Now, &self.saved);
    }
}

/// Hold cbreak (poll) for a timed wait so [`poll_skip`] can see a lone `s`.
pub struct SkipWatch {
    _cbreak: Option<Cbreak>,
}

impl SkipWatch {
    #[must_use]
    pub fn enter() -> Self {
        Self {
            _cbreak: Cbreak::enter_poll(),
        }
    }

    /// True when the operator pressed `s` (no Enter).
    #[must_use]
    pub fn poll_skip(&self) -> bool {
        if self._cbreak.is_none() {
            return false;
        }
        let mut buf = [0u8; 32];
        match rustix::io::read(io::stdin().as_fd(), &mut buf) {
            Ok(0) => false,
            Ok(n) => buf[..n].iter().copied().any(is_skip_key),
            Err(e) if e == rustix::io::Errno::INTR => false,
            Err(_) => false,
        }
    }
}

/// `true` = Enter, `false` = a decline key (`s`, and optionally `n`).
pub fn wait_enter_or_decline(also_n: bool) -> Result<bool, Error> {
    if let Some(_cbreak) = Cbreak::enter_key() {
        loop {
            let mut buf = [0u8; 1];
            match rustix::io::read(io::stdin().as_fd(), &mut buf) {
                Ok(0) => {}
                Ok(_) => {
                    let b = buf[0];
                    if is_skip_key(b) || (also_n && (b == b'n' || b == b'N')) {
                        return Ok(false);
                    }
                    if b == b'\n' || b == b'\r' {
                        return Ok(true);
                    }
                }
                Err(e) if e == rustix::io::Errno::INTR => {}
                Err(e) => return Err(Error::Device(format!("stdin: {e}"))),
            }
        }
    }
    let line = read_line()?;
    let t = line.trim();
    if t.is_empty() {
        return Ok(true);
    }
    if t.eq_ignore_ascii_case("s")
        || t.eq_ignore_ascii_case("skip")
        || (also_n && (t.eq_ignore_ascii_case("n") || t.eq_ignore_ascii_case("no")))
    {
        return Ok(false);
    }
    Ok(true)
}

/// Timeout follow-up: `y` / `n` / `r` (no Enter).
pub fn wait_timeout_reply() -> Result<TimeoutReply, Error> {
    if let Some(_cbreak) = Cbreak::enter_key() {
        loop {
            let mut buf = [0u8; 1];
            match rustix::io::read(io::stdin().as_fd(), &mut buf) {
                Ok(0) => {}
                Ok(_) => {
                    if let Ok(s) = std::str::from_utf8(&buf[..1]) {
                        if let Some(reply) = parse_timeout_reply(s) {
                            return Ok(reply);
                        }
                    }
                }
                Err(e) if e == rustix::io::Errno::INTR => {}
                Err(e) => return Err(Error::Device(format!("stdin: {e}"))),
            }
        }
    }
    Ok(parse_timeout_reply(&read_line()?).unwrap_or(TimeoutReply::DidNotTry))
}

/// One line of typed text (button labels). Needs Enter.
pub fn read_line() -> Result<String, Error> {
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::is_skip_key;

    #[test]
    fn s_skips_without_regard_to_case() {
        assert!(is_skip_key(b's'));
        assert!(is_skip_key(b'S'));
        assert!(!is_skip_key(b'n'));
        assert!(!is_skip_key(b'\n'));
    }
}
