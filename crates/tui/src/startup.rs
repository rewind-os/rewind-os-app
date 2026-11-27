//! Startup programs: what starts with the system, each turned off and on with Space. Turning
//! one off never deletes it, so it acts at once, as in the app.

use crate::app::Effect;
use crate::jobs::{Jobs, Purpose};
use crate::list::{self, Filter};
use crate::style;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState, Wrap};
use rewind_os_engine::startup::{ESSENTIAL, StartupEntry};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};

#[derive(Default)]
pub struct State {
    pub entries: Option<Vec<StartupEntry>>,
    /// Kinds of entries the system did not let the app read, with why (English).
    pub not_shown: Vec<&'static str>,
    pub cursor: usize,
    pub filter: Filter,
    /// What the last change did, or why a key did nothing, and whether it is an error.
    pub note: Option<(String, bool)>,
    /// The app runs with administrator rights.
    pub elevated: bool,
    started: bool,
}

impl State {
    pub fn open(&mut self) -> Vec<Effect> {
        if std::mem::replace(&mut self.started, true) {
            return Vec::new();
        }
        self.elevated = rewind_os_engine::system::is_elevated();
        vec![Effect::Run(Purpose::Startup, Request::StartupList)]
    }

    /// The entries the filter lets through, in order.
    pub fn shown(&self) -> Vec<&StartupEntry> {
        self.entries
            .iter()
            .flatten()
            .filter(|e| {
                self.filter
                    .matches(&[&e.name, &e.command, tr(e.source.label())])
            })
            .collect()
    }

    pub fn captures_keys(&self) -> bool {
        self.filter.editing
    }

    pub fn key(&mut self, key: KeyEvent, jobs: &Jobs) -> Vec<Effect> {
        if let Some(changed) = self.filter.key(key) {
            if changed {
                self.cursor = 0;
            }
            return Vec::new();
        }
        if jobs.running(Purpose::Startup).is_some() || jobs.running(Purpose::StartupSet).is_some() {
            return Vec::new();
        }
        if let Some(cursor) = list::moved(key.code, self.cursor, self.shown().len()) {
            self.cursor = cursor;
            return Vec::new();
        }
        match key.code {
            KeyCode::Char('r') => {
                self.note = None;
                vec![Effect::Run(Purpose::Startup, Request::StartupList)]
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                let Some(entry) = self.shown().get(self.cursor).map(|e| (*e).clone()) else {
                    return Vec::new();
                };
                if entry.enabled && entry.essential {
                    self.note = Some((tr(ESSENTIAL).to_owned(), true));
                    return Vec::new();
                }
                if entry.needs_admin && !self.elevated {
                    self.note = Some((tr("This needs administrator rights.").to_owned(), true));
                    return Vec::new();
                }
                self.note = None;
                vec![Effect::Run(
                    Purpose::StartupSet,
                    Request::StartupSet {
                        id: entry.id,
                        enabled: !entry.enabled,
                    },
                )]
            }
            _ => Vec::new(),
        }
    }

    pub fn finished(&mut self, purpose: Purpose, outcome: Result<Response, String>) -> Vec<Effect> {
        match (purpose, outcome) {
            (Purpose::Startup, Ok(Response::Startup(report))) => {
                self.cursor = self.cursor.min(report.entries.len().saturating_sub(1));
                self.entries = Some(report.entries);
                self.not_shown = report.not_shown;
                Vec::new()
            }
            (Purpose::StartupSet, Ok(Response::Done(message))) => {
                self.note = Some((tr(message).to_owned(), false));
                vec![Effect::Run(Purpose::Startup, Request::StartupList)]
            }
            (Purpose::StartupSet, Err(err)) => {
                self.note = Some((err, true));
                vec![Effect::Run(Purpose::Startup, Request::StartupList)]
            }
            (_, Err(err)) => {
                self.note = Some((err, true));
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.filter.editing {
            return vec![("Enter", tr("Done")), ("Esc", tr("Clear"))];
        }
        vec![
            ("↑↓", tr("Move")),
            ("Space", tr("Turn on or off")),
            ("/", tr("Filter")),
            ("r", tr("Refresh")),
        ]
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, jobs: &Jobs) {
        let shown = self.shown();
        let mut top = Vec::new();
        match &self.entries {
            Some(entries) => {
                let on = entries.iter().filter(|e| e.enabled).count();
                top.push(Line::from(vec![
                    Span::styled(
                        trf("{n} start with the system", &[("n", &on.to_string())]),
                        style::heading(),
                    ),
                    Span::styled(
                        format!(
                            " · {}",
                            trf(
                                "{n} turned off",
                                &[("n", &(entries.len() - on).to_string())]
                            )
                        ),
                        style::muted(),
                    ),
                ]));
            }
            None if jobs.running(Purpose::Startup).is_some() => {
                top.push(Line::styled(tr("Reading what starts…"), style::muted()));
            }
            None => {}
        }
        for why in &self.not_shown {
            top.push(Line::styled(tr(why), style::warn()));
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
        let [summary, body] = Layout::vertical([
            Constraint::Length(u16::try_from(top.len()).unwrap_or(4).max(1)),
            Constraint::Min(3),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(top).wrap(Wrap { trim: true }), summary);

        let [list_area, detail] =
            Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
                .areas(body);
        let rows = shown.iter().map(|e| {
            let (word, word_style) = state(e);
            Row::new(vec![
                Line::styled(word, word_style),
                Line::from(e.name.clone()),
                Line::styled(tr(e.source.label()), style::muted()),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(11),
                Constraint::Fill(3),
                Constraint::Fill(2),
            ],
        )
        .row_highlight_style(style::selected())
        .block(Block::bordered().title(Span::styled(tr("Startup programs"), style::heading())));
        let mut table_state = TableState::default().with_selected(Some(self.cursor));
        frame.render_stateful_widget(table, list_area, &mut table_state);

        let Some(entry) = shown.get(self.cursor) else {
            frame.render_widget(Block::bordered(), detail);
            return;
        };
        let (word, word_style) = state(entry);
        let mut lines = vec![
            Line::from(vec![
                Span::styled(format!("{}: ", tr("Status")), style::muted()),
                Span::styled(word, word_style),
            ]),
            field(tr("When it runs"), tr(entry.when.unwrap_or("At sign-in"))),
            field(tr("Registered in"), tr(entry.source.label())),
            Line::styled(format!("{}:", tr("Command")), style::muted()),
            Line::from(entry.command.clone()),
        ];
        if entry.essential {
            lines.push(Line::raw(""));
            lines.push(Line::styled(tr("Needed by the system"), style::warn()));
            lines.push(Line::styled(tr(ESSENTIAL), style::muted()));
        } else if entry.needs_admin && !self.elevated {
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                tr("This needs administrator rights."),
                style::warn(),
            ));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(Span::styled(entry.name.clone(), style::heading()))),
            detail,
        );
    }
}

/// Whether an entry starts, as a word (the colour only repeats it).
fn state(entry: &StartupEntry) -> (&'static str, ratatui::style::Style) {
    if entry.enabled {
        (tr("On"), style::good())
    } else {
        (tr("Off"), style::muted())
    }
}

pub fn field(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), style::muted()),
        Span::raw(value.to_owned()),
    ])
}
