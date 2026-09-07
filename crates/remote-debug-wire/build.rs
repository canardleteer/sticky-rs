//! Host-only codegen. Firmware `cargo +esp` still runs this on the host.
//!
//! Set `REGEN_PROTO=1` to rewrite `src/gen/` from the `.proto` sources
//! (needs `buf` via `buf-tools`). Default builds include the committed
//! files and stay offline.

fn main() {
    println!("cargo:rerun-if-changed=proto/sticky/remote/v1/remote.proto");
    println!("cargo:rerun-if-changed=proto/buf.yaml");
    println!("cargo:rerun-if-env-changed=REGEN_PROTO");
    if std::env::var_os("REGEN_PROTO").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }

    let buf = buf_tools::buf_bin_path();
    std::env::set_var("PATH", prepend_path(buf.parent().expect("buf dir")));

    let out = std::path::Path::new("src/gen");
    let _ = std::fs::create_dir_all(out);
    buffa_build::Config::new()
        .files(&["proto/sticky/remote/v1/remote.proto"])
        .includes(&["proto"])
        .use_buf()
        .out_dir(out)
        .include_file("_include.rs")
        .compile()
        .expect("buffa codegen");
}

fn prepend_path(dir: &std::path::Path) -> std::ffi::OsString {
    let mut path = dir.as_os_str().to_os_string();
    if let Some(rest) = std::env::var_os("PATH") {
        path.push(":");
        path.push(rest);
    }
    path
}
