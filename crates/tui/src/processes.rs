//! Running programs: what uses the processor and memory, closed after a confirmation, and those
//! that start with the system turned off at startup.

use crate::app::Effect;
use crate::dialog;
use crate::jobs::{Jobs, Purpose};
use crate::list::{self, Filter};
use crate::startup::field;
use crate::style;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState, Wrap};
use rewind_os_engine::format::{bytes, count};
use rewind_os_engine::processes::{Closed, Program, Report};
use rewind_os_engine::{Request, Response, safety};
use rewind_os_i18n::{tr, trf};

#[derive(Default)]
pub struct State {
    pub report: Option<Report>,
    pub by_memory: bool,
    pub filter: Filter,
    pub cursor: usize,
    /// The program to close, and whether to force it, waiting for an answer.
    pub confirming: Option<(Program, bool)>,
    pub note: Option<(String, bool)>,
    /// When the list was last measured, to measure it again every few seconds.
    measured: Option<std::time::Instant>,
    started: bool,
}

/// How often the open list is measured again.
const REFRESH: std::time::Duration = std::time::Duration::from_secs(3);

/// Why a program is not closed, in a word.
fn refusal_word(why: &str) -> &'static str {
    if why == safety::OTHER_ACCOUNT {
        tr("Other account")
    } else if why == safety::THIS_APP || why == safety::HOLDS_APP {
        "Rewind OS"
    } else {
        tr("System")
    }
}

impl State {
    pub fn open(&mut self) -> Vec<Effect> {
        if std::mem::replace(&mut self.started, true) {
            return Vec::new();
        }
        self.measure()
    }

    fn measure(&mut self) -> Vec<Effect> {
        vec![Effect::Run(Purpose::Processes, Request::ProcessesList)]
    }

    /// The programs the filter lets through, heaviest first.
    pub fn shown(&self) -> Vec<&Program> {
        let mut shown: Vec<&Program> = self
            .report
            .iter()
            .flat_map(|r| r.programs.iter())
            .filter(|p| self.filter.matches(&[&p.name]))
            .collect();
        if self.by_memory {
            shown.sort_by_key(|p| std::cmp::Reverse(p.memory));
        }
        shown
    }

    pub fn captures_keys(&self) -> bool {
        self.filter.editing || self.confirming.is_some()
    }

    /// Measures the list again when it is older than a few seconds and nothing else runs.
    pub fn tick(&mut self, jobs: &Jobs) -> Vec<Effect> {
        let busy = [Purpose::Processes, Purpose::Close, Purpose::StartupOff]
            .iter()
            .any(|p| jobs.running(*p).is_some());
        let stale = self.measured.is_some_and(|at| at.elapsed() >= REFRESH);
        if busy || !stale || self.confirming.is_some() {
            return Vec::new();
        }
        self.measured = None;
        self.measure()
    }

    pub fn key(&mut self, key: KeyEvent, jobs: &Jobs) -> Vec<Effect> {
        if let Some((program, force)) = &self.confirming {
            return match dialog::answer(key) {
                Some(true) => {
                    let request = Request::ProcessesClose {
                        name: program.name.clone(),
                        pids: program.pids.clone(),
                        force: *force,
                    };
                    self.confirming = None;
                    vec![Effect::Run(Purpose::Close, request)]
                }
                Some(false) => {
                    self.confirming = None;
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
        // Measuring again does not stop the keys: the list stays usable while it refreshes.
        if [Purpose::Close, Purpose::StartupOff]
            .iter()
            .any(|p| jobs.running(*p).is_some())
        {
            return Vec::new();
        }
        if let Some(cursor) = list::moved(key.code, self.cursor, self.shown().len()) {
            self.cursor = cursor;
            return Vec::new();
        }
        let program = self.shown().get(self.cursor).map(|p| (*p).clone());
        match key.code {
            KeyCode::Char('r') => {
                self.note = None;
                return self.measure();
            }
            KeyCode::Char('s') => {
                self.by_memory = !self.by_memory;
                self.cursor = 0;
            }
            KeyCode::Char('x') | KeyCode::Char('F') | KeyCode::Delete => {
                let Some(program) = program else {
                    return Vec::new();
                };
                if let Some(why) = program.refusal {
                    self.note = Some((tr(why).to_owned(), true));
                } else {
                    self.note = None;
                    self.confirming = Some((program, key.code == KeyCode::Char('F')));
                }
            }
            KeyCode::Char('t') => match program.and_then(|p| p.startup) {
                Some(id) => {
                    self.note = None;
                    return vec![Effect::Run(
                        Purpose::StartupOff,
                        Request::StartupSet { id, enabled: false },
                    )];
                }
                None => {
                    self.note = Some((tr("It does not start with the system.").to_owned(), true));
                }
            },
            _ => {}
        }
        Vec::new()
    }

    pub fn finished(&mut self, purpose: Purpose, outcome: Result<Response, String>) -> Vec<Effect> {
        match (purpose, outcome) {
            (Purpose::Processes, Ok(Response::Processes(report))) => {
                // The order changes as programs work: the cursor stays on the same program.
                let selected = self
                    .shown()
                    .get(self.cursor)
                    .map(|p| (p.name.clone(), p.pids.clone()));
                self.report = Some(report);
                self.measured = Some(std::time::Instant::now());
                let shown = self.shown();
                self.cursor = selected
                    .and_then(|(name, pids)| {
                        shown
                            .iter()
                            .position(|p| p.name == name && p.pids == pids)
                            .or_else(|| shown.iter().position(|p| p.name == name))
                    })
                    .unwrap_or(self.cursor)
                    .min(shown.len().saturating_sub(1));
                Vec::new()
            }
            (Purpose::Close, Ok(Response::ProcessClosed(closed))) => {
                self.note = Some(closed_note(&closed));
                self.measure()
            }
            (Purpose::StartupOff, Ok(Response::Done(message))) => {
                self.note = Some((tr(message).to_owned(), false));
                self.measure()
            }
            (_, Err(err)) => {
                self.note = Some((err, true));
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub fn hints(&self, jobs: &Jobs) -> Vec<(&'static str, &'static str)> {
        if self.confirming.is_some() {
            return vec![("y", tr("Close")), ("n", tr("Cancel"))];
        }
        if self.filter.editing {
            return vec![("Enter", tr("Done")), ("Esc", tr("Clear"))];
        }
        if jobs.running(Purpose::Processes).is_some() {
            return Vec::new();
        }
        vec![
            ("x", tr("Close")),
            ("F", tr("Force close")),
            ("t", tr("Turn off at startup")),
            ("s", tr("Sort by")),
            ("/", tr("Filter")),
            ("r", tr("Measure again")),
        ]
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, jobs: &Jobs) {
        let mut top = Vec::new();
        match &self.report {
            Some(r) => top.push(Line::from(vec![
                Span::raw(format!("{} ", tr("Processor in use"))),
                Span::styled(format!("{:.0} %", r.cpu), style::size()),
                Span::raw(format!(" · {} ", tr("Memory in use"))),
                Span::styled(
                    format!("{} / {}", bytes(r.memory_used), bytes(r.memory_total)),
                    style::size(),
                ),
                Span::styled(
                    format!(
                        " · {}",
                        trf("{n} processes", &[("n", &count(r.processes as u64))])
                    ),
                    style::muted(),
                ),
                Span::styled(
                    format!(
                        " · {}: {}",
                        tr("Sort by"),
                        if self.by_memory {
                            tr("Memory")
                        } else {
                            tr("Processor")
                        }
                    ),
                    style::muted(),
                ),
            ])),
            None if jobs.running(Purpose::Processes).is_some() => top.push(Line::styled(
                tr("Watching what each program uses…"),
                style::muted(),
            )),
            None => {}
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
            Constraint::Length(u16::try_from(top.len()).unwrap_or(3).max(1)),
            Constraint::Min(3),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(top).wrap(Wrap { trim: true }), summary);

        let [list_area, detail] =
            Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
                .areas(body);
        let shown = self.shown();
        let rows = shown.iter().map(|p| {
            let mut tags = Vec::new();
            if p.startup.is_some() {
                tags.push(Span::styled(tr("Starts with the system"), style::size()));
            }
            if let Some(why) = p.refusal {
                if !tags.is_empty() {
                    tags.push(Span::raw(" · "));
                }
                tags.push(Span::styled(refusal_word(why), style::muted()));
            }
            Row::new(vec![
                Line::from(p.name.clone()),
                Line::styled(format!("{:>5.1} %", p.cpu), style::size()),
                Line::styled(format!("{:>10}", bytes(p.memory)), style::size()),
                Line::from(tags),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Fill(2),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Fill(2),
            ],
        )
        .header(
            Row::new(vec![tr("Program"), tr("Processor"), tr("Memory"), ""]).style(style::muted()),
        )
        .row_highlight_style(style::selected())
        .block(Block::bordered().title(Span::styled(tr("Running programs"), style::heading())));
        let mut table_state = TableState::default().with_selected(Some(self.cursor));
        list::render(frame, table, list_area, &mut table_state, 1);

        if let Some(p) = shown.get(self.cursor) {
            let mut lines = vec![field(tr("Processes"), &count(p.pids.len() as u64))];
            if let Some(exe) = &p.exe {
                lines.push(Line::styled(format!("{}:", tr("Program")), style::muted()));
                lines.push(Line::from(exe.display().to_string()));
            }
            if p.startup.is_some() {
                lines.push(Line::raw(""));
                lines.push(Line::styled(tr("Starts with the system"), style::size()));
                lines.push(Line::styled(
                    tr("It stops starting with the system; you can turn it back on in the list of startup programs."),
                    style::muted(),
                ));
            }
            if let Some(why) = p.refusal {
                lines.push(Line::raw(""));
                lines.push(Line::styled(tr(why), style::warn()));
            }
            frame.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .block(Block::bordered().title(Span::styled(p.name.clone(), style::heading()))),
                detail,
            );
        } else {
            frame.render_widget(Block::bordered(), detail);
        }

        if let Some((program, force)) = &self.confirming {
            let (title, body, yes) = if *force {
                (
                    trf("Force {name} to close?", &[("name", &program.name)]),
                    tr("It ends at once, and any unsaved work in it is lost."),
                    tr("Force close"),
                )
            } else {
                (
                    trf("Close {name}?", &[("name", &program.name)]),
                    tr(
                        "It is asked to close as if you closed its window. If it has unsaved work, it may ask you to save it first.",
                    ),
                    tr("Close"),
                )
            };
            dialog::confirm(frame, area, &title, vec![Line::from(body)], yes);
        }
    }
}

/// What closing did, as the app says it.
fn closed_note(closed: &Closed) -> (String, bool) {
    if let Some(why) = closed.refusal {
        return (tr(why).to_owned(), true);
    }
    if closed.running > 0 {
        return (
            trf(
                "{name} did not close: it may be asking to save your work.",
                &[("name", &closed.name)],
            ),
            true,
        );
    }
    (trf("{name} was closed.", &[("name", &closed.name)]), false)
}
