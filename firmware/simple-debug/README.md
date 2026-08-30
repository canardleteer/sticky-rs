# simple-debug-fw

Blocking `esp-hal` proof-of-life image for the reTerminal Sticky. It is not
product firmware. There is no Embassy executor and no RTOS.

It exists to confirm that the board-support crate's latch, rails, and buses
come up, then to print a heartbeat of **raw** GPIO and gauge levels on UART0
(CH343, 115200) without a panel refresh, a gauge configuration write, or a
charge-enable. Host-tested line format:
[`crates/simple-debug`](../../crates/simple-debug).

This image **cannot** close measurement-backlog electrical items. A UART
snapshot is firmware-observed sequencing, not a meter or a schematic.

The Xtensa package lives under `firmware/` (not `crates/`) and is a
workspace member but not a default-member, so host `cargo test` never
tries to compile `esp-hal`.

## Envelope

- Latch GPIO45 then GPIO46 and hold them high.
- Park BQ25616 `/CE` disabled. Do not enable charging.
- GPIO7 is input-only. Do not drive it. Do not enable gauge GPOUT.
- No e-paper LUT or refresh. Panel rail may be up; CS stays idle-high.
- Gauge: standard-command reads only (`bq27220` without `config-write`).
- MicroSD: CS idle-high, power enable on, detect as input. Do not mount.
- No deep sleep. No writes below `0x90000`. No Cargo `runner`.

## What it prints

At boot: latch, `git=<hash> dirty=<0|1>` (from `build.rs`), external-power GPIO9, I2C ACKs, gauge DeviceType, IMU accel
init, rail names plus panel geometry. Then once a second:

```text
simple-debug: t=12 vbus=1 gpio7=1 gpio40=0 sd_cd=1 soc=87 v=3870 i=-12 imu=FaceUp
```

On GPIO edges only: `btn 4 down`, `vbus 1 -> 0`, `sd_cd 1 -> 0`, and the
same for `gpio7` / `gpio40`. Buttons are active-low.

The touch bus is always **100 kHz**. With `--features operator`
the image also prints `simple-debug: prompt <step>` at boot for each human
step, samples GPIO about every 20 ms, writes `Register::Status` =
`STATUS_CLEAR` then `Register::Command` = `COMMAND_READ_COORDINATES`, prints
`simple-debug: gt911 st=0xNN` and `simple-debug: gt911 int=0` each heartbeat,
and prints `simple-debug: contacts=N` when the contact count changes. After
the address dance, GPIO21 (GT911 INT) is a **floating** input (`Pull::None`;
the ESP32-S3 pad has no default pull). Crate `NotReady` is no new buffer and
is ignored. I2C/product-ID errors print `simple-debug: gt911 poll failed`.
Default builds stay at a 1 s GPIO poll and do not talk to GT911 after the
boot ACK probe.

I2C probes: GT911 `0x14` after the INT-during-reset sequence, SHT40 with a
real Sensirion measure command (not a 1-byte read), PCF8563 `0x51`, BQ27220
`0x55`, LSM6DS3TR-C `0x6A`. It does not print factory serial, USB serial, or
MAC, and it does not call the SHT40 serial-number command.

Rows that need an operator (button press, USB unplug, card insert, tilt,
finger on glass) will sit idle if nobody is at the enclosure. Missing
edges are not a hardware failure.

## Build and install

Same contract as every in-repo `app0` write. Partition layout is the
host catalog id `factory-32mb-v1` (not a local CSV). How-to:
[docs/firmware-snapshot-management.md](../../docs/firmware-snapshot-management.md).

1. `cargo xtask backup-factory-firmware` once per unit (gitignored).
   Known factory → write-once `original/`; already-flashed units need
   `--name` (a capture). `flash-app` accepts either as a safety net.
2. From the **repo root**, source the script `espup` printed (example:
   `$HOME/export-esp.sh`) and run `cargo xtask build-fw`. The package
   name is `simple-debug-fw`; `--features operator` on a host crate is a
   different package. If the build fails, do not flash a leftover ELF.

   ```shell
   # source the script `espup` printed (example: . $HOME/export-esp.sh)
   cargo xtask build-fw simple-debug --features operator
   ```

   Omit `--features operator` for the quieter 1 s heartbeat image.
   (`esp_app_desc!()` is required; do not `--merge`. There is no Cargo
   `runner`, so `cargo run` cannot flash.)
3. Human-at-enclosure UART vet (flashes then listens in one session):

   ```shell
   cargo xtask learn-uart \
     --image target/xtensa-esp32s3-none-elf/release-fw/simple-debug.bin \
     --yes \
     --restore-app0
   ```

   YAML is written under gitignored
   `learn-uart/` on the bound snapshot (original if present, else the
   capture), with a living sidecar
   `*.uart.log` (timestamped device lines and host events). A session that
   finishes without aborting copies `learn-uart-latest.yaml` (`complete: true`);
   a crash does not. `cargo xtask learn-uart-only touch` (or `--only touch`)
   retests glass without the other operator questions. The session is
   bounded by the real world: it names each step by the action (press a
   button, unplug USB-C, tilt, touch the glass), states expected duration,
   asks if you can stay the whole time, and if we don't see a response asks
   whether you tried (with a retry). Tilt first waits for the board on the
   desk (Enter) so USB wiggle is not the tilt. After each button wait it asks
   what a human would call that key (or `unknown`) and
   allows a short note if the mapping is still unclear. Optional `--report
   FILE` copies the YAML elsewhere as well. Do not commit it. Optional
   `--restore-app0` puts factory `app0` back after (UART is closed first so
   restore is not busy). One `--yes` confirms both the image write and that
   restore. Compare two units
   with `cargo xtask diff-learn-uart` (host-only; serials redacted unless
   `--show-serials`).
4. Or flash and watch without prompts: `cargo xtask flash-app --image … --yes`
   then `cargo xtask monitor`
5. If it wedges: `cargo xtask restore-factory-firmware --part app0 --yes`

`cargo xtask monitor` claims USB CDC and does not open the ACM TTY.
`--acm-tty` still pulses EN (`POWERON`) because `cdc-acm` asserts DTR+RTS
on open.

Host verify of the log crate (not this package):

```shell
cargo test -p simple-debug --locked
```

Confirm the workspace lockfile still matches (no Xtensa toolchain):

```shell
cargo metadata --locked --format-version 1 --no-deps
```

## On silicon (predecessors)

`firmware/bringup` and `firmware/learn` were the earlier images; this package
replaces both. Those runs already showed: factory 2nd-stage jump to `0x90000`,
I2C ACKs including GT911 `0x14` and SHT40 `0x44` on a real measure, gauge
DeviceType `0x0220`, heartbeats while sitting on USB, then
`restore --part app0` and `confirm-factory-firmware` matching the original.

This does **not** close measurement-backlog items. Missing edges are
“not exercised,” not “broken.” `gpio7=0` with a pull-up is a snapshot,
not an owner.

See [docs/SAFETY.md](../../docs/SAFETY.md) and
[docs/firmware-snapshot-management.md](../../docs/firmware-snapshot-management.md).
