# embassy-debug-fw

Embassy event-logger image. Workspace member, **not** a default-member:
host `cargo test` must not compile this package. The panel is always on.

Live-ask, never-erase, and flash I/O: root
[AGENTS.md](../../AGENTS.md). Parent contract:
[firmware/AGENTS.md](../AGENTS.md). How-to:
[docs/getting-started.md](../../docs/getting-started.md).

## Envelope

- Latch GPIO45 then GPIO46 before logs or buses.
- Park BQ25616 `/CE` disabled. Default image does not enable
  charging. `--features charge` is an attended ≤ 2 s `/CE` pulse
  when GPIO9 is high after a cold boot or a 1 s Page Up resume
  hold, then park. A wake that re-sleeps does not pulse `/CE`.
  Do not combine with `mic`, `radio`, `pair`, or `sd`. Do not
  flash that feature unless the operator is present.
- GPIO7 is input-only (IMU INT1 and gauge GPOUT share it). Do not
  drive it.
- MicroSD: CS idle-high on the default image. `--features sd` is
  read-only identify plus a FAT root list and one `ReadOnly` file
  read. No writes, no CID product serial, no file contents on UART.
  Do not combine with `mic` or `radio` (`compile_error!`).
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
  ([touch.md](../../.agents/skills/seeed-sticky-hardware/references/touch.md#on-a-physical-unit-embassy-debug)).
  `to_screen` takes the 480×800 sample; USB-down ink corners land on
  800×480. INT-high + init Status-clear stayed at `st=0x00`.
- Panel: splash, shapes, legend, tones, pair, and the Ferris
  off-screen follow the four in-plane IMU holds (portrait 480×800
  and landscape 800×480). FaceUp / FaceDown keep the last of those.
  Pair idle is a framed how-to with empty PIN boxes; digits appear
  only after `pair pin=`. Advertise only on that card. Legend is a
  document (keys, sleep / standby / power, OTP), not 72×72 nub boxes.
  OTP gray4 splash / legend / tones / pair / Ferris-off; OTP 1-bit
  shapes. No `0x32` LUT, no Lotus `0x21`.
  Default clock is board `SPI_MAX_HZ` (10 MHz). `--features spi20`
  clocks the panel at 20 MHz; UART prints `spi=20000000`.
  Do not combine `spi20` with `mic` or `radio` (`compile_error!`).
- Microphone: default image leaves `MicRail` disabled. `--features mic`
  enables the rail and I2S PDM RX (16 kHz mono left; energy is live
  on a physical unit and does not close nyc-mic-pdm). AI Voice dumps
  two PCM windows and leaves the buzzer off; it does not change
  the page. How-to:
  [README.md](README.md#microphone-test-instructions).
- Radio: default image leaves Wi-Fi and BLE off. `--features radio`
  scans both at once on the on-board antenna (on a physical unit: `wifi n=`
  and `ble n=` in one listen). Scan only; no NVS writes; no MAC /
  BSSID. Active scan still transmits probe / scan requests. The
  radios stay up until reset; this image does not deinit them
  before deep sleep. How-to:
  [README.md](README.md#radio-test-instructions).
- Pair: default image includes BLE. Advertise `sticky-rs`
  (DisplayOnly passkey) **only while `scene=pair` is showing**.
  Walking away stops advertising and drops a connection. RAM bonds
  this boot only; no factory NVS; no MAC on UART. Do not combine
  with `mic`, `radio`, `charge`, or `sd` (`build-fw` / `ci` pass
  `--no-default-features` for those sits). Pairing success is
  confirmed on a physical unit (host BlueZ Connect, UART
  `pair pin=` then `pair ok`, pair card showed `Paired`).
  Walkthrough (rustdoc on private items too):
  [src/pair.rs](src/pair.rs). How-to:
  [README.md](README.md#pair-test-instructions).
- Panel standby: hold Page Up 2 s. `UpdateSequence::STANDBY` then
  `MasterActivation`. The sit stays until Page Up 1 s (resume) or
  Page Up 5 s (MCU sleep). Stock `RESUME` (`0xC0`) and
  `ENABLE_CLOCK` (`0x80`) left BUSY high on this unit. Firmware
  pulses RST, OTP `init`, UART `epd resume rst` / `resume` /
  same `scene=`. `EPD_EN` stays high. Not MCU deep sleep. Not
  RAM-keep resume.
- Deep sleep: hold Page Up 5 s (the same hold can enter standby
  at 2 s first). The image paints Ferris (splash), sends SSD1677
  `DeepSleepMode` / `DeepSleep::Enter`, cuts `EPD_EN`, keeps the
  latch high, and wakes on GPIO5 `ext1` ANY_LOW. A failed Ferris
  paint stays awake (`EPD_EN` on). A failed `DeepSleepMode` write
  holds `EPD_EN` high (does not cut the rail) and then MCU-sleeps.
  Hold Page Up 1 s after wake to restore Ferris. Early release
  re-sleeps without painting. GPIO4 is still the stock/docs wake
  pin; this image uses GPIO5 because the gesture is Page Up.
- Power off: hold Page Down 5 s. Ferris, panel `DeepSleepMode`,
  cut `EPD_EN`, then `Latch::release`. That is a real power cut.
  Power-on is USB-C plug (firmware latches at boot) or the stock
  ~3 s AI Voice hold. Recessed Reset or a USB unplug/replug is a
  POWERON (splash) when a rail is already present.
  Sit sleep with `cargo xtask monitor` **without** `--acm-tty`
  (`--acm-tty` pulses EN and is a POWERON). No writes
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
`sticky-rs`) → shapes → legend → tones → pair. BLE advertises
only on `scene=pair`. This is not the `learn-uart` operator
format.

Host-tested lines live in `crates/embassy-debug`:

```shell
cargo test -p embassy-debug --locked
```

## Bluetooth pairing verification workflow

When testing the default pair card, two verification pathways are
supported. Always offer the human the option to test with their
own devices. Pairing success is **confirmed on a physical unit**.
Do not print or store a MAC.

1. **Manual external device pairing.** The human walks Page Down
   to `scene=pair`, searches for `sticky-rs` from their phone or
   other central, starts pairing, reads the six-digit passkey on
   the pair card, types it on the phone, and watches for `Paired`
   or `Pair failed`. UART should print `pair pin=` then `pair ok`
   or `pair fail=`. Human how-to:
   [README.md](README.md#pair-test-instructions).
2. **Host-agent self-diagnostic pairing (faster for agents).** If
   the host has an available, unblocked Bluetooth controller
   (BlueZ) **and** the human explicitly asked for this sit, the
   agent may:
   - Listen with `cargo xtask monitor` (not `--acm-tty`) so Drop
     reattaches `cdc-acm`. Default CDC listen needs write access
     on the usbfs node: a udev rule in `/etc/udev/rules.d/` (not
     `/etc/udev/`) for `1a86:55d3`, then reload and a USB replug.
   - Scan LE for advertise name `sticky-rs`. Do not read or print
     the eFuse MAC. Stop discovery before Connect.
   - **Connect only.** Do not call BlueZ `Device1.Pair()` or
     `bluetoothctl pair`. The image’s `request_security()` already
     sends SMP Security Request (`0x0B`). A concurrent `Pair()`
     makes Linux log `unexpected SMP command 0x0b` and return
     `AuthenticationCanceled`.
   - Wait for a **new** UART `pair pin=` after that Connect (do
     not reuse digits from an earlier attempt).
   - Submit those six digits through a KeyboardOnly agent
     `RequestPasskey`. Do not block the D-Bus / GLib loop while
     waiting for UART (that yields `NoReply` or a cancel).
   - Look for `pair ok` (or `pair fail=` plus a why token) and
     host `Paired`. Ask the human to confirm the same PIN and
     `Paired` / `Pair failed` on the pair card.

On a physical unit the host path completed: UART `pair pin=` then
`pair ok`, host `Paired` / `Connected`, pair card showed `Paired`.
`btleplug` cannot enter a DisplayOnly passkey (GATT only). A
Linux xtask would wrap BlueZ (`bluer` or D-Bus), not
`bluetoothctl`. Bonds are RAM this boot only. Do not write
factory NVS. Do not combine `pair` with `mic`, `radio`, `charge`,
or `sd`.

## Firmware examples as tutorial code

Firmware under `embassy-debug/` serves as an educational reference
and walkthrough for async Embassy on ESP32-S3. Every function,
method, struct, enum, and constant (public or private) must have
comprehensive rustdoc explaining what it does, hardware nets/buses
involved, expectations, and error handling. Include abundant in-line
comments explaining hardware register sequencing, GPIO electrical
configurations (pull-ups, input modes), bus arbitration, Embassy task
scheduling, stack buffer usage, and reset/wake-up cycles. Ground
descriptions in authoritative terminology from *The Embedded Rust Book*,
*The Rust on ESP Book*, and *The Embassy Book*.

The pair walkthrough is [src/pair.rs](src/pair.rs) (default image).
Card layouts (IMU page, Koch, document legend, boxed PIN) are
[src/draw.rs](src/draw.rs).

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
