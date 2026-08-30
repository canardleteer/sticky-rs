# sticky-host

Programmatic host API for Seeed reTerminal Sticky UART detect, factory
backup / confirm / restore, host-only `build-fw`, `app0` `flash-app`,
learn-uart, and no-reset monitor.

`cargo xtask` is the clap front-end. Callers pass a `Layout`
(developer-data / backups root), not a hardcoded repo path.

License: MIT
