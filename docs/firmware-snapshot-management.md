# Firmware snapshot management

How to capture, classify, flash, and restore a Seeed reTerminal Sticky
without destroying data you cannot get back.

Hazard rules stay in [SAFETY.md](SAFETY.md). This page is the operator
manual: what lives on disk, what xtask prints, and which commands to run.
Flag catalog: [`.agents/skills/sticky-rs/references/xtask.md`](../.agents/skills/sticky-rs/references/xtask.md).

Do not commit `backups/`. Do not print factory serials or MACs in issues
or chat.

## Honest limit

A snapshot is **this unit’s bytes**, bound by MAC (and USB serial when both
sides have one). It is not a factory-reset image you can share.

- If factory `nvs` (starts at `0x9000`) was already erased or overwritten
  (`espflash flash`, `erase-flash`, flashing unit A onto unit B), **this
  repo cannot restore RF calibration**. Snapshot what is on the chip now
  anyway. Do not label that tree factory.
- Never `erase-flash`. Never `espflash flash` (default bootloader and
  table). Never write below `0x90000` except restore of **that unit**.
- `flash-app` writes factory `app0` only. It is not a factory restore.

## Two snapshot kinds

| Kind | Path | When |
| --- | --- | --- |
| Original | `backups/original/<factory-serial>/` | Known factory catalog match, or you passed `--as-original` on uncertain stock. Write-once. |
| Capture | `backups/captures/<unit-id>/<slug>/` | Everything else: in-tree images, unknown stock, already-flashed units. Named “what is on the chip now.” |

`unit-id` is the factory UART `serial_number` when that line was present,
otherwise `mac-<hex>` (colons stripped). Bind confirm / restore /
`flash-app` by **MAC**, not by the directory name.

`flash-app` prefers a bound original. If there is no original but there is
exactly one capture for this MAC, that capture is the safety net: you can
put **this** dump back. That is not factory restore; lost `nvs` stays
lost.

`confirm-factory-firmware` and `restore-factory-firmware` stay
original-only unless you pass `--capture SLUG`.

## Classify, then ask

Factory images change. A stock UART line `key=serial_number` is **not**
proof the dump is factory. After a full-chip dump the host prints evidence
and picks a bucket:

| Class | Meaning | Default dest |
| --- | --- | --- |
| Known factory | Catalog match: `reterminal_template` `1.1.0` + layout `factory-32mb-v1` | `original/` (write-once) |
| Uncertain stock | Stock-shaped UART or IDF `app_desc`, but name/version not in the catalog | Ask: `--as-original` or `--name SLUG` |
| Not factory | `simple-debug-fw` / `embassy-debug-fw`, `git=` without serial, missing serial + unknown project, mismatched table | `--name SLUG` only. Never `original/` |

Evidence line (project, version, layout id or `unknown` / `mismatch:…`,
`serial_number` present/absent). The host does not claim “this is factory”
unless the catalog matched. `--as-original` records the new fingerprint in
**that unit’s** `MANIFEST.yaml` only; it does not add a row to the in-repo
catalog.

Interactive name prompt (TTY stdin) runs only when classification is not
known factory and `--name` is absent. Non-TTY sessions must pass `--name`.

## On-disk tree

Each snapshot directory has the same binaries as before:

- `flash-32mb.bin`, `bootloader.bin`, `partition-table.bin`, `part-*.bin`
- optional `chunks/`
- `uart-sample.txt`, `board-info.txt`
- `SHA256SUMS`
- **`MANIFEST.yaml`** (schema `sticky-firmware-snapshot/v1`): identity,
  hashes, `kind`, `layout_id`, `classification`, optional `image_name`,
  and the parsed `partitions:` list
- confirm reports: `divergence-<unix>.yaml`

Older gitignored trees may still have `MANIFEST.json` and
`partitions.csv`. The host **reads** YAML first, then JSON. New writes do
not emit `partitions.csv` or JSON. Do not convert or commit `backups/`.

Learn-uart YAML stays under the **bound** snapshot (`learn-uart/`),
original if present, else the capture used as the safety net.

## Known partition layouts

Host catalog (append-only): `factory-32mb-v1` — nvs `0x9000`/`0x7d000`,
otadata `0x86000`, phy_init `0x88000`, app0 `0x90000`/`0x600000`, app1,
sys_storage, usr_storage, coredump. A later table is `factory-32mb-v2`,
not a silent overwrite of v1.

The dump’s table is the source of truth. The catalog names a match or
flags a mismatch (same labels, wrong offsets). `flash-app` refuses an
unknown or mismatched table unless you pass `--allow-unknown-layout`.
`--yes` is not enough; that extra flag is the “don’t land on nvs” check.

In-repo firmware images do not ship a local `partitions.csv`. They use
this layout id and `flash-app`.

## Recipes

From the repo root. Live commands need exactly one QinHeng CH343
(`1a86:55d3`) or `--port` / `ESPFLASH_PORT`.

### First backup of a sealed factory unit

```shell
cargo xtask backup-factory-firmware
# alias: cargo xtask backup-firmware
```

Known factory → `backups/original/<factory-serial>/`. Write-once.

### First backup after someone already flashed

```shell
cargo xtask backup-factory-firmware --name after-their-flash
```

If UART still looks stock but the app is not in the catalog, the host
asks. Either name a capture or, only if you are sure it is factory:

```shell
cargo xtask backup-factory-firmware --as-original
```

If `nvs` was already destroyed, snapshot anyway with `--name`. Do not use
`--as-original`.

### Confirm

```shell
cargo xtask confirm-factory-firmware
cargo xtask confirm-factory-firmware --capture after-their-flash
```

### flash-app (original vs capture only)

```shell
# source the script espup printed (example: . $HOME/export-esp.sh)
cargo xtask build-fw simple-debug --features operator
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/simple-debug.bin --yes
```

With only a capture for this MAC, the same command proceeds and prints
that this is not a factory restore. To pick one slug when several exist:

```shell
cargo xtask flash-app --image FILE --yes --capture after-their-flash
```

Unknown table:

```shell
cargo xtask flash-app --image FILE --yes --allow-unknown-layout
```

### Restore

```shell
cargo xtask restore-factory-firmware --part app0 --yes
cargo xtask restore-factory-firmware --capture after-their-flash --part app0 --yes
```

Still `--yes`. Still no erase. Still never flash unit A’s dump onto unit
B.

### Import an existing dump tree

Host-only (no port). Needs `flash-32mb.bin` (32 MiB) and `board-info.txt`
(`MAC address:` + 32 MB). Factory serial from `MANIFEST.yaml` /
`MANIFEST.json` or `uart-sample.txt` / `serial-samples.txt`
(`key=serial_number`). Sibling dump manifests that are not the xtask
schema are ignored.

```shell
cargo xtask backup-factory-firmware --import DIR
cargo xtask backup-factory-firmware --import DIR --name after-import
cargo xtask backup-factory-firmware --import DIR --as-original
```

### What not to commit

`backups/` is gitignored. Never add a MAC, serial, USB serial, NVS blob,
or flash image. Never add a path to an off-repo device capture.
