# sticky-host

Host library. Callers pass a `Layout` (developer-data / backups root),
not a hardcoded repo path. `cargo xtask` is the clap front-end; do not
put clap types here.

Live methods that reset or listen take the UART session lock. Do not
open a port unless a human **explicitly asked** that live command.
Host-only: inventory without `--probe`, `--import`, `diff-learn-uart`,
`build-fw`, `ci`.

Live-ask set and never-erase: root [AGENTS.md](../../AGENTS.md).
Catalog: [xtask.md](../../.agents/skills/sticky-rs/references/xtask.md).

This crate’s `README.md` is the crates.io landing page. Relative
markdown links there only resolve inside this package.

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
