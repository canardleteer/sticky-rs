# embassy-debug-fw

Embassy event-logger image. Workspace member, **not** a default-member:
host `cargo test` must not compile this package. The panel is always on.

Live-ask, never-erase, and flash I/O: root
[AGENTS.md](../../AGENTS.md). How-to:
[docs/getting-started.md](../../docs/getting-started.md).

## Envelope

- Latch GPIO45 then GPIO46 before logs or buses.
- Park BQ25616 `/CE` disabled. Do not enable charging.
- GPIO7 is input-only. Do not drive it.
- MicroSD: CS idle-high. Do not mount.
- Gauge: not used. No unseal, no data-memory writes.
- Touch: rail on, INT-during-reset, 100 kHz, INT left floating. No
  config-RAM write.
- Panel: splash follows the four in-plane IMU holds (portrait 480×800
  and landscape 800×480). FaceUp / FaceDown keep the last of those.
  Legend, tones, and shapes stay USB-down portrait. OTP gray4 splash /
  legend / tones; OTP 1-bit shapes. No `0x32` LUT, no Lotus `0x21`.
- Microphone: default image leaves `MicRail` disabled. `--features mic`
  enables the rail and I2S PDM RX (16 kHz mono left). AI Voice plays a
  1 kHz buzzer tone and dumps two PCM windows; it does not change the
  page. How-to:
  [README.md](README.md#microphone-test-instructions).
- No deep sleep. No writes below `0x90000`. No Cargo `runner`.

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
