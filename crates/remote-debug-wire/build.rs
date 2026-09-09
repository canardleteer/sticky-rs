//! Host-only codegen. Firmware `cargo +esp` still runs this on the host.
//!
//! Set `REGEN_PROTO=1` to rewrite `src/gen/` from [`protos/`](../../protos)
//! (needs `buf` via `buf-tools` and local buffa plugins). Default builds
//! include the committed files and stay offline.

include!("../../protos/build_support.rs");

fn main() {
    let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = workspace_root(&manifest);
    rerun_if_protos(&root, "wire");
    if std::env::var_os("REGEN_PROTO").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    buf_generate(&root, "wire");
}
