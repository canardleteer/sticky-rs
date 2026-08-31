# bq25616

GPIO control for the TI BQ25616 standalone battery charger.

The part has no I2C interface, so the entire risk surface is one polarity
mistake: `/CE` is **active low**. This crate makes that mistake unrepresentable
rather than documented.

- `Charger::new` parks the charger **disabled**, whatever state the pin was in.
- Charge state lives in the type (`Charger<CE, Disabled>` / `Charger<CE, Enabled>`),
  so no caller writes a raw level.
- `Drop` on `Enabled` drives `/CE` high (best-effort). `release` of an
  enabled charger is still C-FREE and does not park.
- `enable_charging_if_external_power` refuses when VBUS sense is low.
- `hold_disabled` drives `/CE` high again on an already-parked charger.
- A failed transition hands the charger back, so the pin is never lost.
- `ChargeStatus` reports a raw `Level` and not `is_charging()`. On the
reTerminal Sticky that net is BQ25616 STAT (schematic Rev 01): low
while charging when `/CE` is enabled, high after park **and a
settle**. The crate still does not guess for you.

`#![no_std]`, `embedded-hal` 1.0 only, no MCU dependency.

Part: [TI SLUSDF7](https://www.ti.com/lit/ds/symlink/bq25616.pdf).
