# simple-debug

Host-tested UART log contract for the Sticky proof-of-life image
(`firmware/simple-debug`).

This crate owns the line format and GPIO edge rules: a heartbeat of raw
levels, extra lines only on edges, and no identifier fields.

Heartbeat (once a second):

```text
simple-debug: t=12 vbus=1 gpio7=1 gpio40=0 sd_cd=1 soc=87 v=3870 i=-12 imu=FaceUp
```

GPIO edges only: `btn 4 down`, `vbus 1 -> 0`, `sd_cd 1 -> 0`, and the
same for `gpio7` / `gpio40`. An operator image may also print
`simple-debug: prompt <step_id>` at boot, `simple-debug: contacts=<n>`
when GT911 contact count changes, and `simple-debug: gt911 st=0xNN`
plus `simple-debug: gt911 int=0` each heartbeat. Boot may print
`simple-debug: git=<hash> dirty=<0|1>` and `simple-debug: gt911 id=911`.
Host-tested formatters: `format_prompt`, `format_contacts`,
`format_git`, `format_gt911_status`, `format_gt911_id`,
`format_gt911_int`.

License: MIT
