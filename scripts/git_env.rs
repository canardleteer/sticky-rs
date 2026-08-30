// Shared by package `build.rs` files. Included, not a Cargo target.
//
// Walks from `CARGO_MANIFEST_DIR` to the git work tree and emits
// `cargo:rustc-env` for a hash and a dirty flag.

use std::path::{Path, PathBuf};
use std::process::Command;

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string())
}

/// Sets `{git_var}` to a 40-char SHA (or `unknown`) and `{dirty_var}` to `0` or `1`.
pub fn emit_git_env(git_var: &str, dirty_var: &str) {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let Some(root) = find_git_root(&manifest) else {
        println!("cargo:rustc-env={git_var}=unknown");
        println!("cargo:rustc-env={dirty_var}=1");
        return;
    };

    println!("cargo:rerun-if-changed={}", root.join(".git/HEAD").display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join(".git/index").display()
    );
    if let Ok(head) = std::fs::read_to_string(root.join(".git/HEAD")) {
        if let Some(rel) = head.strip_prefix("ref: ") {
            println!(
                "cargo:rerun-if-changed={}",
                root.join(".git").join(rel.trim()).display()
            );
        }
    }

    let hash = git_stdout(&root, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = match git_stdout(&root, &["status", "--porcelain"]) {
        Some(status) => !status.is_empty(),
        None => true,
    };
    println!("cargo:rustc-env={git_var}={hash}");
    println!("cargo:rustc-env={dirty_var}={}", i32::from(dirty));
}
