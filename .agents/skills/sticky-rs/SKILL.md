---
name: sticky-rs
description: >-
  Use when working in the sticky-rs repository: cargo xtask, build-fw,
  flash-app, learn-uart, learn-uart-only, monitor, backup / confirm / restore,
  the UART session lock, crate layout, clap / espflash host CLI rules, or
  this repository's Rust path on the Seeed reTerminal Sticky. Board pins,
  rails, and datasheets live in the sibling seeed-sticky-hardware skill —
  read that first for wiring.
---

# sticky-rs

Host tools and Rust software path for **this repository**. Board wiring,
enclosure, and datasheets are
[`seeed-sticky-hardware`](../seeed-sticky-hardware/SKILL.md). Read that
skill first for pins and rails. Do not mix a stack’s APIs into the pin map.

This repository is **host-verified by default**. Landing xtask source is not
permission to open a port. Do not open a UART unless the human **explicitly
asked to run** a live command on a device in that message. The always-on
copy of that gate is the root `AGENTS.md`.

## How to read this skill

1. **xtask** — [references/xtask.md](references/xtask.md). Command catalog,
   monitor flags, UART session lock, `ESPFLASH_PORT`, no Cargo runner.
   Snapshot how-to:
   [firmware-snapshot-management.md](../../../docs/firmware-snapshot-management.md).
2. **Rust firmware** — [references/rust.md](references/rust.md). `esp-hal`
   vs `esp-idf-hal`, `build-fw` then `flash-app`, crate verdicts. In-tree
   Xtensa images live under `firmware/`.
3. **Layout** — [references/layout.md](references/layout.md). Workspace
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
  `learn-uart`, `learn-uart-only`, `monitor`
- Host-only (no UART): `detect-connected` without `--probe`,
  `backup-factory-firmware --import`, `diff-learn-uart`, `build-fw`

When a live ask is present, the **only** in-repo device I/O is `cargo xtask`
as catalogued in [xtask.md](references/xtask.md). `flash-app` does not
compile; `cargo xtask build-fw` first.

A device may be attached for unrelated reasons; ignore it.

## Crate and firmware map

| Path | Role |
| --- | --- |
| `crates/*` | Default-members. Host-testable, `no_std` / format crates |
| `host/sticky-host/` | Host library (`publish = true`, not crates.io yet). Live methods take the UART lock; callers pass `Layout` |
| `xtask/` | Clap front-end at the repo root (`cargo xtask`, `publish = false`) |
| `developer-data/` | Gitignored private / personalized files. Per-unit originals and `learn-uart/` YAML under `developer-data/backups/`. Not in git |
| `firmware/*` | Xtensa images. Workspace members, not default-members. `build-fw` looks them up by package name |

Chip drivers (`bq25616`, `bq27220`, `ssd1677-gray4`) stay MCU-agnostic.
Board pins, latch, rails, and transforms belong in
`seeed-reterminal-sticky`.

Never `bq27xxx` (wrong gauge family). Never a generic SSD1677 four-gray LUT.
Never commit a MAC, serial number, USB serial string, NVS blob, or flash
image. Never add a Cargo `runner`.
