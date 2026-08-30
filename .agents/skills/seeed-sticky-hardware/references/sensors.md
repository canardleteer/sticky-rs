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

**Enclosure axes** (USB-C positions match the appearance diagram:
glass facing you, USB-C on the bottom short edge is portrait):

| Gravity-dominant axis | USB-C | Orientation |
| --- | --- | --- |
| −Y | Bottom short edge | Portrait 0 |
| +Y | Top short edge | Portrait 180 |
| −X | Right short edge | Landscape 0 |
| +X | Left short edge | Landscape 180 |
| +Z | — | Face up |
| −Z | — | Face down |

A ~0.70 g threshold on the dominant axis classified placement. UART learning
firmware classified **FaceUp** while sitting still, then an in-plane pose
after the operator lifted or rotated and held (~0.70 g map). An earlier
token named USB-down **Landscape0**; the table above is the enclosure
map (USB-down = Portrait 0). Gyro and FIFO wiring
are undocumented. Stock firmware drives explicit low-power enter/exit
transitions on this part around sleep, so expect a mode change rather than a
single fixed ODR. **INT1 is GPIO7**, shared with BQ27220 GPOUT
(schematic `6D_INTn` / `BFG_INT`). Leave GPIO7 an input. Do not enable
both chips as push-pull.

Face-up / face-down are not aliases for portrait/landscape.

## PCF8563 RTC (`0x51`)

Schematic part is **PCF8563M/TR**. Time starts at register `0x02`. Seconds
bit 7 (`0x80`) is the low-voltage / integrity flag. INT is net `RTC_INTn`
and is **NC** to the ESP32. CLKOUT is a test point. The RTC cannot wake
the MCU on a pin.

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

Schematic part is **SHT40-AD1B-R2** (four-pin DFN: VDD, VSS, SDA, SCL).
There is **no ALERT** net. Factory inits an environment sensor at this
address.

A bring-up 1-byte I2C **read** at `0x44` NAKs on silicon. That is not a
Sensirion measurement transaction and does not prove the part is missing.
UART learning firmware issued high-precision measure `0xFD` (`sht4x`
`Precision::High`) and the address **ACKed**. Stock firmware’s
`environment_sensor` init still reports `result=ok` after an `app0` restore.
PCF8563 `0x51`, BQ27220 `0x55`, and LSM6DS3TR-C `0x6A` ACKed a 1-byte probe
on the same bus after latch.

## PDM microphone

Schematic part is **MEMSensing MSM261DDB020**. Factory inits it. Enable
is **TPS22916CYFPR** on GPIO38.
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

That is a **working recipe**. Embassy-debug `--features mic` ran the same
clock/data/enable and **16 kHz / 16-bit / left** on glass. It is not a
high-fidelity close of
[nyc-mic-pdm](../resources/not-yet-confirmed.md#nyc-mic-pdm).

### On glass (embassy-debug mic feature)

Energy (`rms` / `peak`) is live: a whistle jumped and clipped at 32768
(16-bit full scale). One relative-quiet room (fans on) and covering as
many holes as we could sat in the same band (`rms` ~900–1200). That
is one session, not a room spec. The floor is mostly a **DC bias** in
the PCM (samples sit near +975); `pcm_energy` is not AC-coupled.

AI Voice plays a 1 kHz buzzer (GPIO48) and dumps two 256-sample
windows. The tail of those windows repeats about every **16 samples**
(~1 kHz if the PDM clock is 16 kHz). The left slot hears that tone.
The wiggle is ugly (board / EMI coupling into 19/20, not a clean sine
through the USB-C-edge hole). Each dump starts `0`, a spike, then the
DC floor, then the wiggle. Printing the rows at 115200 inserts ~140 ms
between windows.

Still open: a clean known-tone through the hole, slot A/B, and hole
vs waveform polarity.

Enabling GPIO38 and attaching I2S PDM RX is **not** a
[safety.md](safety.md) destroy-the-board row. USB-C
debug is the CH343 on UART0, so 19/20 are free for PDM while that cable
is plugged in. Embassy-debug `--features mic` uses the same `flash-app`
path as the default image. Always-zero or always-max UART energy is a
mux, slot, or rail miss. Do not use native USB-Serial/JTAG on USB-C
while those pins are PDM. After deep sleep, disable the USB pad before
PDM (above). Hold GPIO38 low when unused.
The GPIO38 switch is **TPS22916CYFPR** on schematic Rev 01.

Push-to-talk on GPIO4 is application policy. The board has no speaker, so
voice replies have nowhere to play unless firmware writes them to the panel
or beeps the buzzer.
