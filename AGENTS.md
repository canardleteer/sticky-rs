# sticky-rs

Board contract and safety notes for the Seeed Studio reTerminal Sticky.
Firmware is not in this tree yet.

## Hardware safety (read before writing code)

1. **Never erase this board's flash.** No `erase-flash`, no full-chip erase.
   Do not write below `0x90000` except a restore of **that same unit's**
   original image. The factory NVS holds per-unit Wi-Fi RF calibration,
   device identity, and persisted gauge state; none of it is regenerable by
   hobby tools.
2. **No e-paper waveform may be invented.** The Sticky's confirmed path is
   panel **OTP** sequences (no MCU `0x32` write). Do not add a default
   105-byte LUT without recorded provenance and a compatible license.
3. **Fuel gauge writes stay off.** Reads are safe; unseal, `CFGUPDATE`, and
   Full Charge Capacity writes are not, and the OTP is one-time.
4. **Latch power first, release deliberately.** GPIO45 then GPIO46 high before
   logs or buses.

Full hazard table: [docs/SAFETY.md](docs/SAFETY.md). Board facts and source
precedence: [seeed-sticky-hardware](.agents/skills/seeed-sticky-hardware/SKILL.md).

## Do not connect to a physical device

This repository has **no in-repo device I/O**. Do not open a serial port,
run `espflash` / `esptool` / `idf.py flash` / PlatformIO upload, or
`probe-rs` against hardware unless the human **explicitly asked** to run
that live command on a device in that message.

A device may be attached for unrelated reasons; ignore it. Never commit a
MAC address, serial number, USB serial string, NVS blob, or flash image.
`backups/` is gitignored on purpose.

## Keep skills updated

Project-local skills must stay aligned with the tree. When you change a
topic below, update the matching skill in the **same change**. State source
conflicts in the hardware skill instead of flattening them.

| When you change | Also update |
| --- | --- |
| Pin, rail, display, touch, sensor, enclosure, measurement backlog, or datasheet catalog | [seeed-sticky-hardware](.agents/skills/seeed-sticky-hardware/SKILL.md) (and a [sources.md](.agents/skills/seeed-sticky-hardware/references/sources.md) conflict row if sources disagree) |
| Hardware safety, live-ask set, or “never erase” | this file **and** the hardware skill if it restates it |

## Working rules

- **Do not invent registers or opcodes.** If the datasheet has not been read,
  record the gap in the hardware skill catalog
  ([resources/datasheets.md](.agents/skills/seeed-sticky-hardware/resources/datasheets.md)).
  Cached PDFs and extracted markdown are gitignored under that skill’s
  `resources/datasheets/`. Ask the user to populate the cache
  (`scripts/fetch_datasheets.py` from the skill directory); do not download
  vendor files unless they asked.
- Prefer a named `enum` or `const` over a magic number. Prefer the vendor
  datasheet’s name when the sheet has one.
- Never `bq27xxx` (wrong gauge family); never a generic SSD1677 four-gray LUT.
- Measurement-backlog items in the hardware skill stay open until someone
  measures them. Firmware evidence proves intent and sequencing, never
  electrical fact.

## Agent Documentation Standards

Project-local skills exist under `.agents/skills/` and should remain
discoverable by agents working in this repository. Maintain those skills
according to the [Agent Skills specification](https://agentskills.io/specification),
and maintain this file according to the
[AGENTS.md standard](https://agents.md/). Keep both portable
across compatible agent clients, without assumptions about user-specific paths
or session state.

One skill:

- [seeed-sticky-hardware](.agents/skills/seeed-sticky-hardware/SKILL.md) —
  board contract (pins, rails, datasheets, observed silicon, source
  precedence). The skill user weighs conflicts. Host discovery and flash I/O
  belong to a consuming project's tools, not this skill.

Vendor datasheets are official for registers of chips confirmed on this
model; observed hardware still outranks a datasheet default. See
[Authority](.agents/skills/seeed-sticky-hardware/SKILL.md#authority).
