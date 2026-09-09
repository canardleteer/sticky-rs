# remote-debug-host

Generic Linux host for the remote-debug GATT: framed
`sticky.remote.v1.Envelope` after DisplayOnly pairing.

This crate has **no** clap, **no** UART, and **no** Sticky pin map.
A second codebase implements the same documented 128-bit service
(`remote-debug-wire` `GATT_*` UUIDs) after *its* pairing policy and
feeds six-digit passkeys through [`PasskeySource`].

Git (not crates.io):

```toml
remote-debug-host = { git = "https://github.com/canardleteer/sticky-rs", package = "remote-debug-host" }
```

On Linux the transport is [`bluer`](https://crates.io/crates/bluer)
(BlueZ). Connect the advertise name (default `sticky-rs` is an
in-tree example; pass your own); do **not** call BlueZ
`Device1.Pair()` / `bluetoothctl pair` — that races a peripheral SMP
Security Request. A KeyboardOnly agent answers `RequestPasskey`
without blocking the D-Bus loop. Never print a MAC.

Off Linux, implement [`Transport`] (`write_frame`, `read_chunk`,
`try_read_chunk`, `disconnect`) and wrap it in [`Session`].

## Session API

| Item | Role |
| --- | --- |
| `connect` / `connect_with` | Linux BlueZ Connect (not Pair) |
| `PasskeySource` | Six-digit PIN (`FixedPasskey` / `ChannelPasskey`) |
| `Session::inject_touch` | Framebuffer tap |
| `Session::inject_touch_ex` | Phase + optional page space |
| `Session::inject_button` | Product-key edge |
| `Session::get_snapshot` | Arm LAST DRAW; `SnapshotBusy` if armed |
| `Session::snapshot_ack` | Release the nonce |
| `Session::snapshot_clear` | Abort with no nonce |
| `Session::reboot` | Reset the embedded MCU, not this host |
| `Session::disconnect` | Drop GATT; leftover LTK is stale |
| `Session::drain_logs` | Consume `LogLine` fragments |

Host TX notify must stay FIFO (`VecDeque`). A 96 KiB plane is many
ATT chunks; a LIFO queue fails `frame: Version`.

`Session::reboot` writes a `Reboot` envelope (embedded MCU, not this
host), waits for `RebootAck` or a drop, and forgets the BlueZ bond.

Allowlisted long-term bonds and UART `pair pin=` scraping live in the
caller (`sticky-host` / `cargo xtask remote-debug`), not here. The
ConnectRPC owner process is
[`remote-debug-broker`](https://github.com/canardleteer/sticky-rs/blob/main/host/remote-debug-broker).

License: MIT
