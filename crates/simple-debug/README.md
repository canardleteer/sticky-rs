# simple-debug

Host-tested UART log contract for the Sticky proof-of-life image
(`firmware/simple-debug`).

This crate owns the line format and GPIO edge rules: a heartbeat of raw
levels, extra lines only on edges, and no identifier fields.

Heartbeat (once a second):

```text
simple-debug: t=12 vbus=1 gpio7=1 gpio40=0 sd_cd=1 soc=87 v=3870 i=-12 imu=FaceUp
simple-debug: sht t=23400 rh=45100
simple-debug: rtc y=26 mo=8 d=30 h=15 mi=14 s=0 vl=0
```

[`IdleListen`] vets an unattended `monitor` capture (latch, gauge type,
heartbeat, SHT, RTC). Host: `cargo xtask vet-idle-log --simple FILE`.

`sht t=` is milli °C; `rh=` is milli % RH. `rtc` year is 0–99. `vl`
is the NXP seconds-register VL bit. Failed reads print `sht none` or
`rtc none`. Host-tested formatters: `format_sht`, `format_sht_none`,
`format_rtc`, `format_rtc_none`.

GPIO edges only: `btn 4 down`, `vbus 1 -> 0`, `sd_cd 1 -> 0`, and the
same for `gpio7` / `gpio40`. An operator image may also print
`simple-debug: prompt <step_id>` at boot, `simple-debug: contacts=<n>`
when GT911 contact count changes (`n` is 0..=5 on this FPC), and
`simple-debug: gt911 st=0xNN` plus `simple-debug: gt911 int=0` when
board `touch::STATUS_HEARTBEAT` is on. Boot may print
`simple-debug: git=<hash> dirty=<0|1>` and `simple-debug: gt911 id=911`.
Host-tested formatters: `format_prompt`, `format_contacts`,
`format_git`, `format_gt911_status`, `format_gt911_id`,
`format_gt911_int`, `format_sht`, `format_sht_none`, `format_rtc`,
`format_rtc_none`.
