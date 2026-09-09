# stdio MCP (`cargo xtask remote-debug --mcp`)

How-to and sit notes for the **remote-debug subtree only**.
CLI flags stay in [xtask.md](xtask.md). Pair / inject / snapshot
contract: [firmware/embassy-debug/AGENTS.md](../../../../firmware/embassy-debug/AGENTS.md#remote-debug-verification-workflow).
A phone pair is optional for the human; do not ask them to
pair when they already asked for a host desk sit.

Git-owned text names the argv (`cargo xtask remote-debug --mcp`)
and **stdio MCP**. Do not name a desktop client or a local config
path in this file.

## Attach

`--mcp` is clap-mcp 0.1.0 on this subtree (`xtask/src/remote_debug.rs`).
It is **not** the full xtask CLI: `flash-app`, restore, and backup
are never tools.

The server is **stateful** (`reinvocation_safe`,
`parallel_safe = false`) as a **client** of the ConnectRPC
owner. GATT, the notify assembler, and `last_nonce` live in
that owner (`connect` auto-starts a detached `serve`), not
in the stdio process. A new stdio session with no owner
reports `no broker; run connect or serve` plus the endpoint
path (`$XDG_RUNTIME_DIR/sticky-rs/remote-debug.connect`).

stdio attaches at **process launch**. After you change xtask,
clap-mcp wiring, instructions / prompts / resources, or the
`--features remote-debug` image, start a **new** agent process
before the next sit. An already-running session keeps the old
binary and the old tool table.

Do not also run `monitor` during auto-PIN. Default `connect` takes
the UART lock only while scraping a new `pair pin=`. `--pin` skips
UART.

Never a MAC. `--remember` allowlists factory / CH343 USB serial
only, under gitignored `developer-data/remote-debug/`. Snapshot
planes land in `developer-data/remote-debug/snapshots/` (nonce in
the filename; no serial).

## Pickup (next sit)

Stay on splash for pair (Ferris never shows the PIN). Do not ask
the operator to pair from a phone. Tools are `connect`, `status`,
`list-targets`, `inject-touch`, `inject-button`, `get-snapshot`,
`snapshot-ack`, `snapshot-clear`, `reboot`, `disconnect` (not
`remote-debug_*`). Do not run `monitor` during auto-PIN. No
`--remember` unless the human asked. New clap tools need a new
agent. A host notify / owner change is enough with
`cargo build -p xtask`, then `disconnect` / `connect` (the
detached owner is on-disk `target/debug/xtask`).

1. Stay on splash. `status` — `no broker` until `connect`.
2. `connect` → `pairing`. Poll `status` until `connected`
   (or `pair failed`). After a fresh flash, retry `connect` if
   the first sit is `le-connection-abort-by-local` or CDC busy.
3. `inject-button` `page-down` / `page-up` (`down` true = short
   press) or `inject-touch`. `--page` is page pixels for the
   last compose hold (hit-test inverse, not raw
   `page_to_framebuffer`). `--phase` `down` / `move` / `up`
   (unset = tap). Wait ~2–3 s for compose. Do not tap START
   on the Wi-Fi cards unless asked.
4. `get-snapshot` — LAST DRAW plus `scene` / `hold` /
   `target_step` / expect page. Open the JSON `png` field
   (page-space, portrait 480×800 or landscape 800×480 under
   `developer-data/remote-debug/snapshots/`). Sibling `.bw` /
   `.red` are packed SSD1677 planes (`.red` is the second
   gray4 plane, not pigment) and are omitted from JSON.
   CLI prints `png=` on the same line. `status` `last_log`
   is the last Target / Scene UART copy. A leftover arm is
   `SnapshotBusy`; `snapshot-clear` then retry. Ack when
   done. Read `sticky-rs://remote-debug/pickup`.
5. Targets walk (no `monitor`): seven Page Downs to
   `scene=targets` (persist `7`). Tap `--page` at snapshot
   `expect` (same origin as the page PNG). Dot: one
   tap. Slide: `--phase down` at one **page-end** inset
   (`TARGET_SLIDE_END_INSET` 80), `move` at mid and the
   other inset (`page_len − 80`), then `up`. Painted `r=`
   is the mark, not the span. After id 6 the snapshot
   `target_step` is 0. `last_log` is one line: `target loop`
   is emitted, then `target show id=0` overwrites it on the
   same refresh.
6. `list-targets` shows advertise names the owner holds.
   `disconnect` when the sit is over (empty map also shuts
   the owner down).

Never a MAC.

## Tools

Live `tools/list` names are the clap leaves (`connect`, `status`,
`inject-touch`, …), plus parent `xtask` / `remote-debug` / `serve`.
They are **not** `remote-debug_*` on this clap-mcp 0.1.0 attach.
Leaves match the CLI:

| Tool | Role |
| --- | --- |
| `connect` | Starts a detached owner if needed; returns `pairing`. BlueZ **Connect** (not `Pair()`). UART auto-PIN unless `pin`. `--name` is advertise `target` |
| `status` | `pairing` / `connected` / `disconnected` / `pair failed` |
| `list-targets` | Advertise names the owner currently tracks (never a MAC) |
| `inject-touch` | Framebuffer tap, or `--page` page pixels; `--phase` for slides. Not UART `p0=` |
| `inject-button` | `ok` / `page-up` / `page-down` short-press (`down` true). Wait for compose |
| `get-snapshot` | Arm LAST DRAW. JSON `png` is the page image to open. `.bw` / `.red` are SSD1677 planes (`.red` = gray4 plane 1, not pigment) and are omitted from JSON. scene / hold / expect |
| `snapshot-ack` | Release the armed nonce |
| `snapshot-clear` | Operator abort (no nonce); use after a failed get |
| `reboot` | Software-reset the **embedded MCU**, then re-pair unless `no_reconnect` |
| `disconnect` | Drop one GATT session; empty map also shuts the owner down. Unknown units lose the BlueZ bond |

stdio also advertises initialize **instructions**, prompts
`desk-sit` / `targets-walk` / `after-failed-snapshot`, and
resources `sticky-rs://remote-debug/pickup` /
`tools` / `snapshot`. A new agent process is required after
those clap-mcp serve options change.

Structured output is the generated ConnectRPC `*Response` JSON.
`get-snapshot` also adds host-only `png` (absolute page-image
path) after the write. `message` must not include a MAC.

## Self-test

After clap-mcp wiring or new tool names, self-test in a
**freshly launched subagent** (stdio attaches at process
launch). After a host notify / broker change, rebuild
`target/debug/xtask` and `disconnect` / `connect` so the
detached broker is the new binary; the stdio client can stay.
Same live-ask rules as the root
[AGENTS.md](../../../../AGENTS.md):

1. Stay on splash (remote-debug advertises from boot). Do not
   ask the operator to pair from a phone. `scene=pair` is
   optional.
2. `connect` without `--remember` unless the human asked. Do not
   run `monitor` in parallel. Expect `pairing`.
3. Poll `status` until `connected` (or `pair failed`).
4. `inject-touch` / `inject-button` on known ink. `--page` and
   `--phase` for the targets walk. Wait ~2–3 s for compose.
5. `get-snapshot`, then ack; a second get while armed is
   `SnapshotBusy`. After `frame: Version` or a failed get,
   `snapshot-clear` then retry (host notify must be FIFO).
   Optional: walk `scene=targets` (seven Page Downs) and score
   all seven marks from snapshot `expect`; wrap is
   `target_step=0` (`last_log` may be `target show id=0`).
6. Optional `reboot` (MCU reset; leftover BlueZ LTK is stale;
   UART reprints `pair pin=`).
7. `disconnect`.

Record new facts under [Discoveries](#discoveries) and awkward
edges under [Difficult / crude](#difficult--crude).

## Discoveries

- `--features remote-debug` packs with default `pair` + `wifi`.
  Two extra `PLANE_BYTES` copies in `.bss` (96 KiB LAST) overflow
  ESP32-S3 `dram_seg`
  (`stack.x`: *cannot move location counter backwards*). LAST is
  a 96 KiB **octal PSRAM carve** (`map_last_planes`). DRAW/TX stay
  in DRAM. UART `remote psram last=96000` means the carve is live;
  `remote psram map failed` means snapshots stay empty. Firmware
  intent, not a new electrical measurement (8 MB octal `AP_3v3`
  is already in the hardware skill).
- stdio MCP is process-lifetime. Rebuild xtask or the image, then
  start a new agent process; do not expect an already-open session
  to pick up the new binary.
- A new agent plus an approved project server is **not** enough
  if the stdio command never handshakes. This sit: `cargo xtask
  remote-debug --mcp` left no tool namespace. Launch
  **`target/debug/xtask remote-debug --mcp`** (the leaf is now
  optional so clap can accept `--mcp` without `status`).
  `cursor-agent` / `agent` did **not** expand
  `${workspaceFolder}` in the local stdio table (`spawn … ENOENT`).
  Use an absolute `command` path to this repo’s
  `target/debug/xtask`.
- CLI broker sit (2026-09-08): `connect` printed
  `connected (ephemeral BlueZ bond)` in ~11 s; later `status` /
  `inject-touch` from new processes hit the same socket.
  `get-snapshot` failed `frame: Version` on both CLI and stdio
  MCP until the BlueZ notify queue was a FIFO. MCP `status` saw
  `connected: true` from the CLI-started broker; MCP `disconnect`
  dropped GATT (`status` then `no broker`). Tool names were
  `status` / `inject-touch`, not `remote-debug_*`.
- Host walk sit (2026-09-08, after FIFO): `status` `connected`,
  two `inject-button` `page-down` shorts, `inject-touch` 400,240.
  Immediate `get-snapshot` on the old LIFO broker was still
  `frame: Version`. `disconnect`, rebuild `target/debug/xtask`,
  `connect` (first try `connected`). First get after reconnect
  was `SnapshotBusy` (firmware still armed from the Version
  fail) until `snapshot-clear`. Then three gray4 snapshots
  wrote `snap-<hex>.bw` / `.red` (800×480): splash (Ferris),
  shapes (four bars), pair (`Paired`). A second get while
  armed is `SnapshotBusy`. Planes are pre-rotation framebuffer
  (MSB-first PNG can look 180° or page-rotated vs glass).
  LAST lags glass if you stack shorts before compose finishes.
- Targets walk sit (2026-09-08, after `flash-app` of
  `--features remote-debug`): splash snapshot was Ferris only
  (no six-digit boxes; UART still reprints `pair pin=`). Seven
  `page-down` shorts reached `scene=7`. This attach's
  `inject-touch` tools/list had no `--page` / `--phase`; desk
  CLI / broker RPC with `page: true` worked. First tap at
  landscape centre `400,240` missed: `expect=240,400` (hold 0,
  portrait). Framebuffer PNG still looks landscape. Remaining
  marks used snapshot expect (dots `80,80` / `400,80` /
  `80,720` / `400,720`; slides `--phase` along the portrait
  midlines). After id 6, snapshot `target_step=0` and
  `last_log` `target show id=0` (`target loop` was the previous
  line). CLI `get-snapshot` now prints `scene=` / `step=` /
  `expect=` / `last_log=` on the same line.
- Page PNG (2026-09-08): `.png` is rematerialized from
  `Snapshot.hold` via `page_to_framebuffer` (portrait
  480×800, landscape 800×480). `.bw` / `.red` stay the
  800×480 planes. stdio initialize carries sit
  instructions; prompts `desk-sit` / `targets-walk` /
  `after-failed-snapshot`; resources
  `sticky-rs://remote-debug/pickup` / `tools` /
  `snapshot`.
- ConnectRPC owner sit (2026-09-08): CLI of the new binary
  (`target/debug/xtask remote-debug`) revalidated the prior
  loops. `status` with no owner is `no broker`. `connect` →
  `pairing` → `connected`. A later process `status` /
  `list-targets` still saw GATT (`targets=sticky-rs`; empty
  `message` used to print `ok`). `reboot` → `pairing` →
  `connected`; splash PNG was Ferris, no PIN boxes
  (`scene=0 hold=0`, page 480×800). Second `get-snapshot`
  while armed is Connect `FailedPrecondition` (`snapshot
  busy`); `snapshot-clear` then retry. Seven `page-down`
  reached `scene=7`. Dots at snapshot `expect`. Slides that
  used painted `r=` did not advance; page-end insets 80 /
  `page_len−80` did. After id 6, `target_step=0` and
  `last_log` `target show id=0`. `disconnect` emptied the
  map and stopped the owner. stdio on the new binary lists
  `list-targets` and ConnectRPC instructions. A later
  attach kept those initialize / pickup cards but had an
  empty tool table; the same sit used CLI `run()` (same
  leaf map as MCP). `tools/list` was rejected when
  `outputSchema` was schemars `AnyValue` (`serde_json::Value`):
  `outputSchema.type` was not the literal `"object"`. Leaves
  now advertise a JSON object (`additionalProperties` true)
  so the table can bind. Rebuild `target/debug/xtask` and
  start a new agent so stdio re-reads `tools/list`.
- MCP `connect` (2026-09-09): clap-mcp runs `run` on a Tokio
  worker. `ControlClient` used `Builder::block_on` on that
  worker, panicked (`Cannot start a runtime from within a
  runtime`), and poisoned the stdio mutex so later leaves
  printed `session lock`. The client now `block_on`s on a
  thread with no Tokio context. Rebuild `target/debug/xtask`
  and reload the stdio server.
- MCP revalidation (2026-09-09): same attach, no reload.
  `connect` → `reboot` → splash Ferris (`scene=0 hold=0`, no PIN
  boxes). Second `get-snapshot` is Connect `FailedPrecondition`
  (`snapshot busy`); `snapshot-clear` then retry. Seven
  `page-down` reached `scene=7`. Dots at snapshot `expect`.
  `slide_x` / `slide_y` used page-end insets. After id 6,
  `target_step=0` and `last_log` `target show id=0`.
  `list-targets` `sticky-rs`. `disconnect` → `no broker`.
  `get-snapshot` structured JSON used to include LAST DRAW
  planes (~125 KiB). xtask now clears `bw` / `red` after the
  host write (empty fields omit from JSON) and adds `png`
  (absolute page-image path). CLI prints `png=` on the
  control line. `.red` on disk is the second gray4 SSD1677
  plane, not pigment.
- Fresh attach (2026-09-09): `connect` → splash `png` (Ferris,
  `scene=0 hold=0`, no `bw` / `red` keys) → `page-down` →
  shapes `png` (`scene=1`) → ack → `disconnect`.

## Difficult / crude

- clap-mcp exposes only the `remote-debug` gate. There is no
  stdio path for `flash-app` or restore (on purpose).
- The CLI owner holds the session. stdio MCP is a client of
  the same ConnectRPC endpoint as `cargo xtask remote-debug
  status`. Do not treat an in-process mutex as the GATT owner.
- Parallel tool calls still set `parallel_safe = false`; the
  owner serializes GATT so two snapshot/inject RPCs cannot
  interleave ATT chunks.
- Auto-PIN and `monitor` share the UART lock. A sit that needs
  both will fail; use `--pin` or stop listen first.
- A listen that skipped Drop leaves `cdc-acm` unbound. `lsusb`
  still shows `1a86:55d3`, but `detect-connected` reports no
  QinHeng TTY (this sit: only an unrelated ACM remained).
  Default `connect` then claims the unique QinHeng over usbfs
  (flock on `/dev/bus/usb/{bus}/{dev}`) so auto-PIN does not
  need a replug. Stay on splash; Ferris never shows the PIN.
- `connect` spawns on-disk `target/debug/xtask` when that file
  exists so a deleted MCP inode is not required to start serve.
  New clap tools still need a new agent process. Detached serve
  logs to `$XDG_RUNTIME_DIR/sticky-rs/remote-debug.log`
  (does not inherit CLI/MCP stdio).
- Fresh MCP attach (2026-09-08): `status` reached the broker.
  With no `XDG_RUNTIME_DIR` the socket used to be
  `$TMPDIR/sticky-rs/…` while the desk CLI used
  `$XDG_RUNTIME_DIR/sticky-rs/`. The runtime dir now prefers
  `/run/user/<uid>/sticky-rs` when `XDG_RUNTIME_DIR` is unset
  so CLI and MCP share one broker. `connect` returns `pairing`
  (poll `status`); it no longer sits on the MCP client timeout.
  UART scrape starts at BlueZ `Connect` and covers one GATT
  wait plus one retry. Inject / snapshot still need `connected`.
  Firmware forgets the RAM LTK on drop (and on a leftover
  `Encrypted` without `PassKeyDisplay`) so the next Connect is a
  new PIN; `request_security` errors print `pair fail=pairing`
  even on splash.
- `frame: Version` on `get-snapshot` was a host bug: BlueZ TX
  notify used `Vec` + `pop` (LIFO). A 96 KiB plane is many ATT
  chunks; the assembler saw the last chunk first. `linux.rs`
  now `VecDeque` `push_back` / `pop_front`. Rebuild xtask and
  start a new broker (`disconnect` / `connect`); the stdio
  client can stay. After a Version fail, firmware stays armed
  — `snapshot-clear` before the next get.
- `reboot` resets the MCU, not the host. BlueZ leftover LTK after
  `CoreSw` must not be reused; wait for a new UART `pair pin=`.
- Local stdio tables (how a desktop client launches the server)
  are **not** git-owned. Do not add a client-named ignore line
  for them. Launch **`target/debug/xtask remote-debug --mcp`**,
  not `cargo xtask` (`xtask` is a `cargo run` alias). The leaf
  under `remote-debug` is optional so `--mcp` is not a clap
  “requires a subcommand” miss. Compile banners or a non-repo
  cwd break the JSON-RPC handshake; approving a server that
  never started still leaves no tools. `repo_root()` is
  `CARGO_MANIFEST_DIR`’s parent, so the binary does not need
  the client’s cwd.
