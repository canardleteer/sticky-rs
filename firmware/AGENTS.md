# firmware/

Xtensa images. `simple-debug` and `embassy-debug` are workspace
members, **not** default-members. Host `cargo test` must not
compile them. Root rules still apply: [AGENTS.md](../AGENTS.md).
Board contract:
[seeed-sticky-hardware](../.agents/skills/seeed-sticky-hardware/SKILL.md).

Human how-to (what the image does, numbered flash / listen
steps) lives in each package `README.md` and
[docs/getting-started.md](../docs/getting-started.md). Keep
live-ask, envelope, named constants, and silicon notes in the
package `AGENTS.md`.

Do not `idf.py flash`, `espflash flash`, or `erase-flash` from
this tree. Host I/O stays `cargo xtask`. `build-fw` then
`flash-app` (`app0` at `0x90000`). Do not add a Cargo `runner`.

## Packages

Keep Xtensa packages out of `default-members`. rust-analyzer
excludes `simple-debug-fw` and `embassy-debug-fw`.

| Path | Stack | Status |
| --- | --- | --- |
| `simple-debug/` | blocking `esp-hal` | Member. Latch, park hazards, I2C facts, UART heartbeat. No Embassy. `--features operator` for `learn-uart` |
| `embassy-debug/` | `esp-hal` + Embassy | Member. Panel always on. Default image is keys / glass / IMU / oriented cards / pair (advertise only on that card). `mic` / `radio` / `sd` / `charge` / `spi20` are opt-in and exclusive where the package `AGENTS.md` says so. Pair verification (phone or host BlueZ Connect): [embassy-debug/AGENTS.md](embassy-debug/AGENTS.md#bluetooth-pairing-verification-workflow) |

Envelope for every image:

- Latch GPIO45 then GPIO46 before logs or buses
- No invented e-paper LUT (OTP only)
- No gauge unseal / `CFGUPDATE` / FCC write
- No writes below `0x90000` except restore of that unit
- Never print a MAC, factory serial, or USB serial on UART
  from a custom image’s “interesting” lines (pair / radio /
  learn-uart tokens stay names and counts)

## Firmware examples as tutorial code

Firmware under `firmware/` (`simple-debug-fw` and `embassy-debug-fw`)
must serve as educational reference code. Every function, method,
struct, enum, and constant (public or private) must have comprehensive
rustdoc explaining what it does, hardware nets/buses involved,
expectations, and error handling. Include abundant in-line comments
explaining hardware register sequencing, GPIO electrical configurations
(pull-ups, input modes), bus arbitration, Embassy task scheduling,
stack buffer usage, and reset/wake-up cycles. Ground descriptions
in authoritative terminology from *The Embedded Rust Book*,
*The Rust on ESP Book*, and *The Embassy Book*.

Do not leave a protocol, rail, or UART token unexplained because the
item is `fn` not `pub fn`. Host-tested line format stays in the
`crates/*` twin; the Xtensa file teaches how that line is produced.

## Named constants and datasheets

Do not put magic numbers or bytes in firmware. Use grouped
`enum` / `const` values with logical names. Every definition
comments **what it means** and **where it came from**. Prefer
the board crate over a second copy of a GPIO number. Cite
extract **heading titles**, not page numbers. Do not invent
registers or opcodes. Catalog:
[docs/DATASHEETS.md](../docs/DATASHEETS.md).

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without
assumptions about user-specific paths or session state.
