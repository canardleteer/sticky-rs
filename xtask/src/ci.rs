//! Host-only CI gate. No UART, no [`sticky_host::Layout`].
//!
//! One workspace, one lockfile. Never `--workspace` on the host rustc
//! (that pulls Xtensa members). Firmware jobs use `cargo +esp -p` only.

use std::env;
use std::path::Path;
use std::process::{Command, Stdio};

use sticky_host::Error;

const ESP_TARGET: &str = "xtensa-esp32s3-none-elf";

/// Same hint as `build-fw`: source the script `espup` printed.
const ESP_TOOLCHAIN_HINT: &str = "\
need the esp toolchain (source the script `espup` printed, often \
`$HOME/export-esp.sh`)";

/// Run the full gate from the repository root. First failure wins.
pub fn run(repo_root: &Path) -> Result<(), Error> {
    step(repo_root, "cargo", &["fmt", "--check", "--all"])?;

    host_clippy_test(repo_root, &[])?;
    host_clippy_test(repo_root, &["--all-features"])?;
    host_clippy_test(repo_root, &["-p", "ssd1677-gray4", "--no-default-features"])?;

    require_cargo_esp()?;
    fw_clippy(repo_root, "simple-debug-fw", None)?;
    fw_clippy(repo_root, "simple-debug-fw", Some("operator"))?;
    fw_clippy(repo_root, "embassy-debug-fw", None)?;
    fw_clippy(repo_root, "embassy-debug-fw", Some("mic"))?;
    fw_clippy(repo_root, "embassy-debug-fw", Some("radio"))?;
    fw_clippy(repo_root, "embassy-debug-fw", Some("spi20"))?;
    fw_clippy(repo_root, "embassy-debug-fw", Some("sd"))?;
    fw_clippy(repo_root, "embassy-debug-fw", Some("charge"))?;

    require_on_path("rumdl", "cargo install rumdl")?;
    step(repo_root, "rumdl", &["check"])?;

    require_on_path("cargo-machete", "cargo install cargo-machete")?;
    // `cargo machete` from inside `cargo xtask` forwards `machete` as a search
    // path (cargo plugin argv). The binary takes no subcommand name.
    step(repo_root, "cargo-machete", &[])?;

    require_on_path("cargo-audit", "cargo install cargo-audit")?;
    step(repo_root, "cargo", &["audit"])?;

    Ok(())
}

fn host_clippy_test(repo_root: &Path, extra: &[&str]) -> Result<(), Error> {
    let mut clippy = vec!["clippy", "--locked", "--all-targets"];
    clippy.extend_from_slice(extra);
    clippy.extend_from_slice(&["--", "-D", "warnings"]);
    step(repo_root, "cargo", &clippy)?;

    let mut test = vec!["test", "--locked"];
    test.extend_from_slice(extra);
    step(repo_root, "cargo", &test)
}

fn fw_clippy(repo_root: &Path, package: &str, feature: Option<&str>) -> Result<(), Error> {
    // Bins only. `--all-targets` compiles the implicit test harness, which
    // needs crate `test`; `-Zbuild-std=core,alloc` does not provide it.
    let mut args = vec![
        "+esp",
        "clippy",
        "--locked",
        "--bins",
        "-p",
        package,
        "--target",
        ESP_TARGET,
        "-Zbuild-std=core,alloc",
    ];
    if let Some(feature) = feature {
        args.push("--features");
        args.push(feature);
    }
    args.extend_from_slice(&["--", "-D", "warnings"]);
    step(repo_root, "cargo", &args)
}

fn require_cargo_esp() -> Result<(), Error> {
    let status = Command::new("cargo")
        .args(["+esp", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(Error::Device(ESP_TOOLCHAIN_HINT.into())),
        Err(error) => Err(Error::Device(format!(
            "failed to spawn cargo +esp: {error}; {ESP_TOOLCHAIN_HINT}"
        ))),
    }
}

fn require_on_path(bin: &str, install: &str) -> Result<(), Error> {
    if executable_on_path(bin) {
        return Ok(());
    }
    eprintln!("ci: `{bin}` is not on PATH");
    eprintln!("install: {install}");
    Err(Error::Device(format!(
        "{bin} is not on PATH; install with `{install}`"
    )))
}

fn executable_on_path(name: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|dir| dir.join(name).is_file())
}

fn step(repo_root: &Path, program: &str, args: &[&str]) -> Result<(), Error> {
    let shown = std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!("==> {shown}");
    let status = Command::new(program)
        .args(args)
        .current_dir(repo_root)
        .status()
        .map_err(|error| Error::Device(format!("failed to spawn {program}: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Device(format!("ci failed: {shown}")))
    }
}
