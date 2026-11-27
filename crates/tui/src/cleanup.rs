//! Cleanup: the categories of junk, picked one by one, cleaned after a confirmation.

use crate::app::Effect;
use crate::jobs::{Jobs, Purpose};
use crate::list;
use crate::style;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Row, Table, TableState, Wrap};
use rewind_os_engine::cleanup::{CategoryReport, CategoryResult, Disposal, Risk};
use rewind_os_engine::format::{bytes, count};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct State {
    pub reports: Option<Vec<CategoryReport>>,
    pub selected: BTreeSet<&'static str>,
    pub cursor: usize,
    /// The confirmation is open.
    pub confirming: bool,
    pub results: Option<Vec<CategoryResult>>,
    pub error: Option<String>,
    /// Why the last key did nothing (English key).
    pub note: Option<&'static str>,
    started: bool,
}

/// A category can be picked when it is on this computer and the app may clean it.
pub fn can_pick(r: &CategoryReport) -> bool {
    r.present && !r.blocked
}

impl State {
    /// The first time the view opens, the junk is measured.
    pub fn open(&mut self) -> Vec<Effect> {
        if std::mem::replace(&mut self.started, true) {
            return Vec::new();
        }
        self.analyze()
    }

    fn analyze(&mut self) -> Vec<Effect> {
        self.error = None;
        vec![Effect::Run(Purpose::Analyze, Request::CleanupAnalyze)]
    }

    fn defaults(&mut self) {
        if let Some(reports) = &self.reports {
            self.selected = reports
                .iter()
                .filter(|r| r.category.selected_by_default && can_pick(r))
                .map(|r| r.category.id)
                .collect();
        }
    }

    /// The picked categories that can be cleaned, and the space they take.
    pub fn chosen(&self) -> (Vec<&CategoryReport>, u64) {
        let chosen: Vec<&CategoryReport> = self
            .reports
            .iter()
            .flatten()
            .filter(|r| self.selected.contains(r.category.id) && can_pick(r))
            .collect();
        let size = chosen.iter().map(|r| r.bytes).sum();
        (chosen, size)
    }

    /// The confirmation takes every key while it is open.
    pub fn captures_keys(&self) -> bool {
        self.confirming
    }

    pub fn key(&mut self, key: KeyEvent, jobs: &Jobs) -> Vec<Effect> {
        if self.confirming {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.confirming = false;
                    let (chosen, _) = self.chosen();
                    let categories = chosen.iter().map(|r| r.category.id.to_owned()).collect();
                    return vec![Effect::Run(
                        Purpose::Clean,
                        Request::CleanupRun { categories },
                    )];
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.confirming = false,
                _ => {}
            }
            return Vec::new();
        }
        if jobs.running(Purpose::Clean).is_some() {
            return match key.code {
                KeyCode::Esc => vec![Effect::Stop(Purpose::Clean)],
                _ => Vec::new(),
            };
        }
        if jobs.running(Purpose::Analyze).is_some() {
            return Vec::new();
        }
        let last = self
            .reports
            .as_ref()
            .map_or(0, |r| r.len().saturating_sub(1));
        self.note = None;
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.cursor = (self.cursor + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Home | KeyCode::Char('g') => self.cursor = 0,
            KeyCode::End | KeyCode::Char('G') => self.cursor = last,
            KeyCode::Char(' ') => {
                if let Some(r) = self.reports.as_ref().and_then(|r| r.get(self.cursor))
                    && can_pick(r)
                    && !self.selected.remove(r.category.id)
                {
                    self.selected.insert(r.category.id);
                }
            }
            KeyCode::Char('a') => self.defaults(),
            KeyCode::Char('n') => self.selected.clear(),
            KeyCode::Char('r') => return self.analyze(),
            KeyCode::Enter | KeyCode::Char('c') => {
                if self.chosen().0.is_empty() {
                    self.note = Some("Pick at least one category with Space.");
                } else {
                    self.confirming = true;
                }
            }
            _ => {}
        }
        Vec::new()
    }

    pub fn finished(&mut self, purpose: Purpose, outcome: Result<Response, String>) -> Vec<Effect> {
        match (purpose, outcome) {
            (Purpose::Analyze, Ok(Response::CleanupAnalysis(mut reports))) => {
                // What can be cleaned first, largest first; then what cannot.
                reports.sort_by_key(|r| (!can_pick(r), r.bytes == 0, std::cmp::Reverse(r.bytes)));
                let first = self.reports.is_none();
                self.cursor = self.cursor.min(reports.len().saturating_sub(1));
                self.reports = Some(reports);
                if first {
                    self.defaults();
                } else if let Some(reports) = &self.reports {
                    self.selected
                        .retain(|id| reports.iter().any(|r| r.category.id == *id && can_pick(r)));
                }
                Vec::new()
            }
            (Purpose::Clean, Ok(Response::CleanupDone(results))) => {
                self.results = Some(results);
                // Measured again, so the sizes show what is left.
                self.analyze()
            }
            (_, Err(err)) => {
                self.error = Some(err);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub fn hints(&self, jobs: &Jobs) -> Vec<(&'static str, &'static str)> {
        if self.confirming {
            return vec![("y", tr("Clean")), ("n", tr("Cancel"))];
        }
        if jobs.running(Purpose::Clean).is_some() {
            return vec![("Esc", tr("Stop"))];
        }
        vec![
            ("↑↓", tr("Move")),
            ("Space", tr("Pick")),
            ("a", tr("Suggested")),
            ("n", tr("None")),
            ("Enter", tr("Clean")),
            ("r", tr("Analyze again")),
        ]
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, jobs: &Jobs) {
        let results = self
            .results
            .as_ref()
            .map(|r| result_lines(r))
            .unwrap_or_default();
        let [summary, list, detail, done] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(4),
            Constraint::Length(if results.is_empty() {
                0
            } else {
                u16::try_from(results.len())
                    .unwrap_or(6)
                    .saturating_add(2)
                    .min(10)
            }),
        ])
        .areas(area);

        let running = jobs
            .running(Purpose::Clean)
            .map(|r| (r, tr("Cleaning")))
            .or_else(|| jobs.running(Purpose::Analyze).map(|r| (r, tr("Analyzing"))));
        let top = if let Some((r, what)) = &running {
            let stage = if r.progress.stage.is_empty() {
                what
            } else {
                &tr(r.progress.stage)
            };
            let mut spans = vec![
                Span::styled(stage.to_string(), style::heading()),
                Span::raw(" · "),
                Span::raw(trf("{n} files", &[("n", &count(r.progress.items))])),
                Span::raw(" · "),
                Span::styled(bytes(r.progress.bytes), style::size()),
            ];
            if r.stopping {
                spans.push(Span::styled(
                    format!("  {}", tr("Stopping…")),
                    style::warn(),
                ));
            }
            Line::from(spans)
        } else if let Some(err) = &self.error {
            Line::styled(err.clone(), style::error())
        } else {
            let found: u64 = self
                .reports
                .iter()
                .flatten()
                .filter(|r| can_pick(r))
                .map(|r| r.bytes)
                .sum();
            let (chosen, size) = self.chosen();
            Line::from(vec![
                Span::raw(tr("Junk found")),
                Span::raw(" "),
                Span::styled(bytes(found), style::size()),
                Span::raw(" · "),
                Span::raw(trf("{n} categories", &[("n", &chosen.len().to_string())])),
                Span::raw(" "),
                Span::styled(bytes(size), style::size()),
            ])
        };
        let mut top_lines = vec![top];
        if let Some(note) = self.note {
            top_lines.push(Line::styled(tr(note), style::warn()));
        }
        if self.reports.iter().flatten().any(|r| r.blocked) {
            top_lines.push(Line::styled(
                tr("Some categories need administrator rights."),
                style::warn(),
            ));
        }
        frame.render_widget(Paragraph::new(top_lines), summary);

        let rows = self.reports.iter().flatten().map(|r| {
            let mark = if !can_pick(r) {
                "[-]"
            } else if self.selected.contains(r.category.id) {
                "[x]"
            } else {
                "[ ]"
            };
            let (risk, risk_style) = match r.category.risk {
                Risk::Safe => (tr("Safe"), style::good()),
                Risk::Moderate => (tr("Rebuilt later"), style::warn()),
                Risk::Review => (tr("Review"), style::error()),
            };
            let mut tags = vec![Span::styled(risk, risk_style)];
            if r.blocked {
                tags.push(Span::raw(" · "));
                tags.push(Span::styled(tr("Needs administrator"), style::warn()));
            } else if !r.present {
                tags.push(Span::raw(" · "));
                tags.push(Span::styled(tr("Not on this computer"), style::muted()));
            }
            let name_style = if can_pick(r) {
                ratatui::style::Style::new()
            } else {
                style::muted()
            };
            Row::new(vec![
                Line::from(mark),
                Line::styled(tr(r.category.name), name_style),
                Line::styled(format!("{:>10}", bytes(r.bytes)), style::size()),
                Line::styled(trf("{n} files", &[("n", &count(r.files))]), style::muted()),
                Line::from(tags),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(3),
                Constraint::Fill(1),
                Constraint::Length(10),
                Constraint::Length(18),
                Constraint::Length(36),
            ],
        )
        .row_highlight_style(style::selected())
        .block(Block::bordered().title(Span::styled(tr("Junk cleanup"), style::heading())));
        let mut state = TableState::default().with_selected(Some(self.cursor));
        list::render(frame, table, list, &mut state, 0);

        if let Some(r) = self.reports.as_ref().and_then(|r| r.get(self.cursor)) {
            let mut text = vec![Line::from(tr(r.category.description))];
            if let Some(note) = &r.note {
                text.push(Line::styled(note.clone(), style::warn()));
            }
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: true }).block(
                    Block::bordered().title(Span::styled(tr(r.category.name), style::heading())),
                ),
                detail,
            );
        }
        if !results.is_empty() {
            frame.render_widget(
                Paragraph::new(results).wrap(Wrap { trim: true }).block(
                    Block::bordered().title(Span::styled(tr("Last cleanup"), style::heading())),
                ),
                done,
            );
        }
        if self.confirming {
            self.draw_confirm(frame, area);
        }
    }

    fn draw_confirm(&self, frame: &mut Frame, area: Rect) {
        let (chosen, size) = self.chosen();
        let to_bin = chosen
            .iter()
            .any(|r| r.category.disposal == Disposal::RecycleBin);
        let mut body = trf(
            "{n} categories, about {size}, will be deleted permanently.",
            &[("n", &chosen.len().to_string()), ("size", &bytes(size))],
        );
        if to_bin {
            body.push(' ');
            body.push_str(tr("Old installers go to the recycle bin instead."));
        }
        body.push(' ');
        body.push_str(tr("Files a program has open are skipped."));
        let lines = vec![
            Line::from(body),
            Line::raw(""),
            Line::from(vec![
                Span::styled("[y] ", style::size()),
                Span::styled(tr("Clean"), style::heading()),
                Span::raw("    "),
                Span::styled("[n] ", style::muted()),
                Span::raw(tr("Cancel")),
            ]),
        ];
        let [middle] = Layout::vertical([Constraint::Length(8)])
            .flex(Flex::Center)
            .areas(area);
        let [dialog] = Layout::horizontal([Constraint::Max(64)])
            .flex(Flex::Center)
            .areas(middle);
        frame.render_widget(Clear, dialog);
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: true }).block(
                Block::bordered()
                    .border_style(style::warn())
                    .title(Span::styled(
                        tr("Clean the selected categories?"),
                        style::heading(),
                    )),
            ),
            dialog,
        );
    }
}

/// What the last cleanup freed, kept and skipped, category by category.
fn result_lines(results: &[CategoryResult]) -> Vec<Line<'static>> {
    let freed: u64 = results.iter().map(|r| r.bytes).sum();
    let kept: u64 = results.iter().map(|r| r.failed).sum();
    let mut first = vec![Span::styled(
        trf("Freed {size}.", &[("size", &bytes(freed))]),
        style::good(),
    )];
    if kept > 0 {
        first.push(Span::raw(" "));
        first.push(Span::styled(
            trf("{n} files in use were kept", &[("n", &count(kept))]),
            style::warn(),
        ));
    }
    let mut lines = vec![Line::from(first)];
    for r in results {
        let mut notes = Vec::new();
        if r.failed > 0 {
            notes.push(trf(
                "{n} files in use were kept",
                &[("n", &count(r.failed))],
            ));
        }
        if let Some(reason) = r.skipped {
            notes.push(tr(reason).to_owned());
        }
        if let Some(err) = &r.error {
            notes.push(format!("{}: {err}", tr("Error")));
        }
        if notes.is_empty() {
            continue;
        }
        let note_style = if r.error.is_some() {
            style::error()
        } else {
            style::warn()
        };
        lines.push(Line::from(vec![
            Span::raw(format!("{}: ", r.id)),
            Span::styled(notes.join(" · "), note_style),
        ]));
    }
    lines
}
