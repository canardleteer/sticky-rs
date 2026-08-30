# bq25616

GPIO-only charger control. MCU-agnostic: board pins and latch belong in
`seeed-reterminal-sticky`. Do not enable charging from firmware demos
unless a human asked and the safety notes were read.

Board hazards: [docs/SAFETY.md](../../docs/SAFETY.md). Charge-status
is BQ25616 STAT (low while charging when `/CE` is enabled). Still do
not invent `is_charging()`; report the raw level.

This crate’s `README.md` is the crates.io landing page. Relative
markdown links there only resolve inside this package.

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
