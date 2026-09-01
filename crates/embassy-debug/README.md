# embassy-debug

Host-tested UART log contract for the Sticky Embassy image in
`firmware/embassy-debug`.

This crate owns the line format: timestamped button, touch, IMU,
mic-energy, AI Voice PCM-dump, and radio-scan lines, and no identifier
fields.

```text
embassy-debug: latched
embassy-debug: git=<hash> dirty=0
embassy-debug: t=1204 btn 4 down
embassy-debug: t=2100 touch n=1 p0=123,456
embassy-debug: t=2100 gt911 st=0x00
embassy-debug: t=5000 imu=FaceUp x=12 y=-30 z=16300
embassy-debug: t=1204 mic rms=12 peak=40
embassy-debug: t=1204 mic pcm hz=1000 n=256
embassy-debug: pcm 000 120 -30 400 0 1 2 3 4 5 6 7 8 9 10 11 12
embassy-debug: t=1204 wifi n=2
embassy-debug: t=1204 wifi ssid=Home rssi=-42
embassy-debug: t=1204 ble n=1
embassy-debug: t=1204 ble name=Phone rssi=-70
embassy-debug: sd cd=0
embassy-debug: sd hz=400000 type=sdhc mid=0x03 name=SC16G
embassy-debug: sd hz=10000000 ack
embassy-debug: sd hz=20000000 ack
embassy-debug: sd vol=0
embassy-debug: sd ent name=FOO.TXT bytes=12
embassy-debug: sd dir n=1
embassy-debug: sd read name=FOO.TXT n=12
embassy-debug: ce parked gpio40=1 vbus=1 i=0
embassy-debug: ce on gpio40=0 i=-12
embassy-debug: ce off gpio40=1 i=4
embassy-debug: ce skip no-vbus
```

[`IdleListen`] vets an unattended `monitor` capture (boot dance, idle
`imu=`, `gt911 st=`). Host:
`cargo xtask vet-idle-log --embassy idle-embassy.log`.

IMU reports use [`IMU_REPORT_SECS`] (5). A read-only `gt911 st=` line
follows board `touch::STATUS_HEARTBEAT` (`EverySecs(10)` or `Off`).
This FPC delivers five contacts (Rev.09 §1). `p0=` is physical
800×480 after board `to_screen` (sample is 480×800). A pose that
does not classify is the token `imu=none`; the raw sample is still
printed. The `mic`
and `pcm` lines are printed only by the `--features mic` image. AI Voice
dumps two windows and does not play the buzzer
([`PCM_DUMP_NO_TONE_HZ`] is `hz=0`).
The `wifi` and `ble` lines are printed only by the `--features radio`
image (SSID / local name and RSSI; never a MAC or BSSID). The `sd`
lines are printed only by `--features sd` (read-only identify and FAT
list; never a CID product serial or file contents). The `ce` lines are
printed only by `--features charge` (a ≤ 2 s `/CE` pulse when VBUS is
present, then park).
