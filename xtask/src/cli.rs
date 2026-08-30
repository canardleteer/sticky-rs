//! Clap derive CLI for factory-firmware xtask.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use sticky_host::{
    backup_import, backup_live, build_fw, confirm_live, detect_connected, diff_learn_uart,
    flash_app, learn_uart, load_manifest, monitor, refuse_if_legacy_backups_at_repo_root, restore,
    BackupRequest, BuildFwArgs, Error, FirmwareImage, Layout, LearnUartArgs, MonitorOptions,
    SnapshotKind, FLASH_SIZE,
};

/// Sticky factory-firmware originals (gitignored `developer-data/backups/original/{serial}/`).
///
/// Live commands need `ESPFLASH_PORT` or exactly one Sticky CH343. Agents
/// must not invoke it against hardware unless a human explicitly asked.
#[derive(Debug, Parser)]
#[command(name = "xtask", version, about, long_about = LONG_ABOUT)]
pub struct Cli {
    /// Subcommand.
    #[command(subcommand)]
    pub command: Command,
}

const LONG_ABOUT: &str = "\
detect-connected lists Sticky CH343 UART nodes (no DTR). --all-devices includes \
other USB-serial adapters. --probe opens UART (DTR reset) for stock \
serial_number and board-info. backup, confirm, restore, flash-app, learn-uart, \
and monitor share that same QinHeng port pick. Capture each Sticky under \
developer-data/backups/original/<factory-serial>/ (known factory, write-once) or \
developer-data/backups/captures/<unit-id>/<slug>/ (named snapshot of what is on the chip). \
Both are gitignored. confirm measures drift against the original, or \
--capture SLUG. restore write-bins that unit's original or --capture (never \
erase). Live UART commands take an exclusive flock so a second xtask cannot \
reset the chip during a dump, restore, etc. Subprocesses that might pulse DTR \
must run under that same session (UartSession::status). build-fw is host-only: \
cargo +esp build plus espflash save-image into workspace target/ (no UART). \
flash-app write-bins a custom image into factory app0 only (requires a matching \
original or capture, --yes; never the espflash 'flash' subcommand). \
learn-uart vets UART heartbeats and optional human steps, then writes YAML \
under developer-data/backups/original/<factory-serial>/learn-uart/ (gitignored; records \
factory serial, not MAC). A completed session also copies learn-uart-latest.yaml \
(trust that file only when complete: true — a crash never publishes it). \
Interactive sessions name steps by the action you \
perform, state expected duration, ask if you can stay the whole time, and if \
we don't see a response ask whether you tried (with a retry). \
learn-uart-only STEP… runs the same session but only the named groups \
(touch, buttons, vbus, imu, sd) and skips the rest of the briefing. \
diff-learn-uart compares two reports on the host (serials redacted unless --show-serials). \
monitor reads UART0 at 115200 via USB CDC (no ACM TTY, so no EN pulse). \
Optional --acm-tty opens the TTY anyway (POWERON). Optional --for SECS \
and --lines N stop with success; --output FILE tees the stream (--quiet \
writes the file only).";

const LEARN_UART_ABOUT: &str = "\
UART heartbeat vet plus skippable human steps. YAML is written under \
developer-data/backups/original/<factory-serial>/learn-uart/ (gitignored). The report records \
that unit's factory serial_number so later units can be compared. It does not \
record MAC or CH343 USB serial. Requires a matching original (backup first). \
QinHeng CH343, UART session lock, no DTR/RTS while listening. Optional --image \
flashes app0 first in the same session. Optional --report FILE copies the YAML \
elsewhere as well. A session that finishes without aborting copies \
learn-uart-latest.yaml; trust that alias only when complete: true.

Human steps are named by the action you perform (press a button, unplug USB-C, \
tilt the board, touch the glass). The session states expected duration, \
asks whether you can be present for the full time, lists steps that need you \
to stay with the board, and if we don't see a response asks if you tried \
(with a retry). It asks up front about two hands, USB cable slack (tilt), whether \
you can see the terminal while holding the board, noisy behavior (USB-C unplug \
disconnects this computer until you plug the same cable back in), and a working \
MicroSD — but only the questions that apply to the remaining steps. After each button it asks what you would call that key; blank is stored \
as unknown, with an optional short note if you are still unsure. \
Pass --only STEP (or learn-uart-only) to retest one group without the rest.";

const LEARN_UART_ONLY_ABOUT: &str = "\
Same UART session as learn-uart, but only the named step groups. Others are \
recorded as skipped (not_in_only) and their briefing questions are omitted. \
STEP is one or more of: touch, buttons, vbus, imu, sd (positional and/or --only). \
`--only touch` on this subcommand is the same as a positional `touch`. Example: \
cargo xtask learn-uart-only touch --image target/xtensa-esp32s3-none-elf/release-fw/simple-debug.bin --yes --restore-app0";

const DIFF_LEARN_UART_ABOUT: &str = "\
Host-only comparison of two learn-uart YAML reports. Arguments are factory \
serials (latest YAML under that original) or file paths. Default output uses \
UNIT_A / UNIT_B so a paste does not leak serials; pass --show-serials locally. \
Does not open a UART.";

const BUILD_FW_ABOUT: &str = "\
Host-only. Builds one firmware workspace member with cargo +esp \
(--profile release-fw --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc \
--locked) and packs a flash payload with espflash save-image (no port). ELF \
and .bin land under workspace target/xtensa-esp32s3-none-elf/release-fw/. \
IMAGE is simple-debug or embassy-debug. Features: operator (simple-debug), \
epd (embassy-debug). \
Needs the esp toolchain (source the script `espup` printed, often \
`$HOME/export-esp.sh`) and espflash on PATH. Does not \
open a UART and does not flash.";

/// Factory-firmware operations.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List Sticky CH343 UART nodes (QinHeng `1a86:55d3`). No DTR unless `--probe`.
    DetectConnected(DetectArgs),
    /// Capture this unit: known factory → `original/`, otherwise a named capture.
    #[command(visible_alias = "backup-firmware")]
    BackupFactoryFirmware(BackupArgs),
    /// Compare live flash to the matching original (or `--capture`).
    ConfirmFactoryFirmware(ConfirmArgs),
    /// write-bin this unit's original or `--capture` (full image or --part). Requires --yes.
    RestoreFactoryFirmware(RestoreArgs),
    /// write-bin a custom image into factory `app0` only. Requires `--yes` and a snapshot.
    FlashApp(FlashAppArgs),
    /// UART heartbeat vet plus skippable human steps; YAML under `developer-data/backups/original/<serial>/learn-uart/`.
    #[command(long_about = LEARN_UART_ABOUT)]
    LearnUart(LearnUartCliArgs),
    /// Same as `learn-uart --only …` (one or more step groups).
    #[command(name = "learn-uart-only", long_about = LEARN_UART_ONLY_ABOUT)]
    LearnUartOnly(LearnUartOnlyCliArgs),
    /// Compare two learn-uart reports (host-only; serials redacted unless `--show-serials`).
    #[command(long_about = DIFF_LEARN_UART_ABOUT)]
    DiffLearnUart(DiffLearnUartCliArgs),
    /// Xtensa `cargo +esp` build plus host-only `save-image` (no UART).
    #[command(long_about = BUILD_FW_ABOUT)]
    BuildFw(BuildFwCliArgs),
    /// Read UART0 at 115200 via USB CDC (no ACM TTY / EN pulse).
    Monitor(MonitorArgs),
}

/// USB inventory (default) or UART/chip probe.
#[derive(Debug, Args)]
pub struct DetectArgs {
    /// Open the UART: stock `serial_number`, then flasher board-info.
    ///
    /// Resets the chip (DTR/RTS). Needs exactly one Sticky CH343, or `--port` /
    /// `ESPFLASH_PORT`. Refuses if another xtask already holds the UART session.
    #[arg(long)]
    pub probe: bool,
    /// Serial device. Also `ESPFLASH_PORT`. Used only with `--probe`.
    #[arg(long, env = "ESPFLASH_PORT")]
    pub port: Option<String>,
    /// Also list USB-serial nodes that are not QinHeng CH343.
    #[arg(long)]
    pub all_devices: bool,
}

/// Backup flags.
#[derive(Debug, Args)]
pub struct BackupArgs {
    /// Serial device. Also `ESPFLASH_PORT`. Optional if exactly one Sticky CH343 is present.
    #[arg(long, env = "ESPFLASH_PORT")]
    pub port: Option<String>,
    /// Host-only: copy an existing 32 MiB dump tree (YAML or JSON manifest).
    #[arg(long, value_name = "DIR")]
    pub import: Option<PathBuf>,
    /// Named capture slug (`developer-data/backups/captures/<unit-id>/<slug>/`).
    #[arg(long)]
    pub name: Option<String>,
    /// Store an uncertain-stock dump under `original/` (does not add to the catalog).
    #[arg(long)]
    pub as_original: bool,
}

/// Live confirm.
#[derive(Debug, Args)]
pub struct ConfirmArgs {
    /// Serial device. Also `ESPFLASH_PORT`. Optional if exactly one Sticky CH343 is present.
    #[arg(long, env = "ESPFLASH_PORT")]
    pub port: Option<String>,
    /// Compare against this capture slug instead of `original/`.
    #[arg(long, value_name = "SLUG")]
    pub capture: Option<String>,
}

/// UART0 listen (USB CDC by default).
#[derive(Debug, Args)]
pub struct MonitorArgs {
    /// Serial device. Also `ESPFLASH_PORT`. Optional if exactly one Sticky CH343 is present.
    #[arg(long, env = "ESPFLASH_PORT")]
    pub port: Option<String>,
    /// Stop after this many seconds (from port open). Omit to listen until Ctrl-C or `--lines`.
    #[arg(long = "for", value_name = "SECS", value_parser = clap::value_parser!(u64).range(1..))]
    pub for_secs: Option<u64>,
    /// Stop after this many newline-terminated device lines.
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    pub lines: Option<u64>,
    /// Write a copy of the UART stream to FILE (still prints unless `--quiet`).
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,
    /// Do not print to stdout. Requires `--output`.
    #[arg(long, visible_alias = "output-only", requires = "output")]
    pub quiet: bool,
    /// Open the ACM TTY. Linux cdc-acm asserts DTR+RTS on open (EN pulse / POWERON).
    #[arg(long)]
    pub acm_tty: bool,
}

/// Restore flags.
#[derive(Debug, Args)]
pub struct RestoreArgs {
    /// Serial device. Also `ESPFLASH_PORT`. Optional if exactly one Sticky CH343 is present.
    #[arg(long, env = "ESPFLASH_PORT")]
    pub port: Option<String>,
    /// Required. Restore writes flash.
    #[arg(long)]
    pub yes: bool,
    /// Restore one partition (`nvs`, `app0`, …) instead of the full 32 MiB image.
    #[arg(long)]
    pub part: Option<String>,
    /// Restore this capture slug instead of `original/`.
    #[arg(long, value_name = "SLUG")]
    pub capture: Option<String>,
}

/// App0-only custom image write.
#[derive(Debug, Args)]
pub struct FlashAppArgs {
    /// Serial device. Also `ESPFLASH_PORT`. Optional if exactly one Sticky CH343 is present.
    #[arg(long, env = "ESPFLASH_PORT")]
    pub port: Option<String>,
    /// Application flash payload from `cargo espflash save-image` (not an ELF).
    #[arg(long, value_name = "FILE")]
    pub image: PathBuf,
    /// Required. flash-app writes `app0`.
    #[arg(long)]
    pub yes: bool,
    /// Allow `app0` write when the bound snapshot table is unknown or mismatched.
    #[arg(long)]
    pub allow_unknown_layout: bool,
    /// Use this capture slug instead of the preferred original / unique capture.
    #[arg(long, value_name = "SLUG")]
    pub capture: Option<String>,
}

/// Flags shared by `learn-uart` and `learn-uart-only`.
#[derive(Debug, Args)]
pub struct LearnUartSessionArgs {
    /// Serial device. Also `ESPFLASH_PORT`. Optional if exactly one Sticky CH343 is present.
    #[arg(long, env = "ESPFLASH_PORT")]
    pub port: Option<String>,
    /// Extra YAML copy. Canonical file is always `developer-data/backups/original/<serial>/learn-uart/<stamp>.yaml`.
    #[arg(long, value_name = "FILE")]
    pub report: Option<PathBuf>,
    /// Skip a step: `buttons`, `vbus`, `imu`, `sd_detect`, `touch`.
    #[arg(long = "skip", value_name = "STEP")]
    pub skip: Vec<String>,
    /// Override the per-step UART wait (seconds).
    #[arg(long, value_name = "SECS")]
    pub step_timeout_secs: Option<u32>,
    /// Optional: `flash-app` this image first (same UART session). Needs `--yes`.
    #[arg(long, value_name = "FILE")]
    pub image: Option<PathBuf>,
    /// Required with `--image` or `--restore-app0`. One flag covers both writes
    /// (repeating `--yes` is allowed).
    #[arg(long, action = clap::ArgAction::SetTrue, overrides_with = "yes")]
    pub yes: bool,
    /// After the report, restore factory `app0`. Needs `--yes` (once is enough).
    #[arg(long)]
    pub restore_app0: bool,
}

/// UART learn / operator session.
#[derive(Debug, Args)]
pub struct LearnUartCliArgs {
    #[command(flatten)]
    pub session: LearnUartSessionArgs,
    /// Only these step groups; others are skipped (`touch`, `buttons`, `vbus`, `imu`, `sd`).
    #[arg(long = "only", value_name = "STEP", conflicts_with = "unattended_only")]
    pub only: Vec<String>,
    /// Heartbeats and boot lines only; no operator prompts.
    #[arg(long)]
    pub unattended_only: bool,
}

/// Focused retest: same session as `learn-uart`, named groups only.
#[derive(Debug, Args)]
pub struct LearnUartOnlyCliArgs {
    /// Step groups: `touch`, `buttons`, `vbus`, `imu`, `sd`.
    #[arg(value_name = "STEP")]
    pub steps: Vec<String>,
    /// Same as a positional STEP (`--only touch` is accepted).
    #[arg(long = "only", value_name = "STEP")]
    pub only: Vec<String>,
    #[command(flatten)]
    pub session: LearnUartSessionArgs,
}

/// Host-only firmware image name (`firmware/<name>`).
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum FirmwareImageArg {
    /// Blocking `esp-hal` proof-of-life.
    #[value(name = "simple-debug")]
    SimpleDebug,
    /// Embassy event logger.
    #[value(name = "embassy-debug")]
    EmbassyDebug,
}

impl From<FirmwareImageArg> for FirmwareImage {
    fn from(value: FirmwareImageArg) -> Self {
        match value {
            FirmwareImageArg::SimpleDebug => Self::SimpleDebug,
            FirmwareImageArg::EmbassyDebug => Self::EmbassyDebug,
        }
    }
}

/// Host-only Xtensa build + `save-image`.
#[derive(Debug, Args)]
pub struct BuildFwCliArgs {
    /// `simple-debug` or `embassy-debug`.
    pub image: FirmwareImageArg,
    /// Cargo features on that package (`operator`, `epd`).
    #[arg(long)]
    pub features: Vec<String>,
    /// Build the debug profile instead of `--profile release-fw`.
    #[arg(long)]
    pub debug: bool,
}

/// Host-only learn-uart YAML comparison.
#[derive(Debug, Args)]
pub struct DiffLearnUartCliArgs {
    /// Left factory serial or YAML path.
    pub left: String,
    /// Right factory serial or YAML path.
    pub right: String,
    /// Print factory serials (default: UNIT_A / UNIT_B).
    #[arg(long)]
    pub show_serials: bool,
}

impl Cli {
    /// Parse argv and run.
    pub fn exec() -> ExitCode {
        match Self::parse().run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        }
    }

    /// Run against the repo's `developer-data/backups/` using the in-process `espflash` library.
    pub fn run(self) -> Result<(), Error> {
        let repo = repo_root();
        refuse_if_legacy_backups_at_repo_root(&repo)?;
        let layout = Layout::from_repo_root(repo);
        match self.command {
            Command::DetectConnected(args) => {
                detect_connected(args.probe, args.port, args.all_devices)
            }
            Command::BackupFactoryFirmware(args) => run_backup(&layout, args),
            Command::ConfirmFactoryFirmware(args) => {
                let report = confirm_live(&layout, args.port, args.capture.as_deref())?;
                let drifted: Vec<_> = report
                    .regions
                    .iter()
                    .filter(|r| !r.matches)
                    .map(|r| r.name.as_str())
                    .collect();
                if drifted.is_empty() {
                    println!("confirm: {} matches original", report.factory_serial);
                } else {
                    println!(
                        "confirm: {} drifted in {}",
                        report.factory_serial,
                        drifted.join(", ")
                    );
                }
                Ok(())
            }
            Command::RestoreFactoryFirmware(args) => {
                restore(
                    &layout,
                    args.port,
                    args.yes,
                    args.part.as_deref(),
                    args.capture.as_deref(),
                )?;
                println!("restore write-bin finished");
                Ok(())
            }
            Command::FlashApp(args) => {
                flash_app(
                    &layout,
                    args.port,
                    &args.image,
                    args.yes,
                    args.allow_unknown_layout,
                    args.capture.as_deref(),
                )?;
                println!("flash-app write-bin app0 finished");
                Ok(())
            }
            Command::LearnUart(args) => {
                run_learn_uart(&layout, args.session, args.only, args.unattended_only)
            }
            Command::LearnUartOnly(args) => {
                let only = merge_only_steps(args.steps, args.only);
                if only.is_empty() {
                    return Err(Error::Device(
                        "learn-uart-only needs a step: touch, buttons, vbus, imu, sd (positional or --only)"
                            .into(),
                    ));
                }
                run_learn_uart(&layout, args.session, only, false)
            }
            Command::DiffLearnUart(args) => {
                diff_learn_uart(&layout, &args.left, &args.right, args.show_serials)
            }
            Command::BuildFw(args) => {
                let out = build_fw(
                    &repo_root(),
                    &BuildFwArgs {
                        image: args.image.into(),
                        features: args.features,
                        release: !args.debug,
                    },
                )?;
                println!("elf {}", out.elf.display());
                println!("bin {}", out.bin.display());
                Ok(())
            }
            Command::Monitor(args) => monitor(
                args.port,
                &MonitorOptions {
                    for_secs: args.for_secs,
                    lines: args.lines,
                    output: args.output,
                    quiet: args.quiet,
                    acm_tty: args.acm_tty,
                },
            ),
        }
    }
}

fn run_backup(layout: &Layout, args: BackupArgs) -> Result<(), Error> {
    let request = BackupRequest {
        name: args.name,
        as_original: args.as_original,
    };
    let dest = if let Some(source) = args.import {
        backup_import(layout, &source, &request, prompt_snapshot_name)?
    } else {
        backup_live(layout, args.port, &request, prompt_snapshot_name)?
    };
    let manifest = load_manifest(&dest)?;
    let kind = match manifest.kind {
        SnapshotKind::Original => "original",
        SnapshotKind::Capture => "capture",
    };
    println!(
        "wrote {kind} {} serial={} sha256={} ({} bytes)",
        dest.display(),
        manifest.factory_serial,
        manifest.dump_sha256,
        FLASH_SIZE,
    );
    Ok(())
}

fn prompt_snapshot_name(evidence: &str) -> Result<Option<String>, Error> {
    use std::io::{self, IsTerminal, Write};

    eprintln!("{evidence}");
    if !io::stdin().is_terminal() {
        return Ok(None);
    }
    eprint!("name this snapshot (directory-safe): ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let name = line.trim();
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name.to_string()))
    }
}

/// Repository root (parent of the `xtask` package).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace")
        .to_path_buf()
}

fn merge_only_steps(positionals: Vec<String>, only: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for step in positionals.into_iter().chain(only) {
        if !out.iter().any(|have| have == &step) {
            out.push(step);
        }
    }
    out
}

fn run_learn_uart(
    layout: &Layout,
    session: LearnUartSessionArgs,
    only: Vec<String>,
    unattended_only: bool,
) -> Result<(), Error> {
    learn_uart(
        layout,
        LearnUartArgs {
            port: session.port,
            report: session.report,
            skip: session.skip,
            only,
            step_timeout_secs: session.step_timeout_secs,
            image: session.image,
            yes: session.yes,
            restore_app0: session.restore_app0,
            unattended_only,
        },
    )
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    #[test]
    fn clap_debug_assert() {
        Cli::command().debug_assert();
    }

    #[test]
    fn backup_accepts_name_and_as_original() {
        use clap::Parser;

        let cli = Cli::try_parse_from([
            "xtask",
            "backup-firmware",
            "--name",
            "after-flash",
            "--as-original",
        ])
        .expect("backup-firmware alias");
        match cli.command {
            super::Command::BackupFactoryFirmware(args) => {
                assert_eq!(args.name.as_deref(), Some("after-flash"));
                assert!(args.as_original);
            }
            other => panic!("expected BackupFactoryFirmware, got {other:?}"),
        }
    }

    #[test]
    fn flash_app_accepts_layout_override_and_capture() {
        use clap::Parser;

        let cli = Cli::try_parse_from([
            "xtask",
            "flash-app",
            "--image",
            "app.bin",
            "--yes",
            "--allow-unknown-layout",
            "--capture",
            "after-flash",
        ])
        .expect("flash-app overrides");
        match cli.command {
            super::Command::FlashApp(args) => {
                assert!(args.allow_unknown_layout);
                assert_eq!(args.capture.as_deref(), Some("after-flash"));
            }
            other => panic!("expected FlashApp, got {other:?}"),
        }
    }

    #[test]
    fn learn_uart_accepts_repeated_yes() {
        use clap::Parser;

        let cli = Cli::try_parse_from([
            "xtask",
            "learn-uart",
            "--image",
            "target/xtensa-esp32s3-none-elf/release-fw/simple-debug.bin",
            "--yes",
            "--restore-app0",
            "--yes",
        ])
        .expect("repeating --yes is one confirmation, not two flags");
        match cli.command {
            super::Command::LearnUart(args) => {
                assert!(args.session.yes);
                assert!(args.session.restore_app0);
            }
            other => panic!("expected LearnUart, got {other:?}"),
        }
    }

    #[test]
    fn learn_uart_only_takes_positional_steps() {
        use clap::Parser;

        let cli = Cli::try_parse_from([
            "xtask",
            "learn-uart-only",
            "touch",
            "--image",
            "target/xtensa-esp32s3-none-elf/release-fw/simple-debug.bin",
            "--yes",
            "--restore-app0",
        ])
        .expect("learn-uart-only touch");
        match cli.command {
            super::Command::LearnUartOnly(args) => {
                assert_eq!(args.steps, ["touch"]);
                assert!(args.session.yes);
                assert!(args.session.restore_app0);
            }
            other => panic!("expected LearnUartOnly, got {other:?}"),
        }
    }

    #[test]
    fn learn_uart_only_accepts_redundant_only_flag() {
        use clap::Parser;

        let cli = Cli::try_parse_from([
            "xtask",
            "learn-uart-only",
            "touch",
            "--image",
            "target/xtensa-esp32s3-none-elf/release-fw/simple-debug.bin",
            "--yes",
            "--restore-app0",
            "--only",
            "touch",
        ])
        .expect("positional touch plus --only touch");
        match cli.command {
            super::Command::LearnUartOnly(args) => {
                assert_eq!(
                    super::merge_only_steps(args.steps, args.only),
                    vec!["touch".to_string()]
                );
                assert!(args.session.yes);
            }
            other => panic!("expected LearnUartOnly, got {other:?}"),
        }
    }

    #[test]
    fn build_fw_parses_image_and_features() {
        use clap::Parser;

        let cli = Cli::try_parse_from([
            "xtask",
            "build-fw",
            "simple-debug",
            "--features",
            "operator",
        ])
        .expect("build-fw simple-debug --features operator");
        match cli.command {
            super::Command::BuildFw(args) => {
                assert!(!args.debug);
                assert_eq!(args.features, ["operator"]);
                match args.image {
                    super::FirmwareImageArg::SimpleDebug => {}
                    other => panic!("expected SimpleDebug, got {other:?}"),
                }
            }
            other => panic!("expected BuildFw, got {other:?}"),
        }
    }

    #[test]
    fn monitor_accepts_for_lines_and_quiet_output() {
        use clap::Parser;

        let cli = Cli::try_parse_from([
            "xtask", "monitor", "--for", "12", "--lines", "40", "--output", "uart.log", "--quiet",
        ])
        .expect("monitor listen budget");
        match cli.command {
            super::Command::Monitor(args) => {
                assert_eq!(args.for_secs, Some(12));
                assert_eq!(args.lines, Some(40));
                assert_eq!(
                    args.output.as_deref(),
                    Some(std::path::Path::new("uart.log"))
                );
                assert!(args.quiet);
            }
            other => panic!("expected Monitor, got {other:?}"),
        }
    }

    #[test]
    fn monitor_output_only_is_quiet() {
        use clap::Parser;

        let cli =
            Cli::try_parse_from(["xtask", "monitor", "--output", "uart.log", "--output-only"])
                .expect("output-only alias");
        match cli.command {
            super::Command::Monitor(args) => assert!(args.quiet),
            other => panic!("expected Monitor, got {other:?}"),
        }
    }

    #[test]
    fn monitor_quiet_requires_output() {
        use clap::Parser;

        assert!(Cli::try_parse_from(["xtask", "monitor", "--quiet"]).is_err());
    }

    #[test]
    fn monitor_rejects_a_zero_second_listen() {
        use clap::Parser;

        assert!(Cli::try_parse_from(["xtask", "monitor", "--for", "0"]).is_err());
    }
}
