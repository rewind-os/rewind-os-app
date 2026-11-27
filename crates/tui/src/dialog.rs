//! What several views share: the confirmation box and the folder field.

use crate::style;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use rewind_os_i18n::tr;
use std::path::PathBuf;

/// A question in the middle of `area`, answered with `y` (doing `yes`) or `n`.
pub fn confirm(frame: &mut Frame, area: Rect, title: &str, body: Vec<Line<'static>>, yes: &str) {
    let mut lines = body;
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("[y] ", style::size()),
        Span::styled(yes.to_owned(), style::heading()),
        Span::raw("    "),
        Span::styled("[n] ", style::muted()),
        Span::raw(tr("Cancel")),
    ]));
    // Every line as many rows as it wraps to, inside a border, with a little room spare as
    // wrapping breaks at words.
    let inner = usize::from(area.width.min(70).saturating_sub(4)).max(1);
    let rows: usize = lines.iter().map(|l| l.width().div_ceil(inner).max(1)).sum();
    let height = u16::try_from(rows)
        .unwrap_or(area.height)
        .saturating_add(3)
        .min(area.height);
    let [middle] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [dialog] = Layout::horizontal([Constraint::Max(70)])
        .flex(Flex::Center)
        .areas(middle);
    frame.render_widget(Clear, dialog);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::bordered()
                .border_style(style::warn())
                .title(Span::styled(title.to_owned(), style::heading())),
        ),
        dialog,
    );
}

/// The answer to a confirmation key, if it is one.
pub fn answer(key: KeyEvent) -> Option<bool> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(true),
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(false),
        _ => None,
    }
}

/// A folder typed after `e`.
#[derive(Debug)]
pub struct FolderField {
    pub text: String,
    pub editing: bool,
}

impl FolderField {
    pub fn new(folder: PathBuf) -> Self {
        FolderField {
            text: folder.display().to_string(),
            editing: false,
        }
    }

    pub fn path(&self) -> PathBuf {
        PathBuf::from(self.text.trim())
    }

    /// Handles the key while the field is open. `Some(true)` means Enter: search now.
    pub fn key(&mut self, key: KeyEvent) -> Option<bool> {
        if !self.editing {
            return None;
        }
        match key.code {
            KeyCode::Enter => {
                self.editing = false;
                return Some(true);
            }
            KeyCode::Esc => self.editing = false,
            KeyCode::Backspace => {
                self.text.pop();
            }
            KeyCode::Char(c) => self.text.push(c),
            _ => {}
        }
        Some(false)
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, title: &str) {
        let field_style = if self.editing {
            style::active()
        } else {
            ratatui::style::Style::new()
        };
        let mut spans = vec![Span::styled(self.text.clone(), field_style)];
        if self.editing {
            spans.push(Span::styled("▏", style::size()));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .block(Block::bordered().title(Span::styled(title.to_owned(), style::heading()))),
            area,
        );
    }
}

/// A running search or removal as one line: what it does, what it counted and the time taken.
pub fn progress(running: &crate::jobs::Running, what: &'static str) -> Line<'static> {
    use rewind_os_engine::format::{bytes, count, duration_ms};
    let p = &running.progress;
    let elapsed = u64::try_from(running.elapsed.as_millis()).unwrap_or(u64::MAX);
    let mut spans = vec![
        Span::styled(
            tr(if p.stage.is_empty() { what } else { p.stage }),
            style::heading(),
        ),
        Span::raw(" · "),
        Span::raw(rewind_os_i18n::trf("{n} items", &[("n", &count(p.items))])),
        Span::raw(" · "),
        Span::styled(bytes(p.bytes), style::size()),
        Span::raw(" · "),
        Span::styled(duration_ms(elapsed - elapsed % 1000), style::muted()),
    ];
    if running.stopping {
        spans.push(Span::styled(
            format!("  {}", tr("Stopping…")),
            style::warn(),
        ));
    }
    Line::from(spans)
}

/// `path` from `root` on, when it is inside it, so long paths fit.
pub fn under(path: &std::path::Path, root: &std::path::Path) -> String {
    match path.strip_prefix(root) {
        Ok(rest) if !rest.as_os_str().is_empty() => rest.display().to_string(),
        _ => path.display().to_string(),
    }
}
