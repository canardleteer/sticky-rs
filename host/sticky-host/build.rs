//! Stamp the host library with the repo git hash.

include!("../../scripts/git_env.rs");

fn main() {
    emit_git_env("PACKAGE_GIT", "PACKAGE_GIT_DIRTY");
}
