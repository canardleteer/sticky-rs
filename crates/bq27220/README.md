# bq27220

Read-only driver for the TI BQ27220 battery fuel gauge.

The BQ27220 is a **CEDV** gauge, not an Impedance Track part, which is why
[`bq27xxx`](https://crates.io/crates/bq27xxx) (BQ27426/427) is the wrong driver
for this silicon rather than an incomplete one.

## Reads are safe; writes are opt-in

Default features expose reads only. Configuration lives in data memory behind
an unseal, and the documented update path is enter `CFGUPDATE`, write, verify,
exit, re-seal — every step timeout-prone, with a one-time-programmable OTP
behind it.

The `config-write` feature unlocks **raw primitives only**. There is
deliberately no `enter_cfgupdate()` or `set_full_charge_capacity()` helper: a
destructive sequence should not be reachable by autocomplete.

## Narrow on purpose

Typed accessors cover `Control`, `Voltage`, `Current`, `StateOfCharge`, and
`MACData`. Other offsets go through `read_u16`, and the CEDV data-memory block
layout is not implemented pending a page-by-page read of TI's technical
reference manual. A plausible-looking constant is worse than an honest gap.

`#![no_std]`, `embedded-hal` 1.0 only. The bus is never locked internally, so
the caller arbitrates the shared sensor bus.
