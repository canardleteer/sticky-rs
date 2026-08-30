# embassy-debug

Host-tested UART line format for `firmware/embassy-debug`. This crate is
a default-member. The Xtensa image is not: do not `cargo test -p
embassy-debug-fw` on host rustc.

Flash, `monitor`, restore, and live-ask:
[firmware AGENTS.md](../../firmware/embassy-debug/AGENTS.md) and the
root [AGENTS.md](../../AGENTS.md).

```shell
cargo test -p embassy-debug --locked
```

This crate’s `README.md` is the crates.io landing page. Relative
markdown links there only resolve inside this package.

## Agent Documentation Standards

Maintain this file according to the [AGENTS.md standard](https://agents.md/),
and keep it portable across compatible agent clients, without assumptions
about user-specific paths or session state.
