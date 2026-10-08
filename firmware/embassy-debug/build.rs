//! Stamp the Embassy event-logger image with the repo git hash.

include!("../../scripts/git_env.rs");

/// Run on the build host and publish Git provenance for the image's UART banner.
/// Git lookup and fallback handling belong to `emit_git_env`; this build hook
/// never opens a device or runs code on the Xtensa target.
fn main() {
    emit_git_env("EMBASSY_DEBUG_GIT", "EMBASSY_DEBUG_GIT_DIRTY");
}
