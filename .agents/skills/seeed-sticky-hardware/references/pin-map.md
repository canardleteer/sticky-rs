# Pin and bus map

GPIO numbers are ESP32-S3 package pins. Levels are those observed on working
hardware unless marked otherwise.

## GPIO table

| GPIO | Direction | Signal | Notes |
| ---: | --- | --- | --- |
| 0 | OD clock | Sensor I2C SCL | 400 kHz. **Strapping pin** (WPU at reset, boot mode with GPIO46). |
| 1 | OD data | Sensor I2C SDA | SHT40, PCF8563, BQ27220, LSM6DS3TR-C |
| 2 | OD clock | GT911 SCL | Dedicated touch bus, 100 kHz on glass. After reset: IE, no internal pull (v2.2 Table 2-1). |
| 3 | OD data | GT911 SDA | Dedicated touch bus. **Strapping pin** (JTAG source, v2.2 §3.4): floating at reset, then ordinary IO. |
| 4 | Input | AI / OK / power | Active low, external 10 kΩ. RTC `ext1` wake. Seeed: **AI Voice Button**, right-edge top (glass facing you, USB-C down) |
| 5 | Input | Up / left | Active low, external 10 kΩ. Seeed: **Page Up Button**, right-edge middle |
| 6 | Input | Down / right | Active low, external 10 kΩ. Seeed: **Page Down Button**, right-edge bottom |
| 7 | Input | **Ambiguous** | Seeed: LSM6DS3TR-C INT. `sticky-2048`: BQ27220 `PIN_BFG_INT`. **Do not drive as output.** |
| 8 | Output | MicroSD CS | Shared SPI; idle high |
| 9 | Input | External power | Digital, edge-driven: high = VBUS present. Divider / ADC use unconfirmed |
| 10 | Output | MicroSD power enable | Active high. Sleep: hold low |
| 11 | Input | MicroSD card detect | Use a pull-up. Interrupt / wake source. Polarity unconfirmed |
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
| 40 | Input | BQ25616 `CHARGE_STATE` | Wired in compiled profiles; polarity unconfirmed. Default F0 is JTAG `MTDO`. |
| 41 | Output | GT911 RST | Address-select sequence. Default F0 is JTAG `MTDI` (input); mux to GPIO. After reset: IE, no pull. |
| 42 | Output | GT911 power enable | Active high. ~250 ms settle. Default F0 is JTAG `MTMS` (input); mux to GPIO. |
| 43 | UART0 TX | CH343P | Typical ESP32-S3 U0TXD; not independently scoped |
| 44 | UART0 RX | CH343P | Typical ESP32-S3 U0RXD |
| 45 | Output | `PWR_HOLD` | **Must be high** to stay powered. **Strapping pin** (VDD_SPI voltage). Default **WPD**. |
| 46 | Output | `PWR_LOCK` | **Must be high.** **Strapping pin** (boot mode with GPIO0). Default **WPD**. |
| 47 | Output | EPD power enable | Active high. ~100 ms settle |
| 48 | PWM | Passive buzzer | Drive with LEDC/PWM; hold low in sleep |

UART0 TX/RX are ESP32-S3 defaults to the CH343P. Confirm before reassigning
43/44.

### GPIO7

Do not treat GPIO7 as free GPIO. Official Hardware Overview assigns it to the
IMU interrupt. Playground `sticky-2048` names it `PIN_BFG_INT` (gauge GPOUT).
On-glass IMU bring-up has polled I2C and left this pin unused. Leave it an
input until [nyc-gpio7](../resources/not-yet-confirmed.md#nyc-gpio7) is closed.

UART learning firmware (input + pull-up, IMU polled over I2C) read GPIO7
**low** with no edges while sitting still, during a tilt that changed the
IMU pose token, and across a USB-C unplug/replug. That is not an owner. Do
not drive the pin.

Recessed **Reset** on the bottom edge is a hardware reset net, not a GPIO.
Same edge: microphone hole, lanyard hole, charge LED, USB-C. The three keys
are on the **right** long edge. Layout:
[enclosure.md](enclosure.md). Dual-color charge LED left of USB-C is
charger-driven, not MCU GPIO in these sources.

## I2C buses

| Bus | SDA | SCL | Speed | Devices |
| --- | ---: | ---: | --- | --- |
| Touch | 3 | 2 | 100 kHz | GT911 `SlaveAddress::Pair28_29` (working units) or `PairBaBb`. ESP32-S3 I2C Standard mode (v2.2 §4.2.1.2). |
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

Panel: **10 MHz**, SPI mode 0. SSD1677 spec max is 20 MHz; 40 MHz on this
shared bus is out of spec. Whether **20 MHz** is safe with the card on the
same controller is unconfirmed.

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
| Humidity/temp | Sensirion SHT40 | Sensor I2C `0x44`. Suffix: [nyc-sht40-package](../resources/not-yet-confirmed.md#nyc-sht40-package) |
| RTC | NXP PCF8563 | Sensor I2C `0x51` |
| Fuel gauge | TI BQ27220 | Sensor I2C `0x55` |
| IMU | ST LSM6DS3TR-C | Sensor I2C `0x6A`. INT net: GPIO7, owner unconfirmed |
| Charger | TI BQ25616 | GPIO only, **not** I2C |
| USB-UART | WCH CH343P | UART0, USB `1a86:55d3` |
| Microphone | MEMSensing MSM261DDB020 (single-source ID) | PDM GPIO19/20, EN 38. No loudspeaker. Pins 19/20 are also USB-Serial-JTAG; see [sensors.md](sensors.md#pdm-microphone). |

No frontlight. No 4-bit SDMMC; MicroSD is SPI only.
