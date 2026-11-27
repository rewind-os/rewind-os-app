//! What the list views share: moving the cursor and the `/` filter.

use crate::style;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::{Line, Span};
use rewind_os_i18n::tr;

/// Where a movement key puts the cursor in a list of `len` rows, or `None` for other keys.
pub fn moved(code: KeyCode, cursor: usize, len: usize) -> Option<usize> {
    let last = len.saturating_sub(1);
    Some(match code {
        KeyCode::Down | KeyCode::Char('j') => (cursor + 1).min(last),
        KeyCode::Up | KeyCode::Char('k') => cursor.saturating_sub(1),
        KeyCode::PageDown => (cursor + 10).min(last),
        KeyCode::PageUp => cursor.saturating_sub(10),
        KeyCode::Home | KeyCode::Char('g') => 0,
        KeyCode::End | KeyCode::Char('G') => last,
        _ => return None,
    })
}

/// Rows whose text contains what was typed after `/`, ignoring case.
#[derive(Debug, Default)]
pub struct Filter {
    pub text: String,
    /// The field has the keys.
    pub editing: bool,
}

impl Filter {
    /// Handles the key if it belongs to the filter. Returns whether it did and the text changed
    /// or the field closed, so the view can put its cursor back on the first row.
    pub fn key(&mut self, key: KeyEvent) -> Option<bool> {
        if !self.editing {
            return match key.code {
                KeyCode::Char('/') => {
                    self.editing = true;
                    Some(false)
                }
                KeyCode::Esc if !self.text.is_empty() => {
                    self.text.clear();
                    Some(true)
                }
                _ => None,
            };
        }
        match key.code {
            KeyCode::Enter => self.editing = false,
            KeyCode::Esc => {
                self.editing = false;
                self.text.clear();
            }
            KeyCode::Backspace => {
                self.text.pop();
            }
            KeyCode::Char(c) => self.text.push(c),
            _ => return Some(false),
        }
        Some(true)
    }

    pub fn matches(&self, texts: &[&str]) -> bool {
        let wanted = self.text.trim().to_lowercase();
        wanted.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(&wanted))
    }

    /// The field, when open or in use.
    pub fn line(&self) -> Option<Line<'static>> {
        if !self.editing && self.text.is_empty() {
            return None;
        }
        let mut spans = vec![
            Span::styled("/", style::size()),
            Span::raw(self.text.clone()),
        ];
        if self.editing {
            spans.push(Span::styled("▏", style::size()));
        } else {
            spans.push(Span::styled(
                format!("  {}", tr("Esc clears")),
                style::muted(),
            ));
        }
        Some(Line::from(spans))
    }
}
