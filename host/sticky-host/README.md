# sticky-host

Programmatic host API for Seeed reTerminal Sticky UART detect, factory
backup / confirm / restore, host-only `build-fw`, `app0` `flash-app`,
learn-uart, no-reset monitor, and the Sticky remote-debug desk:
UART `pair pin=` scrape, remember-me allowlist, and page-space
snapshot PNG, and correlated storage controls.

This crate is the **Sticky desk**, not the generic GATT stack.
Framed envelopes and BlueZ Connect live in
[`remote-debug-host`](https://github.com/canardleteer/sticky-rs/blob/main/host/remote-debug-host).
The ConnectRPC owner (`serve_with`, `SpawnSpec`, loopback
endpoint file) lives
in
[`remote-debug-broker`](https://github.com/canardleteer/sticky-rs/blob/main/host/remote-debug-broker).
A foreign firmware host should depend on those two crates, not on
`xtask` and not on this crate's UART / CH343 / PNG path.

`cargo xtask` is the clap / MCP front-end. Callers pass a `Layout`
(developer-data / backups root), not a hardcoded repo path.
`connect` auto-starts a detached serve (`SpawnSpec` argv is xtask
`remote-debug serve`) and returns `pairing`; poll `status` until
`connected`.

License: MIT

`storage_control` waits for the requested job receipt and reports completion
or failure. `stage_storage` validates the complete ESP32-S3 app image, sends
ordered 512-byte chunks, and waits for readback/publication. These functions
use the existing broker session; filesystem operations run on the device.

`flash-app --force --yes` supports an externally held backup while retaining
live partition-table and complete-image validation. Secure boot and flash
encryption must both be explicitly disabled. `BoardInfo` and `Manifest` use
`Option<bool>` for those flags so unknown state cannot authorize a write.
The flash writer uses a stable copy of the validated image and writes app0
only. See the [storage guide](https://github.com/canardleteer/sticky-rs/blob/main/docs/storage.md)
for operator steps and durability limits.
