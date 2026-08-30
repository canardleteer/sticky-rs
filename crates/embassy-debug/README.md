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
```

IMU reports use [`IMU_REPORT_SECS`] (5). A read-only `gt911 st=` line
follows board `touch::STATUS_HEARTBEAT` (`EverySecs(10)` or `Off`).
This FPC delivers five contacts (Rev.09 §1). A pose that does not classify
is the token `imu=none`; the raw sample is still printed. The `mic`
and `pcm` lines are printed only by the `--features mic` image. AI Voice
plays a 1 kHz buzzer tone ([`BUZZER_TONE_HZ`]) and dumps two windows.
The `wifi` and `ble` lines are printed only by the `--features radio`
image (SSID / local name and RSSI; never a MAC or BSSID).
