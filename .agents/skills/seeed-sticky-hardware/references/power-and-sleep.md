# Power, charger, and sleep

## Power latch (product-critical)

The board stays on only while both latch outputs are high:

| Signal | GPIO | Level to stay powered |
| --- | ---: | --- |
| `PWR_HOLD` | 45 | 1 |
| `PWR_LOCK` | 46 | 1 |

On every boot, including deep-sleep wake:

1. If waking from deep sleep, drive GPIO45 and GPIO46 high **before** releasing
   ESP32-S3 GPIO/RTC holds (those pins were held high across sleep).
2. Drive both high as ordinary outputs.
3. Wait ~**100 ms** before talking to powered peripherals.

GPIO46 is a strapping pin (boot mode with GPIO0). GPIO45 is a strapping pin
(VDD_SPI voltage). Both default to **weak pull-down** at reset (v2.2 Table
2-1 / 3-1), so the latch must be driven high against silicon default. Do
not use GPIO46 as a general-purpose toggle. Some write-ups pulse GPIO46
instead of holding it high — do not switch recipes until
[nyc-gpio46-pulse](../resources/not-yet-confirmed.md#nyc-gpio46-pulse). The
maximum delay from reset to assertion:
[nyc-latch-deadline](../resources/not-yet-confirmed.md#nyc-latch-deadline).

Driving both pins low is software power-off. Unplugging USB without a latch
also powers the unit off. When testing on battery, confirm the latch is high
before display init.

**Release is a policy decision, not a failure.** Stock firmware latches inside
its HAL init — before buses, before the app — and then *releases* the latch
when the power button was not the boot cause, so the board powers itself down
instead of running headless. Two things follow:

- Treat "latch acquired" and "latch retained" as separate decisions in your
  own firmware. Make release an explicit, named operation, not a fallthrough.
- If you copy the stock policy, a USB-powered boot with no button press will
  shut down mid-bring-up and look like a crash. Choose deliberately.

## Charger (BQ25616)

Not an I2C device.

| Signal | GPIO | Behavior |
| --- | --- | --- |
| Charge enable (`EN_BAT_CHGn`) | 39 | **Active low**: 0 = charging enabled |
| External power | 9 | Digital, **edge-capable**: high = USB/external present. Schematic net `PWR_IN_VOLT`: 5.1 kΩ / 5.1 kΩ from `VIN_5V` (~½ VBUS). 2.5 V at 5 V still reads high |
| `CHARGE_STATE` | 40 | BQ25616 STAT. **Low** while charging when `/CE` is enabled; high-Z/**high** when charge is done or `/CE` is parked. UART learning firmware read **high** with USB present, `/CE` disabled, and gauge `i=0`. Do not treat that as “charging.” STAT with `/CE` enabled: [nyc-charge-stat](../resources/not-yet-confirmed.md#nyc-charge-stat). |

Treat GPIO9 as a digital **edge source**, not a level you poll: stock firmware
installs an any-edge GPIO interrupt on it and raises a power-state-changed
event from the handler. FreeInk’s analog `PWR_IN_VOLT` name matches the
divider. Firmware still uses the pin digitally.

USB-C feeds the charger (5 V sink; CC1/CC2 are 5.1 kΩ Rd). Red/green LED
left of the port is charger-driven. Schematic charge set: **Vset 4.2 V**,
charge **~555 mA**, input limit **~937 mA**.

## Battery

**750 mAh** single-cell lithium. State of charge is **BQ27220** on sensor I2C
(`0x55`), not GPIO9. The gauge runs **CEDV** (not Impedance Track), and stock
firmware actively maintains its Full Charge Capacity — see
[sensors.md](sensors.md) before writing anything to it. Remaining unknowns:
[nyc-gauge-profile](../resources/not-yet-confirmed.md#nyc-gauge-profile).

## Deep-sleep rails

Keep the **latch high** so the MCU can sleep without collapsing the board.
Turn **peripheral rails off** so they do not draw:

| Pin | Hold across sleep | Why |
| --- | ---: | --- |
| GPIO45 `PWR_HOLD` | 1 | Board stays powered |
| GPIO46 `PWR_LOCK` | 1 | Board stays powered |
| GPIO47 EPD_EN | 0 | After SSD1677 sleep command |
| GPIO42 TOUCH_EN | 0 | No touch-to-wake. This is rail-cut, not the datasheet I2C Sleep (INT low then screen-off command; [touch.md](touch.md)) |
| GPIO41 TOUCH_RST | 0 | Held with touch rail |
| GPIO10 SD_EN | 0 | Card unpowered |
| GPIO48 buzzer | 0 | Silent |
| GPIO38 mic EN | 0 | Mic rail off. After wake, if you will record: disable the USB-Serial-JTAG pad on GPIO19/20, hold GPIO38 low briefly, then enable. [sensors.md](sensors.md#pdm-microphone) |

Use ESP32-S3 **GPIO hold** (and deep-sleep hold enable) so those levels survive
the sleep entry. Hold/release is a **TRM** topic, not datasheet v2.2. Wake:
GPIO4 as input with pull-up, `ext1` **ANY_LOW**. Optional
RTC timer wakeup is MCU-side. PCF8563 INT (`RTC_INTn`) is **NC** to the
ESP32; it cannot wake the chip.

Stock firmware also holds pin levels across **light** sleep, not just deep
sleep, and arms the SD card-detect pin as a wake source alongside the buttons.
Its shipped defaults are automatic light sleep on a ~60 s idle interval with
deep sleep **disabled** and a ~1800 s deep-sleep interval configured but
unused, and it notes that enabling Bluetooth blocks light sleep. Those are
application policy — useful as a sanity check on your own numbers, not a
requirement.

Send the SSD1677 deep-sleep command **while EPD_EN is still high**, then drop
EPD_EN. The last image stays on glass with the analog rail off.

Sleep current: [nyc-sleep-current](../resources/not-yet-confirmed.md#nyc-sleep-current).
In-repo enter/wake: [nyc-deep-sleep-wake](../resources/not-yet-confirmed.md#nyc-deep-sleep-wake).
Which keys request sleep is application policy (top-button hold vs both side
keys); electrically, GPIO4 is the proven wake input and GPIO5/6 are ordinary
active-low keys.
