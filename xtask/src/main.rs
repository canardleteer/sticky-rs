//! `cargo xtask` binary.

mod ci;
mod cli;

fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();
    cli::Cli::exec()
}
