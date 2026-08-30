# UART, flash, PSRAM

How to read chip, JEDEC, and partitions on the unit in hand:
[measure.md](measure.md). Source layers: [sources.md](sources.md).

This page is the UART and flash geometry, not a host-tool cheatsheet.
Consuming projects supply their own flash path.

Product-class values seen on real Stickies (not DevKit / eval-board JSON):

| Item | Value |
| --- | --- |
| USB ID | `1a86:55d3` (QinHeng, “USB Single Serial”); udev by-id uses an underscore before the USB serial |
| Host driver | `cdc_acm` |
| Monitor baud | **115200** (factory UART) |
| Download | **921600**, 32 × 1 MiB reads: ~14 s/MiB, ~8 min for 32 MiB (confirmed). A one-shot 32 MiB `read-flash` at 1.5 Mbaud can buffer in RAM and never write a file |
| `probe-rs` | **No** JTAG/SWD/RTT on this connector |

QinHeng is not an Espressif VID. Prefer a by-id node matching
`usb-1a86_USB_Single_Serial_*-if00` (underscore before the USB serial). ACM
numbers move. Do not commit a specific USB serial string. Bare Espressif
CLIs need `--list-all-ports` / `--port` because QinHeng is not an Espressif
VID.

DTR/RTS ROM reset is expected. Glass keeps the last e-paper frame. The board
stays powered across those resets if USB and the power latch are up.

Chip, eFuse, JEDEC, factory DIO, PSRAM heap, and the factory partition table
are in [measure.md](measure.md). Physical flash is **0x2000000**. Table at
`0x8000`. Each replacement image must ship a 32 MB-aware table — not a
16 MB `n16r8` limit, and not Bunny’s map unless that is the chosen layout.

Keep `*.bin` flash images out of git. Do not restore one unit’s full-chip
image onto another (NVS secrets, MAC, serial). Never `erase-flash`. Custom
images belong in factory `app0` only.

## Flash geometry

Physical size **0x2000000** (32 MiB). Partition table at `0x8000`, bootloader
at `0x0`. Factory OTA+LittleFS (as read from hardware) is in
[measure.md](measure.md). Bunny factory+spiffs is in
[cpp-platformio.md](cpp-platformio.md).

## PSRAM

Octal 8 MB (confirmed `esptool flash-id`: Embedded PSRAM 8MB `AP_3v3`).
80 MHz is a proven firmware configuration (`esp-hal` and IDF),
not an eFuse field. Gray4 frames (~96 KiB) belong in PSRAM. DMA
descriptors/bounce buffers stay in internal RAM. A 48 KiB 1-bit frame can
live in DRAM while proving PSRAM.

## Radio

ESP32-S3: 2.4 GHz Wi-Fi + BLE 5.0 (ROM features). Station and BLE MACs are
per-device; read them on the unit, do not copy them from notes.
embassy-debug `--features radio` scanned both at once on ANT1
([pin-map.md](pin-map.md#on-glass-embassy-debug-radio-feature)).
