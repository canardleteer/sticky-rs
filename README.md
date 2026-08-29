# `sticky-rs`

> **Embedded Rust Tooling & Crates for the [Sticky](https://www.seeedstudio.com/sticky/docs/)**

> [!NOTE]
> I do have a "functioning" Embedded Rust dev environment for the Sticky,
> but I'm porting it over to clean git history slowly.
>
> For now, I'm just going to include the skill and safety information,
> but am working on moving the rest over as I can.

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
