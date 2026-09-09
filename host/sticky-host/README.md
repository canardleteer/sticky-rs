# sticky-host

Programmatic host API for Seeed reTerminal Sticky UART detect, factory
backup / confirm / restore, host-only `build-fw`, `app0` `flash-app`,
learn-uart, no-reset monitor, and remote-debug UART `pair pin=`
scrape, remember-me allowlist, and the Unix-socket broker that
owns one GATT `Session` (`connect` auto-starts a detached serve
and returns `pairing`; poll `status` until `connected`).

`cargo xtask` is the clap front-end. Callers pass a `Layout`
(developer-data / backups root), not a hardcoded repo path.

License: MIT
