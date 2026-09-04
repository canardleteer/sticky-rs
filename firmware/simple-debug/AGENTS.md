# simple-debug-fw

Blocking `esp-hal` proof-of-life image. Workspace member, **not** a
default-member: host `cargo test` must not compile this package.

Live-ask, never-erase, and flash I/O: root
[AGENTS.md](../../AGENTS.md). Parent contract:
[firmware/AGENTS.md](../AGENTS.md). How-to:
[docs/getting-started.md](../../docs/getting-started.md).

## Envelope

- Latch GPIO45 then GPIO46 before logs or buses.
- Park BQ25616 `/CE` disabled. Do not enable charging.
- GPIO7 is input-only (IMU INT1 and gauge GPOUT share it). Do not
  drive it. Do not enable gauge GPOUT as push-pull.
- No e-paper LUT or refresh. Panel rail may be up; CS stays idle-high.
- Gauge: standard-command reads only (`bq27220` without `config-write`).
- MicroSD: CS idle-high. Do not mount. `sd_cd` is GPIO only.
- SHT40 / PCF8563: measure and time **reads** only. Do not print the
  SHT serial. Do not `init` or set the RTC.
- GT911: ACK and `id=911` only on this image. Attended
  `gt911_contacts` has always timed out (`st=0x00`, `int=1`). Do not
  treat that as a successful touch capture. Contacts closed on
  embassy-debug INT-low address select (`touch n=5`).
- No deep sleep. No writes below `0x90000`. No Cargo `runner`.

## Flash and UART

`flash-app` does not compile. From the repo root, after a matching
snapshot:

```shell
cargo xtask build-fw simple-debug --features operator
cargo xtask learn-uart --image target/xtensa-esp32s3-none-elf/release-fw/simple-debug.bin --yes --restore-app0
```

Omit `--features operator` for the 1 s heartbeat (no GT911 after the
boot ACK). `--features operator` on a host crate is a different package.

Host-tested lines live in `crates/simple-debug`:

```shell
cargo test -p simple-debug --locked
```

## Firmware examples as tutorial code

Firmware under `simple-debug/` serves as an educational reference
and walkthrough for bare-metal blocking `esp-hal`. Every function,
method, struct, enum, and constant (public or private) must have
comprehensive rustdoc explaining what it does, hardware nets/buses
involved, expectations, and error handling. Include abundant in-line
comments explaining hardware register sequencing, GPIO electrical
configurations (pull-ups, input modes), stack buffer usage, and
reset cycles. Ground descriptions in authoritative terminology from
*The Embedded Rust Book* and *The Rust on ESP Book*.

This image has no BLE pair path. Pairing lives on default
`embassy-debug` (`scene=pair`).

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
