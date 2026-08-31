# ssd1677-gray4

MCU-agnostic SSD1677 driver. Board pins, latch, and OTP mode pick
belong in `seeed-reterminal-sticky`.

Never invent a four-gray LUT or ship a default 105-byte `0x32` table.
`standby` / `resume` are `UpdateSequence::STANDBY` / `RESUME` then
`Command::MasterActivation` on `Active`. On this Sticky, `resume`
does not drop BUSY; firmware may RST + OTP `init`. Deep sleep is
`Command::DeepSleepMode` / `DeepSleep::Enter`. Do not write those
opcodes as raw hex in new code.
Sticky path is panel OTP (`Config::lut = None`) with
`PlaneMapping::SEEED_OTP`. Do not mix that mapping with an MCU table.

Board hazards: [docs/SAFETY.md](../../docs/SAFETY.md). Waveform
write-up: [docs/ssd1677.md](../../docs/ssd1677.md).

This crate’s `README.md` is the crates.io landing page. Relative
markdown links there only resolve inside this package.

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
