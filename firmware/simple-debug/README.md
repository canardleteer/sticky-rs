# simple-debug-fw

Blocking `esp-hal` proof-of-life image for the reTerminal Sticky. No
Embassy, no RTOS, no panel refresh. Host-tested line format:
[`crates/simple-debug`](../../crates/simple-debug).

```text
simple-debug: t=12 vbus=1 gpio7=1 gpio40=0 sd_cd=1 soc=87 v=3870 i=-12 imu=FaceUp
```

On the unit:

- Default: a 1 s heartbeat of USB-C present, the three right-edge keys
  as levels, battery SoC / V / I, and IMU pose. The glass does not
  refresh.
- `--features operator` polls GPIO every 20 ms so `learn-uart` sees
  key / VBUS / SD edges, and prints GT911 contact-count lines when a
  finger is on the glass.

Agent / toolchain:

- Agent flash contract and envelope: [AGENTS.md](AGENTS.md).
- First-time toolchain:
  [docs/getting-started.md](../../docs/getting-started.md).
