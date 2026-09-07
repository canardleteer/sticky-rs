# remote-debug-wire

Protobuf codec for Sticky remote-debug snapshot and inject.

This crate is the **codec**, not a transport. One framed message is a
little-endian `u32` length plus a `sticky.remote.v1.Envelope`. SoftAP,
BLE, or a later UART side-channel pick the byte stream.

UART on the embassy-debug image stays plaintext. A proto `LogLine` is
a copy of that same `format_event` text, not a second schema.

Snapshot capacity is **one frozen slot** (`SnapshotSlot`). Arm from the
last consistent compose, retry the same host nonce, refuse a different
nonce (`SnapshotBusy`), and release on a matching `SnapshotAck` or a
nonce-free `SnapshotClear`. Encode planes from borrowed slices. Do not
heap-clone a 48 KiB panel on the device.

Injects are framebuffer pixels (`TouchSample`) and product keys
(`Ok` / `PageUp` / `PageDown` → GPIO 4/5/6). Not UART `p0=` glass
space, and not a raw GT911 480×800 sample.

`#![no_std]` plus `alloc`. Runtime is `buffa` with default features
off. Generated Rust under `src/gen/` is committed so default builds
stay offline. Rewrite it with `REGEN_PROTO=1 cargo build -p
remote-debug-wire` (needs `buf` via `buf-tools`).
