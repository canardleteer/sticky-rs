# simple-debug-fw

Blocking `esp-hal` proof-of-life image for the reTerminal Sticky. No
Embassy, no RTOS, no panel refresh. Host-tested line format:
[`crates/simple-debug`](../../crates/simple-debug).

```text
simple-debug: t=12 vbus=1 gpio7=1 gpio40=0 sd_cd=1 soc=87 v=3870 i=-12 imu=FaceUp
simple-debug: sht t=23400 rh=45100
simple-debug: rtc y=26 mo=8 d=30 h=15 mi=14 s=0 vl=0
```

`sht t=` is milli °C, `rh=` is milli % RH. `rtc y=` is the chip's 0–99
year. `vl=1` is the NXP seconds-register VL bit (integrity not
guaranteed). `sht none` / `rtc none` means that I2C read failed.

On the unit:

- Default: a 1 s heartbeat of USB-C present, STAT, SD detect, the
  three right-edge keys, battery SoC / V / I, IMU pose, then SHT40
  and RTC read lines. The glass does not refresh. CS stays idle-high
  (no card mount). `/CE` stays disabled.
- `--features operator` polls GPIO every 20 ms so `learn-uart` sees
  key / VBUS / SD edges. It **ACKs** GT911 `0x14` / `id=911` and
  prints `gt911 st=` / `int=` when board
  `touch::STATUS_HEARTBEAT` is on. It has **never**
  printed `contacts=` on an attended try (`st` stayed `0x00`,
  `int=1`). Contacts closed on embassy-debug INT-low address
  select
  ([touch.md](../../.agents/skills/seeed-sticky-hardware/references/touch.md#on-a-physical-unit-embassy-debug)),
  not on this image.

## Sensor and card-detect test

Snapshot first:
[docs/getting-started.md](../../docs/getting-started.md).

### Step 1: Is the port free?

Only one `monitor` at a time. Ctrl-C an old listen. Do not `kill -9`.

```shell
cargo xtask detect-connected
```

You should see a Sticky path. If you do not, and you already killed a
listen the hard way, unplug USB-C and plug it back in once. Run
`detect-connected` again.

### Step 2: Build, flash, and listen

```shell
. $HOME/export-esp.sh
cargo xtask build-fw simple-debug --features operator
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/simple-debug.bin --yes
cargo xtask monitor
```

The image is on the chip only after `flash-app` finishes. Ctrl-C when
you are done. Do not `kill -9`.

### Step 3: What you should see

USB-C plugged, left-edge slot empty:

```text
simple-debug: t=12 vbus=1 gpio7=0 gpio40=1 sd_cd=1 soc=87 v=3870 i=0 imu=FaceUp
simple-debug: sht t=23400 rh=45100
simple-debug: rtc y=26 mo=8 d=30 h=15 mi=14 s=0 vl=0
```

`vbus=1` is USB present (GPIO9 digital). `gpio40=1` is STAT with
`/CE` parked (not a charge proof). `sd_cd=1` is an empty slot.
`sht` numbers should look like room air, not `sht none`. `rtc`
prints whatever the chip has; `vl=1` is allowed if the clock was
never set. Right-edge keys still print `btn 4` / `5` / `6`.

### Step 4: Optional — insert a card

Push a MicroSD into the **left** long-edge slot. You should see
`sd_cd 1 -> 0`, then heartbeats with `sd_cd=0`. Pull it out:
`sd_cd 0 -> 1`. The image does not mount the card.

### Step 5: Observe and paste

Paste about ten seconds of UART: a heartbeat, the `sht` and `rtc`
lines, and any `sd_cd` edge if you used the slot. Fail: hang,
panic, or `sht none` / `rtc none` every second while the other
sensor-bus ACKs at boot still printed.

On a physical unit: `sht t` ~28900 / `rh` ~27900 (one room), `rtc` seconds
tick with `vl=0`. `gt911 st=0x00` / `int=1` with no `contacts=` is
the **only** simple-debug touch result so far, including every
attended `learn-uart` `gt911_contacts` step. Use embassy-debug
INT-low address select for a finger line
([touch.md](../../.agents/skills/seeed-sticky-hardware/references/touch.md#on-a-physical-unit-embassy-debug)).

Agent / toolchain:

- Agent flash contract and envelope: [AGENTS.md](AGENTS.md).
- First-time toolchain:
  [docs/getting-started.md](../../docs/getting-started.md).
