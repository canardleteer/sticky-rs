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
the rail and I2S PDM RX (16 kHz, 16-bit, left; a community recipe
(source?), and not yet confirmed on this unit). Snapshot first:
[docs/getting-started.md](../../docs/getting-started.md).

To perform the test:

### Step 1: Is the port free?

`flash-app` and `monitor` need a Sticky serial port. `lsusb` showing
QinHeng `1a86:55d3` is not enough.

Only one `monitor` at a time. If an old listen is still running, Ctrl-C
that terminal. Do not `kill -9`.

Then:

```shell
cargo xtask detect-connected
```

You should see a Sticky path. If you do not, and you already killed a
listen the hard way, unplug the USB-C cable and plug it back in once.
Run `detect-connected` again.

### Step 2: Build, flash, and listen

```shell
. $HOME/export-esp.sh
cargo xtask build-fw embassy-debug --features mic
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
cargo xtask monitor
```

The image is on the chip only after `flash-app` finishes. A successful
build alone does not flash. If `flash-app` says no QinHeng CH343, go
back to Step 1.

Ctrl-C when you are done so the next `flash-app` can see the device.
Do not `kill -9` that listen. If you already did, unplug and replug
once (same as Step 1).

### Step 3: What you should see

You should still see `embassy-debug: latched` and the
usual `btn` / `touch` / `imu=` lines. About four times a second:

```text
embassy-debug: t=1204 mic rms=12 peak=40
```

### Step 4: Observe and report

- **Quiet room**: low, stable `rms` / `peak` values.
- **Make Noise**: Scratch or tap the **microphone hole** on the USB-C
  side/edge of the device.
  - Those numbers should jump. A key-down beep is a weaker extra
    stimulus.

**If you observe**: Always-zero or always-max means the mux, slot, or
rail is wrong, and should not be considered a passing test. Do not treat
that result as closing
[`nyc-mic-pdm`](../../.agents/skills/seeed-sticky-hardware/resources/not-yet-confirmed.md#nyc-mic-pdm).
