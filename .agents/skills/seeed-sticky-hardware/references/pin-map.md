# Pin and bus map

GPIO numbers are ESP32-S3 package pins. Levels are those observed on working
hardware unless marked otherwise.

## GPIO table

| GPIO | Direction | Signal | Notes |
| ---: | --- | --- | --- |
| 0 | OD clock | Sensor I2C SCL | 400 kHz. **Strapping pin** (WPU at reset, boot mode with GPIO46). |
| 1 | OD data | Sensor I2C SDA | SHT40, PCF8563, BQ27220, LSM6DS3TR-C |
| 2 | OD clock | GT911 SCL | Dedicated touch bus, ≤400 kHz (Rev.09 §6.1). After reset: IE, no internal pull (v2.2 Table 2-1). Schematic Rev 01. |
| 3 | OD data | GT911 SDA | Dedicated touch bus. **Strapping pin** (JTAG source, v2.2 §3.4): floating at reset, then ordinary IO. Schematic Rev 01. |
| 4 | Input | AI / OK / power | Active low, external 10 kΩ. RTC `ext1` wake. Seeed: **AI Voice Button**, right-edge top (glass facing you, USB-C down) |
| 5 | Input | Up / left | Active low, external 10 kΩ. Seeed: **Page Up Button**, right-edge middle |
| 6 | Input | Down / right | Active low, external 10 kΩ. Seeed: **Page Down Button**, right-edge bottom. In-repo embassy-debug: `ext1` ANY_LOW wake ([power-and-sleep.md](power-and-sleep.md)) |
| 7 | Input | IMU INT1 + gauge GPOUT | Schematic: LSM6DS3TR-C INT1 (`6D_INTn`) and BQ27220 GPOUT (`BFG_INT`) share this pin. **Do not drive as output.** |
| 8 | Output | MicroSD CS | Shared SPI; idle high |
| 9 | Input | External power (`PWR_IN_VOLT`) | Digital, edge-driven: high = VBUS present. Schematic: 5.1 kΩ / 5.1 kΩ from `VIN_5V` (~½ VBUS). 2.5 V at 5 V still reads as GPIO high |
| 10 | Output | MicroSD power enable | Active high. Sleep: hold low |
| 11 | Input | MicroSD card detect | 10 kΩ pull-up; insert = **0**. Interrupt / wake source. Card rail 3.3 V |
| 12 | Input | Shared SPI MISO | Needed by SD; panel is MOSI-only in practice |
| 13 | Output | Shared SPI SCLK | EPD + SD |
| 14 | Output | Shared SPI MOSI | EPD + SD |
| 15 | Output | EPD CS | Active low |
| 16 | Output | EPD D/C | |
| 17 | Output | EPD RST | 10 ms low, 10 ms high works |
| 18 | Input | EPD BUSY | Busy when **high**. Prefer an edge interrupt over polling |
| 19 | Output | PDM mic clock | Also ESP32-S3 USB D− / USB-Serial-JTAG pad. USB-C debug is the CH343 on UART0, so this pad is free for PDM while that cable is plugged in. After deep sleep the USB function reclaims it; disable the pad before PDM RX. Factory ACK. |
| 20 | Input | PDM mic data | Also ESP32-S3 USB D+ / USB-Serial-JTAG pad. Same CH343 / reclaim notes as GPIO19. Factory ACK. |
| 21 | I/O | GT911 INT | Address-select during reset, then input. **No default pull** at/after reset (v2.2 Table 2-1). RTC-capable. |
| 38 | Output | Mic power enable | Active-high load switch. Hold low when unused and in sleep (floating across sleep can leave the capsule half-powered). Cycle low after wake before recording. Enabling the rail is not a [safety.md](safety.md) destroy-the-board row. |
| 39 | Output | BQ25616 charge enable | **Active low**. Default IO MUX F0 is JTAG `MTCK` (v2.2 Table 2-4); mux to GPIO before driving. |
| 40 | Input | BQ25616 `CHARGE_STATE` | STAT pin. Low while charging (`/CE` enabled); high-Z/high when done or `/CE` parked. On a physical unit: `1→0→1` across a 2 s `/CE` pulse after a settle on disable. Default F0 is JTAG `MTDO`. |
| 41 | Output | GT911 RST | Address-select sequence. Default F0 is JTAG `MTDI` (input); mux to GPIO. After reset: IE, no pull. |
| 42 | Output | GT911 power enable | Active high. ~250 ms settle. Default F0 is JTAG `MTMS` (input); mux to GPIO. |
| 43 | UART0 TX | CH343P `USB_RXD` | MCU TX to the bridge (ESP32-S3 U0TXD). Schematic Rev 01 |
| 44 | UART0 RX | CH343P `USB_TXD` | MCU RX from the bridge (ESP32-S3 U0RXD). Schematic Rev 01 |
| 45 | Output | `PWR_HOLD` | **Must be high** to stay powered. **Strapping pin** (VDD_SPI voltage). Default **WPD**. |
| 46 | Output | `PWR_LOCK` | **Must be high.** **Strapping pin** (boot mode with GPIO0). Default **WPD**. |
| 47 | Output | EPD power enable | Active high. ~100 ms settle |
| 48 | PWM | Passive buzzer | Drive with LEDC/PWM; hold low in sleep |

UART0 TX/RX are ESP32-S3 defaults to the CH343P (schematic Rev 01). Do not
reassign 43/44.

### GPIO7

Do not treat GPIO7 as free GPIO. Schematic Rev 01 ties **both** LSM6DS3TR-C
INT1 (`6D_INTn`) and BQ27220 GPOUT (`BFG_INT`) to this pin. Seeed’s overview
and `sticky-2048` `PIN_BFG_INT` were naming the same net. Leave it an input.
Do not enable both chips as push-pull.

UART learning firmware (input + pull-up, IMU polled over I2C) read GPIO7
**low** with no edges while sitting still, during a tilt that changed the
IMU pose token, and across a USB-C unplug/replug. Interrupts were not
armed on either chip. Do not drive the pin. Armed-edge recipe:
[nyc-gpio7-edge](../resources/not-yet-confirmed.md#nyc-gpio7-edge).

Recessed **Reset** on the bottom edge is a hardware reset net, not a GPIO.
Same edge: microphone hole, lanyard hole, charge LED, USB-C. The three keys
are on the **right** long edge. Layout:
[enclosure.md](enclosure.md). Dual-color charge LED left of USB-C is
charger-driven, not MCU GPIO in these sources.

## I2C buses

| Bus | SDA | SCL | Speed | Devices |
| --- | ---: | ---: | --- | --- |
| Touch | 3 | 2 | ≤400 kHz | GT911 `SlaveAddress::PairBaBb` (INT-low, contacts) or `Pair28_29` (INT-high ACK). Rev.09 §6.1. ESP32-S3 I2C Fast mode cap (v2.2 §4.2.1.2). |
| Sensors | 1 | 0 | 400 kHz | SHT40 `0x44`, PCF8563 `0x51`, BQ27220 `0x55`, LSM6DS3TR-C `0x6A` |

External pull-ups exist on the I2C **SDA/SCL** nets; MCU internal pull-ups
have also been enabled on those clocks/data lines without harm. Do **not**
enable an MCU pull on GPIO21 (GT911 INT) after address select. Do not put
the gauge on the touch bus. Do not hang anything that drives GPIO0 or GPIO3
during strap sampling.

On ESP32-S3 these map cleanly to two I2C masters (I2C0 touch, I2C1 sensors).

## SPI bus

One controller (typically SPI2) for panel and card:

| Signal | GPIO | Device |
| --- | ---: | --- |
| SCLK | 13 | EPD + SD |
| MOSI | 14 | EPD + SD |
| MISO | 12 | SD |
| CS | 15 | SSD1677 |
| CS | 8 | MicroSD |

Panel: default **10 MHz**, SPI mode 0. SSD1677 spec max is 20 MHz;
embassy-debug `--features spi20` painted a clean splash at 20 MHz.
Read-only card identify (`sd` module) ACKs `send_status` at 10 MHz
and 20 MHz after 400 kHz init. 40 MHz on this shared bus is out of
spec.

**GPIO0 must not appear on this SPI bus.** It is sensor SCL. Pass only the
pins above. In ESP-IDF, unused `quadwp` / `quadhd` / `data4`–`data7` must be
**`-1`** (zero-init assigns GPIO0 and kills sensor I2C after display init).

## Named parts

| Function | Part | Interface |
| --- | --- | --- |
| MCU | Espressif ESP32-S3R8 | — |
| Flash | Winbond W25Q256-class, JEDEC `ef4019` | Quad SPI, 32 MB |
| PSRAM | In-package 8 MB octal, 3.3 V | ESP32-S3R8 |
| Panel controller | SSD1677-compatible | Shared SPI |
| Touch | Goodix GT911 | Touch I2C |
| Humidity/temp | Sensirion **SHT40-AD1B-R2** | Sensor I2C `0x44`. Four-pin DFN; no ALERT |
| RTC | NXP **PCF8563M/TR** | Sensor I2C `0x51`. INT (`RTC_INTn`) is NC to the ESP32 |
| Fuel gauge | TI BQ27220 | Sensor I2C `0x55`. GPOUT shares GPIO7 with the IMU |
| IMU | ST LSM6DS3TR-C | Sensor I2C `0x6A`. INT1 shares GPIO7 with the gauge |
| Charger | TI BQ25616 | GPIO only, **not** I2C |
| USB-UART | WCH CH343P | UART0 GPIO43/44, USB `1a86:55d3` |
| Microphone | MEMSensing **MSM261DDB020** | PDM GPIO19/20; EN GPIO38 is **TPS22916CYFPR**. No loudspeaker. Pins 19/20 are also USB-Serial-JTAG; see [sensors.md](sensors.md#pdm-microphone). |
| Buzzer | **FUET-5018** (passive) | GPIO48 PWM through CJ2324 |
| Antenna | On-board ANT1 | 2.4 GHz match. Shared Wi-Fi / BLE. No external antenna. On a physical unit: [below](#on-a-physical-unit-embassy-debug-radio-feature) |

No frontlight. No 4-bit SDMMC; MicroSD is SPI only.

### On a physical unit (embassy-debug radio feature)

embassy-debug `--features radio` scanned Wi-Fi and BLE together on
ANT1. One listen printed `wifi n=` and `ble n=` while `imu=` kept
running. Wi-Fi hit the eight-SSID line cap; BLE windows reported
well over a hundred advertisements and a few unique local names
(plus `name=?` when the ad had no name). Printed SSID RSSI ran from
the high −40s to the mid −90s. Scan only: no STA join, no BLE
connect, no MAC or BSSID on UART. Not an NYC pin.
Default embassy-debug includes pair (advertise `sticky-rs` only on
`scene=pair`, DisplayOnly passkey, RAM bonds). On a physical unit
(earlier `--features pair` image): UART printed `scene=pair`, then
`pair pin=` (six digits), then `pair fail=pairing`. Advertisement
and passkey display worked. On a later default-image sit a host
BlueZ **Connect** (not `Pair()`) typed a new UART `pair pin=` and
completed SMP: `pair ok`, host `Paired` / `Connected`, pair card
showed `Paired`. A concurrent BlueZ `Pair()` raced the image’s
SMP Security Request (`0x0B`) and canceled.
Default embassy-debug also paints idle-until-touch Wi-Fi survey
and WPA2 SoftAP cards (`sticky-rs-AP` / `sticky26` at
`192.168.4.1`). On a physical unit (2026-09-04): Page Down
printed `scene=wifi_survey` then `scene=wifi_ap`; START taps
printed `touch n=1` (`p0=679,189` on survey while
`imu=Portrait0`). An image that hit-tested UART `to_screen`
against gray4 `page_to_framebuffer` produced **no**
`wifi_survey` / `wifi_ap` line (tap mapped to the top of the
page). Landscape0 START uses `gray4_touch_framebuffer`
(OTP 180° only). Same-day sit: `wifi tap page=362,53
hit=1` / `p0=362,426` then `wifi_ap state=active` (FaceUp,
last landscape page); the OR image also toggled the empty
opposite side. After the portrait framebuffer
hit-test fix, a host spare STA
joined `sticky-rs-AP` / `sticky26` (2026-09-04): DHCP
`192.168.4.50`, `GET /` JSON `device` / `scene=wifi_ap` /
`wifi` counts (`clients=1`, `requests=1`). After STOP +
replug + START: UART `wifi_ap … clients=1` then
`wifi_http req=1 path=/`. Host disconnect produced no later
`wifi_ap` decrement on that image. Same evening, after a
host leave with no UART, operator glass showed `clients=0`
and `http=2`. Hit-test space:
[touch.md](touch.md#coordinate-transform-on-a-physical-unit).
