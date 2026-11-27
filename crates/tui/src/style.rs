//! Colours after the CLI's: headings in bold, sizes in the accent blue, secondary text dimmed,
//! warnings in yellow, errors in red, the selected row in the accent blue. Plain text when
//! `NO_COLOR` is set; the selection is then shown reversed, so it never depends on colour.

use ratatui::style::{Color, Modifier, Style};

fn plain() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

fn fg(color: Color) -> Style {
    if plain() {
        Style::new()
    } else {
        Style::new().fg(color)
    }
}

pub fn heading() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// A size or another key figure.
pub fn size() -> Style {
    fg(Color::LightBlue).add_modifier(Modifier::BOLD)
}

pub fn muted() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

pub fn warn() -> Style {
    fg(Color::Yellow)
}

pub fn error() -> Style {
    fg(Color::Red).add_modifier(Modifier::BOLD)
}

pub fn good() -> Style {
    fg(Color::Green)
}

/// The row under the cursor.
pub fn selected() -> Style {
    if plain() {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    }
}

/// The current view's tab and other active marks.
pub fn active() -> Style {
    if plain() {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::new()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    }
}

/// A bar of `width` cells, `share` of it filled (0.0 to 1.0).
pub fn bar(share: f64, width: usize) -> String {
    let share = if share.is_finite() {
        share.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = ((share * width as f64).round() as usize).min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}
