# embassy-debug

Host-tested UART log contract for the Sticky Embassy image in
`firmware/embassy-debug`.

This crate owns the line format: timestamped button, touch, IMU, and
mic-energy events, and no identifier fields.

```text
embassy-debug: latched
embassy-debug: git=<hash> dirty=0
embassy-debug: t=1204 btn 4 down
embassy-debug: t=2100 touch n=1 p0=123,456
embassy-debug: t=5000 imu=FaceUp x=12 y=-30 z=16300
embassy-debug: t=1204 mic rms=12 peak=40
```

IMU reports use [`IMU_REPORT_SECS`] (5). A pose that does not classify
is the token `imu=none`; the raw sample is still printed. The `mic`
line is printed only by the `--features mic` image.
