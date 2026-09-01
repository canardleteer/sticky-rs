# seeed-reterminal-sticky

Board types only: pins, latch, rails, transforms, read-only SD
identify. Chip registers live in driver crates; UART / I2C / SPI live
in the firmware HAL. Do not turn this crate into an `esp-hal` wrapper.
`touch::to_screen` takes a GT911 **480×800** sample.

- GPIO0 (sensor SCL) and GPIO3 (touch SDA) are straps. Never assign
  them to the SPI controller.
- GPIO7 is input-only. Schematic Rev 01 ties IMU INT1 and gauge GPOUT
  to the same pin. Do not enable both chips as push-pull.
- Never erase flash or write below `0x90000` except a restore of that
  unit. Custom images: `cargo xtask flash-app` into factory `app0`.
- Do not ship a waveform LUT from this crate.

Schematic Rev 01 settled GPIO7 (shared), GPIO9 divider, `CHARGE_STATUS`
(STAT), SD detect (insert = 0), and UART0 43/44. STAT on a physical unit:
low while `/CE` enabled, high after park and a settle. Panel+SD SPI
at 10 MHz and 20 MHz ACKed on a physical unit; crate `SPI_MAX_HZ` stays 10 MHz.
Still open: `MicRail` / `SdRail` settle times, GPIO46 pulse,
charge-to-done.

Live-ask and never-erase: root [AGENTS.md](../../AGENTS.md). Pin map:
[seeed-sticky-hardware](../../.agents/skills/seeed-sticky-hardware/SKILL.md).

This crate’s `README.md` is the crates.io landing page. Relative
markdown links there only resolve inside this package.

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
