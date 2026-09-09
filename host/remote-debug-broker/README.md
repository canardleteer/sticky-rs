# remote-debug-broker

Length-prefixed JSON over a Unix socket that owns one
remote-debug GATT `Session`. Not JSON-RPC 2.0.

This crate has **no** clap, **no** UART, and **no** Sticky pin map.
A second project's CLI or MCP front-end depends on this crate plus
[`remote-debug-host`](https://github.com/canardleteer/sticky-rs/blob/main/host/remote-debug-host)
and supplies its own opener (pairing / passkey policy) and serve
binary. It does **not** take `xtask`.

Client hangup does **not** drop GATT.
`disconnect` or serve exit is the only close. The socket key is the
advertise name, never a MAC.

## Request `op` tags

One request is a little-endian `u32` length plus a serde JSON
body with `op` (not a JSON-RPC `method` / `id`):

| `op` | Role |
| --- | --- |
| `connect` | Start pair on a worker; returns `pairing` |
| `status` | `pairing` / `connected` / `disconnected` |
| `inject-touch` | Framebuffer or page tap / slide |
| `inject-button` | `ok` / `page-up` / `page-down` |
| `get-snapshot` | Arm LAST DRAW; planes in the reply |
| `snapshot-ack` | Release the armed nonce |
| `snapshot-clear` | Abort with no nonce |
| `reboot` | Software-reset the embedded MCU |
| `disconnect` | Drop GATT and stop the broker |

Poll `status` after `connect` until `connected` or `pair failed`.
Inject and snapshot need `connected`. The caller writes snapshot
files; this crate only returns plane bytes.

`ConnectReq.pin` / `port` / `remember` stay on the JSON so existing
Sticky clients keep working. A foreign opener may ignore `port` and
`remember`.

## Serve and spawn

`serve_with` takes a socket directory and an opener
`FnMut(&ConnectReq) -> Result<Session<T>, _>`. Tests pass
`FakeTransport`. After a successful pair with `remember`, an optional
`RememberHook` runs (Sticky allowlist lives in `sticky-host`).

`ensure_broker` takes a [`SpawnSpec`] (exe + full argv). It does
**not** hardcode `remote-debug serve`. The Sticky desk wraps that
as xtask `remote-debug serve --name … --socket-dir …`.

Pass the advertise name at connect time. The in-tree default
`sticky-rs` is only an example.

License: MIT
