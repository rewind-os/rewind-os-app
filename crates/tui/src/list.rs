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

/// Where the last list was drawn, so a click can tell which row it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rows {
    pub area: ratatui::layout::Rect,
    /// The first row shown (the list scrolls to keep the cursor in view).
    pub offset: usize,
    /// Header lines above the rows, inside the border.
    pub header: u16,
}

impl Rows {
    /// The row at a point, counted from the first row of the list.
    pub fn row_at(&self, column: u16, line: u16) -> Option<usize> {
        let top = self.area.y.saturating_add(1).saturating_add(self.header);
        let bottom = self.area.bottom().saturating_sub(1);
        let inside = column > self.area.x
            && column < self.area.right().saturating_sub(1)
            && line >= top
            && line < bottom;
        inside.then(|| self.offset + usize::from(line - top))
    }
}

thread_local! {
    static ROWS: std::cell::Cell<Option<Rows>> = const { std::cell::Cell::new(None) };
}

/// Draws a bordered list and remembers where, for the mouse.
pub fn render(
    frame: &mut ratatui::Frame,
    table: ratatui::widgets::Table,
    area: ratatui::layout::Rect,
    state: &mut ratatui::widgets::TableState,
    header: u16,
) {
    frame.render_stateful_widget(table, area, state);
    let rows = Rows {
        area,
        offset: state.offset(),
        header,
    };
    ROWS.with(|r| r.set(Some(rows)));
}

/// The list drawn in the last frame, if any.
pub fn last_rows() -> Option<Rows> {
    ROWS.with(std::cell::Cell::get)
}

/// Forgets the last list; called before each frame.
pub fn forget_rows() {
    ROWS.with(|r| r.set(None));
}
