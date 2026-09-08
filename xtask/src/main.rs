//! `cargo xtask` binary.

mod ci;
mod cli;
mod remote_debug;

fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();
    let mut args = std::env::args();
    let _bin = args.next();
    if args.next().as_deref() == Some("remote-debug") {
        return remote_debug::exec();
    }
    cli::Cli::exec()
}
