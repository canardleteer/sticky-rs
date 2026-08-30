# bq25616

GPIO control for the TI BQ25616 standalone battery charger.

The part has no I2C interface, so the entire risk surface is one polarity
mistake: `/CE` is **active low**. This crate makes that mistake unrepresentable
rather than documented.

- `Charger::new` parks the charger **disabled**, whatever state the pin was in.
- Charge state lives in the type (`Charger<CE, Disabled>` / `Charger<CE, Enabled>`),
  so no caller writes a raw level.
- A failed transition hands the charger back, so the pin is never lost.
- `ChargeStatus` reports a raw `Level` and not `is_charging()`: on the
  reTerminal Sticky that net's polarity is unmeasured, and guessing would
  convert an open question into a silent assumption.

`#![no_std]`, `embedded-hal` 1.0 only, no MCU dependency.

See [docs/SAFETY.md](https://github.com/canardleteer/sticky-rs/blob/main/docs/SAFETY.md)
for the board-level hazards and
[TI SLUSDF7](https://www.ti.com/lit/ds/symlink/bq25616.pdf) for the part.

License: MIT
