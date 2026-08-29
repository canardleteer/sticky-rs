# sticky-rs

Hardware notes and a board contract for the Seeed Studio reTerminal Sticky.
Firmware is not in this tree yet.

**Read [docs/SAFETY.md](docs/SAFETY.md) before flashing or probing a unit.**
A mistake can destroy factory NVS (per-unit RF calibration), the fuel-gauge
OTP, or the panel. The pin map, rails, and source precedence live in
[`.agents/skills/seeed-sticky-hardware/`](.agents/skills/seeed-sticky-hardware/SKILL.md).

## License

Sources in this repository are licensed under the MIT license. See
[LICENSE](LICENSE).

Seeed, reTerminal, Sticky, Espressif, and other product or company names are
trademarks of their respective owners. This project does not claim those
marks or their copyrights, and is not affiliated with or endorsed by those
owners.
