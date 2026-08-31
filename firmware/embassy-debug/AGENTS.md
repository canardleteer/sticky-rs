# embassy-debug-fw

Embassy event-logger image. Workspace member, **not** a default-member:
host `cargo test` must not compile this package. The panel is always on.

Live-ask, never-erase, and flash I/O: root
[AGENTS.md](../../AGENTS.md). How-to:
[docs/getting-started.md](../../docs/getting-started.md).

## Envelope

- Latch GPIO45 then GPIO46 before logs or buses.
- Park BQ25616 `/CE` disabled. Default image does not enable
  charging. `--features charge` is an attended ≤ 2 s `/CE` pulse
  when GPIO9 is high, then park. Do not combine with `mic`,
  `radio`, or `sd`. Do not flash that feature unless the operator
  is present.
- GPIO7 is input-only (IMU INT1 and gauge GPOUT share it). Do not
  drive it.
- MicroSD: CS idle-high on the default image. `--features sd` is
  read-only identify plus a FAT root list and one `ReadOnly` file
  read. No writes, no CID product serial, no file contents on UART.
- Gauge: default image does not use it. `--features charge` reads
  `Current()` for the `ce` lines only. No unseal, no data-memory
  writes.
- Touch: rail on, then Rev.09 §6.1 INT-during-reset address select
  (400 kHz cap, INT=low then INT=high, INT driven after RST rises).
  No config-RAM write. No init `StatusWrite::Clear`. No
  `Register::Command`. Poll `Register::Points` (coords at byte 0);
  clear Status only after a ready frame. Read-only `gt911 st=` follows
  board [`STATUS_HEARTBEAT`](../../crates/seeed-reterminal-sticky)
  (`EverySecs(10)` or `Off`). On this
  unit: INT=0 → `0x5d` ACK, `touch n=5`, `st=0x85`
  ([touch.md](../../.agents/skills/seeed-sticky-hardware/references/touch.md#on-glass-embassy-debug)).
  `to_screen` takes the 480×800 sample; USB-down ink corners land on
  800×480. INT-high + init Status-clear stayed at `st=0x00`.
- Panel: splash follows the four in-plane IMU holds (portrait 480×800
  and landscape 800×480). FaceUp / FaceDown keep the last of those.
  Legend, tones, and shapes stay USB-down portrait. OTP gray4 splash /
  legend / tones; OTP 1-bit shapes. No `0x32` LUT, no Lotus `0x21`.
  Default clock is board `SPI_MAX_HZ` (10 MHz). `--features spi20`
  clocks the panel at 20 MHz; UART prints `spi=20000000`.
- Microphone: default image leaves `MicRail` disabled. `--features mic`
  enables the rail and I2S PDM RX (16 kHz mono left; on-glass energy
  is live, not a high-fidelity close of nyc-mic-pdm). AI Voice plays
  a 1 kHz buzzer tone and dumps two PCM windows; it does not change
  the page. How-to:
  [README.md](README.md#microphone-test-instructions).
- Radio: default image leaves Wi-Fi and BLE off. `--features radio`
  scans both at once on the on-board antenna (on glass: `wifi n=`
  and `ble n=` in one listen). Scan only; no NVS writes; no MAC /
  BSSID. How-to:
  [README.md](README.md#radio-test-instructions).
- Panel standby sit: hold Page Up 2 s.
  `UpdateSequence::STANDBY` then `MasterActivation`, look 2 s.
  Stock `RESUME` (`0xC0`) and `ENABLE_CLOCK` (`0x80`) left BUSY
  high on this unit. Firmware pulses RST, OTP `init`, UART
  `epd resume rst` / `resume` / same `scene=`. `EPD_EN` stays
  high. Not MCU deep sleep. Not RAM-keep resume.
- Deep sleep: hold Page Down 4 s. The image paints a sleep card, sends
  SSD1677 `DeepSleepMode` / `DeepSleep::Enter`, cuts `EPD_EN`, keeps
  the latch high, and wakes on GPIO6 `ext1` ANY_LOW. Hold Page Down
  1 s after wake to restore the same card. Early release re-sleeps
  without painting. Recessed Reset or a USB unplug/replug is a
  POWERON (splash). GPIO4 is still the stock/docs wake pin; this
  image uses GPIO6 because the gesture is Page Down. Do not
  `Latch::release`. Sit with `cargo xtask monitor` **without**
  `--acm-tty` (`--acm-tty` pulses EN and is a POWERON). No writes
  below `0x90000`. No Cargo `runner`.

## Flash and UART

`flash-app` does not compile. From the repo root, after a matching
snapshot:

```shell
cargo xtask build-fw embassy-debug
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
cargo xtask monitor
```

Right-edge keys change the page (`scene=…`): splash (Ferris +
`sticky-rs`) → shapes → legend → tones. This is not the `learn-uart`
operator format.

Host-tested lines live in `crates/embassy-debug`:

```shell
cargo test -p embassy-debug --locked
```

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
