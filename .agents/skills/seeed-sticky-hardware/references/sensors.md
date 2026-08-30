# Sensors, RTC, gauge, microphone

All except the microphone sit on **sensor I2C** (SDA GPIO1, SCL GPIO0) at
400 kHz. Probe after the power latch. GPIO0 must stay a clean open-drain clock
at reset.

## LSM6DS3TR-C IMU (`0x6A`)

WHO_AM_I (`0x0F`) = **`0x6A`**. Factory and working firmware both talk to it.

A working accelerometer-only setup:

| Register | Value | Meaning |
| ---: | ---: | --- |
| `0x12` CTRL3_C | `0x04` | Auto-increment |
| `0x10` CTRL1_XL | `0x40` | 104 Hz, ±2 g |
| `0x28` OUTX_L_XL | 6 bytes LE int16 | X, Y, Z |

Scale at ±2 g: `0.000061 g/LSB`.

**Enclosure axes (calibrated on a unit):**

| Gravity-dominant axis | Orientation |
| --- | --- |
| +X | Portrait 0 |
| −X | Portrait 180 |
| −Y | Landscape 0 |
| +Y | Landscape 180 |
| +Z | Face up |
| −Z | Face down |

A ~0.70 g threshold on the dominant axis classified placement. UART learning
firmware classified **FaceUp** while sitting still, then **Landscape0** after
the operator lifted or rotated and held (~0.70 g map). Gyro and FIFO wiring
are undocumented. Stock firmware drives explicit low-power enter/exit
transitions on this part around sleep, so expect a mode change rather than a
single fixed ODR. **INT is GPIO7 in Seeed’s overview and is also named
as the gauge interrupt in `sticky-2048`.** Leave GPIO7 an input. Confirmation:
[nyc-gpio7](../resources/not-yet-confirmed.md#nyc-gpio7).

Face-up / face-down are not aliases for portrait/landscape.

## PCF8563 RTC (`0x51`)

Time starts at register `0x02`. Seconds bit 7 (`0x80`) is the low-voltage /
integrity flag. Do not assume the RTC can wake the ESP32 until INT is
confirmed: [nyc-pcf8563-wake](../resources/not-yet-confirmed.md#nyc-pcf8563-wake).

## BQ27220 fuel gauge (`0x55`)

Little-endian standard commands:

| Register | Use |
| ---: | --- |
| `0x00` Control | DeviceType command `0x0001` |
| `0x40` MAC Data | DeviceType response; expect **`0x0220`** |
| `0x2C` StateOfCharge | Unsigned percent |
| `0x08` Voltage | Unsigned mV |
| `0x0C` Current | Signed mA |

After Control=DeviceType, wait ~15 ms before MAC Data.

UART learning firmware verified DeviceType `0x0220` and logged standard-command
voltage, current, and SOC while sitting on USB with `/CE` disabled (example
readback `soc=100 v=4195 i=0`). That is not a CEDV data-memory dump and does
not close [nyc-gauge-profile](../resources/not-yet-confirmed.md#nyc-gauge-profile).

### CEDV, not Impedance Track

This gauge runs **CEDV**. Stock firmware reads and logs the full parameter
set, which is the practical read surface for a driver:

| Group | Parameters |
| --- | --- |
| CEDV core | `FCC`, `DC`, `RC`, `NF`, `SDR`, `EMF`, `C0`, `R0`, `T0`, `R1`, `TC`, `C1` |
| Thresholds | `EDV0`, `EDV1`, `EDV2`, `DOD20` |
| Status | OperationStatus (shows seal state), firmware and hardware version, checksum |

Consequence for crate choice: `bq27xxx` targets BQ27426/427 **Impedance
Track** and is the wrong family. Do not use it.

### Reads are safe; the configuration path is not

Reads of standard commands and the CEDV set are harmless. On silicon, after
latch, `bq27220` DeviceType plus voltage / current / SoC reads succeeded
(USB-powered unit; do not treat a single SoC as a profile proof). The
dangerous path is data-memory configuration, and it is worth knowing that
stock firmware
really does walk it:

1. Unseal, then enter `CFGUPDATE` (with a timeout — it can hang).
2. Write `FullChargeCapacity`, read it back, and verify; retry on mismatch.
3. Exit `CFGUPDATE` (also timeout-guarded), then re-seal.
4. Persist the resulting FCC outside the gauge and restore it at next boot.

It also gates an `OCV` command on the pack current having settled, and toggles
the gauge's low-power **snooze** mode around sleep transitions.

So: keep writes behind an explicit, off-by-default switch, quote the TI TRM in
whatever wrapper you build, and never write OTP — that part is one-time. A
plausible-looking voltage and SOC is not proof the profile is right.
Remaining unknowns:
[nyc-gauge-profile](../resources/not-yet-confirmed.md#nyc-gauge-profile).

## SHT40 (`0x44`)

Factory inits an environment sensor at this address. Package suffix and alert
pin: [nyc-sht40-package](../resources/not-yet-confirmed.md#nyc-sht40-package).

A bring-up 1-byte I2C **read** at `0x44` NAKs on silicon. That is not a
Sensirion measurement transaction and does not prove the part is missing.
UART learning firmware issued high-precision measure `0xFD` (`sht4x`
`Precision::High`) and the address **ACKed**. Stock firmware’s
`environment_sensor` init still reports `result=ok` after an `app0` restore.
PCF8563 `0x51`, BQ27220 `0x55`, and LSM6DS3TR-C `0x6A` ACKed a 1-byte probe
on the same bus after latch.

## PDM microphone

Identified (single source) as **MEMSensing MSM261DDB020**. Factory inits it.
The enclosure hole is on the **bottom edge** (Reset, lanyard, charge LED,
and USB-C on the same edge; [enclosure.md](enclosure.md)).
There is **no loudspeaker**; the only sound out is the passive buzzer on
GPIO48.

| Signal | GPIO |
| --- | ---: |
| PDM clock | 19 |
| PDM data | 20 |
| Enable (active high) | 38 |

Hold GPIO38 low when unused and across sleep. ESP32-S3 has hardware
PDM-to-PCM RX. Do **not** copy reTerminal **E-series** wiki pins (those pages
use GPIO42/41 for PDM clock/data).

### GPIO19/20 are also the USB-Serial-JTAG pads

On ESP32-S3, GPIO19 is USB D− and GPIO20 is USB D+. The Sticky’s USB-C debug
path is the **CH343P on UART0**, not native USB-Serial/JTAG, so those pads
are free for the mic — except that after **deep-sleep wake** the USB pad
function reclaims them. `gpio_reset_pin` alone does not free a dedicated PAD
connection; the USB-Serial-JTAG PHY must be disabled, then the pins reset,
then I2S PDM RX attached.

Firmware that has run this on production Stickys
([sira-fiinikkusu/reterminal-sticky-voice-companion](https://github.com/sira-fiinikkusu/reterminal-sticky-voice-companion)):

1. Early boot (before the I2S driver): clear `USB_SERIAL_JTAG.conf0.usb_pad_enable`,
   `gpio_reset_pin` GPIO19 and GPIO20.
2. Force GPIO38 **low** ~150 ms (that firmware’s comment: GPIO38 can float in
   deep sleep and leave the load switch / capsule half-powered). Then enable
   the rail (GPIO38 high) before recording.
3. I2S PDM RX: `i2s_lrclk_pin` GPIO19 (PDM clock), `i2s_din_pin` GPIO20,
   `pdm: true`, **left** channel, **16 kHz**, 16-bit.

That is a **working recipe**, not a close of
[nyc-mic-pdm](../resources/not-yet-confirmed.md#nyc-mic-pdm). Sample rate,
slot, and which way the hole faces vs the waveform are still unmeasured.
The GPIO38 switch is named **TPS22916** only in that firmware; treat the
part number like the capsule ID until a schematic confirms it.

Push-to-talk on GPIO4 is application policy. The board has no speaker, so
voice replies have nowhere to play unless firmware writes them to the panel
or beeps the buzzer.
