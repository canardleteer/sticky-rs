# Remote-debug git consumer

How a **second firmware or host** takes this stack without
crates.io. This repository does not publish these crates. Cargo
git deps resolve in-repo `path` members from the same commit.

Never a MAC. Do not take `xtask`. Sticky UART PIN, remember-me,
and page PNG stay in `sticky-host`.

## Layers

| Crate | Role |
| --- | --- |
| `remote-debug-wire` | `Envelope`, framing, `SnapshotSlot`, `ControlLayout` |
| `remote-debug-peripheral` | Device dispatch; optional `gatt` Trouble service |
| `remote-debug-host` | Linux BlueZ `Session` (Connect, not Pair) |
| `remote-debug-broker` | ConnectRPC owner (`serve_with`, `ControlClient`) |
| `seeed-reterminal-sticky` | Sticky page map / pins (one layout) |
| `sticky-host` / `xtask` | In-tree desk only |

`sticky.remote.*` proto names stay. Optional snapshot
`scene` / `target_step` / expect may be omitted.

## Cargo.toml

```toml
remote-debug-wire = { git = "https://github.com/canardleteer/sticky-rs", package = "remote-debug-wire" }
remote-debug-peripheral = { git = "https://github.com/canardleteer/sticky-rs", package = "remote-debug-peripheral", features = ["gatt"] }
remote-debug-host = { git = "https://github.com/canardleteer/sticky-rs", package = "remote-debug-host" }
remote-debug-broker = { git = "https://github.com/canardleteer/sticky-rs", package = "remote-debug-broker" }
```

## Second device

1. Implement `ControlLayout` (advertise name, plane size, key ids).
2. Implement `remote_debug_peripheral::Device` (injects, LAST ready,
   slot, reboot). Page inject uses your board map (Sticky:
   `inject_framebuffer_for_page` / `inject_framebuffer_for_hold`).
3. Build your BLE `Stack`. Include `RemoteDebugService` on the
   `#[gatt_server]`. Optional `PairTokenService` (`6b1d…`).
   Advertise, DisplayOnly SMP, and RAM bonds stay in the image
   (in-tree: `firmware/embassy-debug/src/pair.rs`).
4. Host: `serve_with` + opener. Pass `--name` / `ConnectRequest.target`.
   Ignore `pin` / `port` / `remember` unless you scrape UART.

In-tree example: `embassy-debug-fw --features remote-debug`
(`FwDevice` + `StickyLayout`). Advertise default `sticky-rs`.
