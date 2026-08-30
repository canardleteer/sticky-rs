//! Stamp the Embassy event-logger image with the repo git hash.

include!("../../scripts/git_env.rs");

fn main() {
    emit_git_env("EMBASSY_DEBUG_GIT", "EMBASSY_DEBUG_GIT_DIRTY");
}
