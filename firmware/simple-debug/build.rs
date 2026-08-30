//! Stamp the operator / proof-of-life image with the repo git hash.

include!("../../scripts/git_env.rs");

fn main() {
    emit_git_env("SIMPLE_DEBUG_GIT", "SIMPLE_DEBUG_GIT_DIRTY");
}
