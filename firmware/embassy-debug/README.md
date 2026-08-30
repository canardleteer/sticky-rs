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

GPIO5 / GPIO6 cycle splash / shapes / legend / tones. Agent flash
contract and envelope: [AGENTS.md](AGENTS.md). First-time toolchain:
[docs/getting-started.md](../../docs/getting-started.md).
