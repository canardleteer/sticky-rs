# remote-debug-host

Generic Linux host for the remote-debug GATT: framed
`sticky.remote.v1.Envelope` after DisplayOnly pairing.

This crate has **no** clap, **no** UART, and **no** Sticky pin map.
A second codebase implements the same documented 128-bit service
(`remote-debug-wire` `GATT_*` UUIDs) after *its* pairing policy and
feeds six-digit passkeys through [`PasskeySource`].

On Linux the transport is [`bluer`](https://crates.io/crates/bluer)
(BlueZ). Connect the advertise name (default `sticky-rs`); do **not**
call BlueZ `Device1.Pair()` / `bluetoothctl pair` — that races a
peripheral SMP Security Request. A KeyboardOnly agent answers
`RequestPasskey` without blocking the D-Bus loop. Never print a MAC.

`Session::reboot` writes a `Reboot` envelope (embedded MCU, not this
host), waits for `RebootAck` or a drop, and forgets the BlueZ bond.

Allowlisted long-term bonds and UART `pair pin=` scraping live in the
caller (`sticky-host` / `cargo xtask remote-debug`), not here.

License: MIT
