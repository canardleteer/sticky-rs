# sticky-rs

Board contract, host tools, and safety notes for the Seeed Studio
reTerminal Sticky. Xtensa images live under `firmware/` (workspace
members, not default-members).

## Hardware safety (read before writing code)

1. **Never erase this board's flash.** No `erase-flash`, no full-chip erase.
   Do not write below `0x90000` except a restore of **that same unit's**
   original or `--capture`. The factory NVS holds per-unit Wi-Fi RF
   calibration, device identity, and persisted gauge state; lost `nvs` is
   not regenerable by hobby tools.
2. **No e-paper waveform may be invented.** The Sticky's confirmed path is
   panel **OTP** sequences (no MCU `0x32` write). Do not add a default
   105-byte LUT without recorded provenance and a compatible license.
3. **Fuel gauge writes stay off.** Reads are safe; unseal, `CFGUPDATE`, and
   Full Charge Capacity writes are not, and the OTP is one-time.
4. **Latch power first, release deliberately.** GPIO45 then GPIO46 high before
   logs or buses.

Full hazard table: [docs/SAFETY.md](docs/SAFETY.md). Board facts and source
precedence: [seeed-sticky-hardware](.agents/skills/seeed-sticky-hardware/SKILL.md).
Host tools: [sticky-rs](.agents/skills/sticky-rs/SKILL.md)
(`sticky-host` is the library; `cargo xtask` is the clap front-end).

## Layout

| Path | Role |
| --- | --- |
| `crates/*` | Default-members. Host-testable `no_std` |
| `host/*` | Default-members. `host/sticky-host/` is the host library |
| `xtask/` | Default-member at the repo root (`cargo xtask`) |
| `firmware/simple-debug` | Workspace member, not a default-member |
| `firmware/embassy-debug` | Workspace member, not a default-member. Panel is `--features epd` |

Host verify uses default-members. rust-analyzer excludes the two firmware
packages via [rust-analyzer.toml](rust-analyzer.toml). Full path table:
[sticky-rs layout.md](.agents/skills/sticky-rs/references/layout.md).
Fresh-start how-to: [docs/getting-started.md](docs/getting-started.md).

## Do not connect to a physical device

This repository is **host-verified by default**. Landing xtask source is not
permission to open a port. Discovery and flash I/O go through `cargo xtask`,
not bare `espflash`, `esptool`, `idf.py flash`, or PlatformIO upload. Do not
run those tools, `probe-rs`, or `cargo xtask` against hardware unless the
human **explicitly asked to run** that live command on a device in that
message (`detect-connected --probe`, live `backup-factory-firmware`,
`confirm-factory-firmware`, `restore-factory-firmware`, `flash-app`,
`learn-uart`, `learn-uart-only`, or `monitor`). Host-only xtask
(`detect-connected` without `--probe`, `backup-factory-firmware --import`,
`diff-learn-uart`, `build-fw`) does not open a UART.

When a live ask is present, the **only** in-repo device I/O is `cargo xtask`.
`flash-app` does not compile; `cargo xtask build-fw` first. Flag catalog:
[sticky-rs xtask.md](.agents/skills/sticky-rs/references/xtask.md).
`cargo xtask --help` is the flag source of truth.

A device may be attached for unrelated reasons; ignore it. There is no Cargo
`runner`, so `cargo run` cannot flash. Never commit a MAC address, serial
number, USB serial string, NVS blob, or flash image. `backups/` is gitignored
on purpose.

## Keep skills updated

Project-local skills must stay aligned with the tree. When you change a
topic below, update the matching skill in the **same change**. State source
conflicts in the hardware skill instead of flattening them.

| When you change | Also update |
| --- | --- |
| Pin, rail, display, touch, sensor, enclosure, measurement backlog, or datasheet catalog | [seeed-sticky-hardware](.agents/skills/seeed-sticky-hardware/SKILL.md) (and a [sources.md](.agents/skills/seeed-sticky-hardware/references/sources.md) conflict row if sources disagree) |
| `cargo xtask` CLI, `sticky-host` API, UART lock, `flash-app` / backup / restore contract, firmware packages, crate layout | [sticky-rs](.agents/skills/sticky-rs/SKILL.md) (especially [xtask.md](.agents/skills/sticky-rs/references/xtask.md) and [layout.md](.agents/skills/sticky-rs/references/layout.md)) **and** the root [README.md](README.md) xtask list |
| Hardware safety, live-ask set, or “never erase” | this file **and** both skills if they restate it |

Do not treat `cargo xtask --help` as a substitute for the sticky-rs catalog
and the README list.

## Working rules

- Follow [docs/API-RULES.md](docs/API-RULES.md) for any new crate: typestate
  for hazardous state, `C-FREE` destructors, no internal bus locking, datasheet
  citations in rustdoc.
- Each published crate's `README.md` is the crates.io landing page. Relative
  markdown links there only resolve to files **inside that crate's package**.
  Do not link `../../docs/...`, a sibling crate, or a skill with a
  repo-relative path from a crate README; use an absolute URL into this
  repository (`https://github.com/canardleteer/sticky-rs/blob/main/...`) or
  name the item in backticks. Relative links remain fine in repo-root docs,
  this file, and `.agents/skills/`.
- **Do not invent registers or opcodes.** If the datasheet has not been read,
  expose a documented raw primitive and record the gap in the hardware skill
  catalog ([docs/DATASHEETS.md](docs/DATASHEETS.md)). Cached PDFs and
  extracted markdown are gitignored under that skill’s
  `resources/datasheets/`. Ask the user to populate the cache
  (`scripts/fetch_datasheets.py` from the skill directory); do not download
  vendor files unless they asked.
- Prefer a named `enum` or `const` over a magic number. Prefer the vendor
  datasheet’s name when the sheet has one. If it never names the encoding,
  use the on-glass / crate name or a raw primitive.
- Adopt a crates.io driver only with a recorded verdict in
  [docs/CRATES.md](docs/CRATES.md). Never `bq27xxx` (wrong gauge family);
  never a generic SSD1677 four-gray LUT.
- Host CLI and UART-lock implementation rules live in
  [sticky-rs layout.md](.agents/skills/sticky-rs/references/layout.md).
- Verify with `cargo test --locked`,
  `cargo clippy --locked --all-targets -- -D warnings`, and
  `cargo fmt --check`. Do not advertise `cargo test --workspace` (that
  pulls Xtensa firmware members). rust-analyzer excludes those packages
  via [rust-analyzer.toml](rust-analyzer.toml).
- One workspace lockfile is committed. Pass `--locked` and keep the claimed
  MSRV. After changing `host/sticky-host/Cargo.toml`, `xtask/Cargo.toml`,
  `firmware/*/Cargo.toml`, or workspace members, refresh it with
  `cargo generate-lockfile`.
- Use [Conventional Commits](https://www.conventionalcommits.org/).
- Measurement-backlog items in the hardware skill stay open until someone
  measures them. Firmware evidence proves intent and sequencing, never
  electrical fact.

## Agent Documentation Standards

Project-local skills exist under `.agents/skills/` and should remain
discoverable by agents working in this repository. Maintain those skills
according to the [Agent Skills specification](https://agentskills.io/specification),
and maintain this file according to the
[AGENTS.md standard](https://agents.md/). Keep both portable
across compatible agent clients, without assumptions about user-specific paths
or session state.

Two skills:

- [seeed-sticky-hardware](.agents/skills/seeed-sticky-hardware/SKILL.md) —
  board contract (pins, rails, datasheets, observed silicon, source
  precedence). The skill user weighs conflicts.
- [sticky-rs](.agents/skills/sticky-rs/SKILL.md) — this repository’s host
  tools (`cargo xtask` / `sticky-host`), crate layout, and Rust firmware
  path.

Vendor datasheets are official for registers of chips confirmed on this
model; observed hardware still outranks a datasheet default. See
[Authority](.agents/skills/seeed-sticky-hardware/SKILL.md#authority).
