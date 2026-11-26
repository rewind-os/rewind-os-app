//! Colours for people reading the CLI in a terminal, after the app's look: headings in bold,
//! sizes in the accent blue, secondary text dimmed, warnings in yellow and errors in red.
//!
//! Text is printed through `anstream`, which drops the colours when the output is not a
//! terminal (a pipe, a file, `--json`) or `NO_COLOR` is set, and turns them into console calls
//! on older Windows consoles. Pad a value to its column before colouring it, so columns stay
//! aligned.

use anstyle::{AnsiColor, Color, Style};
use std::fmt::Display;

const HEADING: Style = Style::new().bold();
const SIZE: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::BrightBlue)));
const MUTED: Style = Style::new().dimmed();
const WARN: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow)));
const ERROR: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Red)));
const GOOD: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));

fn paint(style: Style, text: impl Display) -> String {
    format!("{style}{text}{style:#}")
}

/// A section's title.
pub fn heading(text: impl Display) -> String {
    paint(HEADING, text)
}

/// A size or another key figure.
pub fn size(text: impl Display) -> String {
    paint(SIZE, text)
}

/// Dates, notes and other secondary text.
pub fn muted(text: impl Display) -> String {
    paint(MUTED, text)
}

/// Something incomplete or worth attention.
pub fn warn(text: impl Display) -> String {
    paint(WARN, text)
}

pub fn error(text: impl Display) -> String {
    paint(ERROR, text)
}

/// Something done or in good shape.
pub fn good(text: impl Display) -> String {
    paint(GOOD, text)
}
