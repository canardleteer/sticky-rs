# embassy-debug-fw

Embassy event-logger image for the reTerminal Sticky. Timestamped
button / GT911 / IMU lines on UART0, a short beep, and the panel
(OTP 1-bit scenes plus four-tone gray4 boxes). Host-tested line format:
[`crates/embassy-debug`](../../crates/embassy-debug).

```text
embassy-debug: t=1204 btn 4 down
embassy-debug: t=2100 touch n=1 p0=123,456
embassy-debug: t=5000 imu=FaceUp x=12 y=-30 z=16300
```

On the unit:

- Cold boot paints a portrait splash (USB-C down) or a landscape
  splash (USB-C right / left) so Ferris and `sticky-rs` stay upright.
  Shapes, legend, tones, pair, Wi-Fi survey / SoftAP, and the Ferris
  off-screen follow that same hold. FaceUp / FaceDown keep the last
  in-plane page. Tilt after walking pages: the current card should
  stay readable.
- Default image: AI Voice / Page Up / Page Down (right-edge top /
  middle / bottom) walk splash → shapes → legend → four-tone OTP gray
  boxes → pair → wifi_survey → wifi_ap. `--features mic`: AI Voice
  dumps PCM and does not play the buzzer or change the page. Page Up
  / Page Down still walk the drawings. `--features radio`: Wi-Fi and
  BLE scan together on the on-board antenna; keys still walk the
  drawings. Do not combine `mic` and `radio` in one image for a desk
  test.
  Default image includes a DisplayOnly passkey card (Page Up /
  Page Down walk to `scene=pair`). BLE advertises `sticky-rs` only
  on that card. It also includes idle-until-touch Wi-Fi survey and
  WPA2 SoftAP cards. Do not combine `pair` / `wifi` with `mic`,
  `radio`, `charge`, or `sd`.
  `--features spi20` clocks the panel at 20 MHz (`spi=20000000`);
  default stays 10 MHz. `--features sd` runs a read-only card
  identify (`sd cd=`, `sd hz=` / `ack`); no writes. `--features
  charge` pulses `/CE` for ≤ 2 s when USB is present after a cold
  boot or a 1 s Page Up resume hold, then parks. A wake that
  re-sleeps does not pulse `/CE`.
  Do not combine `spi20` / `sd` / `charge` with `mic` or `radio`.
  Do not combine `charge` with `sd` or `wifi`.
- Tilt the card for `imu=…`. A short beep answers a key-down. Tap the
  glass for `touch n=` (Rev.09 INT-low address select; on a physical
  unit through `n=5`).
- Page Up under 2 s walks to the previous drawing. Hold 2 s to
  send panel `standby()` on the **current** card (`EPD_EN` stays
  high). UART prints `standby` at once (BUSY may stay high). The
  sit stays until you hold Page Up about 1 s (resume) or 5 s
  (MCU sleep). The same hold can do 2 s then 5 s. Stock
  `resume()` (`0xC0`) and `ENABLE_CLOCK` (`0x80`) left `busy=1`
  on this unit. The image then pulses RST, OTP-inits, prints
  `epd resume rst` then `resume`, and redraws the same `scene=`.
  That is not MCU deep sleep and not RAM-keep resume.
- Page Up hold 5 s sleeps: the glass shows Ferris (`sticky-rs`
  splash), UART prints `scene=sleeping` then `embassy-debug:
  sleeping`, and UART0 goes quiet. The CH343 stays enumerated
  (the latch stays high; the board does not drop USB). Hold
  Page Up about 1 s to wake Ferris. A shorter press after wake
  goes back to sleep without painting. Listen with
  `cargo xtask monitor` **without** `--acm-tty`. `--acm-tty`
  pulses EN and is a POWERON, not a resume.
- Page Down under 5 s walks to the next drawing. Hold 5 s to
  power off: Ferris, then `embassy-debug: t=… poweroff` and
  `embassy-debug: poweroff`, then the latch drops. That is not
  sleep. Power-on is plug USB-C (the image latches at boot) or
  the stock ~3 s hold of AI Voice. Recessed Reset or
  unplug/replug USB is a POWERON (splash) when a rail is
  already present.

Agent / toolchain:

- Agent flash contract and envelope: [AGENTS.md](AGENTS.md).
- First-time toolchain:
  [docs/getting-started.md](../../docs/getting-started.md).

## Touch Test Instructions

Default `embassy-debug` (no `mic`, no `radio`) already polls the GT911.
A new contact set should print `touch n=` and beep on first down.
`p0=` is physical 800×480 after `to_screen` (GT911 sample is 480×800).
Snapshot first:
[docs/getting-started.md](../../docs/getting-started.md).

### Step 1: Is the port free?

Same as the microphone test. Only one `monitor` at a time. Ctrl-C an
old listen. Do not `kill -9`.

Then:

```shell
cargo xtask detect-connected
```

You should see a Sticky path. If you do not, and you already killed a
listen the hard way, unplug the USB-C cable and plug it back in once.
Run `detect-connected` again.

### Step 2: Build, flash, and listen

```shell
. $HOME/export-esp.sh
cargo xtask build-fw embassy-debug
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
cargo xtask monitor
```

The image is on the chip only after `flash-app` finishes. A successful
build alone does not flash. If `flash-app` says no QinHeng CH343, go
back to Step 1.

Ctrl-C when you are done so the next `flash-app` can see the device.
Do not `kill -9` that listen. If you already did, unplug and replug
once (same as Step 1).

### Step 3: What you should see

Boot should print `embassy-debug: latched`, then
`gt911 addr dance`, `gt911 int=0` / `int=1`, address
`ack`/`nak` lines, `gt911 no init status clear`, and
`gt911 no command write`. Keys
and `imu=` should keep working. Tap the **glass** (not the right-edge
keys). A change in contacts should look like:

```text
embassy-debug: t=2100 touch n=1 p0=123,456
embassy-debug: t=2180 touch n=0
```

`n` is 0–5. `p0=` is the 800×480 screen point. A still finger is
silent after the first line. About every
ten seconds a read-only status line should appear even when idle
(`touch::STATUS_HEARTBEAT`; `Off` silences it):

```text
embassy-debug: t=2100 gt911 st=0x00
```

That read does not write `Register::Status` unless a ready frame was
just consumed. `st=0x00` with no `touch n=` is still a miss. This
image uses Rev.09 §6.1 address select (400 kHz cap, INT=0 then
INT=1, no init status-clear, points at `Register::Points`). Look
for `gt911 addr dance` at boot.

### What we already learned on this unit

2026-08-30, default image (`git=80eaf8f` dirty). INT-high + init
Status-clear ACKed `0x14` and stayed at `st=0x00` with no
`touch n=`. INT-low address select: `gt911 int=0`, `0x5d ack`,
first `st=0x80`. Attended taps printed `touch n=1` / `n=2` /
**`n=5`** and `gt911 st=0x85`. This FPC delivers five contacts
(Rev.09 §1). USB-down finger-pad taps near the ink corners (in one
sample) after the 480×800 `to_screen` map: `795,470`, `795,4`,
`4,475`, `4,4`. Facts:
[touch.md](../../.agents/skills/seeed-sticky-hardware/references/touch.md#on-a-physical-unit-embassy-debug).

## Microphone Test Instructions

Default `embassy-debug` leaves `MicRail` disabled. This feature enables
the rail and I2S PDM RX (16 kHz, 16-bit, left). On a physical unit that path
prints live `mic rms=` / `peak=` energy; high-fidelity hole vs
waveform is still `nyc-mic-pdm`. Snapshot first:
[docs/getting-started.md](../../docs/getting-started.md).

To perform the test:

### Step 1: Is the port free?

`flash-app` and `monitor` need a Sticky serial port. `lsusb` showing
QinHeng `1a86:55d3` is not enough.

Only one `monitor` at a time. If an old listen is still running, Ctrl-C
that terminal. Do not `kill -9`.

Then:

```shell
cargo xtask detect-connected
```

You should see a Sticky path. If you do not, and you already killed a
listen the hard way, unplug the USB-C cable and plug it back in once.
Run `detect-connected` again.

### Step 2: Build, flash, and listen

```shell
. $HOME/export-esp.sh
cargo xtask build-fw embassy-debug --features mic
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
cargo xtask monitor
```

The image is on the chip only after `flash-app` finishes. A successful
build alone does not flash. If `flash-app` says no QinHeng CH343, go
back to Step 1.

Ctrl-C when you are done so the next `flash-app` can see the device.
Do not `kill -9` that listen. If you already did, unplug and replug
once (same as Step 1).

### Step 3: What you should see

You should still see `embassy-debug: latched` and the
usual `btn` / `touch` / `imu=` lines. About four times a second:

```text
embassy-debug: t=1204 mic rms=12 peak=40
```

### Step 4: Dump PCM (you make the tone)

Hold a known tone at the **microphone hole** on the USB-C side/edge.
Then press **AI Voice** (right-edge top). You should **not** hear the
board’s 1 kHz buzz. The page does not change. UART should print
`btn 4 down`, then a header and 16-sample rows:

```text
embassy-debug: t=1204 btn 4 down
embassy-debug: t=1204 mic rms=1800 peak=4000
embassy-debug: t=1204 mic pcm hz=0 n=256
embassy-debug: pcm 000 120 -30 400 0 1 2 3 4 5 6 7 8 9 10 11 12
```

`hz=0` means the board did not play a tone. Two windows (32 ms) dump
after the key. A 400 Hz sine at 16 kHz would repeat about every 40
samples — look for that period in the rows. If the rows stay a flat
floor, the tone did not couple through the hole.

Page Up / Page Down still change the drawing and play the short chirp.

### Step 5: Observe and report

- **Quiet (relative)**: one room, one opinion — fans and a little
  background noise. `rms` / `peak` should sit at a stable floor (not
  zero, not pegged). Covering the holes can still leave a floor near
  `rms` 1000 / `peak` 2500 — treat that as the PDM path, not a failed
  rail, and not a spec for every room.
- **Make Noise**: Press AI Voice (hear the buzz), or scratch, tap, or
  whistle at the **microphone hole** on the USB-C side/edge.
  - Those numbers should jump. A loud whistle can clip `peak` at 32768
    (16-bit full scale). That is not “always-max” unless the quiet
    floor was pegged too.

**If you observe**: Always-zero or always-max (quiet and loud look the
same) means the mux, slot, or rail is wrong, and should not be
considered a passing test. Do not treat that result as closing
[`nyc-mic-pdm`](../../.agents/skills/seeed-sticky-hardware/resources/not-yet-confirmed.md#nyc-mic-pdm).

### What we already learned on this unit

Desk notes from one session in one room. “Quiet” here is relative —
  fans were on — not a chamber measurement.

- A leftover `monitor` can own the CH343. `lsusb` still shows QinHeng
  `1a86:55d3`, but `detect-connected` / `flash-app` see no serial port.
  Ctrl-C that listen. `kill -9` leaves the USB interfaces unbound —
  unplug and replug once.
- In that room, with fans on, energy sat around
  `rms` 1209 / `peak` 2580.
- Covering as many holes as we could only moved that to about
  `rms` 1040–890 / `peak` 2940–2640. Same band. That leftover floor is
  the PDM path, not the room.
- A whistle jumped to `rms` 2770–6749 and `peak` 13421, then clipped
  at 32768 (16-bit full scale). That is a live capsule, not a stuck
  rail.
- A phone tone at the USB-C-edge hole, dump with the buzzer off
  (`hz=0`), left the quiet floor (`rms` ~1900 / `peak` ~5200–5400).
  After the usual `0` / spike / DC prefix, the last ~64 samples of
  each window are a sine with about a 36–40-sample period. That is
  through-hole, not GPIO48. One phone, not a lab oscillator.

`nyc-mic-pdm` is still open: we have energy, a GPIO48 ~16-sample
period, and a through-hole ~36–40-sample period, not slot / polarity
/ settle / deep-sleep pad reclaim. Facts:
[sensors.md](../../.agents/skills/seeed-sticky-hardware/references/sensors.md#on-a-physical-unit-embassy-debug-mic-feature).

## Radio Test Instructions

Default `embassy-debug` does not start the radio. This feature scans
Wi-Fi and BLE **at the same time** on the on-board antenna (schematic
ANT1). Scan only: it does not join an AP, start a SoftAP, or connect
BLE. It does not print a MAC or BSSID. Active scan still transmits
Wi-Fi probe requests and BLE scan requests. The radios stay up until
reset; this image does not deinit them before deep sleep. Snapshot first:
[docs/getting-started.md](../../docs/getting-started.md).

Use this image **without** `--features mic` so GPIO19/20 stay unused.

To perform the test:

### Step 1: Is the port free?

Same as the microphone test. Only one `monitor` at a time. Ctrl-C an
old listen. Do not `kill -9`.

Then:

```shell
cargo xtask detect-connected
```

You should see a Sticky path. If you do not, and you already killed a
listen the hard way, unplug the USB-C cable and plug it back in once.
Run `detect-connected` again.

### Step 2: Build, flash, and listen

```shell
. $HOME/export-esp.sh
cargo xtask build-fw embassy-debug --features radio
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
cargo xtask monitor
```

The image is on the chip only after `flash-app` finishes. A successful
build alone does not flash. If `flash-app` says no QinHeng CH343, go
back to Step 1.

Ctrl-C when you are done so the next `flash-app` can see the device.
Do not `kill -9` that listen. If you already did, unplug and replug
once (same as Step 1).

### Step 3: What you should see

You should still see `embassy-debug: latched` and the usual `btn` /
`touch` / `imu=` lines. About every ten seconds, both radios should
print in the same listen (overlapping timestamps):

```text
embassy-debug: t=1204 wifi n=2
embassy-debug: t=1204 wifi ssid=Home rssi=-42
embassy-debug: t=1800 ble n=1
embassy-debug: t=1800 ble name=Phone rssi=-70
```

`n=0` is a pass if that scan finished (no hang). A name of `?` means
the advertisement had no local name (still not a MAC).

Right-edge keys still change the page and play the short chirp.

### Step 4: Optional — a known AP or BLE advertiser

Sit near an access point you already know, or put a phone in BLE
advertising / pairing so it has a local name. Wait for the next
ten-second print (each `ble n=` is a fresh scan, same as Wi-Fi).

You should see that SSID or name, or `wifi n=` / `ble n=` go up.
At most eight `ssid=` lines and eight `name=` lines; a stronger
RSSI can displace a weaker one. Classic Bluetooth (not BLE
advertising) will not appear. Missing one extra device is not a
fail if Step 3 already printed both radios.

### Step 5: Observe and report

- **Both radios**: `wifi n=` and `ble n=` both appear in one
  `monitor` session. That is concurrent use. On a physical unit: one listen
  printed `wifi n=8` (line cap) and `ble n=` well over 100, with
  `imu=` still running. Facts:
  [pin-map.md](../../.agents/skills/seeed-sticky-hardware/references/pin-map.md#on-a-physical-unit-embassy-debug-radio-feature).
- **Fail**: hang, panic, or only one of `wifi` / `ble` ever prints.

This is a stack / RF-cal-after-`flash-app` check, not an NYC pin. Do
not treat a successful scan as a license to write NVS or print a MAC.

## Pair Test Instructions

Default `embassy-debug` starts BLE but advertises `sticky-rs` only
while the pair card is showing. Walking to that page advertises
as `sticky-rs` and shows a six-digit passkey only after a phone
starts pairing. Bonds stay in RAM for this boot. The image does
not write factory NVS and does not print a MAC. The MCU walkthrough
is [src/pair.rs](src/pair.rs) (rustdoc on the private helpers too).
Snapshot first:
[docs/getting-started.md](../../docs/getting-started.md).

Do not combine with `mic`, `radio`, `charge`, or `sd`.

To perform the test:

### Step 1: Is the port free?

Same as the microphone test. Only one `monitor` at a time. Ctrl-C an
old listen. Do not `kill -9`.

Then:

```shell
cargo xtask detect-connected
```

You should see a Sticky path. If you do not, and you already killed a
listen the hard way, unplug the USB-C cable and plug it back in once.
Run `detect-connected` again.

### Step 2: Build, flash, and listen

```shell
. $HOME/export-esp.sh
cargo xtask build-fw embassy-debug
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
cargo xtask monitor
```

The image is on the chip only after `flash-app` finishes. A successful
build alone does not flash. If `flash-app` says no QinHeng CH343, go
back to Step 1.

Ctrl-C when you are done so the next `flash-app` can see the device.
Do not `kill -9` that listen. If you already did, unplug and replug
once (same as Step 1).

### Step 3: Open the pair card

You should still see `embassy-debug: latched` and the usual `btn` /
`touch` / `imu=` lines. There is no `pair advertise` yet. Pages
are splash → shapes → legend → tones → pair → wifi_survey →
wifi_ap. Press Page Down until UART prints `scene=pair`, then
`pair advertise sticky-rs`.

The glass should show `BLUETOOTH PAIRING`, `Device: sticky-rs`,
empty PIN boxes, and a short how-to. There is no PIN yet. Tilt
the card: this page stays upright like splash. When the six
digits appear they sit in those boxes (`FONT_10X20`, not a
3× scale).

Right-edge keys still change the page. AI Voice is not a pair
confirm.

### Step 4: Pair from the phone

On the phone, open Settings, then Bluetooth, then tap `sticky-rs`.

When the board asks for a passkey, UART should print
`pair pin=` plus six digits, and the glass should show those
digits. Type that code on the phone.

You should then see either:

```text
embassy-debug: t=1800 pair ok
```

and `Paired` on the glass, or:

```text
embassy-debug: t=2100 pair fail=cancelled
```

and `Pair failed` plus a short why (`cancelled`, `timeout`,
`pairing`, `bond_lost`, `advertise`, `ble_start`, or `unknown`).
A fail card sits a few seconds, then the how-to comes back.

If BLE never starts, you should see `pair fail=ble_start` (or
`advertise`) and no PIN. That is a fail for this sit.

### Step 5: Observe and report

- **Idle first**: the pair card starts as a how-to, not a fake PIN.
- **PIN only after the phone starts pairing**: `pair pin=` matches
  the six digits on the glass.
- **Outcome**: `pair ok` and `Paired`, or `pair fail=` plus a why
  token. No MAC on UART.
- **Fail**: hang, panic, a PIN before you tap the phone, or a
  `pair` line that includes a MAC.

On a physical unit a host BlueZ Connect typed the UART passkey:
`pair ok`, and the pair card showed `Paired`. A phone Settings
sit is still a valid second path.

## Wifi Test Instructions

Default `embassy-debug` starts Wi-Fi idle. Walk to
`scene=wifi_survey` or `scene=wifi_ap`, then tap START on the
glass. Survey and SoftAP cannot run at once: starting one stops
the other. Walking to another page does **not** stop the radio.
Page Up 5 s (MCU sleep) or Page Down 5 s (latch off) does.
Bonds and scan results stay in RAM. The image does not write
factory NVS and does not print a neighbor SSID, BSSID, or MAC.
The MCU walkthrough is [src/wifi.rs](src/wifi.rs). Snapshot
first:
[docs/getting-started.md](../../docs/getting-started.md).

Do not combine with `mic`, `radio`, `charge`, or `sd`.

To perform the test:

### Step 1: Is the port free?

Same as the microphone test. Only one `monitor` at a time. Ctrl-C an
old listen. Do not `kill -9`.

Then:

```shell
cargo xtask detect-connected
```

You should see a Sticky path. If you do not, and you already killed a
listen the hard way, unplug the USB-C cable and plug it back in once.
Run `detect-connected` again.

### Step 2: Build, flash, and listen

```shell
. $HOME/export-esp.sh
cargo xtask build-fw embassy-debug
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
cargo xtask monitor
```

The image is on the chip only after `flash-app` finishes. A successful
build alone does not flash. If `flash-app` says no QinHeng CH343, go
back to Step 1.

Ctrl-C when you are done so the next `flash-app` can see the device.
Do not `kill -9` that listen. If you already did, unplug and replug
once (same as Step 1).

### Step 3: Run a channel survey

You should still see `embassy-debug: latched` and the usual `btn` /
`touch` / `imu=` lines. Pages are splash → shapes → legend → tones
→ pair → wifi_survey → wifi_ap. Press Page Down until UART prints
`scene=wifi_survey`.

The glass should show `WIFI SURVEY` and `[ START SURVEY ]`. Tilt
the card: this page stays upright like splash. Right-edge keys
still change the page. AI Voice is not a start/stop.

Tap `[ START SURVEY ]`. UART should print a counts line, for
example:

```text
embassy-debug: t=1204 wifi_survey count=5 ch1=1 ch6=2 ch11=1 other=1
```

The glass should show those channel counts and up to four nearby
names. Those names stay on the glass. They must not appear on
UART.

Tap `[ STOP SURVEY ]` if you want idle. Starting the hotspot
from the next card also stops the survey.

### Step 4: Start the hotspot

Press Page Down until UART prints `scene=wifi_ap`. The glass
should show `WIFI HOTSPOT`, SSID `sticky-rs-AP`, password
`sticky26`, and `http://192.168.4.1/`.

Tap `[ START HOTSPOT ]`. UART should print:

```text
embassy-debug: t=1204 wifi_ap state=active ssid=sticky-rs-AP pass=sticky26 ip=192.168.4.1 clients=0
```

On a phone or PC, open Wi-Fi settings, join `sticky-rs-AP` with
password `sticky26`, then open `http://192.168.4.1/` in a
browser. You should see JSON with `device`, `scene`, and a
`wifi` object (`hotspot`, `ssid`, `clients`, `requests`). UART
should print `wifi_http req=` plus `path=/`.

This is WPA2 only. The board does not offer WPA3.

Tap `[ STOP HOTSPOT ]` when you are done. UART should print
`wifi_ap state=stopped`. Walking away without tapping STOP
leaves the hotspot up until sleep or power-off.

### Step 5: Observe and report

- **Idle first**: neither card starts a radio until you tap
  START.
- **Survey**: `wifi_survey count=` on UART; nearby names on
  glass only.
- **Hotspot**: `wifi_ap state=active` with the fixed demo
  SSID/pass, then `wifi_http` after a `GET /`.
- **Fail**: hang, panic, a neighbor SSID or MAC on UART, or a
  SoftAP that stays up after latch power-off.

A host `nmcli` / `curl` sit is optional and only when you asked
for it. On a physical unit a spare host STA joined
`sticky-rs-AP`, got `192.168.4.50`, and `GET /` returned JSON
with `device`, `scene=wifi_ap`, and `wifi` counts.

## Charge Test Instructions

Default `embassy-debug` parks `/CE` and never enables charging. This
feature is an attended sit for
[`nyc-charge-stat`](../../.agents/skills/seeed-sticky-hardware/resources/not-yet-confirmed.md#nyc-charge-stat):
print parked STAT / VBUS / gauge current, enable `/CE` for two
seconds only when GPIO9 is high, print STAT and `i=`, then park
again. A cold boot or a 1 s Page Up resume hold repeats that
pulse. A wake that re-sleeps does not. Do not leave it as a daily
driver. Do not combine with `mic`, `radio`, `pair`, `wifi`, or `sd`.

FreeInk is the SDK wiring: GPIO40 STAT low = charging, GPIO39
undriven at idle. Bunny enables charge at boot; this image does
not copy that.

USB-C stays plugged (flash uses that port, so VBUS should read
high). Snapshot first:
[docs/getting-started.md](../../docs/getting-started.md).

Do **not** flash this feature unless the operator is at the desk.

### Step 1: Is the port free?

Same as the microphone test. Only one `monitor` at a time. Ctrl-C an
old listen. Do not `kill -9`.

Then:

```shell
cargo xtask detect-connected
```

You should see a Sticky path. If you do not, and you already killed a
listen the hard way, unplug the USB-C cable and plug it back in once.
Run `detect-connected` again.

### Step 2: Build, flash, and listen

```shell
. $HOME/export-esp.sh
cargo xtask build-fw embassy-debug --features charge
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
cargo xtask monitor
```

The image is on the chip only after `flash-app` finishes. A successful
build alone does not flash. If `flash-app` says no QinHeng CH343, go
back to Step 1.

Ctrl-C when you are done so the next `flash-app` can see the device.
Do not `kill -9` that listen. If you already did, unplug and replug
once (same as Step 1).

### Step 3: What you should see

Early after `latched` / `git=`:

```text
embassy-debug: ce parked gpio40=1 vbus=1 i=0
embassy-debug: ce on gpio40=0 i=0
embassy-debug: ce on gpio40=0 i=5702
embassy-debug: ce off gpio40=1 i=5702
```

`gpio40=1` parked and `gpio40=0` while enabled is the STAT proof.
A second `ce on` is the end of the 2 s window. `ce off` is after a
settle (and `hold_disabled` if STAT was still low). On a physical unit STAT
was `1→0→1` only with that settle. `i=` is BQ27220 `Current()`
(sheet: mA, positive is charge). `0` at 200 ms and `5702` at 2 s
is not the schematic ~555 mA set. `ce skip no-vbus` means GPIO9
was low; `/CE` stayed parked.

Keys, glass, and `imu=` still run after the pulse. `/CE` stays
parked for the rest of the listen.

### Step 4: Observe and report

- **STAT (on a physical unit)**: low while enabled, high after park and a
  settle. Immediate post-disable STAT stayed low and the LED stayed
  green/yellow.
- **Still open**: charge-to-done, a credible `i=` vs the 555 mA
  set, LED off / done color.
- **Fail**: `ce on` with `gpio40=1`, a hang, or `/CE` left enabled
  after the pulse.

Do not arm GPIO7 on this sit. Do not treat a STAT pass as a license
to enable charging in default images. After the sit, flash default
`embassy-debug` (no `charge`) so every boot does not pulse `/CE`.
