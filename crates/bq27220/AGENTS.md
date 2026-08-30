# bq27220

CEDV gauge. Never adopt `bq27xxx` (wrong family). Keep `config-write`
off unless a human asked; there is no `enter_cfgupdate` helper on
purpose.

Do not invent datasheet-looking constants. Gaps go in the hardware
skill catalog. Ask the user to populate the datasheet cache; do not
download vendor PDFs unless they asked.

Board hazards: [docs/SAFETY.md](../../docs/SAFETY.md).

This crate’s `README.md` is the crates.io landing page. Relative
markdown links there only resolve inside this package.

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
