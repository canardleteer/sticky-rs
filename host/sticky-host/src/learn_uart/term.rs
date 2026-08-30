//! Operator prompt styling via [`anstyle`] / [`anstream`] (clap's stack).
//! [`anstream`] honors TTY, `NO_COLOR`, and `CLICOLOR`.

use anstyle::{AnsiColor, Color, Style};

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();
const TOPIC: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Cyan)));
const STEP: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Cyan)));
const OK: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Green)));
const GO: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Yellow)));
const BAD: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Red)));

fn paint(style: Style, text: &str) -> String {
    format!("{style}{text}{style:#}")
}

/// Bold text (command letters).
#[must_use]
pub fn bold(text: &str) -> String {
    paint(BOLD, text)
}

/// Dim secondary text.
#[must_use]
pub fn dim(text: &str) -> String {
    paint(DIM, text)
}

/// `Do:` / `Recording:` / `Now:` labels.
#[must_use]
pub fn topic(label: &str) -> String {
    paint(TOPIC, &format!("{label}:"))
}

/// Step banner.
#[must_use]
pub fn step_title(index: usize, total: usize, title: &str) -> String {
    paint(STEP, &format!("STEP {index} of {total}  {title}"))
}

/// Success / READY.
#[must_use]
pub fn ok(text: &str) -> String {
    paint(OK, text)
}

/// Action the operator should take now.
#[must_use]
pub fn go(text: &str) -> String {
    paint(GO, text)
}

/// Failure / did not see a response.
#[must_use]
pub fn bad(text: &str) -> String {
    paint(BAD, text)
}

/// A key or letter the operator types.
#[must_use]
pub fn key(label: &str) -> String {
    paint(BOLD, label)
}

/// `s skip` — the letter is the whole command.
#[must_use]
pub fn skip_hint() -> String {
    format!("{} skip", key("s"))
}

/// Shown under skip so nobody waits for Enter.
#[must_use]
pub fn skip_no_enter() -> String {
    dim("Press s. You do not need Enter.")
}

/// `[Y/n]` or `[y/N]`.
#[must_use]
pub fn yn_hint(default_yes: bool) -> String {
    if default_yes {
        format!("[{}/n]", key("Y"))
    } else {
        format!("[y/{}]", key("N"))
    }
}

/// Horizontal rule between steps.
#[must_use]
pub fn rule() -> String {
    dim("────────────────────────────────")
}

#[cfg(test)]
mod tests {
    use super::{paint, yn_hint, BOLD};

    #[test]
    fn yn_hint_marks_the_default() {
        let yes = yn_hint(true);
        assert!(yes.contains('Y'));
        assert!(yes.contains('n'));
        let no = yn_hint(false);
        assert!(no.contains('y'));
        assert!(no.contains('N'));
    }

    #[test]
    fn paint_wraps_with_anstyle_reset() {
        let out = paint(BOLD, "Enter");
        assert!(out.contains("Enter"));
    }
}
