# `sticky-rs`

> **Embedded Rust Tooling & Crates for the [Sticky](https://www.seeedstudio.com/sticky/docs/)**

> [!NOTE]
> I do have a "functioning" Embedded Rust dev environment for the Sticky,
> but I'm porting it over to clean git history slowly.
>
> For now, I'm just going to include the skill and safety information,
> but am working on moving the rest over as I can.

Hardware notes and a board contract for the Seeed Studio reTerminal Sticky.
Host tools are: `cargo xtask`
(clap front-end) over `host/sticky-host`.

**Read [docs/SAFETY.md](docs/SAFETY.md) before flashing or probing a unit.**
A mistake can destroy factory NVS (per-unit RF calibration), the fuel-gauge
OTP, or the panel. Getting started (host verify, Xtensa, both firmware
paths): [docs/getting-started.md](docs/getting-started.md). Snapshot
how-to:
[docs/firmware-snapshot-management.md](docs/firmware-snapshot-management.md).
The pin map, rails, and source precedence live in
[`.agents/skills/seeed-sticky-hardware/`](.agents/skills/seeed-sticky-hardware/SKILL.md).
Host I/O is `cargo xtask` only. Do not open a UART unless a human asked.
`build-fw` is host-only. Flag
catalog: [`.agents/skills/sticky-rs/references/xtask.md`](.agents/skills/sticky-rs/references/xtask.md).

## cargo xtask

From the repo root (`cargo xtask <subcommand>`). `cargo xtask --help` lists
flags. Live commands take `--port` or `ESPFLASH_PORT`; if unset they need
exactly one QinHeng CH343 (`1a86:55d3`). Safety: [docs/SAFETY.md](docs/SAFETY.md).

| Command | UART? | Summary |
| --- | --- | --- |
| `detect-connected` | no, unless `--probe` | List Sticky CH343 nodes. `--probe` opens the UART |
| `backup-factory-firmware` | live dump yes; `--import` no | Classify then store: known factory → `original/<serial>/` (write-once); else `--name` capture under `captures/<unit-id>/<slug>/`. Alias `backup-firmware`. `--as-original` for uncertain stock |
| `confirm-factory-firmware` | yes | Compare live flash to that unit's original, or `--capture SLUG` |
| `restore-factory-firmware` | yes | write-bin that unit's original or `--capture` (`--yes`). Never a full-chip erase |
| `flash-app` | yes | write-bin `--image FILE` into factory `app0` only. Needs a matching original or capture. Does not compile |
| `learn-uart` | yes | UART heartbeat vet plus skippable human steps |
| `learn-uart-only` | yes | Same session, only named groups |
| `diff-learn-uart` | no | Host-only compare of two reports or factory serials |
| `build-fw` | no | Host-only. `cargo +esp` + `save-image` for `simple-debug` or `embassy-debug` |
| `monitor` | yes | UART0 at 115200 |

## License

Sources in this repository are licensed under the MIT license. See
[LICENSE](LICENSE).

Seeed, reTerminal, Sticky, Espressif, and other product or company names are
trademarks of their respective owners. This project does not claim those
marks or their copyrights, and is not affiliated with or endorsed by those
owners.
