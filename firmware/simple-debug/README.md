# simple-debug-fw

Blocking `esp-hal` proof-of-life image for the reTerminal Sticky. No
Embassy, no RTOS, no panel refresh. Host-tested line format:
[`crates/simple-debug`](../../crates/simple-debug).

```text
simple-debug: t=12 vbus=1 gpio7=1 gpio40=0 sd_cd=1 soc=87 v=3870 i=-12 imu=FaceUp
```

`--features operator` adds GT911 contact lines and a 20 ms GPIO poll for
`learn-uart`. Agent flash contract and envelope:
[AGENTS.md](AGENTS.md). First-time toolchain:
[docs/getting-started.md](../../docs/getting-started.md).
