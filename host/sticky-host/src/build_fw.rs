//! Host-only Xtensa build + `save-image`. No UART.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;

/// In-repo firmware image (`firmware/<name>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareImage {
    /// Blocking `esp-hal` proof-of-life (`simple-debug-fw`).
    SimpleDebug,
    /// Embassy event logger (`embassy-debug-fw`).
    EmbassyDebug,
}

impl FirmwareImage {
    /// Cargo package name.
    #[must_use]
    pub fn package(self) -> &'static str {
        match self {
            Self::SimpleDebug => "simple-debug-fw",
            Self::EmbassyDebug => "embassy-debug-fw",
        }
    }

    /// `save-image` payload stem (`simple-debug.bin`).
    #[must_use]
    pub fn bin_stem(self) -> &'static str {
        match self {
            Self::SimpleDebug => "simple-debug",
            Self::EmbassyDebug => "embassy-debug",
        }
    }
}

/// `cargo +esp build` profile and features, then host-only `save-image`.
#[derive(Debug, Clone)]
pub struct BuildFwArgs {
    /// Which firmware package to build.
    pub image: FirmwareImage,
    /// Cargo features on that package (`operator` on simple-debug).
    ///
    /// Embassy-debug defaults to `pair` + `wifi`. Exclusive sits
    /// (`mic` / `radio` / `charge` / `sd`) add `--no-default-features`.
    pub features: Vec<String>,
    /// `true` is `--profile release-fw` (the documented default).
    pub release: bool,
}

/// Paths under workspace `target/xtensa-esp32s3-none-elf/<profile>/`.
#[derive(Debug, Clone)]
pub struct BuildFwOutput {
    /// Linked ELF (`simple-debug-fw` / `embassy-debug-fw`).
    pub elf: PathBuf,
    /// `espflash save-image` payload (not an ELF).
    pub bin: PathBuf,
}

/// Build one firmware member and pack a flash payload next to the ELF.
///
/// Invokes `cargo +esp` with `--target xtensa-esp32s3-none-elf` and
/// `-Zbuild-std=core,alloc`. Does not open a UART.
pub fn build_fw(repo_root: &Path, args: &BuildFwArgs) -> Result<BuildFwOutput, Error> {
    let package = args.image.package();
    let mut cargo = Command::new("cargo");
    cargo
        .current_dir(repo_root)
        .arg("+esp")
        .arg("build")
        .arg("-p")
        .arg(package)
        .arg("--locked")
        .arg("--target")
        .arg("xtensa-esp32s3-none-elf")
        .arg("-Zbuild-std=core,alloc");
    if args.release {
        cargo.arg("--profile").arg("release-fw");
    }
    if args.image == FirmwareImage::EmbassyDebug
        && embassy_debug_needs_no_default_features(&args.features)
    {
        cargo.arg("--no-default-features");
    }
    if !args.features.is_empty() {
        cargo.arg("--features").arg(args.features.join(","));
    }
    let status = cargo
        .status()
        .map_err(|error| Error::Device(format!("failed to spawn cargo +esp: {error}")))?;
    if !status.success() {
        return Err(Error::Device(format!(
            "cargo +esp build -p {package} failed"
        )));
    }

    let profile = if args.release { "release-fw" } else { "debug" };
    let elf = repo_root
        .join("target/xtensa-esp32s3-none-elf")
        .join(profile)
        .join(package);
    if !elf.is_file() {
        return Err(Error::Device(format!(
            "expected ELF missing after build: {}",
            elf.display()
        )));
    }

    let bin = elf.with_file_name(format!("{}.bin", args.image.bin_stem()));
    let status = Command::new("espflash")
        .current_dir(repo_root)
        .args([
            "save-image",
            "--chip",
            "esp32s3",
            "--flash-size",
            "32mb",
            "--skip-update-check",
        ])
        .arg(&elf)
        .arg(&bin)
        .status()
        .map_err(|error| Error::Device(format!("failed to spawn espflash save-image: {error}")))?;
    if !status.success() {
        return Err(Error::Device(
            "espflash save-image failed (need host-only espflash on PATH)".into(),
        ));
    }
    if !bin.is_file() {
        return Err(Error::Device(format!(
            "expected save-image payload missing: {}",
            bin.display()
        )));
    }

    Ok(BuildFwOutput { elf, bin })
}

/// Cargo features that cannot share a binary with default `pair`.
pub const EMBASSY_DEBUG_EXCLUSIVE_OF_PAIR: &[&str] = &["mic", "radio", "charge", "sd"];

/// Whether `build-fw` / firmware clippy must pass `--no-default-features`.
#[must_use]
pub fn embassy_debug_needs_no_default_features(features: &[String]) -> bool {
    features
        .iter()
        .any(|feature| EMBASSY_DEBUG_EXCLUSIVE_OF_PAIR.contains(&feature.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_sits_drop_default_pair() {
        assert!(embassy_debug_needs_no_default_features(
            &["mic".to_string()]
        ));
        assert!(embassy_debug_needs_no_default_features(&[
            "radio".to_string()
        ]));
        assert!(embassy_debug_needs_no_default_features(&[
            "charge".to_string()
        ]));
        assert!(embassy_debug_needs_no_default_features(&["sd".to_string()]));
        assert!(!embassy_debug_needs_no_default_features(&[
            "pair".to_string()
        ]));
        assert!(!embassy_debug_needs_no_default_features(&[
            "wifi".to_string()
        ]));
        assert!(!embassy_debug_needs_no_default_features(&[
            "spi20".to_string()
        ]));
        assert!(!embassy_debug_needs_no_default_features(&[]));
    }
}
