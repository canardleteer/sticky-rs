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
- Panel: USB-down portrait pages (FaceUp / FaceDown too). OTP gray4
  splash (dark Ferris + `sticky-rs`) and four-tone boxes; OTP 1-bit
  shapes / legend. No `0x32` LUT, no Lotus `0x21`.
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
