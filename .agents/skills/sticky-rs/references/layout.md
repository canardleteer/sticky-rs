# Repository layout and host conventions

## Paths

| Path | Contents |
| --- | --- |
| `crates/*` | Default-members. Host-testable format crates and (when present) `no_std` `embedded-hal` 1.0 drivers |
| `host/` | Default-members. Host libraries and future host CLIs (not `xtask`) |
| `host/sticky-host/` | Host library (`publish = true`, not crates.io yet). Detect, factory backup / confirm / restore, `build-fw`, `flash-app`, learn-uart, monitor. Callers pass `Layout`; live methods take the UART lock |
| `xtask/` | Clap front-end at the repo root (`cargo xtask`). Maps flags to `sticky-host`; `repo_root()` is the parent of this package |
| `backups/` | Gitignored per-unit `original/<serial>/` and `captures/<unit-id>/<slug>/` plus `learn-uart/` YAML. Not in git |
| `firmware/*` | Workspace members, not default-members. ELFs in workspace `target/` |
| `docs/` | [SAFETY.md](../../../../docs/SAFETY.md), [firmware-snapshot-management.md](../../../../docs/firmware-snapshot-management.md), [API-RULES.md](../../../../docs/API-RULES.md), [CRATES.md](../../../../docs/CRATES.md), [ssd1677.md](../../../../docs/ssd1677.md) |
| `.agents/skills/seeed-sticky-hardware/` | Board contract |
| `.agents/skills/sticky-rs/` | This skill |

Chip drivers (`bq25616`, `bq27220`, `ssd1677-gray4`) are MCU-agnostic and
carry no `esp-hal` dependency. Board specifics — pins, latch, rails,
transforms — belong in `seeed-reterminal-sticky`. Keep that split.

## Working rules (this repository)

- Follow [docs/API-RULES.md](../../../../docs/API-RULES.md): typestate for
  hazardous state, `C-FREE` destructors, no internal bus locking, datasheet
  citations in rustdoc.
- Each published crate's `README.md` is the crates.io landing page. Relative
  markdown links there only resolve to files **inside that crate's package**.
  Do not link `../../docs/...`, a sibling crate, or a skill with a
  repo-relative path from a crate README; use an absolute URL into this
  repository (`https://github.com/canardleteer/sticky-rs/blob/main/...`) or name the
  item in backticks. Relative links remain fine in repo-root docs, `AGENTS.md`,
  and `.agents/skills/`.
- Prefer a named `enum` or `const` over a magic number in code. Prefer the
  vendor datasheet’s name for that number when the sheet has one (for example
  `SlaveAddress::Pair28_29` rather than `0x14` at the call site). If the
  sheet never names the encoding, do not invent a datasheet-looking alias;
  use the documented on-glass / crate name or a raw primitive.
- Adopt a crates.io driver only with a recorded verdict in
  [docs/CRATES.md](../../../../docs/CRATES.md). Never `bq27xxx` (wrong
  gauge family); never a generic SSD1677 four-gray LUT.
- Host CLIs (`xtask` and any future CLI) use [`clap`](https://docs.rs/clap)
  **derive** (`Parser`, `Subcommand`, `Args`). Do not put clap types in
  `sticky-host`. Do not use clap builder, `pico-args`, or hand-rolled
  `std::env::args` dispatch. Device I/O lives in `sticky-host` via the
  [`espflash`](https://crates.io/crates/espflash) **library**
  (`default-features = false`, `serialport`); [`cargo-espflash`](https://crates.io/crates/cargo-espflash)
  is a binary-only Cargo plugin wrapping that crate. Do not enable
  espflash's `cli` feature. `sticky-host` and xtask require rustc 1.88
  (espflash 4.5); the `no_std` crates stay at workspace MSRV 1.85. Live
  `sticky-host` methods take [`uart_lock::try_acquire`](xtask.md#uart-session-lock-shared)
  internally. New UART-touching in-repo tools reuse that same lock; do not
  invent another. UART-touching subprocesses go through
  `UartSession::status` / `output` so the flock covers the child.
- Verify with `cargo test --locked`,
  `cargo clippy --locked --all-targets -- -D warnings`, and
  `cargo fmt --check`. Do not advertise `cargo test --workspace` (that
  will pull Xtensa firmware members when they exist).
- One workspace lockfile is committed. Pass `--locked` and keep the claimed
  MSRV. After changing `host/sticky-host/Cargo.toml`, `xtask/Cargo.toml`,
  or workspace members, refresh it with `cargo generate-lockfile`.
- Use [Conventional Commits](https://www.conventionalcommits.org/).
- Silicon discovery and device I/O use `cargo xtask`. Do not add a parallel
  `esptool` / `espflash` CLI recipe when xtask already answers. Host-only
  `cargo xtask build-fw` wraps `cargo +esp` and `espflash save-image` (no
  port) for a `flash-app` payload.
- When the `cargo xtask` CLI changes (new or removed subcommand, renamed
  flag, host-vs-live split, or safety contract), update
  [xtask.md](xtask.md) **and** the xtask command list in
  [README.md](../../../../README.md) in the same change. Touch `AGENTS.md`
  only if the live-ask or safety set changed. `cargo xtask --help` is not a
  substitute for those two lists.
