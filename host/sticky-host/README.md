# sticky-host

Programmatic host API for Seeed reTerminal Sticky UART detect, factory
backup / confirm / restore, host-only `build-fw`, `app0` `flash-app`,
learn-uart, no-reset monitor, and the Sticky remote-debug desk:
UART `pair pin=` scrape, remember-me allowlist, and page-space
snapshot PNG.

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
