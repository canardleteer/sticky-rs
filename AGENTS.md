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
`docs/SAFETY.md` and [docs/DATASHEETS.md](docs/DATASHEETS.md) are
symlinks into that skill (`references/safety.md`,
`resources/datasheets.md`). Edit the skill files; rumdl lints those
pages and excludes the symlinks. Host tools:
[sticky-rs](.agents/skills/sticky-rs/SKILL.md)
(`sticky-host` is the library; `cargo xtask` is the clap front-end).

## Layout

| Path | Role |
| --- | --- |
| `crates/*` | Default-members. Host-testable `no_std` |
| `host/*` | Default-members. `host/sticky-host/` is the host library |
| `xtask/` | Default-member at the repo root (`cargo xtask`) |
| `firmware/simple-debug` | Workspace member, not a default-member |
| `firmware/embassy-debug` | Workspace member, not a default-member. Panel is always on |

Host verify uses default-members. rust-analyzer excludes the two firmware
packages via [rust-analyzer.toml](rust-analyzer.toml). Full path table:
[sticky-rs layout.md](.agents/skills/sticky-rs/references/layout.md).
Fresh-start how-to: [docs/getting-started.md](docs/getting-started.md).
Some directories also have a topical `AGENTS.md`; the nearest file wins
on conflict with this one.

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
`diff-learn-uart`, `vet-idle-log`, `build-fw`, `ci`) does not open a
UART.

When a live ask is present, the **only** in-repo device I/O is `cargo xtask`.
`flash-app` does not compile; `cargo xtask build-fw` first. Flag catalog:
[sticky-rs xtask.md](.agents/skills/sticky-rs/references/xtask.md).
`cargo xtask --help` is the flag source of truth.

A device may be attached for unrelated reasons; ignore it. There is no Cargo
`runner`, so `cargo run` cannot flash. Never commit a MAC address, serial
number, USB serial string, NVS blob, flash image, or a `monitor --output`
capture. Host-check examples use the static names `idle-embassy.log` and
`idle-simple.log` (local only). `developer-data/` is gitignored on
purpose. Put per-unit dumps, learn-uart YAML, and any other
private or personalized files there (`developer-data/backups/` for
sealed snapshots, `developer-data/uart-inspection-records/` for learn-uart,
`developer-data/confirm-records/` for confirm reports). Do not use a leftover
repo-root `backups/`.

## Keep skills updated

Project-local skills must stay aligned with the tree. When you change a
topic below, update the matching skill in the **same change**. State source
conflicts in the hardware skill instead of flattening them.

| When you change | Also update |
| --- | --- |
| Pin, rail, display, touch, sensor, enclosure, measurement backlog, or datasheet catalog | [seeed-sticky-hardware](.agents/skills/seeed-sticky-hardware/SKILL.md) (and a [sources.md](.agents/skills/seeed-sticky-hardware/references/sources.md) conflict row if sources disagree). `docs/DATASHEETS.md` is a symlink of `resources/datasheets.md` |
| `cargo xtask` CLI, `sticky-host` API, UART lock, `flash-app` / backup / restore contract, firmware packages, crate layout | [sticky-rs](.agents/skills/sticky-rs/SKILL.md) (especially [xtask.md](.agents/skills/sticky-rs/references/xtask.md) and [layout.md](.agents/skills/sticky-rs/references/layout.md)) **and** the root [README.md](README.md) xtask list |
| Hardware safety or “never erase” | [safety.md](.agents/skills/seeed-sticky-hardware/references/safety.md) (`docs/SAFETY.md` is a symlink) **and** this file if it restates a row |
| Live-ask set | this file **and** the sticky-rs skill |
| Agent rules that belong to one directory | that directory’s `AGENTS.md` (nearest file wins on conflict) |
| How-to voice | this file (working-rules how-to bullet) |
| Firmware examples as tutorial code | [firmware/AGENTS.md](firmware/AGENTS.md#firmware-examples-as-tutorial-code) (and each package `AGENTS.md`) **and** this file if it restates the bar |

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
  catalog ([docs/DATASHEETS.md](docs/DATASHEETS.md), a symlink of
  [datasheets.md](.agents/skills/seeed-sticky-hardware/resources/datasheets.md)).
  Cached PDFs and extracted markdown are gitignored under that skill’s
  `resources/datasheets/`. Ask the user to populate the cache
  (`scripts/fetch_datasheets.py` from the skill directory); do not download
  vendor files unless they asked.
- Prefer a named `enum` or `const` over a magic number. Prefer the vendor
  datasheet’s name when the sheet has one. If it never names the encoding,
  use the on-unit / crate name or a raw primitive. Cite extract **heading
  titles** (see [docs/API-RULES.md](docs/API-RULES.md)), not page numbers.
  Capture safe datasheet rows even if unused; leave hazardous encodings
  commented (`UNCONFIRMED_*` / danger blocks).
- Adopt a crates.io driver only with a recorded verdict in
  [docs/CRATES.md](docs/CRATES.md). Never `bq27xxx` (wrong gauge family);
  never a generic SSD1677 four-gray LUT.
- Host CLI and UART-lock implementation rules live in
  [sticky-rs layout.md](.agents/skills/sticky-rs/references/layout.md).
- Verify with `cargo test --locked`,
  `cargo clippy --locked --all-targets -- -D warnings`, and
  `cargo fmt --check` (the default host trio). `cargo xtask ci` is the
  full gate: that trio, host `--all-features` and
  `ssd1677-gray4 --no-default-features`, firmware `cargo +esp` clippy,
  `rumdl check`, `cargo machete`, and `cargo audit`. Do not advertise
  `cargo test --workspace` (that pulls Xtensa firmware members).
  rust-analyzer excludes those packages via
  [rust-analyzer.toml](rust-analyzer.toml). Owned Markdown is checked
  with `rumdl check` (config [`.rumdl.toml`](.rumdl.toml)). Do not run
  rumdl on vendor PDF extracts under the hardware skill
  `resources/datasheets/md/`.
- One workspace lockfile is committed. Pass `--locked` and keep the claimed
  MSRV. After changing `host/sticky-host/Cargo.toml`, `xtask/Cargo.toml`,
  `firmware/*/Cargo.toml`, or workspace members, refresh it with
  `cargo generate-lockfile`.
- Use [Conventional Commits](https://www.conventionalcommits.org/).
  Tracked git text (commits, rustdoc, README, skills, this file,
  branch names) describes the why. Do not name private review
  scratch, review-phase labels, or finding codes. Gitignored notes
  under `developer-data/` may.
- A rustc newer than MSRV can fail the host trio clippy on
  default-members. Keep `cargo clippy --locked --all-targets -- -D
  warnings` green; do not pin an older clippy. Workspace MSRV 1.85
  treats `const { assert!(…) }` items as experimental.
- Measurement-backlog items in the hardware skill stay open until someone
  measures them. Firmware evidence proves intent and sequencing, never
  electrical fact.
- Operator how-to (firmware README test recipes, getting-started command
  blocks) is for a person at the desk. Numbered steps with human titles;
  what to type, then what they should see, then what to do with their
  hands; pass and fail as observations. Keep live-ask, envelope, and
  backlog ids in this file and the skills. A backlog item may close a
  how-to as a note, not as the voice of the steps. Do not write those
  pages as agent notes.
- Firmware under `firmware/` (`simple-debug-fw` and
  `embassy-debug-fw`) must serve as educational reference code.
  Every function, method, struct, enum, and constant (public or
  private) must have comprehensive rustdoc (nets/buses, expectations,
  error handling) and abundant in-line comments (register sequencing,
  GPIO electrical configuration, bus arbitration, Embassy scheduling,
  stack buffers, reset/wake). Ground descriptions in *The Embedded
  Rust Book*, *The Rust on ESP Book*, and *The Embassy Book*. The
  always-on copy is
  [firmware/AGENTS.md](firmware/AGENTS.md#firmware-examples-as-tutorial-code).

## Agent Documentation Standards

Project-local skills exist under `.agents/skills/` and should remain
discoverable by agents working in this repository. Maintain those skills
according to the [Agent Skills specification](https://agentskills.io/specification),
and maintain this file according to the
[AGENTS.md standard](https://agents.md/). Some directories have their
own topical `AGENTS.md`; maintain those the same way. Keep both
portable across compatible agent clients, without assumptions about
user-specific paths or session state.

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
