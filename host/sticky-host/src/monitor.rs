//! Read UART0 at 115200 without pulsing DTR/RTS (no ROM stub, no EN reset).

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serialport::FlowControl;

use crate::Error;

/// Factory and bring-up monitor baud.
pub const MONITOR_BAUD: u32 = 115_200;

/// How long to listen, how much to keep, and where to write it.
#[derive(Debug, Clone, Default)]
pub struct MonitorOptions {
    /// Stop this many seconds after the port opens. `None` means no time cap.
    pub for_secs: Option<u64>,
    /// Stop after this many newline-terminated device lines. `None` means no line cap.
    pub lines: Option<u64>,
    /// Optional copy of the UART stream (tee unless [`Self::quiet`]).
    pub output: Option<PathBuf>,
    /// Write [`Self::output`] only; do not print to stdout.
    pub quiet: bool,
    /// Open the ACM TTY instead of claiming USB CDC. Linux `cdc-acm` asserts
    /// DTR+RTS on that open, which pulses EN (`POWERON`) on this board.
    pub acm_tty: bool,
}

/// Copy the CH343 UART to stdout (and optionally a file) until interrupted
/// or a listen budget is exhausted.
///
/// Default listen claims the CH343 over USB CDC so Linux `cdc-acm` never
/// activates the TTY (that open asserts DTR+RTS and pulses EN).
/// [`MonitorOptions::acm_tty`] is the old ACM path. [`crate::monitor`] holds
/// [`crate::uart_lock::UartSession`] for the whole read so backup, restore,
/// etc. cannot reset the chip while this session is open. Ctrl-C sets a
/// flag so [`crate::cdc_listen::CdcListen`] Drop reattaches `cdc-acm`
/// (a default SIGINT would leave the TTY gone until the cable is
/// replugged). The flock still drops with the process.
/// [`MonitorOptions::for_secs`] or [`MonitorOptions::lines`] end with
/// [`Ok`] instead. A USB unplug after the reader is open (`UnexpectedEof`,
/// `BrokenPipe`, `NotConnected`, `ConnectionReset`) is the same clean
/// end, not [`Error::Device`].
pub fn monitor(port: &str, options: &MonitorOptions) -> Result<(), Error> {
    crate::detect::require_sticky_ch343(port)?;
    {
        let mut reader: Box<dyn Read> = if options.acm_tty {
            Box::new(open_acm_tty(port)?)
        } else {
            Box::new(crate::cdc_listen::CdcListen::open(port)?)
        };

        let mut file = match &options.output {
            Some(path) => Some(File::create(path).map_err(|error| {
                Error::Device(format!("monitor --output {}: {error}", path.display()))
            })?),
            None => None,
        };
        let mut stdout = io::stdout();
        let mut budget = ListenBudget::new(options.for_secs, options.lines);
        let mut buf = [0u8; 4096];
        loop {
            if budget.is_exhausted() || crate::cdc_listen::interrupt_requested() {
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => {
                    emit(&mut stdout, file.as_mut(), options.quiet, &buf[..n])?;
                    budget.note_bytes(&buf[..n]);
                }
                Err(error) if error.kind() == io::ErrorKind::TimedOut => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => break,
                Err(error) if listen_read_is_clean_end(error.kind()) => break,
                Err(error) => return Err(Error::Device(format!("UART read failed: {error}"))),
            }
        }
    }
    if !options.acm_tty && !crate::cdc_listen::wait_for_kernel_tty(Duration::from_secs(2)) {
        log::warn!("cdc-acm did not reappear; unplug/replug if flash-app cannot see the CH343");
    }
    Ok(())
}

fn open_acm_tty(port: &str) -> Result<Box<dyn serialport::SerialPort>, Error> {
    log::warn!("monitor {port} via ACM TTY (cdc-acm will pulse DTR+RTS / EN)");
    let mut serial = serialport::new(port, MONITOR_BAUD)
        .flow_control(FlowControl::None)
        .timeout(Duration::from_millis(250))
        .open()
        .map_err(|error| Error::Device(format!("UART open failed: {error}")))?;
    serial
        .write_data_terminal_ready(false)
        .map_err(|error| Error::Device(error.to_string()))?;
    serial
        .write_request_to_send(false)
        .map_err(|error| Error::Device(error.to_string()))?;
    Ok(serial)
}

fn listen_read_is_clean_end(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::UnexpectedEof
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::NotConnected
            | io::ErrorKind::ConnectionReset
    )
}

fn emit(
    stdout: &mut impl Write,
    file: Option<&mut File>,
    quiet: bool,
    bytes: &[u8],
) -> Result<(), Error> {
    if !quiet {
        stdout.write_all(bytes)?;
        stdout.flush()?;
    }
    if let Some(file) = file {
        file.write_all(bytes)?;
        file.flush()?;
    }
    Ok(())
}

/// Time and line caps for a monitor session.
pub(crate) struct ListenBudget {
    deadline: Option<Instant>,
    max_lines: Option<u64>,
    lines_seen: u64,
}

impl ListenBudget {
    fn new(for_secs: Option<u64>, max_lines: Option<u64>) -> Self {
        Self {
            deadline: for_secs.map(|secs| Instant::now() + Duration::from_secs(secs)),
            max_lines,
            lines_seen: 0,
        }
    }

    fn note_bytes(&mut self, bytes: &[u8]) {
        self.lines_seen = self
            .lines_seen
            .saturating_add(bytes.iter().filter(|b| **b == b'\n').count() as u64);
    }

    fn is_exhausted(&self) -> bool {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return true;
        }
        matches!(self.max_lines, Some(max) if self.lines_seen >= max)
    }
}

#[cfg(test)]
mod tests {
    use super::{ListenBudget, MONITOR_BAUD};

    #[test]
    fn usb_unplug_kinds_are_a_clean_end() {
        use std::io::ErrorKind;
        for kind in [
            ErrorKind::UnexpectedEof,
            ErrorKind::BrokenPipe,
            ErrorKind::NotConnected,
            ErrorKind::ConnectionReset,
        ] {
            assert!(super::listen_read_is_clean_end(kind));
        }
        assert!(!super::listen_read_is_clean_end(
            ErrorKind::PermissionDenied
        ));
        assert!(!super::listen_read_is_clean_end(ErrorKind::TimedOut));
    }

    #[test]
    fn baud_is_stock_uart0() {
        assert_eq!(MONITOR_BAUD, 115_200);
    }

    #[test]
    fn a_line_cap_counts_newlines_across_chunks() {
        let mut budget = ListenBudget::new(None, Some(3));
        budget.note_bytes(b"ab\ncd");
        assert!(!budget.is_exhausted());
        budget.note_bytes(b"\nef\n");
        assert!(budget.is_exhausted());
    }

    #[test]
    fn crlf_is_one_line() {
        let mut budget = ListenBudget::new(None, Some(1));
        budget.note_bytes(b"hello\r\n");
        assert!(budget.is_exhausted());
    }

    #[test]
    fn no_caps_never_exhaust_from_bytes_alone() {
        let mut budget = ListenBudget::new(None, None);
        budget.note_bytes(b"a\nb\nc\n");
        assert!(!budget.is_exhausted());
    }

    #[test]
    fn a_zero_second_budget_is_already_exhausted() {
        let budget = ListenBudget::new(Some(0), None);
        assert!(budget.is_exhausted());
    }
}
