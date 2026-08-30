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

- Cold boot paints splash: small Ferris, `sticky-rs`, then a smaller
  hint to use the right-edge keys.
- AI Voice / Page Up / Page Down (right-edge top / middle / bottom)
  walk splash → shapes → legend → four-tone OTP gray boxes.
- Tap the glass for `touch` lines; tilt the card for `imu=…`. A short
  beep answers a key-down and the first finger on the glass.

Agent / toolchain:

- Agent flash contract and envelope: [AGENTS.md](AGENTS.md).
- First-time toolchain:
  [docs/getting-started.md](../../docs/getting-started.md).
