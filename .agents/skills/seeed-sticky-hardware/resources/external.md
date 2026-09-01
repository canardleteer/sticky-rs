# External skills and sources

Companion material, not this board contract. Prefer live silicon
([measure.md](../references/measure.md)) and the pin map in this skill when
they disagree.

## varo6 `sticky-device` skill

ESP-IDF + Playground documentation skill. Installs as `sticky-device`. MIT.
Not affiliated with Seeed. Snapshot: their docs and the registry move.

- Skill repository: https://github.com/varo6/reTerminal-sticky-skill
- Portable skill tree: https://github.com/varo6/reTerminal-sticky-skill/tree/main/skills/sticky-device
- `SKILL.md`: https://github.com/varo6/reTerminal-sticky-skill/blob/main/skills/sticky-device/SKILL.md
- `references/hardware.md`: https://github.com/varo6/reTerminal-sticky-skill/blob/main/skills/sticky-device/references/hardware.md
- `references/esp-idf-dev.md`: https://github.com/varo6/reTerminal-sticky-skill/blob/main/skills/sticky-device/references/esp-idf-dev.md
- `references/display.md`: https://github.com/varo6/reTerminal-sticky-skill/blob/main/skills/sticky-device/references/display.md
- `references/peripherals.md`: https://github.com/varo6/reTerminal-sticky-skill/blob/main/skills/sticky-device/references/peripherals.md
- `references/playground-registry.md`: https://github.com/varo6/reTerminal-sticky-skill/blob/main/skills/sticky-device/references/playground-registry.md
- `references/ecosystem-and-docs.md`: https://github.com/varo6/reTerminal-sticky-skill/blob/main/skills/sticky-device/references/ecosystem-and-docs.md
- Driver fetch script: https://github.com/varo6/reTerminal-sticky-skill/blob/main/skills/sticky-device/scripts/fetch_sticky_sources.sh

Where they disagree with this skill (GPIO46 pulse vs hold-high, “2048 is the
only public physical-unit source”, GPIO40 polarity as settled): this skill wins
until [not-yet-confirmed.md](not-yet-confirmed.md) closes the item. They are
stronger on Playground `integration.json` and the `seeed_epaper` C API.

## Sources that skill distilled

These are the upstream trees their README and `ecosystem-and-docs.md` point at.
Use them directly; do not treat the skill as a substitute for the git history.

| Source | URL | What they used it for |
| --- | --- | --- |
| Seeed docs hub | https://www.seeedstudio.com/sticky/docs/ | Device guides (still being written) |
| Hardware overview | https://www.seeedstudio.com/sticky/docs/en/device-guide/hardware-overview/ | Pin tables, GPIO7 as IMU INT |
| ESP-IDF basics | https://www.seeedstudio.com/sticky/docs/en/device-guide/esp-basics/ | Dashboard demo layout |
| Pages and peripherals | https://www.seeedstudio.com/sticky/docs/en/device-guide/esp-pages/ | Peripheral patterns |
| Display refresh | https://www.seeedstudio.com/sticky/docs/en/device-guide/esp-refresh/ | `seeed_epaper` refresh modes |
| Playground site | https://www.seeedstudio.com/sticky/playground/ | Flash catalog |
| ESPHome Playground | https://www.seeedstudio.com/sticky/docs/en/playground-docs/esphome/ | Generated YAML |
| Playground registry | https://github.com/Seeed-Projects/reterminal-sticky-playground-registry | `integration.json`, CI, `sticky-2048` source |
| Registry CONTRIBUTING | https://github.com/Seeed-Projects/reterminal-sticky-playground-registry/blob/main/CONTRIBUTING.md | Contribution paths |
| Registry schema | https://github.com/Seeed-Projects/reterminal-sticky-playground-registry/blob/main/schemas/integration.schema.json | Catalog validation |
| `sticky-2048` upstream | https://github.com/Lukilyy/reterminal-sticky-2048-eink-game | ESP-IDF app, `pin_config.h`, drivers |
| Official firmware repo | https://github.com/Seeed-Projects/OSHW-reTerminal-Sticky | Referenced; has been 404 |
| E-series wiki (concepts only) | https://wiki.seeedstudio.com/reterminal_e10xx_main_page/ | Not Sticky pins; UF2/XIAO flow does not apply |
| E-series ESPHome | https://wiki.seeedstudio.com/reterminal_e10xx_with_esphome/ | Idioms only |
| E-series OSHW | https://github.com/Seeed-Projects/OSHW-reTerminal-Series-E-D | Different product line |

Full URL list in this skill: [catalog.md](../references/catalog.md).

## ESPHome audio on a physical unit

[sira-fiinikkusu/reterminal-sticky-voice-companion](https://github.com/sira-fiinikkusu/reterminal-sticky-voice-companion)
is household ESPHome on production Stickys, not a documentation skill. Use it
for PDM bring-up after deep sleep (USB-Serial-JTAG pad on GPIO19/20, GPIO38
rail cycle, 16 kHz left). Wiring absorbed in
[sensors.md](../references/sensors.md#pdm-microphone). It is not a substitute
for the pin map, and it does not close
[nyc-mic-pdm](not-yet-confirmed.md#nyc-mic-pdm).
