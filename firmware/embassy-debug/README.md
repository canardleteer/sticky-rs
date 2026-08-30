# embassy-debug-fw

Embassy event-logger image for the reTerminal Sticky. Timestamped
button / GT911 / IMU lines on UART0, a short beep, and the panel
(OTP 1-bit scenes plus four-tone gray4 boxes). Host-tested line format:
[`crates/embassy-debug`](../../crates/embassy-debug).

```text
embassy-debug: t=1204 btn 4 down
embassy-debug: t=2100 touch n=1 p0=123,456
embassy-debug: t=5000 imu=FaceUp x=12 y=-30 z=16300
```

On the unit:

- Cold boot paints a portrait splash (USB-C down) or a landscape
  splash (USB-C right / left) so Ferris and `sticky-rs` stay upright.
  FaceUp / FaceDown keep the last in-plane page.
- AI Voice / Page Up / Page Down (right-edge top / middle / bottom)
  walk splash → shapes → legend → four-tone OTP gray boxes.
- Tap the glass for `touch` lines; tilt the card for `imu=…`. A short
  beep answers a key-down and the first finger on the glass.

Agent / toolchain:

- Agent flash contract and envelope: [AGENTS.md](AGENTS.md).
- First-time toolchain:
  [docs/getting-started.md](../../docs/getting-started.md).

## Microphone Test Instructions

Default `embassy-debug` leaves `MicRail` disabled. This feature enables
the rail and I2S PDM RX (16 kHz, 16-bit, left; community recipe, not
confirmed on this unit). Snapshot first:
[docs/getting-started.md](../../docs/getting-started.md).

```shell
. $HOME/export-esp.sh
cargo xtask build-fw embassy-debug --features mic
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
cargo xtask monitor
```

Ctrl-C ends monitor and hands `cdc-acm` back so the next `flash-app`
can see the CH343. Do not `kill -9` that listen.

You should still see `embassy-debug: latched` and the usual `btn` /
`touch` / `imu=` lines. About four times a second:

```text
embassy-debug: t=1204 mic rms=12 peak=40
```

Quiet room: low, stable `rms` / `peak`. Scratch or tap the **microphone
hole** on the USB-C short edge (Reset / lanyard / charge LED / USB-C).
Those numbers should jump. A key-down beep is a weaker extra stimulus.

Always-zero or always-max means the mux, slot, or rail is wrong — not
a passing test. Do not treat this as closing nyc-mic-pdm.
