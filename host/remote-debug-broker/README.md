# remote-debug-broker

ConnectRPC owner that holds 0..N remote-debug GATT `Session`s.
The map key is the BLE advertise name (`target`), never a MAC.

This crate has **no** clap, **no** UART, and **no** Sticky pin map.
A second project's CLI or MCP front-end depends on this crate plus
[`remote-debug-host`](https://github.com/canardleteer/sticky-rs/blob/main/host/remote-debug-host)
and supplies its own opener (pairing / passkey policy) and serve
binary. It does **not** take `xtask`.

Client hangup does **not** drop GATT.
`Disconnect` drops one session; `Shutdown` or serve exit drops
every session and unbinds the loopback listener.

## Control plane

IDL lives in the repository `protos/` tree
(`sticky.remote.shared.v1`, GATT `Envelope`,
`sticky.remote.control.v1.RemoteDebugControlService`).
This crate serves Connect/HTTP on `127.0.0.1:0` and writes the
URI to `$dir/remote-debug.connect`.

| RPC | Role |
| --- | --- |
| `Connect` | Start pair on a worker; returns `pairing` |
| `Status` | `pairing` / `connected` / `disconnected` |
| `ListTargets` | Advertise names the owner currently tracks |
| `InjectTouch` | Framebuffer or page tap / slide |
| `InjectButton` | `ok` / `page-up` / `page-down` |
| `GetSnapshot` | Arm LAST DRAW; planes in the reply |
| `SnapshotAck` | Release the armed nonce |
| `SnapshotClear` | Abort with no nonce |
| `Reboot` | Software-reset the embedded MCU |
| `Disconnect` | Drop one GATT session |
| `Shutdown` | Stop the owner |

Poll `Status` after `Connect` until `connected` or `pair failed`.
Inject and snapshot need `connected`. The caller writes snapshot
files; this crate only returns plane bytes. A leftover arm is
Connect `FailedPrecondition` (`snapshot busy`).

`ConnectRequest.pin` / `port` / `remember` stay on the control
request so existing Sticky clients keep working. A foreign opener
may ignore `port` and `remember`.

## Serve and spawn

`serve_with` takes an endpoint directory and an opener
`FnMut(&ConnectRequest) -> Result<Session<T>, _>`. Tests pass
`FakeTransport`. After a successful pair with `remember`, an
optional `RememberHook` runs (Sticky allowlist lives in
`sticky-host`).

`ensure_broker` takes a [`SpawnSpec`] (exe + full argv). It does
**not** hardcode `remote-debug serve`. The Sticky desk wraps that
as xtask `remote-debug serve --socket-dir …`.

Pass the advertise name at connect time. The in-tree default
`sticky-rs` is only an example.

License: MIT
