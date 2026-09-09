# remote-debug-peripheral

Device-side half of remote-debug: decode one framed
`sticky.remote.v1.Envelope`, drive a `Device`
implementor, and (optional `gatt` feature) expose the encrypted RX/TX
GATT service.

This crate has **no** `esp-hal`, **no** PSRAM carve, and **no**
UART `format_event`. The image owns LAST planes, synthetic mux, and
the radio. A second product implements [`Device`] plus
`remote_debug_wire::ControlLayout` and keeps the same Envelope.

Git (not crates.io):

```toml
remote-debug-peripheral = { git = "https://github.com/canardleteer/sticky-rs", package = "remote-debug-peripheral" }
# Trouble GATT service + notify helpers:
# remote-debug-peripheral = { git = "https://github.com/canardleteer/sticky-rs", package = "remote-debug-peripheral", features = ["gatt"] }
```

Cargo resolves this crate’s `path` deps from the same git commit.

| Item | Role |
| --- | --- |
| `handle_envelope` | Decode + inject / slot / reboot |
| `Device` | Injects, LAST ready, `SnapshotSlot`, reboot |
| `ControlLayout` (wire) | Advertise name, plane size, key ids |
| `RemoteDebugService` (`gatt`) | Local UUIDs `c81e1000/1001/1002` |

The in-tree Sticky image is `embassy-debug-fw --features remote-debug`.
Host owner: `remote-debug-broker`. Codec: `remote-debug-wire`.

License: MIT
