# remote-debug-wire

Protobuf codec for remote-debug snapshot and inject.

Git (not crates.io):

```toml
remote-debug-wire = { git = "https://github.com/canardleteer/sticky-rs", package = "remote-debug-wire" }
```

A second board implements [`ControlLayout`](https://github.com/canardleteer/sticky-rs/blob/main/crates/remote-debug-wire/src/map.rs)
(advertise name, plane size, key ids) and keeps this Envelope.
`StickyLayout` is the in-tree Seeed reTerminal Sticky profile
(`sticky-rs`, 800×480, GPIO 4/5/6). Page↔framebuffer stays on
the board crate.

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
space, and not a raw GT911 480×800 sample. `Reboot` software-resets
the **embedded MCU**, not the host. After that the session is dead;
implementing firmware should re-advertise so a central can Connect
and pair again. Do not persist SMP keys to factory NVS.

`#![no_std]` plus `alloc`. Runtime is `buffa` with default features
off. Generated Rust under `src/gen/` is committed so default builds
stay offline. Rewrite it with `REGEN_PROTO=1 cargo build -p
remote-debug-wire` (needs `buf` via `buf-tools`).

## BLE GATT (optional transport)

Local 128-bit UUIDs (not SIG, not the pair-card token service):

| Role | UUID |
| --- | --- |
| Service | `c81e1000-5c8a-4f0e-9c3a-2e7b1a0d4f11` |
| RX write (host → device) | `c81e1001-5c8a-4f0e-9c3a-2e7b1a0d4f11` |
| TX notify (device → host) | `c81e1002-5c8a-4f0e-9c3a-2e7b1a0d4f11` |

Both characteristics require an encrypted link. ATT is not
self-delimiting: reassemble one u32-LE frame (`FrameAssembler`), then
`decode_envelope`. Constants live on `GATT_SERVICE_UUID` /
`GATT_RX_UUID` / `GATT_TX_UUID`.
