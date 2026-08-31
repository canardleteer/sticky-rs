# Datasheets and provenance

Vendor documents are the official source for registers, opcodes, and timings
of parts **confirmed on this model**. Observed hardware still outranks a
datasheet default. Both GT911 7-bit addresses ACK here (Rev.09 §6.1:
INT=0 → `0x5D` with contacts; INT=1 → `0x14` ACK, no points on an
init Status-clear path). Wiring authority is the board contract in
[SKILL.md](../SKILL.md). Precedence:
[sources.md](../references/sources.md).

This file is the committed catalog. Links below are relative to this skill
`resources/` directory.

**Vendor the local cache** when the work is registers, opcodes, timings,
strapping, or a datasheet-versus-glass conflict. Search extracted markdown
rather than loading a whole TRM. The cache does not replace the pin map or
enclosure. For official Seeed / Playground HTML as an offline markdown
corpus, use the user-global `skill-corpus-vendoring` skill.

**No PDFs or extracted markdown are committed.** They live under
[datasheets/](datasheets/README.md) (`pdf/` and `md/`, gitignored). If those
files are missing, ask the user to populate the cache before inventing a
constant. Do not download vendor files unless they asked.

```shell
# from this skill directory
python3 scripts/fetch_datasheets.py status
# if the user asked to populate the cache:
python3 scripts/fetch_datasheets.py fetch
```

Some vendor portals (ST, sometimes TI) refuse a scripted GET. In that case
the user saves the PDF into `datasheets/pdf/` using the filename in the table,
then runs `fetch_datasheets.py convert`.

When a cached markdown file exists, search that rather than loading a whole
TRM. The extraction is text for agents; figures stay in the PDF.

## Documents

| Id | Part | Document | Revision used | Cache | Datasheet notations |
| --- | --- | --- | --- | --- | --- |
| `ssd1677` | SSD1677 EPD controller | [Waveshare SSD1677 Rev 1.0](https://files.waveshare.com/upload/2/2a/SSD1677_1.0.pdf) ([solumco copy](https://www.solumco.com/files/SSD1677.pdf)) | **Rev 1.0, Nov 2018** | `pdf/ssd1677.pdf`, `md/ssd1677.md` | Table 7-1 opcodes; 105-byte LUT (0x32); `8.3`–`8.5` window/cursor; dual RAM planes `6.5 RAM`, Tables 6-4/6-5 |
| `bq27220-sluscb7` | BQ27220 fuel gauge | [TI SLUSCB7](https://www.ti.com/lit/ds/symlink/bq27220.pdf) ([GitHub mirror](https://github.com/kodediy/kode_bq27220-idf/raw/main/BQ27220_Datasheet_RevA.pdf)) | SLUSCB7 | `pdf/bq27220-sluscb7.pdf`, `md/bq27220-sluscb7.md` | `7.3.1.1 I2C Interface` 7-bit address; DeviceType via MAC |
| `bq27220-sluubd4` | BQ27220 fuel gauge | [TI SLUUBD4](https://www.ti.com/lit/ug/sluubd4/sluubd4.pdf) | SLUUBD4 | `pdf/bq27220-sluubd4.pdf`, `md/bq27220-sluubd4.md` | `Table 2-1. Standard Commands`; `2.2 Control()`; CEDV data-memory layout |
| `bq25616` | BQ25616 charger | [TI SLUSDF7](https://www.ti.com/lit/ds/symlink/bq25616.pdf) | SLUSDF7 | `pdf/bq25616.pdf`, `md/bq25616.md` | `Table 7-1. Pin Functions` CE active-low; `9.3.8.2` / `Table 9-6` STAT |
| `lsm6ds3tr-c` | LSM6DS3TR-C IMU | [ST product page](https://www.st.com/en/mems-and-sensors/lsm6ds3tr-c.html) ([ST PDF](https://www.st.com/resource/en/datasheet/lsm6ds3tr-c.pdf); fetch used a [public copy](https://www.makerguides.com/wp-content/uploads/2025/09/lsm6ds3tr-c-datasheet.pdf) after ST timed out) | — | `pdf/lsm6ds3tr-c.pdf`, `md/lsm6ds3tr-c.md` | Orientation / axis registers |
| `gt911` | GT911 touch | [Waveshare GT911](https://files.waveshare.com/wiki/common/GT911_EN_Datasheet.pdf) ([Pine64 copy](https://files.pine64.org/doc/datasheet/pine64/GT911%20Capacitive%20Touch%20Controller%20Datasheet.pdf)) | **Rev.09, 11 Mar 2015** | `pdf/gt911.pdf`, `md/gt911.md` | 8-bit→7-bit addresses; 400 kbps cap; 5-point max; register map deleted in Rev.07 (on-glass names for remaining encodings) |
| `sht4x` | SHT40 | [Sensirion catalog](https://sensirion.com/products/catalog/SHT40) ([PDF V7.3](https://sensirion.com/media/documents/33FD6951/6A7C10A0/HT_DS_Datasheet_SHT4x_V7.3.pdf)); drivers on [GitHub](https://github.com/Sensirion/embedded-sht) | V7.3; measure `0xFD` / `0xF6` / `0xE0` | `pdf/sht4x.pdf`, `md/sht4x.md` | High/medium/low precision measure; serial command `0x89` (do not print a unit serial) |
| `pcf8563` | PCF8563 RTC | [NXP](https://www.nxp.com/docs/en/data-sheet/PCF8563.pdf) (that path 404’d; fetch used [Rev 11 copy](https://datasheet.chipsfind.com/PCF8563T-F4-112-436673.pdf)) | Rev 11, 26 Oct 2015 | `pdf/pcf8563.pdf`, `md/pcf8563.md` | Timekeeping; seconds-register VL / integrity flag |
| `esp32-s3-datasheet` | ESP32-S3 | [Datasheet PDF](https://documentation.espressif.com/esp32-s3_datasheet_en.pdf) on the [Espressif document list](https://documentation.espressif.com/en/documentList?eol=false) | **Version 2.2** | `pdf/esp32-s3-datasheet.pdf`, `md/esp32-s3-datasheet.md` | Strapping (GPIO0/3/45/46), JTAG pads GPIO39–42, GPIO21 no default pull, I2C 100/400 kbit/s, USB 19/20 |
| `esp32-s3-trm` | ESP32-S3 | [TRM PDF](https://documentation.espressif.com/esp32-s3_technical_reference_manual_en.pdf) | — | `pdf/esp32-s3-trm.pdf`, `md/esp32-s3-trm.md` | Strapping pins, GPIO hold, `ext1` wake |
| `seeed-sticky-schematic` | reTerminal Sticky (board) | [Seeed schematic PDF](https://files.seeedstudio.com/wiki/reterminal_sticky/res/reTerminal_Sticky_Schematic_diagram_260609.pdf) (Hardware Overview Resources) | **Rev 01, 2026-06-05**, CC BY-SA 4.0 | `pdf/seeed-sticky-schematic.pdf`, `md/seeed-sticky-schematic.md` | Nets and orderable PNs; no BOM. Walk the PDF pages, not only the extract |

Cache paths are relative to [datasheets/](datasheets/README.md). Download URL order lives in
`scripts/fetch_datasheets.py` (vendor first, then GitHub or other public copies).

## Captured SHA-256

PDFs and extracted markdown are gitignored. Their SHA-256 digests are
committed so a later IPFS CIDv1 (raw codec + sha2-256) can be derived
without re-hosting the files in git.

- [datasheets.sha256](datasheets.sha256) — `sha256sum` format
- [datasheets.sha256.json](datasheets.sha256.json) — same records plus byte lengths

`fetch_datasheets.py hash` rewrites both after a convert. `status` checks
the local files against this list.

| Id | PDF SHA-256 | bytes |
| --- | --- | ---: |
| `ssd1677` | `7abe5dedcd565d3cb5720a0c426e99beabc803a1d095e3cdf804fd0875def64e` | 4890203 |
| `bq27220-sluscb7` | `9fffe0b632cd287c9210aa1d2e71af1816b31764c11f1b62425de3a4a7cfe5f4` | 570379 |
| `bq27220-sluubd4` | `44779f229423d649e410439d1d1a3a65026d5919cdd43112ee3a3d8d9811ead9` | 2768446 |
| `bq25616` | `edd24a0dd6232faf9e6d995fdd45d29d12ac4bc265badeccbc87b78ee1944449` | 3092998 |
| `lsm6ds3tr-c` | `414688492abb05e48c5b680726f5aa9da177f6be1154d4ab2cafe6ba9b530abf` | 1178887 |
| `gt911` | `227d240eb4344a643237ffcbff0da08d4765dd75f6d335382c283a77fc26c8ab` | 1634301 |
| `sht4x` | `8db4a43f17149b76811cfb504caaeca4ef844ddc710cb9b45905c51c7ddfe3c2` | 1049911 |
| `pcf8563` | `871273b1062f7ace8e03db6679ca6aba31aa8028227830f262c18d1b8da2e05f` | 495457 |
| `esp32-s3-datasheet` | `2d5a7cb7fd559d8d972bd88db32669c0196d23f22d7afaafb0f63d099b589a3f` | 1098115 |
| `esp32-s3-trm` | `4484bf8a69035ec42a731c58c64ada6fbd1f1618c5559409f134d9ea083f444f` | 15215232 |
| `seeed-sticky-schematic` | `32232320af8a733b9eb2c8c85877aa233a025b28096fabea1954d428e4aaefde` | 982991 |

## Verified against the SSD1677 datasheet

Facts below were read out of Rev 1.0 rather than assumed, because each one is a
place where a plausible guess produces a corrupt frame or unnecessary panel
stress:

- **Opcodes** come from Table 7-1. Use the datasheet name for each opcode.
- **`Write LUT register` (0x32) takes 105 bytes.** Section `6.6 Programmable
  Waveform for Gate, Source and VCOM` describes on-chip waveform storage
  including gate/source voltage and frame rate; the MCU-facing Table 7-1
  command is 105 bytes. (`6.2 Oscillator` is a different section.)
- **Two RAM planes exist** (`Write RAM (Black White)` 0x24 and
  `Write RAM (RED)` 0x26), each 960x680 bits, and the bit pair selects a LUT
  index (section `6.5 RAM`, Tables 6-4 / 6-5). That is the mechanism
  four-gray is built on.
- **Window and cursor values are 10-bit**, with X limited to `0x000..=0x3BF`
  and Y to `0x000..=0x2A7` (sections `8.3 Set RAM X - Address Start / End
  Position (44h)`, `8.4 Set RAM Y… (45h)`, `8.5 Set RAM Address Counter`).
  Keep these in datasheet address units; do **not** silently divide by 8.
- **Deep sleep is 0x10 with `A[1:0] = 0b11`**, and BUSY stays high afterwards.
- **Stock "standby" is not a 0x10 mode.** `ssd1677_standby` runs
  Display Update Control 2 `0x22 = 0x03` (disable analog, then
  clock) and Master Activation `0x20`. `ssd1677_resume` runs
  `0x22 = 0xC0` (enable clock, then analog) and `0x20`. Table 7-1
  already names those stage combinations. RAM stays until deep
  sleep.

## Verified against the GT911 datasheet

Facts below were read out of **Rev.09 (11 Mar 2015)**. Rev.07 **deleted the
register map**; this PDF is not a source of `0x814E` / command-`0` / bit
`0x80`.

- **I2C slave pairs** are 8-bit `0x28`/`0x29` and `0xBA`/`0xBB` (§6.1).
  The 7-bit addresses are `0x14` and `0x5D`. INT+Reset during power-on
  selects the pair (diagrams on p.10; extracted markdown has no T2/T3
  numbers — board timings stay the on-glass 20/20/80 ms).
- **Stay at or below 400 kbps** (§6.1). embassy-debug uses the cap;
  simple-debug uses 100 kHz (inside it).
- **Up to 5 concurrent touches** (§1). This FPC delivers **5**
  ([touch.md](../references/touch.md#on-glass-embassy-debug)).
- **Init, including idle-capacitance self-cal, is under 200 ms** (features,
  §8.6).
- **`/RSTB` is active-low** and wants a 10 kΩ pull-up (pin table).
- **I2C Sleep** is INT low, then the screen-off command; wake by driving INT
  high 2–5 ms, ≥58 ms after screen-off (§8.1 / §8.3). Cutting `TOUCH_EN` is
  a different, board-level power-off.
- **INT edge polarity is a config bit** (§8.2): 0 = rising (idle low), 1 =
  falling (idle high).
- **`0x8040` is still named as a command port** for Gesture mode (command
  `8` to `0x8046` then `0x8040`, §8.1). The command-`0` “read coordinates”
  encoding is not in this PDF.
- **Stationary Configuration** (§8.4) is a host-to-chip parameter lock, not
  a license to invent a 186-byte config table. Tx channel order must match
  the sensor (§5.2); that is module programming.

## Verified against the ESP32-S3 datasheet

Facts below were read out of **Version 2.2**. GPIO hold and pad-JTAG eFuse
details stay in the TRM (not in this cache yet).

- **Strapping pins** are GPIO0, GPIO3, GPIO45, GPIO46 (§3). Defaults: GPIO0
  WPU, GPIO3 floating, GPIO45/46 WPD. Latched at chip reset; ordinary IO
  after hold time `tH` ≥ 3 ms. This board uses GPIO0 as sensor SCL, GPIO3 as
  GT911 SDA, GPIO45/46 as the power latch.
- **GPIO21 (GT911 INT)** has empty At Reset / After Reset pull columns
  (Table 2-1): no internal WPU/WPD. An MCU pull-up is firmware policy.
- **GPIO41 / GPIO42** default IO MUX F0 is JTAG `MTDI` / `MTMS` (Table 2-4).
  §2.3.4 lists them with GPIO39/40 as the pad JTAG interface (Priority 3).
  Mux to GPIO before using them as GT911 RST / `TOUCH_EN`.
- **Two I2C controllers** (§4.2.1.2): Standard 100 kbit/s, Fast 400 kbit/s,
  up to 800 kbit/s limited by pull-up strength. Touch ≤400 kHz and
  sensor 400 kHz are both in spec.
- **GPIO19/20** default to USB Serial/JTAG (already in the board contract).

## Gaps, deliberately not filled by guessing

A register we have not read stays unnamed rather than becoming a
plausible-looking constant:

| Gap | What to do |
| --- | --- |
| BQ27220 CEDV data-memory *block* addresses beyond SLUUBD4 `3 Data Memory Interface` headings | Standard Commands live in `crates/bq27220` `Command` (SLUUBD4 `Table 2-1`). Use a raw read/write for garbled extract rows; do not infer block maps. |
| BQ27220 CEDV data-memory block addresses and subcommand codes | Stock firmware reads the CEDV core and thresholds; the block layout must come from SLUUBD4, not from inference. |
| Four-gray LUT contents for this glass | No default LUT. The Sticky confirmed path is OTP (no 0x32). An MCU table stays optional and attributed; record its source and license here before adding one. |
| GT911 coordinate / command / status bit encodings | Rev.09 deleted the register map (Rev.07). Remaining encodings are on-glass `GT911_REG_*` names, not a Rev.09 table. Do not invent a 186-byte config. |
| ESP32-S3 GPIO hold, pad-JTAG eFuse, `ext1` register details | Datasheet v2.2 names the pads. The TRM is catalogued (`esp32-s3-trm`); search the local markdown cache when citing it. |
| GT911 contacts this FPC actually delivers | **5** on glass (Rev.09 §1 silicon max). |
| Glass part number on this FPC | Schematic Rev 01 names analog rails, not a glass PN. Candidate GDEY0397T81P sheet on file. Close only with official evidence. [nyc-panel-glass](not-yet-confirmed.md). |

## Verified against the schematic

Facts below were read out of **Rev 01 (2026-06-05)**, CC BY-SA 4.0. Walk
the PDF pages for a net; the extract drops some wires.

- **SHT40** is **SHT40-AD1B-R2** at `0x44`, four pins, no ALERT.
- **GPIO9** is `PWR_IN_VOLT`: 5.1 kΩ / 5.1 kΩ from `VIN_5V` (~½ VBUS).
- **GPIO7** is shared: LSM6DS3TR-C INT1 (`6D_INTn`) and BQ27220 GPOUT
  (`BFG_INT`). Input-only.
- **GPIO43 / GPIO44** are CH343P UART0 TX / RX (`USB_RXD` / `USB_TXD`).
- **GPIO40** is BQ25616 STAT (`CHARGE_STATE`).
- **GPIO11** is the MicroSD switch with a 10 kΩ pull-up (insert = 0).
  Card rail is 3.3 V (`TPS22916CYFPR`).
- **PCF8563M/TR** INT is `RTC_INTn` and is **NC** to the ESP32. CLKOUT
  is a test point.
- **Buzzer** is FUET-5018 on GPIO48 through CJ2324.
- **USB-C** CC1/CC2 are 5.1 kΩ Rd to GND: 5 V sink, no PD controller.
- **Mic** is MSM261DDB020; enable is TPS22916CYFPR on GPIO38.
- **Antenna** is on-board ANT1 (2.4 GHz match). No extra antenna.
- Charge set: Vset 4.2 V, ~555 mA charge, ~937 mA input limit.
- Panel FPC names VGH / VGL / VSH1 / VSH2 / VCOM / VPP. No glass PN.
- No BOM in this PDF.

## Waveform provenance

| LUT | Source | License | Status |
| --- | --- | --- | --- |
| *(none shipped)* | Sticky uses OTP; MCU 0x32 is optional | — | Do not add a row for bytes extracted from vendor firmware |

Do not add a row for bytes extracted from vendor firmware.
