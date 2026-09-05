# embassy-debug-fw

Embassy event-logger image. Workspace member, **not** a default-member:
host `cargo test` must not compile this package. The panel is always on.

Live-ask, never-erase, and flash I/O: root
[AGENTS.md](../../AGENTS.md). Parent contract:
[firmware/AGENTS.md](../AGENTS.md). How-to:
[docs/getting-started.md](../../docs/getting-started.md).

## Envelope

- Latch GPIO45 then GPIO46 before logs or buses.
- Park BQ25616 `/CE` disabled. Default image does not enable
  charging. `--features charge` is an attended ≤ 2 s `/CE` pulse
  when GPIO9 is high after a cold boot or a 1 s Page Up resume
  hold, then park. A wake that re-sleeps does not pulse `/CE`.
  Do not combine with `mic`, `radio`, `pair`, `wifi`, or `sd`. Do not
  flash that feature unless the operator is present.
- GPIO7 is input-only (IMU INT1 and gauge GPOUT share it). Do not
  drive it.
- MicroSD: CS idle-high on the default image. `--features sd` is
  read-only identify plus a FAT root list and one `ReadOnly` file
  read. No writes, no CID product serial, no file contents on UART.
  Do not combine with `mic`, `radio`, or `wifi` (`compile_error!`).
- Gauge: default image does not use it. `--features charge` reads
  `Current()` for the `ce` lines only. No unseal, no data-memory
  writes.
- Touch: rail on, then Rev.09 §6.1 INT-during-reset address select
  (400 kHz cap, INT=low then INT=high, INT driven after RST rises).
  No config-RAM write. No init `StatusWrite::Clear`. No
  `Register::Command`. Poll `Register::Points` (coords at byte 0);
  clear Status only after a ready frame. Read-only `gt911 st=` follows
  board [`STATUS_HEARTBEAT`](../../crates/seeed-reterminal-sticky)
  (`EverySecs(10)` or `Off`). On this
  unit: INT=0 → `0x5d` ACK, `touch n=5`, `st=0x85`
  ([touch.md](../../.agents/skills/seeed-sticky-hardware/references/touch.md#on-a-physical-unit-embassy-debug)).
  `to_screen` takes the 480×800 sample; USB-down ink corners land on
  800×480. INT-high + init Status-clear stayed at `st=0x00`.
- Panel: splash, shapes, legend, tones, pair, Wi-Fi survey /
  SoftAP, and the Ferris off-screen follow the four in-plane IMU
  holds (portrait 480×800 and landscape 800×480). FaceUp /
  FaceDown keep the last of those.
  Pair idle is a framed how-to with empty PIN boxes; digits appear
  only after `pair pin=`. Advertise only on that card. Wi-Fi cards
  stay idle until a tap on `[ START SURVEY ]` / `[ START HOTSPOT ]`.
  Landscape0 START uses only the OTP `set_gray` 180° of the
  canvas. Landscape180 and portrait invert the tap canvas.
  Do not OR both.
  Legend is a document (keys, sleep / standby / power, OTP), not
  72×72 nub boxes.
  OTP gray4 splash / legend / tones / pair / Wi-Fi / Ferris-off;
  OTP 1-bit shapes. No `0x32` LUT, no Lotus `0x21`.
  Default clock is board `SPI_MAX_HZ` (10 MHz). `--features spi20`
  clocks the panel at 20 MHz; UART prints `spi=20000000`.
  Do not combine `spi20` with `mic` or `radio` (`compile_error!`).
- Microphone: default image leaves `MicRail` disabled. `--features mic`
  enables the rail and I2S PDM RX (16 kHz mono left; energy is live
  on a physical unit and does not close nyc-mic-pdm). AI Voice dumps
  two PCM windows and leaves the buzzer off; it does not change
  the page. How-to:
  [README.md](README.md#microphone-test-instructions).
- Radio: `--features radio` is the exclusive scan sit (Wi-Fi + BLE
  together on ANT1; on a physical unit: `wifi n=` and `ble n=` in
  one listen). Scan only; no NVS writes; no MAC / BSSID. Active
  scan still transmits probe / scan requests. Do not combine with
  `pair` or `wifi`. The radios stay up until reset; this sit does
  not deinit them before deep sleep. How-to:
  [README.md](README.md#radio-test-instructions).
- Pair: default image includes BLE. Advertise `sticky-rs`
  (DisplayOnly passkey) **only while `scene=pair` is showing**.
  Walking away stops advertising and drops a connection. RAM bonds
  this boot only; no factory NVS; no MAC on UART. Packs with
  `wifi`. Do not combine with `mic`, `radio`, `charge`, or `sd`
  (`build-fw` / `ci` pass `--no-default-features` for those sits).
  Pairing success is confirmed on a physical unit (host BlueZ
  Connect, UART `pair pin=` then `pair ok`, pair card showed
  `Paired`). Walkthrough (rustdoc on private items too):
  [src/pair.rs](src/pair.rs). How-to:
  [README.md](README.md#pair-test-instructions).
- Wi-Fi: default image includes survey + WPA2 SoftAP cards
  (`scene=wifi_survey` / `scene=wifi_ap`), idle until a tap.
  Starting one mode stops the other. UART is
  `wifi_survey count=` / `wifi_ap` / `wifi_http` — never a
  neighbor SSID, BSSID, or station MAC. SoftAP may print the
  fixed demo SSID/pass (`sticky-rs-AP` / `sticky26`). No factory
  NVS. Walking away does not auto-stop; deep sleep and latch
  power-off tear SoftAP down. On a physical unit (2026-09-04):
  Page Down printed both scene tokens; START taps printed
  `touch n=1` but **no** `wifi_survey` / `wifi_ap` until
  hit-test used `to_framebuffer` + `framebuffer_to_page` (UART
  `p0=` is `to_screen` / glass). Landscape START still
  missed after that: gray4 `set_gray` writes
  `(W-1-x, H-1-y)` and landscape `page_to_framebuffer` is
  mirror-X only, so ink is the OTP 180 of the canvas.
  Landscape0 uses only that complement; Landscape180 and
  portrait invert the tap canvas. Do not OR both: the OR
  image toggled the empty opposite side (operator, same
  sit). Confirmed on a physical unit (2026-09-04): FaceUp
  with the last landscape page, `wifi tap page=362,53
  hit=1` and glass `p0=362,426`, then `wifi_ap
  state=active`; STOP `page=382,78 hit=1`; second START
  then `imu=Landscape180`. SoftAP
  join / `GET /` is **host-verified**
  (2026-09-04): spare STA, DHCP `192.168.4.50`, JSON
  `device` / `scene=wifi_ap` / `wifi` counts. After STOP +
  replug + START: UART `wifi_ap … clients=1` then
  `wifi_http req=1 path=/`. Host disconnect produced no later
  `wifi_ap` decrement on that image. Leave now keeps one event
  subscriber, emits the new count, and wakes gray4; SoftAP idle
  timeout is 10 s. That drop is **not measured** on the new
  image yet. Do not
  combine with `mic`, `radio`, `charge`, or `sd`. Walkthrough:
  [src/wifi.rs](src/wifi.rs). How-to:
  [README.md](README.md#wifi-test-instructions).
- Panel standby: hold Page Up 2 s. `UpdateSequence::STANDBY` then
  `MasterActivation`. The sit stays until Page Up 1 s (resume) or
  Page Up 5 s (MCU sleep). Stock `RESUME` (`0xC0`) and
  `ENABLE_CLOCK` (`0x80`) left BUSY high on this unit. Firmware
  pulses RST, OTP `init`, UART `epd resume rst` / `resume` /
  same `scene=`. `EPD_EN` stays high. Not MCU deep sleep. Not
  RAM-keep resume.
- Deep sleep: hold Page Up 5 s (the same hold can enter standby
  at 2 s first). The image paints Ferris (splash), sends SSD1677
  `DeepSleepMode` / `DeepSleep::Enter`, cuts `EPD_EN`, keeps the
  latch high, and wakes on GPIO5 `ext1` ANY_LOW. A failed Ferris
  paint stays awake (`EPD_EN` on). A failed `DeepSleepMode` write
  holds `EPD_EN` high (does not cut the rail) and then MCU-sleeps.
  Hold Page Up 1 s after wake to restore Ferris. Early release
  re-sleeps without painting. GPIO4 is still the stock/docs wake
  pin; this image uses GPIO5 because the gesture is Page Up.
- Power off: hold Page Down 5 s. Ferris, panel `DeepSleepMode`,
  cut `EPD_EN`, then `Latch::release`. That is a real power cut.
  Power-on is USB-C plug (firmware latches at boot) or the stock
  ~3 s AI Voice hold. Recessed Reset or a USB unplug/replug is a
  POWERON (splash) when a rail is already present.
  Sit sleep with `cargo xtask monitor` **without** `--acm-tty`
  (`--acm-tty` pulses EN and is a POWERON). No writes
  below `0x90000`. No Cargo `runner`.

## Flash and UART

`flash-app` does not compile. From the repo root, after a matching
snapshot:

```shell
cargo xtask build-fw embassy-debug
cargo xtask flash-app --image target/xtensa-esp32s3-none-elf/release-fw/embassy-debug.bin --yes
cargo xtask monitor
```

Right-edge keys change the page (`scene=…`): splash (Ferris +
`sticky-rs`) → shapes → legend → tones → pair → wifi_survey →
wifi_ap. BLE advertises only on `scene=pair`. Wi-Fi stays idle
until a tap on those cards. This is not the `learn-uart`
operator format.

Host-tested lines live in `crates/embassy-debug`:

```shell
cargo test -p embassy-debug --locked
```

## Bluetooth pairing verification workflow

When testing the default pair card, two verification pathways are
supported. Always offer the human the option to test with their
own devices. Pairing success is **confirmed on a physical unit**.
Do not print or store a MAC.

1. **Manual external device pairing.** The human walks Page Down
   to `scene=pair`, searches for `sticky-rs` from their phone or
   other central, starts pairing, reads the six-digit passkey on
   the pair card, types it on the phone, and watches for `Paired`
   or `Pair failed`. UART should print `pair pin=` then `pair ok`
   or `pair fail=`. Human how-to:
   [README.md](README.md#pair-test-instructions).
2. **Host-agent self-diagnostic pairing (faster for agents).** If
   the host has an available, unblocked Bluetooth controller
   (BlueZ) **and** the human explicitly asked for this sit, the
   agent may:
   - Listen with `cargo xtask monitor` (not `--acm-tty`) so Drop
     reattaches `cdc-acm`. Default CDC listen needs write access
     on the usbfs node: a udev rule in `/etc/udev/rules.d/` (not
     `/etc/udev/`) for `1a86:55d3`, then reload and a USB replug.
   - Scan LE for advertise name `sticky-rs`. Do not read or print
     the eFuse MAC. Stop discovery before Connect.
   - **Connect only.** Do not call BlueZ `Device1.Pair()` or
     `bluetoothctl pair`. The image’s `request_security()` already
     sends SMP Security Request (`0x0B`). A concurrent `Pair()`
     makes Linux log `unexpected SMP command 0x0b` and return
     `AuthenticationCanceled`.
   - Wait for a **new** UART `pair pin=` after that Connect (do
     not reuse digits from an earlier attempt).
   - Submit those six digits through a KeyboardOnly agent
     `RequestPasskey`. Do not block the D-Bus / GLib loop while
     waiting for UART (that yields `NoReply` or a cancel).
   - Look for `pair ok` (or `pair fail=` plus a why token) and
     host `Paired`. Ask the human to confirm the same PIN and
     `Paired` / `Pair failed` on the pair card.

On a physical unit the host path completed: UART `pair pin=` then
`pair ok`, host `Paired` / `Connected`, pair card showed `Paired`.
`btleplug` cannot enter a DisplayOnly passkey (GATT only). A
Linux xtask would wrap BlueZ (`bluer` or D-Bus), not
`bluetoothctl`. Bonds are RAM this boot only. Do not write
factory NVS. Do not combine `pair` with `mic`, `radio`, `charge`,
or `sd`.

## Wi-Fi survey and SoftAP verification workflow

Survey and SoftAP are mutually exclusive: starting one stops the
other. One Wi-Fi manager task owns the mode machine (`Idle` /
`SurveyScanning` / `SurveyComplete` / `Hotspot`). Stack under
`wifi`: `esp-radio` (STA scan + SoftAP), `embassy-net`,
`edge-dhcp`, `edge-nal` / `edge-nal-embassy` (DHCP + HTTP).
Packs with `pair` (BLE stays in `pair.rs`; this path owns
`WIFI` only).

| Constant | Value |
| --- | --- |
| SSID | `sticky-rs-AP` |
| Auth | WPA2-Personal only (`sticky26`) |
| Gateway | `192.168.4.1/24` |
| HTTP | `GET /` JSON on port 80 (`device`, `scene`, `wifi.{hotspot,ssid,clients,requests}`) |
| Survey | Channels 1–13; top **4** APs by RSSI on glass; UART counts only |

WPA3/SAE is unavailable in the precompiled `esp-radio` ESP32-S3
wireless blob — do not advertise WPA3 on glass. No foreign
MAC/BSSID/IRK on UART. SoftAP UART may print the fixed demo
SSID/password. Survey glass may show truncated nearby SSIDs;
do not echo those on the wire. JSON is Sticky-shaped: no
PaperMono sku/lamp/battery and do not turn on `--features charge`
to fill a gauge field.

Walking away from SoftAP/survey does **not** auto-stop. Deep
sleep and latch power-off stop the hotspot (`sleep::is_requested`
tears it down before `Latch::release`).

On a physical unit (2026-09-04) the walk printed
`scene=wifi_survey` / `scene=wifi_ap`. A START tap that used
UART `to_screen` as if it were the gray4 canvas produced **no**
radio line (`p0=679,189` while `imu=Portrait0` mapped to the
top of the portrait page). Hit-test must use `to_framebuffer`
then `framebuffer_to_page`. Landscape0 uses only the OTP
`set_gray` 180° of that canvas; Landscape180 inverts the
tap canvas. Do not OR both. After
the portrait fix, a host spare STA
joined `sticky-rs-AP` / `sticky26`: DHCP `192.168.4.50`,
`GET /` JSON `device` / `scene=wifi_ap` /
`wifi.{hotspot,ssid,clients,requests}` (`clients=1`,
`requests=1`). First DHCP lease was `.50`. After STOP +
replug + START: UART `wifi_ap state=active … clients=1` then
`wifi_http req=1 path=/`. Host `nmcli` disconnect produced no
later `wifi_ap` line. Glass still `clients=1` after that
disconnect (same sit).

Always offer the human the option to join from their own phone
or PC. A host STA check is allowed only when the human
**explicitly asked** for that sit.

1. **Channel survey.** Human walks to `scene=wifi_survey` and taps
   `[ START SURVEY ]` (portrait or landscape; Landscape0 uses
   only the OTP 180° canvas). UART prints `wifi tap page=… hit=1`
   then `wifi_survey count=… ch1=… ch6=… ch11=… other=…`.
   START and the result each run a **full OTP gray4** (whole
   panel). Extra hits while scanning toggle stop/start and
   queue more waveforms. On a physical unit (2026-09-04)
   Landscape180: opposite `page=435,53 hit=0`; ink
   `page=452,399 hit=1`; then `wifi_survey count=16` /
   `count=18`. Glass shows
   occupancy and the strongest APs. Starting survey tears down an
   active SoftAP.
2. **SoftAP + JSON HTTP.** Human walks to `scene=wifi_ap` and taps
   `[ START HOTSPOT ]` (Landscape0 OTP 180° sat 2026-09-04:
   `page=362,53 hit=1` / `p0=362,426`; OR image also
   toggled the empty opposite side).
   Glass shows SSID, password, URL, client
   count, and HTTP request count. UART prints
   `wifi_ap state=active ssid=sticky-rs-AP …`.
3. **Host-agent SoftAP check** (when the human asks for a live
   test and a spare host Wi-Fi adapter is available):
   - Listen with `cargo xtask monitor` (not `--acm-tty`) for
     `wifi_ap` / `wifi_http`.
   - Scan: `nmcli dev wifi list ifname IFACE`.
   - Connect:
     `nmcli dev wifi connect sticky-rs-AP password sticky26 ifname IFACE`.
   - Fetch: `curl -s http://192.168.4.1/` and confirm JSON
     `device` / `scene` / `wifi` fields; UART should print
     `wifi_http`.
   - Disconnect and confirm the client count drops on glass /
     UART (`wifi_ap … clients=0`) and a gray4 refresh. A
     2026-09-04 host `nmcli` disconnect left glass at
     `clients=1` (subscriber was recreated each wait; UART
     used `fetch_update`'s previous value). Firmware now keeps
     one subscriber, emits the new count, and sets SoftAP idle
     timeout 10 s for a USB STA that does not deauth. That
     leave-drop is **not measured** on the landscape-button
     image (2026-09-04 host join had no UART: USB-C in other
     use). That sit: spare STA, DHCP `192.168.4.50`, `GET /`
     `clients=1` `requests=1` then `requests=2`. Operator:
     landscape START/STOP did the right thing on glass.
   - Do not print a station MAC.

Keys still walk pages. AI Voice is not a start/stop. Touch
START/STOP uses composed page coords
([`draw::wifi_action_hit`](src/draw.rs) on raw
[`to_framebuffer`](../../crates/seeed-reterminal-sticky/src/touch.rs)
[`gray4_touch_framebuffer`](../../crates/seeed-reterminal-sticky/src/display.rs)
then
[`framebuffer_to_page`](../../crates/seeed-reterminal-sticky/src/display.rs)
(Landscape0: OTP 180° only). UART prints `wifi tap page=… hit=`;
`p0=` stays `to_screen`).
Human how-to:
[README.md](README.md#wifi-test-instructions).

## Firmware examples as tutorial code

Firmware under `embassy-debug/` serves as an educational reference
and walkthrough for async Embassy on ESP32-S3. Every function,
method, struct, enum, and constant (public or private) must have
comprehensive rustdoc explaining what it does, hardware nets/buses
involved, expectations, and error handling. Include abundant in-line
comments explaining hardware register sequencing, GPIO electrical
configurations (pull-ups, input modes), bus arbitration, Embassy task
scheduling, stack buffer usage, and reset/wake-up cycles. Ground
descriptions in authoritative terminology from *The Embedded Rust Book*,
*The Rust on ESP Book*, and *The Embassy Book*.

The pair walkthrough is [src/pair.rs](src/pair.rs) (default image).
The Wi-Fi walkthrough is [src/wifi.rs](src/wifi.rs) (default image).
Card layouts (IMU page, Koch, document legend, boxed PIN, Wi-Fi
START/STOP) are [src/draw.rs](src/draw.rs).

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
