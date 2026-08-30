# sticky-host

Programmatic host API for Seeed reTerminal Sticky UART detect, factory
backup / confirm / restore, host-only `build-fw`, `app0` `flash-app`,
learn-uart, and no-reset monitor.

`cargo xtask` is the clap front-end; a later standalone CLI will depend
here too. Callers pass a `Layout` (developer-data / backups root), not a
hardcoded repo path. Live methods that reset or listen take the UART session lock
internally.

Do not open a port unless a human explicitly asked. Inventory without probe
and host-only import / `diff-learn-uart` / `build-fw` / `ci` do not take
the lock.

Safety and live-ask rules:
[AGENTS.md](https://github.com/canardleteer/sticky-rs/blob/main/AGENTS.md).
Snapshot how-to:
[firmware-snapshot-management.md](https://github.com/canardleteer/sticky-rs/blob/main/docs/firmware-snapshot-management.md).
Command catalog:
[xtask.md](https://github.com/canardleteer/sticky-rs/blob/main/.agents/skills/sticky-rs/references/xtask.md).

License: MIT
