# simple-debug

Host-tested UART line format for `firmware/simple-debug`. This crate is a
default-member. The Xtensa image is not: do not `cargo test -p
simple-debug-fw` on host rustc.

Flash, `learn-uart`, and live-ask:
[firmware AGENTS.md](../../firmware/simple-debug/AGENTS.md) and the
root [AGENTS.md](../../AGENTS.md).

```shell
cargo test -p simple-debug --locked
```

This crate’s `README.md` is the crates.io landing page. Relative
markdown links there only resolve inside this package.

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
