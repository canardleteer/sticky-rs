# Measure silicon on a Sticky

Observed hardware on a **real reTerminal Sticky** outranks SDK pin sheets,
DevKit board JSON, and compiled profiles that were never run on this
product. Layers and conflicts: [sources.md](sources.md). The skill user
weighs disagreements.

This page records **what was seen on silicon**. It does **not** include GPIO
numbers, GT911 addresses, or panel orientation — those still come from
firmware pin maps. Do not record another person’s MAC, serial number, USB
serial string, NVS, or flash image into shared docs.

Consuming projects supply their own host capture tools. Do not open a port
unless a human asked.

Vendor C++ trees are wiring evidence in [cpp-platformio.md](cpp-platformio.md).

## Find the UART

USB-C debug is a WCH CH343P CDC-ACM bridge (`1a86:55d3`, QinHeng “USB Single
Serial”), not Espressif USB-Serial/JTAG.

Prefer a stable by-id node (ACM numbers move). udev names this product's
QinHeng node with an **underscore** before the USB serial:

```text
/dev/serial/by-id/usb-1a86_USB_Single_Serial_<SERIAL>-if00
```

The host user needs `dialout` (or equivalent). Ignore other CDC devices on
the same machine.

ROM-download DTR/RTS into the stub, then a hard-reset, is expected for
board-info and flash I/O. The board stays powered if USB and the power latch
are up. E-paper keeps the last frame while the ROM stub holds the CPU (a
frozen loading bar on glass is not a hung MCU).

QinHeng is not an Espressif VID. Bare Espressif port lists often print
“no known ports” without `--list-all-ports`:

```shell
espflash list-ports --list-all-ports --skip-update-check
probe-rs list   # expected: no probes on this connector
```

Prefer a consuming project’s QinHeng inventory over that fallback when it
already answers.

## Confirmed live

Product-class results on a physical Sticky. No conflict with the snapshot in
[SKILL.md](../SKILL.md). Per-unit MAC, USB serial, and factory
`serial_number` omitted. Host-tool names below are **provenance** of how
the fact was captured, not a requirement that every consumer use those
commands.

| Item | Confirmed |
| --- | --- |
| USB-C UART | QinHeng CH343P `1a86:55d3`, product “USB Single Serial”, `cdc_acm` |
| udev by-id | `usb-1a86_USB_Single_Serial_<SERIAL>-if00` (underscore before the serial) |
| Chip | ESP32-S3 QFN56 **rev v0.2**, crystal **40 MHz** |
| PSRAM / JEDEC | **8 MB** Embedded PSRAM `AP_3v3`; flash **ef 4019**, **32 MB**, eFuse **quad**, eFuse **3.3 V** |
| ROM board-info | Reports 32 MB and `Embedded Flash`; **omits PSRAM** (already confirmed by `esptool flash-id`; do not re-run it unless asked) |
| Secure boot / flash encryption | **Off**, `SPI_BOOT_CRYPT_CNT` 0 |
| Factory UART `serial_number` | Printed ~**4.5–6.5 s** after an EN/RTS pulse into **run mode** (IDF `I (5672)`). Opening the ACM node does **not** reprint it. ROM download reset is a different sequence. |
| Chunked 32 MiB read | 32 × 1 MiB at **921600** baud: ~**14 s/MiB**, ~**8 min** full dump. espflash may warn about baud > 115200; the dump completed. Stub loaded once for board-info and again for the dump. |
| Custom `app0` + UART listen | A ~105–114 KiB `save-image` payload at factory `app0` (`0x90000`) booted. Factory 2nd-stage **ESP-IDF v5.4-dirty** logged DIO, 32 MB, and `Loaded app from partition at offset 0x90000`. Opening a 115200 listen produced a `rst:0x1 (POWERON)` log even after DTR/RTS were set inactive. Heartbeats showed raw GPIO/gauge/IMU levels (`vbus=1`, `gpio7=0` with pull-up, `gpio40=1`, `sd_cd=1`, `imu=FaceUp` while sitting on USB). I2C ACKs included GT911 `0x14` and SHT40 `0x44` on a real measure command. |
| Operator UART session | Attended: `btn 4` / `5` / `6` on the three right-edge keys; USB-C unplug as host CH343 drop+return (firmware `vbus` edge usually lost); IMU `FaceUp` then `Landscape0` after tilt; GPIO7 stayed low with no edges; `gpio40=1` / `i=0` with `/CE` off after replug. GT911 `0x14` ACK. **No simple-debug `contacts=` line, ever.** Every attended `gt911_contacts` YAML is `timeout` / `operator_says_tried: true` / `gt911_st_max=0x00` / `gt911_int=1`. An earlier poll treated crate `NotReady` as failure and skipped status-clear; later images leave INT floating and still never set status bit `0x80`. Bunny on glass does report points (100 kHz, 30 ms, tap on release). MicroSD not inserted. Restoring factory `app0` then returned stock. |
| Embassy-debug touch listen | Default image (`git=80eaf8f` dirty), 2026-08-30. INT-high + init Status-clear: `0x14 ack`, `st=` stayed `0x00`, no `touch n=`. Rev.09 INT-low select: `gt911 int=0`, **`0x5d ack`**, `0x14 nak`, first `st=0x80`. Attended: `touch n=1` / `n=0`, `n=2`, then **`n=5`** with `gt911 st=0x85` (t=43705–44046). This FPC delivers 5 contacts (Rev.09 §1). |
| Unattended idle UART | 2026-08-30, sit still on USB, no keys / taps / `/CE`. embassy-debug default: `latched`, INT-low dance **`0x5d ack`** / `0x14 nak`, `imu accel init ok`, `imu=FaceUp`, `gt911 st=` (`0x80` then later `0x00`), `scene=splash`. simple-debug default: latch, INT-high `0x14 ack` (that image's dance), gauge `0x0220`, SHT `t=` / RTC `vl=0`, heartbeat `vbus=1 gpio7=0 gpio40=1 sd_cd=1` and `i=0` after settle, `imu=FaceUp`. Host `vet-idle-log` accepted both captures. Does not close glass PN, corners, or CEDV. Factory `app0` restored after. |
| Panel+SD 10 MHz | 2026-08-30. Card inserted, embassy-debug default (`SPI_MAX_HZ` 10 MHz), CS high, rail off, no SD writes. UART `scene=splash` then `btn 6` → `scene=shapes`. Operator: splash and later pages looked clean. Does not close 20 MHz, card-read, or `nyc-sd-mount`. |
| Panel+SD 20 MHz | 2026-08-30. embassy-debug `--features spi20`. UART `spi=20000000`, `scene=splash` twice, no `epd busy timeout`. Card inserted, CS high, rail off, no SD writes. Operator: splash looked right vs the 10 MHz sit. Does not close card-read or `nyc-sd-mount`. Board `SPI_MAX_HZ` stays 10 MHz. |
| SD read-only identify | 2026-08-30. embassy-debug `--features sd`. POWERON reprint: `sd cd=0`, `sd hz=400000 type=sdhc mid=0x03 name=SS32G`, `sd hz=10000000 ack`, `sd hz=20000000 ack`, then `spi=10000000` and `scene=splash`. No CID product serial printed. No writes. Closes `nyc-spi-ceiling`. Does not close `nyc-sd-mount`. |
| SD FAT ReadOnly | 2026-08-30. Same `--features sd` image after identify. `sd vol=0`, eight root `sd ent` lines, `sd dir n=8`, `sd read name=MANIFEST n=64` (file size 114981; contents not printed). No writes. Closes `nyc-sd-mount`. |
| GT911 glass corners | 2026-08-30. USB-C down, ink corners (not the frame). Old `to_screen` (sample as 800×480): keys-side `p0=` `792,195` / `0,195`; mid left→right Y only `477→199`. After map from 480×800: first `p0=` `795,470`, `795,4`, `4,475`, `4,4`. Closes `nyc-gt911-corners`. |
| Restore factory `app0` | 6 MiB write-bin at `0x90000` (~90 s including verify at 921600). Stock `reterminal_template` **1.1.0** (compile **Aug 7 2026**, IDF `v5.4-dirty`, **160 MHz**, Winbond **DIO**) booted; HAL blocks including `environment_sensor` ACK’d `result=ok`. |
| Confirm after `app0` restore | After restoring factory `app0` (bring-up restore, and again after the UART learning image), a 32 × 1 MiB live read (~**463 s**) **matched** the original (no drifted regions). |

ROM board-info omits PSRAM. Trust the PSRAM / JEDEC row above rather than a
second `esptool flash-id` run. `esptool flash-id` also listed dual-core +
LP core and a **240 MHz** chip capability; that is not the factory app clock
(UART has logged **160 MHz**).

## Factory UART log

After a hard-reset, 115200 on UART0. Sample a few seconds (the HMI loading
bar may not finish). Look for:

- CPU frequency, IDF version, project name / app version
- `spi_flash: detected chip` and `flash io:`
- `esp_psram:` heap size and SRAM test
- `board: init … result=ok` lines (which blocks ACK)
- `power_en_lock` / “power latched inside”

Factory firmware seen on hardware: `reterminal_template` 1.1.0 (app compile
**Aug 7 2026 20:00:35**), ESP-IDF `v5.4-dirty`, PROD, **160 MHz**, Winbond
**DIO** at runtime, ~5 MiB PSRAM added to the heap, instructions mapped into
SPIRAM, 256 KiB internal reserved for DMA. 2nd-stage bootloader compile time
**Jul 30 2026 16:19:37**.

HAL blocks that have ACK’d `result=ok` on a Sticky:

button_up, button_down, button_ok, `power_en_lock`, buzzer, spi, display,
sensor_i2c, touch_i2c, touch, battery_gauge, rtc, environment_sensor, imu,
battery_charge_input, microphone, sd_card, sd_hotplug_monitor.

That proves those peripherals exist. It is not a GPIO map. NVS keys
(timezone, language, bind state, sleep intervals) are per-device settings.

## Partition table

Factory layout seen on hardware (32 MiB, table at `0x8000`, bootloader at
`0x0`):

| Name | Type | Offset | Size |
| --- | --- | --- | --- |
| nvs | nvs | `0x9000` | `0x7d000` (500 KiB) |
| otadata | ota | `0x86000` | `0x2000` |
| phy_init | phy | `0x88000` | `0x1000` |
| app0 | ota_0 | `0x90000` | `0x600000` (6 MiB) |
| app1 | ota_1 | `0x690000` | `0x600000` (6 MiB) |
| sys_storage | littlefs `0x83` | `0xc90000` | `0x930000` |
| usr_storage | littlefs `0x83` | `0x15c0000` | `0xa00000` |
| coredump | coredump | `0x1fc0000` | `0x40000` |

Read `otadata` to see which slot is active. `app_desc` in the active app
gives project name, version, and IDF version. Do not commit those ELF hashes
or copy another unit’s NVS. A matching original dump may keep
`partitions.csv` next to the image (keep that tree out of git).

### What NVS holds (never erase it)

The `nvs` partition at `0x9000` is **not** regenerable by you. It carries:

- **Wi-Fi RF calibration** (`nvs.net80211` calibration data, calibration MAC,
  calibration version) written during factory test
- **Device identity** — serial number, provisioning UUID, client id
- **Persisted gauge state**, including a saved Full Charge Capacity the app
  restores into the BQ27220 at boot
- Per-device settings (timezone, language, bind state, sleep intervals)

Consequences, in order of how much they will ruin your evening:

1. No `erase-flash` / `erase_flash`, no full-chip erase, no writes below
   `0x90000`. Radio performance after losing calibration is not something you
   can re-derive with hobby tools.
2. The only restore is a full-chip image of **that same unit**, taken before
   the damage. Take that backup before your first write.
3. Secure boot and flash encryption have been seen **off**, so a restore needs
   no key material — but that also means nothing stops a careless erase.

Keep replacement firmware inside factory `app0` and leave the data
partitions alone.

## Stock firmware image inspection

Static reads of a stock image you already have on disk are host-only work: no
port, no reset, no device. They answer *how vendor firmware drives this board*
where UART logs only prove a block ACK’d.

Useful, in rough order of yield:

- **Board init order and names** — the `board: init …` sequence, plus HAL pin
  macro identifiers (`HAL_EPD_*`, `HAL_SD_*`, `HAL_PWRIN_VOLT`, `HAL_SD_DET`),
  which show which nets the vendor owns and how they are grouped.
- **Driver API shape** — exported symbol and log-format strings reveal the
  panel, touch, and gauge call sequences, including operations a datasheet
  lists but firmware never uses.
- **Interrupt and wake wiring** — which pins get ISRs, which are armed as
  `ext1` wake sources, and which levels are held across sleep.
- **Device configuration sequences** — for example the gauge’s
  unseal / CFGUPDATE / verify / seal path, which tells you what the vendor
  considered necessary and therefore what is risky.

Limits, and they matter:

- Firmware proves **intent and sequencing, not electrical fact**
  ([SKILL.md](../SKILL.md#authority) layer 3). A digital-input ISR does not
  by itself prove there is no divider; schematic Rev 01 does (GPIO9 is
  `PWR_IN_VOLT`).
- Vendor binaries are **not a source of bytes**. Do not lift LUTs, calibration
  tables, or code out of a stock image into your own project; they are
  copyrighted. Port from a licensed source instead.
- Per-unit data (MAC, serial, UUID, NVS blobs, calibration) stays on the unit.
  Record derived facts as "stock firmware does X", with no path, filename, or
  unit identifier.

## Full-flash backup

A single `espflash read-flash 0x0 0x2000000` at 1.5 Mbaud can buffer the
entire 32 MiB in RAM and **never create the output file**. Do not run that.
Chunk 32 × 1 MiB at 921600 (~14 s/MiB, ~8 min). Restore factory `app0` is
write-bin of that unit’s `app0` at `0x90000` (confirmed: stock app booted,
confirm matched). A full-image restore is write-bin at offset `0x0` of a
32 MiB image **from that same unit**. Do not flash someone else’s dump.
Never `erase-flash`.

## Re-measuring PSRAM, JEDEC, and security (human-asked only)

ROM board-info omits PSRAM. The product-class values are already in
[Confirmed live](#confirmed-live). If a human asked to re-measure those
fields on the unit in hand:

```shell
esptool --chip esp32s3 --port "$PORT" flash-id
esptool --chip esp32s3 --port "$PORT" get-security-info
```

(`esptool` 5.x uses hyphens: `flash-id`. Older `esptool.py flash_id` is the
same report.) Do not run them while another tool holds the UART. Station and
BLE MACs and any `serial_number` in factory logs are **per-device**. Keep
them out of the skill and out of committed notes.

## What this method does not give you

GPIO pinout, SPI clock, BUSY polarity, GT911 address/transform, IMU axes,
charger GPIO polarity, or other firmware’s partition tables. Measure those
from pin maps / on-glass bring-up, not from ROM `board-info`.
