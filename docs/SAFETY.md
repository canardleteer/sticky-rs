# Safety

Every row below is something that can destroy hardware, destroy data you
cannot regenerate, or produce damage that shows up weeks later. Sources are
the vendor datasheet catalog
([resources/datasheets.md](../.agents/skills/seeed-sticky-hardware/resources/datasheets.md))
plus the board contract in
[`.agents/skills/seeed-sticky-hardware/`](../.agents/skills/seeed-sticky-hardware/SKILL.md).
When those sources disagree, name both sides; the skill user weighs them.
Precedence: [Authority](../.agents/skills/seeed-sticky-hardware/SKILL.md#authority).
Operator toolchain and both firmware paths:
[getting-started.md](getting-started.md).

## Hazard table

| Hazard | Safe default | Forbidden until proven |
| --- | --- | --- |
| Factory NVS at `0x9000` | Treat as irreplaceable: Wi-Fi RF calibration, device identity, persisted gauge state. Capture a full-chip snapshot of **that unit** first (factory `original/` or a named capture). Lost `nvs` is not regenerable | `erase-flash`, any full-chip erase, writes below `0x90000` except restore of **that unit**, flashing one unit's dump onto another |
| Power latch GPIO45 / GPIO46 | Drive high and settle before logs or bus init; restore before releasing GPIO holds on deep-sleep wake | Pulsing GPIO46; dropping the latch while on battery |
| GPIO0 / GPIO46 straps | GPIO0 is sensor I2C SCL only | GPIO0 on the SPI bus (zero-initialised `quadwp`/`quadhd` claims it) |
| GPIO7 | Input only; owner is unconfirmed (IMU INT vs gauge GPOUT) | Driving it as an output |
| SSD1677 SPI | 10 MHz, mode 0, BUSY active high | 40 MHz (spec max is 20 MHz); dropping `EPD_EN` before the deep-sleep command |
| Four-gray LUT / OTP waveform | Sticky: Seeed OTP full / partial / gray4. MCU `0x32` stays optional and attributed. Recorded in [display.md](../.agents/skills/seeed-sticky-hardware/references/display.md) | An invented or generic-example 105-byte table; analog 0x03/0x04/0x2C guesses; 40 MHz SPI |
| BQ27220 gauge | Read-only standard commands and CEDV reads | Unseal, `CFGUPDATE`, Full Charge Capacity writes, OTP writes; crate `bq27xxx` (wrong family) |
| BQ25616 charge enable (GPIO39) | Active-low; start disabled (inactive / high) | Raw “enable charge” writes; assuming active-high |
| GT911 touch | Probe `0x14` first, after the INT-during-reset sequence | Assuming `0x5D`; rewriting config RAM to fake a resolution |
| Shared SPI (EPD + MicroSD) | One bus mutex; exactly one CS asserted | Overlapping transactions on the two devices |
| Flash images | Partition table mirroring the factory 32 MB layout | 16 MB `n16r8` limits; `probe-rs` on the USB-C connector |

## Why the flash rule comes first

The `nvs` partition is written during factory test and contains that unit's
Wi-Fi RF calibration alongside its identity and saved gauge state. A full-chip
erase is not a factory reset — it is permanent, and radio performance after
losing calibration is not something hobby tooling can restore.

If you intend to flash your own firmware:

1. Take a full-chip snapshot of **your** unit first. Prefer the unique
   Sticky CH343 (`1a86:55d3`). Factory-classified trees are write-once
   under `original/`; already-flashed units go under `captures/`. Persist
   then seals that tree read-only. Both are gitignored. Do not commit them.
   If `nvs` was already overwritten, snapshot
   anyway — this repo cannot regenerate RF calibration.
2. Write only factory `app0`. Leave `nvs`, `otadata`, `phy_init`, `app1`,
   and the LittleFS partitions alone on that first custom write. Do not
   flip `otadata` (that is a write below `0x90000`). Do not use a tool that
   installs a default bootloader and partition table (plain `espflash flash`
   does). In-repo writes use `cargo xtask flash-app`.
3. Use the host-known layout id `factory-32mb-v1` (see the snapshot
   manual). A later factory table is `v2`, not a silent overwrite.
4. Restore only from **that unit's** snapshot (original, or `--capture`).
   Never `erase-flash`. Never flash one unit's dump onto another.

Operator how-to (classify, YAML trees, copy-paste xtask recipes):
[firmware-snapshot-management.md](firmware-snapshot-management.md).

`developer-data/backups/original/<factory-serial>/` and
`developer-data/backups/captures/<unit-id>/<slug>/` hold `flash-32mb.bin`, split
`part-*.bin`, and `MANIFEST.yaml` (identity, hashes, layout id, OTA slot).
Older trees may still have `MANIFEST.json` / `partitions.csv` (read
fallback only). Confirm reports live in
`developer-data/confirm-records/<serial>/`. Learn-uart YAML lives in
`developer-data/uart-inspection-records/<serial>/`. Do not commit them.

## Why gauge writes stay off

The BQ27220 is a **CEDV** gauge. Its configuration lives in data memory
reachable only after an unseal, and the documented update path is enter
`CFGUPDATE`, write, verify, exit, re-seal — every step timeout-prone. Stock
firmware really does walk that path to maintain Full Charge Capacity, which
tells you it is both necessary and delicate. The OTP is one-time.

Keep reads unconditional. Do not unseal, enter `CFGUPDATE`, write Full
Charge Capacity, or touch OTP unless a human asked for that exact sequence
and accepted the risk.

## Why Sticky has a default waveform *path*, not a default LUT

The SSD1677 OTP can hold panel waveforms; the datasheet also provides
`Write LUT register` (0x32) for a 105-byte MCU table. Seeed’s Sticky driver
uses **OTP** for full, partial, and gray4 (no 0x32). That path is recorded in
[display.md](../.agents/skills/seeed-sticky-hardware/references/display.md).

Consequences:

- Do not invent or ship a default 105-byte LUT.
- The Sticky OTP sequences (full `DISPLAY_MODE_1_WITH_TEMP` / partial
  `DISPLAY_MODE_2_WITH_TEMP` / gray4 `SEEED_GRAY4` + temperature prefix)
  are controller parameters confirmed against the datasheet (where named)
  plus Seeed `seeed_epaper` plus stock `reterminal_template` app0.
- A FreeInk MCU table was **not** in that stock image; it is commented out,
  not compiled. Leave it commented. There is no in-repo way to read factory
  OTP back from the panel; uncommenting is not a measurement step.
- Analog rails (0x03 / 0x04 / 0x2C) are separate from the 105-byte command.
  Seeed does not write them on this panel. Do not fill them from another
  product’s LUT tail.

## Host-only scope

Agents do not open a serial port unless a human asked. Host I/O is
`cargo xtask`. There is no Cargo `runner`. Do not treat bare `espflash`,
`esptool`, `idf.py`, or PlatformIO as an in-repo path. Prefer region
read/write over a full-chip erase. Never `espflash flash` (default
bootloader and table). QinHeng (`1a86:55d3`) is the Sticky UART; refuse a
non-QinHeng plug before DTR.

When firmware lands here, encode latch, EPD rail, charger enable, and GPIO7
as compile-time constraints so the sequences above are type errors rather
than field failures.
