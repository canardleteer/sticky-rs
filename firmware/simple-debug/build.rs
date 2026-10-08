//! Stamp the operator / proof-of-life image with the repo git hash.

include!("../../scripts/git_env.rs");

/// Run on the build host and publish Git provenance for the image's UART banner.
/// `emit_git_env` handles Git availability and Cargo rebuild directives; this
/// hook does not open the CH343 or access target GPIOs.
fn main() {
    emit_git_env("SIMPLE_DEBUG_GIT", "SIMPLE_DEBUG_GIT_DIRTY");
}
