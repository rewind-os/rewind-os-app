//! Program leftovers: folders that no installed program, service or startup entry refers to,
//! picked one by one and moved to the recycle bin after a confirmation.

use crate::app::Effect;
use crate::dialog;
use crate::duplicates::trashed;
use crate::jobs::{Jobs, Purpose};
use crate::list::{self, Filter};
use crate::style;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState, Wrap};
use rewind_os_engine::format::{bytes, count, local_date};
use rewind_os_engine::leftovers::{Confidence, Leftover};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Default)]
pub struct State {
    pub found: Option<Vec<Leftover>>,
    pub selected: HashSet<PathBuf>,
    pub moved: HashSet<PathBuf>,
    /// Only folders at least this likely to be unused are shown.
    pub min_confidence: Option<Confidence>,
    pub filter: Filter,
    pub cursor: usize,
    pub confirming: bool,
    pub note: Option<(String, bool)>,
    started: bool,
}

/// How likely a folder is unused, as people read it.
pub fn confidence_word(c: Confidence) -> (&'static str, ratatui::style::Style) {
    match c {
        Confidence::High => (tr("Likely unused"), style::good()),
        Confidence::Medium => (tr("Possibly unused"), style::warn()),
        Confidence::Low => (tr("Recently used"), style::muted()),
    }
}

fn rank(c: Confidence) -> u8 {
    match c {
        Confidence::Low => 0,
        Confidence::Medium => 1,
        Confidence::High => 2,
    }
}

impl State {
    pub fn open(&mut self) -> Vec<Effect> {
        if std::mem::replace(&mut self.started, true) {
            return Vec::new();
        }
        self.search()
    }

    fn search(&mut self) -> Vec<Effect> {
        self.note = None;
        vec![Effect::Run(Purpose::Leftovers, Request::LeftoversFind)]
    }

    /// The folders shown: not moved, as likely unused as asked, matching the filter.
    pub fn shown(&self) -> Vec<&Leftover> {
        self.found
            .iter()
            .flatten()
            .filter(|l| !self.moved.contains(&l.path))
            .filter(|l| {
                self.min_confidence
                    .is_none_or(|min| rank(l.confidence) >= rank(min))
            })
            .filter(|l| self.filter.matches(&[&l.path.to_string_lossy()]))
            .collect()
    }

    pub fn chosen(&self) -> (Vec<PathBuf>, u64) {
        let chosen: Vec<&Leftover> = self
            .shown()
            .into_iter()
            .filter(|l| self.selected.contains(&l.path))
            .collect();
        let size = chosen.iter().map(|l| l.bytes).sum();
        (chosen.iter().map(|l| l.path.clone()).collect(), size)
    }

    pub fn captures_keys(&self) -> bool {
        self.filter.editing || self.confirming
    }

    pub fn key(&mut self, key: KeyEvent, jobs: &Jobs) -> Vec<Effect> {
        if self.confirming {
            return match dialog::answer(key) {
                Some(true) => {
                    self.confirming = false;
                    let (paths, _) = self.chosen();
                    vec![Effect::Run(
                        Purpose::LeftoversTrash,
                        Request::FilesTrash { paths },
                    )]
                }
                Some(false) => {
                    self.confirming = false;
                    Vec::new()
                }
                None => Vec::new(),
            };
        }
        if let Some(changed) = self.filter.key(key) {
            if changed {
                self.cursor = 0;
            }
            return Vec::new();
        }
        if jobs.running(Purpose::Leftovers).is_some() {
            return match key.code {
                KeyCode::Esc => vec![Effect::Stop(Purpose::Leftovers)],
                _ => Vec::new(),
            };
        }
        if jobs.running(Purpose::LeftoversTrash).is_some() {
            return Vec::new();
        }
        let len = self.shown().len();
        if let Some(cursor) = list::moved(key.code, self.cursor, len) {
            self.cursor = cursor;
            return Vec::new();
        }
        self.note = None;
        match key.code {
            KeyCode::Char('r') => return self.search(),
            KeyCode::Char('c') => {
                self.min_confidence = match self.min_confidence {
                    None => Some(Confidence::Medium),
                    Some(Confidence::Medium) => Some(Confidence::High),
                    Some(_) => None,
                };
                self.cursor = 0;
            }
            KeyCode::Char(' ') => {
                if let Some(path) = self.shown().get(self.cursor).map(|l| l.path.clone())
                    && !self.selected.remove(&path)
                {
                    self.selected.insert(path);
                }
            }
            KeyCode::Char('a') => {
                self.selected = self
                    .shown()
                    .iter()
                    .filter(|l| l.confidence == Confidence::High)
                    .map(|l| l.path.clone())
                    .collect();
            }
            KeyCode::Char('n') => self.selected.clear(),
            KeyCode::Enter => {
                if self.chosen().0.is_empty() {
                    self.note = Some((tr("Pick at least one folder with Space.").to_owned(), true));
                } else {
                    self.confirming = true;
                }
            }
            _ => {}
        }
        Vec::new()
    }

    pub fn finished(&mut self, purpose: Purpose, outcome: Result<Response, String>) {
        match (purpose, outcome) {
            (Purpose::Leftovers, Ok(Response::Leftovers(found))) => {
                self.found = Some(found);
                self.selected.clear();
                self.moved.clear();
                self.cursor = 0;
            }
            (Purpose::LeftoversTrash, Ok(Response::Trashed(results))) => {
                let (note, moved) = trashed(results);
                self.moved.extend(moved);
                self.selected.retain(|p| !self.moved.contains(p));
                self.cursor = self.cursor.min(self.shown().len().saturating_sub(1));
                self.note = Some(note);
            }
            (_, Err(err)) => self.note = Some((err, true)),
            _ => {}
        }
    }

    pub fn hints(&self, jobs: &Jobs) -> Vec<(&'static str, &'static str)> {
        if self.confirming {
            return vec![("y", tr("Move to recycle bin")), ("n", tr("Cancel"))];
        }
        if self.filter.editing {
            return vec![("Enter", tr("Done")), ("Esc", tr("Clear"))];
        }
        if jobs.running(Purpose::Leftovers).is_some() {
            return vec![("Esc", tr("Stop"))];
        }
        vec![
            ("Space", tr("Pick")),
            ("a", tr("Likely unused")),
            ("Enter", tr("Move to recycle bin")),
            ("c", tr("Show")),
            ("/", tr("Filter")),
            ("r", tr("Search again")),
        ]
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, jobs: &Jobs) {
        if let Some(running) = jobs.running(Purpose::Leftovers) {
            frame.render_widget(
                Paragraph::new(dialog::progress(&running, "Searching")).block(Block::bordered()),
                area,
            );
            return;
        }
        let shown = self.shown();
        let (chosen, chosen_size) = self.chosen();
        let total: u64 = shown.iter().map(|l| l.bytes).sum();
        let showing = match self.min_confidence {
            None => tr("Show all"),
            Some(Confidence::Medium) => tr("Possibly unused"),
            Some(_) => tr("Likely unused"),
        };
        let mut top = Vec::new();
        if self.found.is_some() {
            top.push(Line::from(vec![
                Span::raw(tr("Leftover folders")),
                Span::raw(" "),
                Span::styled(bytes(total), style::size()),
                Span::styled(
                    format!(
                        " · {} · {}: {showing}",
                        trf("{n} folders", &[("n", &count(shown.len() as u64))]),
                        tr("Show")
                    ),
                    style::muted(),
                ),
                Span::raw(" · "),
                Span::raw(trf("{n} picked", &[("n", &count(chosen.len() as u64))])),
                Span::raw(" "),
                Span::styled(bytes(chosen_size), style::size()),
            ]));
        }
        if let Some((text, error)) = &self.note {
            top.push(Line::styled(
                text.clone(),
                if *error {
                    style::error()
                } else {
                    style::good()
                },
            ));
        }
        if let Some(line) = self.filter.line() {
            top.push(line);
        }
        let [summary, list_area] = Layout::vertical([
            Constraint::Length(u16::try_from(top.len()).unwrap_or(3)),
            Constraint::Min(3),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(top).wrap(Wrap { trim: true }), summary);
        if self.found.is_some() && shown.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    tr("No leftover folders found."),
                    style::good(),
                ))
                .block(Block::bordered()),
                list_area,
            );
        } else {
            let rows = shown.iter().map(|l| {
                let mark = if self.selected.contains(&l.path) {
                    "[x]"
                } else {
                    "[ ]"
                };
                let (word, word_style) = confidence_word(l.confidence);
                Row::new(vec![
                    Line::from(mark),
                    Line::styled(format!("{:>10}", bytes(l.bytes)), style::size()),
                    Line::from(l.path.display().to_string()),
                    Line::styled(word, word_style),
                    Line::styled(
                        l.last_change.map(local_date).unwrap_or_default(),
                        style::muted(),
                    ),
                ])
            });
            let table = Table::new(
                rows,
                [
                    Constraint::Length(3),
                    Constraint::Length(10),
                    Constraint::Fill(1),
                    Constraint::Length(18),
                    Constraint::Length(12),
                ],
            )
            .row_highlight_style(style::selected())
            .block(
                Block::bordered().title(Span::styled(tr("Program leftovers"), style::heading())),
            );
            let mut state = TableState::default().with_selected(Some(self.cursor));
            frame.render_stateful_widget(table, list_area, &mut state);
        }
        if self.confirming {
            let body = vec![Line::from(trf(
                "{n} items ({size}) will be moved to the recycle bin. You can restore them from there.",
                &[
                    ("n", &chosen.len().to_string()),
                    ("size", &bytes(chosen_size)),
                ],
            ))];
            dialog::confirm(
                frame,
                area,
                tr("Move to the recycle bin?"),
                body,
                tr("Move to recycle bin"),
            );
        }
    }
}
