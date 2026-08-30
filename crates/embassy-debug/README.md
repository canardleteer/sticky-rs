# embassy-debug

Host-tested UART log contract for the Sticky Embassy image in
`firmware/embassy-debug`.

That **firmware package** targets Xtensa (`esp-hal` + `esp-rtos` / Embassy,
`#![no_main]`) and cannot run `cargo test` on the host compiler, so it is a
workspace member but not a default-member. This crate owns the line format:
timestamped button, touch, and IMU events, and no identifier fields.

The firmware prints these strings on UART0 at 115200 through a dedicated
Embassy log task. Host tools: `cargo xtask build-fw embassy-debug`, then
`cargo xtask flash-app` (writes the `.bin` only), then `cargo xtask monitor`
(USB CDC listen; no ACM TTY). The panel is an opt-in `epd` feature on the
firmware package, not this crate. Restore factory `app0` if the image wedges.

```text
embassy-debug: latched
embassy-debug: git=<hash> dirty=0
embassy-debug: t=1204 btn 4 down
embassy-debug: t=2100 touch n=1 p0=123,456
embassy-debug: t=5000 imu=FaceUp x=12 y=-30 z=16300
```

IMU reports use [`IMU_REPORT_SECS`] (5). A pose that does not classify is
the token `imu=none`; the raw sample is still printed.

Desk demo (`cargo xtask build-fw`, `flash-app`, `monitor`, restore):
[firmware/embassy-debug/README.md](https://github.com/canardleteer/sticky-rs/blob/main/firmware/embassy-debug/README.md).

License: MIT
