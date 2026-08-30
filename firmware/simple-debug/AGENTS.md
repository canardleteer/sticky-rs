# simple-debug-fw

Blocking `esp-hal` proof-of-life image. Workspace member, **not** a
default-member: host `cargo test` must not compile this package.

Live-ask, never-erase, and flash I/O: root
[AGENTS.md](../../AGENTS.md). How-to:
[docs/getting-started.md](../../docs/getting-started.md).

## Envelope

- Latch GPIO45 then GPIO46 before logs or buses.
- Park BQ25616 `/CE` disabled. Do not enable charging.
- GPIO7 is input-only. Do not drive it. Do not enable gauge GPOUT.
- No e-paper LUT or refresh. Panel rail may be up; CS stays idle-high.
- Gauge: standard-command reads only (`bq27220` without `config-write`).
- MicroSD: CS idle-high. Do not mount.
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

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
