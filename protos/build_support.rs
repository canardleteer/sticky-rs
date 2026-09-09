// Shared REGEN_PROTO=1 helper. Included from crate build.rs files.
// Locates official buf via buf-tools and local protoc plugins
// under target/proto-plugins (installed on first regen).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Walk from a crate manifest to the workspace root (`protos/` sibling).
pub fn workspace_root(manifest_dir: &Path) -> PathBuf {
    manifest_dir
        .ancestors()
        .find(|p| p.join("protos").join("buf.yaml").is_file())
        .map(Path::to_path_buf)
        .expect("workspace root with protos/buf.yaml")
}

/// Install missing local plugins into `target/proto-plugins`.
pub fn ensure_plugin_bin_dir(root: &Path) -> PathBuf {
    let dest = root.join("target").join("proto-plugins");
    let bin = dest.join("bin");
    let plugins: &[(&str, &str, &str)] = &[
        ("protoc-gen-buffa", "protoc-gen-buffa", "0.9.2"),
        (
            "protoc-gen-buffa-packaging",
            "protoc-gen-buffa-packaging",
            "0.9.2",
        ),
        ("protoc-gen-connect-rust", "connectrpc-codegen", "0.9.0"),
    ];
    for (exe, crate_name, version) in plugins {
        let path = bin.join(exe);
        if path.is_file() {
            continue;
        }
        let status = Command::new(env!("CARGO"))
            .args([
                "install",
                "--locked",
                "--root",
            ])
            .arg(&dest)
            .args(["--version", version, crate_name])
            .status()
            .unwrap_or_else(|error| panic!("cargo install {crate_name}: {error}"));
        assert!(
            status.success(),
            "cargo install {crate_name} {version} failed"
        );
    }
    bin
}

/// `dir` first, then the existing `PATH`.
pub fn prepend_path(dir: &Path) -> OsString {
    let mut path = dir.as_os_str().to_os_string();
    if let Some(rest) = std::env::var_os("PATH") {
        path.push(":");
        path.push(rest);
    }
    path
}

/// Run `buf generate --template protos/buf.gen.<kind>.yaml` from the workspace root.
///
/// Templates live next to [`buf.yaml`] so `paths` stay inside the module.
/// `kind` is `wire` or `broker`.
pub fn buf_generate(root: &Path, kind: &str) {
    let buf = buf_tools::buf_bin_path();
    let plugins = ensure_plugin_bin_dir(root);
    let template = root
        .join("protos")
        .join(format!("buf.gen.{kind}.yaml"));
    let mut cmd = Command::new(&buf);
    cmd.args(["generate", "--template"]);
    cmd.arg(&template);
    cmd.current_dir(root.join("protos"));
    cmd.env("PATH", prepend_path(&plugins));
    let status = cmd
        .status()
        .unwrap_or_else(|error| panic!("buf generate: {error}"));
    assert!(
        status.success(),
        "buf generate failed for {}",
        template.display()
    );
}

/// Rerun when IDL or gen templates change.
pub fn rerun_if_protos(root: &Path, kind: &str) {
    println!("cargo:rerun-if-env-changed=REGEN_PROTO");
    println!(
        "cargo:rerun-if-changed={}",
        root.join("protos").join("buf.yaml").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join("protos")
            .join(format!("buf.gen.{kind}.yaml"))
            .display()
    );
    let proto_root = root.join("protos").join("sticky");
    if let Ok(walker) = std::fs::read_dir(&proto_root) {
        walk_protos(&proto_root, walker);
    }
}

fn walk_protos(_dir: &Path, entries: std::fs::ReadDir) {
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Ok(inner) = std::fs::read_dir(&path) {
                walk_protos(&path, inner);
            }
        } else if path.extension() == Some(OsStr::new("proto"))
            || path.file_name() == Some(OsStr::new("buf.yaml"))
        {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
