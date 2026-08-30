# embassy-debug-fw

Embassy event-logger image for the reTerminal Sticky. It is not product
firmware. Host-tested line format: [`crates/embassy-debug`](../../crates/embassy-debug).

Default builds latch power, print timestamped button / GT911 / IMU lines on
UART0 (CH343, 115200), and beep the passive buzzer. The panel is **off**
unless you pass `--features epd`.

This image **cannot** close measurement-backlog electrical items. A UART
snapshot is firmware-observed sequencing, not a meter or a schematic.

The Xtensa package lives under `firmware/` (not `crates/`) and is a
workspace member but not a default-member, so host `cargo test` never
tries to compile `esp-hal`.

## Envelope

- Latch GPIO45 then GPIO46 and hold them high.
- Park BQ25616 `/CE` disabled. Do not enable charging.
- GPIO7 is input-only. Do not drive it.
- MicroSD: CS idle-high, power enable off. Do not mount.
- Gauge: not used. No unseal, no data-memory writes.
- Touch: rail on, INT-during-reset dance, bus 100 kHz, INT left floating.
  Status clear then command `0`. No config-RAM write.
- IMU: accel only. No `init_irqs`. Reports every 5 s (`IMU_REPORT_SECS`).
- Buzzer: LEDC ~1 kHz on GPIO48, short beep on button down and first contact.
- Panel: only with `--features epd`. OTP 1-bit full refresh, no `0x32` LUT,
  no Lotus `0x21`.
- No deep sleep. No writes below `0x90000`. No Cargo `runner`.

## What it prints

At boot: `embassy-debug: latched`, `git=<hash> dirty=<0|1>` (from `build.rs`),
GT911 ACK, then IMU init. Then, from a dedicated Embassy log task:

```text
embassy-debug: t=1204 btn 4 down
embassy-debug: t=2100 touch n=1 p0=123,456
embassy-debug: t=5000 imu=FaceUp x=12 y=-30 z=16300
```

Touch coordinates are mapped with `touch::to_screen`. A pose that does not
classify is `imu=none`; the raw sample is still printed. If the log channel
overflows: `drop=N`.

With `--features epd`, Page Up / Page Down cycle splash / shapes / legend
and print `scene=…`.

## Desk demo

This is the unattended-plus-hands check: flash the **default** image (no
`--features epd`, so the glass is not refreshed), then watch UART0.

First-time toolchain and both image paths:
[docs/getting-started.md](../../docs/getting-started.md).

1. Once per unit, capture a snapshot (gitignored). Known factory →
   write-once `original/`; already-flashed units need `--name`. Layout id
   `factory-32mb-v1`. How-to:
   [docs/firmware-snapshot-management.md](../../docs/firmware-snapshot-management.md).

   ```shell
   cargo xtask backup-factory-firmware
   # or: cargo xtask backup-factory-firmware --name after-flash
   ```

   `flash-app` refuses without a matching original or capture. If more than
   one USB-serial device is present, set `ESPFLASH_PORT` to the Sticky
   CH343 (`cargo xtask detect-connected` prints a by-id suggestion).

2. From the **repo root**, source the script `espup` printed (example:
   `$HOME/export-esp.sh`) and run `cargo xtask build-fw`. `flash-app`
   does not compile. If the build fails, do not flash a leftover ELF.

   ```shell
   # source the script `espup` printed (example: . $HOME/export-esp.sh)
   cargo xtask build-fw embassy-debug
   ```

   A `LOAD segment with RWX permissions` linker warning is expected for
   esp-hal images. (`esp_app_desc!()` is required; do not `--merge`.
   There is no Cargo `runner`, so `cargo run` cannot flash.)

3. Write that `.bin` into factory `app0` only, then listen. `monitor` is
   UART0 at 115200
   over USB CDC (it does not open the ACM TTY, so Linux `cdc-acm` cannot
   pulse EN). It takes the same UART session lock as `flash-app`. Ctrl-C
   ends the listen.

   ```shell
   cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
   cargo xtask monitor
   ```

   After the factory 2nd-stage jump to `0x90000` you should see:

   ```text
   embassy-debug: latched
   embassy-debug: git=<hash> dirty=<0|1>
   embassy-debug: 0x14 ack
   embassy-debug: gt911 status cleared
   embassy-debug: gt911 command read-coordinates
   embassy-debug: imu accel init ok
   embassy-debug: t=5000 imu=FaceUp x=12 y=-30 z=16300
   ```

   Sitting still, an IMU line repeats about every 5 s. That is the
   unattended pass.

4. At the enclosure (optional): press a right-edge key (GPIO4 / 5 / 6)
   — a short beep and `btn N down` / `btn N up`. Touch the glass — a
   beep on first contact and `touch n=…` with mapped coordinates. Tilt
   the board and wait for the next IMU line (`imu=Landscape0` or
   another pose, or `imu=none` if no axis dominates). Missing edges
   mean that step was not exercised, not that the silicon is broken.

5. Put stock firmware back:

   ```shell
   cargo xtask restore-factory-firmware --part app0 --yes
   ```

A second image with `cargo xtask build-fw embassy-debug --features epd`
(then the same `flash-app` / `monitor`) also refreshes the panel: white clear,
splash, then GPIO5 / GPIO6 cycle splash / shapes / legend and print
`scene=…`. Skip that until you want glass updates. Do not invent a LUT.

This image is not the `learn-uart` operator format (`simple-debug`
`--features operator` is). Use `flash-app` then `monitor` here.

Host verify of the log crate (not this package):

```shell
cargo test -p embassy-debug --locked
```

Confirm the workspace lockfile still matches (no Xtensa toolchain):

```shell
cargo metadata --locked --format-version 1 --no-deps
```

See [docs/SAFETY.md](../../docs/SAFETY.md) and
[docs/firmware-snapshot-management.md](../../docs/firmware-snapshot-management.md).
