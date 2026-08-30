//! UART learn session: unattended facts plus skippable human steps.

pub mod diff;
pub mod input;
pub mod parse;
pub mod report;
pub mod session;
pub mod stamp;
pub mod steps;
pub mod term;

pub use session::{run, LearnUartArgs};
