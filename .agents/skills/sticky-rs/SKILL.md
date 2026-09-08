---
name: sticky-rs
description: >-
  Use when working in the sticky-rs repository: cargo xtask, build-fw, ci,
  flash-app, learn-uart, learn-uart-only, monitor, remote-debug,
  backup / confirm / restore, the UART session lock, crate layout,
  clap / espflash host CLI rules, or
  this repository's Rust path on the Seeed reTerminal Sticky. Board pins,
  rails, and datasheets live in the sibling seeed-sticky-hardware skill —
  read that first for wiring.
---

# sticky-rs

Host tools and Rust software path for **sticky-rs** (and the sundries-sticky
xtask subset that reuses sticky-host / remote-debug-host). Board wiring,
enclosure, and datasheets are
[`seeed-sticky-hardware`](../seeed-sticky-hardware/SKILL.md). Read that
skill first for pins and rails. Do not mix a stack’s APIs into the pin map.

**sundries-sticky differences:** advertise name `sundries-sticky`; build with
`cargo xtask build-fw --features remote-debug`; flash via sticky-rs
`flash-app`. Never open `/dev/ttyACM*` for app UART — use CDC listen (see
root [AGENTS.md](../../../AGENTS.md#ch343-uart-do-not-break-acm)).

This repository is **host-verified by default**. Landing xtask source is not
permission to open a port. Do not open a UART unless the human **explicitly
asked to run** a live command on a device in that message. The always-on
copy of that gate is the root `AGENTS.md`.

## How to read this skill

1. **xtask** — [references/xtask.md](references/xtask.md). Command catalog,
   monitor flags, UART session lock, `ESPFLASH_PORT`, no Cargo runner.
   Snapshot how-to:
   [firmware-snapshot-management.md](../../../docs/firmware-snapshot-management.md).
2. **Draw and hit-test** —
   [draw-and-touch.md](../seeed-sticky-hardware/references/draw-and-touch.md)
   (board facts) plus this crate’s
   `DigitizerSample` / `FramebufferPoint` / `GlassPoint` /
   `PagePoint`, `HitRect`, and `View::hit_raw` /
   `View::hit_framebuffer`. Remote-debug inject is
   `FramebufferPoint`, not UART `p0=`.
3. **Rust firmware** — [references/rust.md](references/rust.md). `esp-hal`
   vs `esp-idf-hal`, `build-fw` then `flash-app`, crate verdicts. In-tree
   Xtensa images live under `firmware/`.
4. **Layout** — [references/layout.md](references/layout.md). Workspace
   paths, clap/espflash/MSRV, lockfiles, crate README URLs.

Hardware facts and source precedence:
[`seeed-sticky-hardware`](../seeed-sticky-hardware/SKILL.md#authority).

## Do not connect unless asked

Discovery and flash I/O go through `cargo xtask`, not bare `espflash`,
`esptool`, `idf.py flash`, or PlatformIO upload. Do not run those tools,
`probe-rs`, or `cargo xtask` against hardware unless the human asked to run
that live command:

- Live: `detect-connected --probe`, live `backup-factory-firmware`,
  `confirm-factory-firmware`, `restore-factory-firmware`, `flash-app`,
  `learn-uart`, `learn-uart-only`, `monitor`, `remote-debug`,
  `remote-debug --mcp`
- Host-only (no UART): `detect-connected` without `--probe`,
  `backup-factory-firmware --import`, `diff-learn-uart`, `vet-idle-log`,
  `build-fw`, `ci`

When a live ask is present, the **only** in-repo device I/O is `cargo xtask`
as catalogued in [xtask.md](references/xtask.md). `flash-app` does not
compile; `cargo xtask build-fw` first. Host BLE pairing (BlueZ **Connect**
against advertise name `sticky-rs`, not `bluetoothctl pair`) is a
separate live ask; see
[Bluetooth testing options](../../../AGENTS.md#bluetooth-testing-options).
`remote-debug` / `remote-debug --mcp` is a separate live BLE ask
(and UART when auto-PIN scrapes `pair pin=`); see
[Remote-debug testing options](../../../AGENTS.md#remote-debug-testing-options).
A host SoftAP check (`nmcli` join `sticky-rs-AP` / `curl` on
`192.168.4.1`) is a separate live ask; see
[Wi-Fi SoftAP testing options](../../../AGENTS.md#wi-fi-softap-testing-options).

A device may be attached for unrelated reasons; ignore it.

## Crate and firmware map

| Path | Role |
| --- | --- |
| `crates/*` | Default-members. Host-testable, `no_std` / format crates |
| `host/sticky-host/` | Host library (`publish = true`, not crates.io yet). Live methods take the UART lock; callers pass `Layout` |
| `host/remote-debug-host/` | Generic BLE central (`publish = true`). No clap, no UART, no Sticky pins. Linux uses `bluer` |
| `xtask/` | Clap front-end at the repo root (`cargo xtask`, `publish = false`). `remote-debug --mcp` is this subtree only |
| `developer-data/` | Gitignored private / personalized files. Sealed per-unit originals under `developer-data/backups/`; learn-uart YAML under `uart-inspection-records/<serial>/`; confirm reports under `confirm-records/<serial>/`; remote-debug allowlist and snapshot planes under `remote-debug/`. Private scratch notes stay here too. Not in git |
| `firmware/*` | Xtensa images. Workspace members, not default-members. `build-fw` looks them up by package name. [Firmware examples as tutorial code](../../../firmware/AGENTS.md#firmware-examples-as-tutorial-code) |

Chip drivers (`bq25616`, `bq27220`, `ssd1677-gray4`) stay MCU-agnostic.
`panel-view` is the shared canvas trait plus last-compose / tagged-touch
companions (remap only; no `HitRect`). `remote-debug-wire` is the
protobuf codec plus documented GATT UUIDs / ATT reassembly for an
insecure desk snapshot / inject (UART stays plaintext).
`remote-debug-host` is the generic Linux central. Board pins,
latch, rails, and typed spaces
(`DigitizerSample`, `FramebufferPoint`, `GlassPoint`,
`PagePoint`, `HitRect`) belong in `seeed-reterminal-sticky`.

Never `bq27xxx` (wrong gauge family). Never a generic SSD1677 four-gray LUT.
Never commit a MAC, serial number, USB serial string, NVS blob, or flash
image. Never add a Cargo `runner`.
