# Rust software paths

This skill is the board contract, not a host toolchain. Consuming projects
supply their own flash and UART tools. Two stacks are valid on this MCU:

| Stack | When |
| --- | --- |
| `no_std`: `esp-hal` + `esp-rtos` / Embassy | Bare-metal async |
| `std`: `esp-idf-hal` + `esp-idf-svc` | Share ESP-IDF drivers/partition story with vendor C++ firmware |

Encode [pin-map.md](pin-map.md) in a board-support crate. Chip drivers stay
MCU-agnostic. Register facts come from
[datasheets.md](../resources/datasheets.md). UART geometry:
[flashing.md](flashing.md). Observed silicon: [measure.md](measure.md).

Do not mix this page with PlatformIO / `idf.py`. Those trees are wiring
evidence in [cpp-platformio.md](cpp-platformio.md). Do not treat `esp-hal`
as the only legal Rust stack. Never `bq27xxx` (wrong gauge family). Never a
generic SSD1677 four-gray LUT.
