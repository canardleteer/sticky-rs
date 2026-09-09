# cargo xtask

Invoke from the repo root (`cargo xtask <subcommand>`). `cargo xtask --help`
and `cargo xtask <cmd> --help` are the flag source of truth. Keep this page
and the root [README.md](../../../../README.md) xtask list in the same
change as any CLI change.

Live commands take `--port` or `ESPFLASH_PORT`; if unset they require exactly
one QinHeng CH343 (`1a86:55d3`) and refuse a non-QinHeng plug **before** DTR.
Live commands that reset or listen take the [UART session lock](#uart-session-lock-shared).
`flash-app` does not compile; `cargo xtask build-fw` first.

Do not open a port unless a human asked. Silicon facts (chip, 32 MB, ACKs)
live in
[seeed-sticky-hardware measure.md](../../seeed-sticky-hardware/references/measure.md).

## Catalog

| Command | UART? | How to use it |
| --- | --- | --- |
| `detect-connected` | no, unless `--probe` | USB inventory of Sticky CH343 (sysfs / by-id). `--all-devices` includes other USB-serial adapters. `--probe` opens the UART (DTR reset): stock `serial_number`, then board-info |
| `backup-factory-firmware` | live dump yes; `--import` no | Classify then store. Known factory (`reterminal_template` 1.1.0 + `factory-32mb-v1`) → write-once `developer-data/backups/original/<factory-serial>/`. Otherwise `--name SLUG` → `developer-data/backups/captures/<unit-id>/<slug>/` (`unit-id` is factory serial or `mac-<hex>`). Persist seals the dest tree read-only. Uncertain stock: `--as-original` (under `original/`, not a capture slug) or `--name`. Alias `backup-firmware`. `--import DIR` is host-only: `flash-32mb.bin` (32 MiB), `board-info.txt` (`MAC address:` + 32 MB), serial from xtask `MANIFEST.yaml` / `MANIFEST.json` **or** `uart-sample.txt` / `serial-samples.txt`. Sibling dump manifests that are not the xtask schema are ignored. Import clears CH343 USB serial. `--import` refuses `--port` / `ESPFLASH_PORT`. Operator how-to: [firmware-snapshot-management.md](../../../../docs/firmware-snapshot-management.md) |
| `confirm-factory-firmware` | yes | Compare live flash to the matching original, or `--capture SLUG`. Writes `developer-data/confirm-records/<serial>/divergence-<unix>.yaml`. Does not rewrite the sealed snapshot binaries |
| `restore-factory-firmware` | yes | `write_bin_to_flash` of **that unit's** original, or `--capture SLUG`. Requires `--yes`. Full image at `0x0`, or `--part LABEL` (`nvs`, `app0`, …). `--part` refuses when `part-*.bin` is longer than that partition (a shorter file is allowed). Never a full-chip erase. Writes in 1 MiB windows (same size as backup `read-flash`); per-window device MD5 can skip a match; reconnects after each window and retries a dropped window (a second write on the same stub hangs before `chunks=`). Prints `write-bin window i/n` then chunk `%` (`init`/`update` are **chunk counts**, not bytes) |
| `flash-app` | yes | `write_bin_to_flash` of `--image FILE` (a `save-image` payload, not an ELF) into factory `app0` only. Requires `--yes` and a matching original or unique capture. `--capture SLUG` picks a capture. Refuses an unknown/mismatched snapshot table unless `--allow-unknown-layout`. Never `espflash flash`, never a caller-chosen offset. Same 1 MiB write-bin windows as restore (reconnect after each window) |
| `learn-uart` | yes | Heartbeat vet plus skippable human steps. Needs a matching original or capture. YAML + sidecar `*.uart.log` under `developer-data/uart-inspection-records/<serial>/` (factory serial, not MAC). `--image FILE` flashes `app0` first (needs `--yes`). `--restore-app0` puts factory `app0` back after (UART closed first; needs an original; one `--yes` covers both writes). `--report FILE` extra YAML copy: prints the absolute path and warns (does not refuse) when that path is outside `developer-data/`. `--skip STEP` (`buttons`, `vbus`, `imu`, `sd_detect`, `touch`). `--only STEP` or `--unattended-only`. `--step-timeout-secs N`. Press `s` to skip a wait. Tilt waits for a rest pose after Enter. After each key, a human label (or `unknown`) and optional note. A finished session copies `learn-uart-latest.yaml` (`complete: true`); a crash does not. Firmware / host stamp `git=` / `package_git` |
| `learn-uart-only` | yes | Same session as `learn-uart`, only named groups: `touch`, `buttons`, `vbus`, `imu`, `sd` (positional and/or `--only`). Example: `learn-uart-only touch --image FILE --yes --restore-app0` |
| `diff-learn-uart` | no | Host-only compare of two YAML reports or factory serials. Default paste uses `UNIT_A` / `UNIT_B`; `--show-serials` prints serials locally |
| `vet-idle-log` | no | Host-only. `--embassy FILE` or `--simple FILE` from `monitor --output`. Checks unattended tokens (latch, GT911 dance / `0x5D`, idle `imu=` / `gt911 st=`, or simple-debug heartbeat + SHT/RTC). Does not open a UART |
| `build-fw` | no | Host-only. `cargo +esp build -p <fw> --profile release-fw --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc --locked` then `espflash save-image` (no port). IMAGE is `simple-debug` or `embassy-debug`. Default embassy-debug includes `pair` + `wifi` (advertise only on the pair card; Wi-Fi idle until a tap). `--features operator` on simple-debug; `--features mic`, `radio`, `pair`, `wifi`, `spi20`, `sd`, `charge`, or `remote-debug` on embassy-debug. Exclusive sits (`mic` / `radio` / `charge` / `sd`) pass `--no-default-features`. `wifi` packs with `pair`. `remote-debug` packs with default `pair` + `wifi` (insecure desk snapshot + synthetic tap / key mux + protobuf codec + encrypted GATT after pair; not default). LAST planes are a 96 KiB octal PSRAM carve (`.bss` overflowed `dram_seg`). ELF and `.bin` under workspace `target/xtensa-esp32s3-none-elf/release-fw/` |
| `ci` | no | Host-only CI gate. `cargo fmt --check --all`; host clippy+test (default-members, `--all-features`, `ssd1677-gray4 --no-default-features`); `cargo +esp` clippy for `simple-debug-fw` (default and `operator`) and `embassy-debug-fw` (default, `mic`, `radio`, `pair`, `spi20`, `sd`, `charge`, and `remote-debug`); then `rumdl check`, `cargo machete`, `cargo audit`. Missing extra tools print `cargo install …` and fail. Does not open a UART and does not refuse leftover `backups/` |
| `monitor` | yes | UART0 listen. Flags below; not nested subcommands. Pair with `flash-app` / `restore-factory-firmware` / `confirm-factory-firmware` |
| `remote-debug` | live BLE; UART on auto-PIN | Encrypted GATT after DisplayOnly pair (`--features remote-debug` image). Advertise `sticky-rs` from splash on every boot (pair card optional). BlueZ **Connect**, not `Pair()`. A Unix-socket broker (`$XDG_RUNTIME_DIR/sticky-rs/remote-debug-<name>.sock`, advertise name, never a MAC) owns `Session` / assembler / last nonce. `connect` starts a detached broker if needed (stdio to `$XDG_RUNTIME_DIR/sticky-rs/remote-debug-<name>.log` or `/run/user/<uid>/sticky-rs/` when `XDG_RUNTIME_DIR` is unset), returns `pairing`, and exits; poll `status` until `connected` or `pair failed`. GATT stays up after pair. `serve` is an optional foreground log (Ctrl-C disconnects). Other leaves (`status`, inject, snapshot, `reboot`, `disconnect`) are RPC only and refuse with `no broker; run connect or serve` plus the socket path when the socket is missing. Client hangup does **not** drop GATT. `disconnect` or serve exit is the only close. Default `connect` scrapes a new UART `pair pin=` (UART lock only for that scrape; fails if `monitor` holds it). If inventory has no QinHeng TTY but usbfs still sees exactly one `1a86:55d3` (`cdc-acm` left detached after a listen skipped Drop), auto-PIN claims that unique device over usbfs instead of requiring a replug. That image reprints `pair pin=` every 5 s on splash or the pair card until `pair ok`. `--pin` skips UART. `--remember` allowlists factory / CH343 USB serial in gitignored `developer-data/remote-debug/` (never a MAC, never the PIN). After `pair ok` the image holds GATT when walking off the pair card. `reboot` software-resets the **embedded MCU** (not the host) and re-pairs unless `--no-reconnect`; leftover BlueZ LTK is forgotten. `--mcp` is the same client on **this subtree only** (not `flash-app` / restore); do not treat stdio MCP as the session owner. Do not also run `monitor` during auto-PIN. `inject-touch` is `FramebufferPoint` (pre-rotation 800×480), or `--page` page pixels (gray4 hit-test inverse), not UART `p0=` / `GlassPoint`. `--phase` `down` / `move` / `up` (unset = tap). `inject-button` `down` is a short press; wait for LAST compose before `get-snapshot`. Planes are `developer-data/remote-debug/snapshots/snap-<hex>.bw` / `.red` / page-space `.png`; the envelope also carries `scene` / `hold` / `target_step` / expect. BlueZ TX notify is a FIFO (`VecDeque`); a `Vec` + `pop` reversed multi-chunk snapshots (`frame: Version`). How-to and sit notes: [mcp-interactions.md](mcp-interactions.md) |

```shell
cargo xtask detect-connected
# cargo xtask detect-connected --all-devices
# cargo xtask detect-connected --probe
cargo xtask backup-factory-firmware
# cargo xtask backup-factory-firmware --name after-flash
# cargo xtask backup-firmware
cargo xtask confirm-factory-firmware
# cargo xtask confirm-factory-firmware --capture after-flash
# cargo xtask restore-factory-firmware --yes
# cargo xtask restore-factory-firmware --part app0 --yes
# cargo xtask restore-factory-firmware --capture after-flash --part app0 --yes
# after build-fw (no port): cargo xtask flash-app --image FILE --yes
# cargo xtask flash-app --image FILE --yes --capture after-flash
# cargo xtask flash-app --image FILE --yes --allow-unknown-layout
# cargo xtask learn-uart
# cargo xtask learn-uart-only touch
# host-only: cargo xtask diff-learn-uart LEFT RIGHT
# host-only: cargo xtask build-fw simple-debug --features operator
# host-only: cargo xtask build-fw embassy-debug
# host-only: cargo xtask ci
# host-only: cargo xtask vet-idle-log --embassy idle-embassy.log
# host-only: cargo xtask vet-idle-log --simple idle-simple.log
# cargo xtask monitor
# live BLE (UART lock only during auto-PIN scrape):
# cargo xtask remote-debug connect
# cargo xtask remote-debug status
# cargo xtask remote-debug --mcp
# optional foreground log: cargo xtask remote-debug serve
```

`detect-connected` prints QinHeng `1a86:55d3` Sticky UART nodes and a
suggested `ESPFLASH_PORT` (by-id when udev created one). Other USB-serial
adapters are omitted unless `--all-devices`. Dummy USB ids (`vid:0 pid:0`)
are enough for espflash’s UART reset-strategy pick on QinHeng. 32 × 1 MiB
`read-flash` at 921600 baud takes ~8 min.

`--import DIR` is host-only (no port). `flash-app` write-bins a custom
payload into factory `app0` only and refuses without a matching
original or capture. How-to:
[firmware-snapshot-management.md](../../../../docs/firmware-snapshot-management.md).

## `monitor` (listen)

`monitor` has **flags**, not child subcommands. It only reads UART0. It does
not flash, restore, or compile. Hold the [UART session lock](#uart-session-lock-shared)
for the whole listen. Default listen claims the CH343 over USB CDC so
Linux `cdc-acm` never opens the ACM TTY (that open asserts DTR+RTS and
pulses EN / `POWERON`). Baud is 115200. Needs write access on the usbfs
node (`/dev/bus/usb/…`). A udev rule for `1a86:55d3` in group `dialout`
must live in `/etc/udev/rules.d/` (a file in `/etc/udev/` is ignored);
reload rules, then a USB replug (POWERON). `--acm-tty` is the old TTY
path (embassy will reboot).
Ctrl-C reattaches `cdc-acm` so the next live command can see the TTY.
If Drop never ran, `remote-debug` auto-PIN still claims the unique
QinHeng over usbfs (same modem-line contract) instead of requiring a
replug.
`CdcListen::open` must also reattach if the first interface claim
succeeds and the listen then fails (`ClaimedIfaces`; Drop is the
success path only). After Drop, the ACM TTY path can move; the next
live command re-resolves and must not assume the pre-listen path.
Prefer `--for` / `--lines` over `timeout`(1) or `kill -9`.

| Flag | What it does |
| --- | --- |
| `--port` / `ESPFLASH_PORT` | QinHeng CH343. Omit if exactly one Sticky is plugged in |
| `--for SECS` | Stop with success this many seconds after the port opens (`1` or more) |
| `-n` / `--lines N` | Stop with success after N newline-terminated device lines (`1` or more) |
| `-o` / `--output FILE` | Tee the UART stream to FILE and stdout. Prints the absolute path. Warns (does not refuse) when that path is outside `developer-data/` |
| `--quiet` | File only (requires `--output`) |
| `--output-only` | Alias for `--quiet` |
| `--acm-tty` | Open `/dev/ttyACM*` instead of USB CDC. Pulses EN (`POWERON`) |

`--for` and `--lines` may be combined; the first limit hit wins and the
process exits `0`. With neither, listen until Ctrl-C. A USB unplug
after the reader is open (`UnexpectedEof`, `BrokenPipe`,
`NotConnected`, `ConnectionReset`) is the same clean `0`, not a
device error. `--for 0` and `--quiet` without `--output` are
rejected. Prefer a path under `developer-data/` (gitignored). A path
outside that tree still writes, and xtask prints a warning. Do not
commit UART captures (they can contain factory serials).

Typical desk check after a matching snapshot exists (human asked):

```shell
cargo xtask build-fw embassy-debug
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
cargo xtask monitor --for 20 --output idle-embassy.log
cargo xtask vet-idle-log --embassy idle-embassy.log
# or: cargo xtask monitor --lines 80 --quiet --output uart.log
```

`flash-app` does not build that `.bin` (`build-fw` does). A Playground
`.factory.bin` is a merge, not a `save-image` payload: extract the app
at `0x10000` first. Do not write that merge at `0x0`. If the listen
shows a boot loop (checksum / “not bootable”) or the image wedges:
`restore-factory-firmware`
(`--part app0` or full, `--yes` of **that unit's** original; never erase),
then `confirm-factory-firmware`. `learn-uart` / `learn-uart-only` are a
different path (operator YAML, `simple-debug --features operator`), not a
`monitor` mode.

## Contracts

`flash-app` **must** call `require_safety_net` (original preferred, else
unique capture) and refuse when no snapshot matches or the live identity
does not match, and **must** take the shared UART session lock. Do not add
a Cargo `runner`. Do not put a device path in tracked source
(`*.toml`, `*.rs`); xtask reads `ESPFLASH_PORT`.

There is no Cargo `runner`, so `cargo run` cannot flash. xtask may use the
`espflash` library for region read/write only; never a full-chip erase,
never `espflash flash`.

Never commit a MAC address, serial number, USB serial string, NVS blob, or
flash image, and never add a path to an off-repo device capture.
`developer-data/` is gitignored on purpose (snapshots under
`developer-data/backups/`). Facts learned from a stock firmware image are
recorded unattributed — as "stock firmware does X" — and only when they are
product-general.

Chip `ESP32-S3` and 32 MB board-info remain silicon checks after the UART
is open. Product-class PSRAM / JEDEC are already in the hardware skill;
do not re-run Espressif CLIs for those fields unless a human asked.

## UART session lock (shared)

`sticky_host::try_acquire` is the **one** exclusive UART session for
this board. It is a shared resource, not an implementation detail of backup
or restore. Live `sticky-host` methods take it internally so a CLI cannot
forget the lock. Lock files live in `$XDG_RUNTIME_DIR/sticky-rs` when
that variable is an absolute path, otherwise a writable
`/run/user/<uid>/sticky-rs`, otherwise `$TMPDIR/sticky-rs`
(`std::env::temp_dir()`). Tests inject a directory; do not add a second
lock file.

Any new `cargo xtask` or `sticky-host` entry point that would open the CH343
for a reset (DTR/RTS, EN/IO0, ROM stub, write-bin, read-flash, `--probe`) or
a long-running UART read (`monitor`, `learn-uart`, `learn-uart-only`)
**must** hold that guard for the whole command, **including while any
child process runs**. `remote-debug` auto-PIN holds it only while scraping
a new `pair pin=`, then releases so `monitor` is exclusive of that window,
not of the whole broker sit. Do not add a second lock file, a
per-subcommand flock, or a “this command is short so skip it” path. The
remote-debug broker socket and detached-serve log live in the same
`$XDG_RUNTIME_DIR/sticky-rs` (or `/run/user/<uid>/sticky-rs`, or
`$TMPDIR/sticky-rs`) directory; that
is not a second UART flock.

If that command shells out (`esptool`, `espflash`, `cargo espflash`, nested
`cargo xtask`, a script), run the child with `UartSession::status` or
`UartSession::output` (not a detached `Command::spawn`). Those wait with the
flock still held and set `STICKY_UART_LOCK` so a cooperating child joins the
same session instead of pulsing DTR on its own. Do not `Command::status` on a
UART tool without a live session.

The same pattern applies to **non-xtask** host tools in this repository that
could reset the UART. Call `try_acquire` first, then `status`/`output` for
subprocesses, or do not talk to the port. Inventory that does not reset
(`detect-connected` without `--probe`) and host-only import /
`diff-learn-uart` / `build-fw` / `ci` are the exceptions. Bare `esptool` /
`espflash` CLI typed at a shell still do not take the lock; wrapping them
in-repo is how they participate.

## What has been run

Detect, `--probe`, and backup have been run on a Sticky CH343 (udev
underscore by-id, EN/RTS run-mode UART sample, 32 MiB dump at 921600).
`flash-app` and `monitor` have been run: factory ESP-IDF v5.4-dirty
2nd-stage loaded `app0` at `0x90000` and bring-up printed on UART0.
`restore-factory-firmware --part app0` has been run: stock
`reterminal_template` 1.1.0 booted from `0x90000`, and
`confirm-factory-firmware` then matched the original. Full-chip restore
has not. Bring-up has met latch, UART0, sensor I2C, and gauge reads; it
has not refreshed the panel.

`firmware/simple-debug` is the in-repo proof-of-life image.
`firmware/embassy-debug` is a separate Embassy event-logger (panel always
on). `learn-uart` with the operator image has been run (right-edge
`btn 4` / `5` / `6`, USB-C unplug, IMU poses). That image's INT-high
poll has not printed `contacts=`. embassy-debug INT-low printed
`touch n=5`. Those runs do not close measurement-backlog electrical
items — see
[not-yet-confirmed.md](../../seeed-sticky-hardware/resources/not-yet-confirmed.md).
