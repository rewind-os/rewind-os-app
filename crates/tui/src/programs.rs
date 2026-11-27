//! Installed programs: the list, the selected one's details and its uninstall, which always asks
//! first. On Linux the package manager asks for the password through the desktop (pkexec), so
//! without one, or over SSH, the interface has to be started with sudo; the confirmation says so.

use crate::app::Effect;
use crate::jobs::{Jobs, Purpose};
use crate::list::{self, Filter};
use crate::startup::field;
use crate::style;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Row, Table, TableState, Wrap};
use rewind_os_engine::Os;
use rewind_os_engine::format::bytes;
use rewind_os_engine::programs::{InstalledProgram, Scope};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Name,
    Size,
    Date,
}

#[derive(Default)]
pub struct State {
    pub programs: Option<Vec<InstalledProgram>>,
    pub cursor: usize,
    pub filter: Filter,
    pub sort: Sort,
    /// The program whose uninstall is being confirmed.
    pub confirming: Option<InstalledProgram>,
    pub note: Option<(String, bool)>,
    started: bool,
}

impl State {
    pub fn open(&mut self) -> Vec<Effect> {
        if std::mem::replace(&mut self.started, true) {
            return Vec::new();
        }
        vec![Effect::Run(Purpose::Programs, Request::ProgramsList)]
    }

    /// The programs the filter lets through, in the chosen order.
    pub fn shown(&self) -> Vec<&InstalledProgram> {
        let mut shown: Vec<&InstalledProgram> = self
            .programs
            .iter()
            .flatten()
            .filter(|p| {
                self.filter.matches(&[
                    &p.name,
                    p.publisher.as_deref().unwrap_or(""),
                    p.manager.unwrap_or(""),
                ])
            })
            .collect();
        match self.sort {
            Sort::Name => {}
            Sort::Size => shown.sort_by_key(|p| std::cmp::Reverse(p.estimated_bytes.unwrap_or(0))),
            Sort::Date => shown.sort_by_key(|p| std::cmp::Reverse(p.installed_on)),
        }
        shown
    }

    pub fn captures_keys(&self) -> bool {
        self.filter.editing || self.confirming.is_some()
    }

    pub fn key(&mut self, key: KeyEvent, jobs: &Jobs) -> Vec<Effect> {
        if let Some(program) = &self.confirming {
            return match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let key = program.key.clone();
                    self.confirming = None;
                    vec![Effect::Run(
                        Purpose::Uninstall,
                        Request::ProgramUninstall { key },
                    )]
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.confirming = None;
                    Vec::new()
                }
                _ => Vec::new(),
            };
        }
        if let Some(changed) = self.filter.key(key) {
            if changed {
                self.cursor = 0;
            }
            return Vec::new();
        }
        if jobs.running(Purpose::Programs).is_some() {
            return Vec::new();
        }
        if let Some(cursor) = list::moved(key.code, self.cursor, self.shown().len()) {
            self.cursor = cursor;
            return Vec::new();
        }
        match key.code {
            KeyCode::Char('r') => {
                self.note = None;
                return vec![Effect::Run(Purpose::Programs, Request::ProgramsList)];
            }
            KeyCode::Char('s') => {
                self.sort = match self.sort {
                    Sort::Name => Sort::Size,
                    Sort::Size => Sort::Date,
                    Sort::Date => Sort::Name,
                };
                self.cursor = 0;
            }
            KeyCode::Char('u') | KeyCode::Delete => {
                if !rewind_os_engine::tool("programs.uninstall").is_some_and(|t| t.available()) {
                    self.note = Some((
                        tr("Uninstalling is not available on this system yet.").to_owned(),
                        true,
                    ));
                } else if let Some(program) = self.shown().get(self.cursor).map(|p| (*p).clone()) {
                    self.note = None;
                    self.confirming = Some(program);
                }
            }
            _ => {}
        }
        Vec::new()
    }

    pub fn finished(&mut self, purpose: Purpose, outcome: Result<Response, String>) -> Vec<Effect> {
        match (purpose, outcome) {
            (Purpose::Programs, Ok(Response::Programs(programs))) => {
                self.cursor = self.cursor.min(programs.len().saturating_sub(1));
                self.programs = Some(programs);
                Vec::new()
            }
            (Purpose::Uninstall, Ok(Response::Done(message))) => {
                self.note = Some((tr(message).to_owned(), false));
                vec![Effect::Run(Purpose::Programs, Request::ProgramsList)]
            }
            (_, Err(err)) => {
                self.note = Some((err, true));
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.confirming.is_some() {
            return vec![("y", tr("Uninstall")), ("n", tr("Cancel"))];
        }
        if self.filter.editing {
            return vec![("Enter", tr("Done")), ("Esc", tr("Clear"))];
        }
        vec![
            ("↑↓", tr("Move")),
            ("u", tr("Uninstall…")),
            ("/", tr("Filter")),
            ("s", tr("Sort by")),
            ("r", tr("Refresh")),
        ]
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, jobs: &Jobs) {
        let shown = self.shown();
        let sort = match self.sort {
            Sort::Name => tr("Name"),
            Sort::Size => tr("Size"),
            Sort::Date => tr("Install date"),
        };
        let mut top = Vec::new();
        match &self.programs {
            Some(programs) => top.push(Line::from(vec![
                Span::styled(
                    trf("{n} programs", &[("n", &programs.len().to_string())]),
                    style::heading(),
                ),
                Span::styled(format!(" · {}: {sort}", tr("Sort by")), style::muted()),
            ])),
            None if jobs.running(Purpose::Programs).is_some() => {
                top.push(Line::styled(
                    tr("Reading the installed programs…"),
                    style::muted(),
                ));
            }
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
        let rows = shown.iter().map(|p| {
            Row::new(vec![
                Line::from(p.name.clone()),
                Line::styled(p.version.clone().unwrap_or_default(), style::muted()),
                Line::styled(
                    p.estimated_bytes
                        .map(|b| format!("{:>10}", bytes(b)))
                        .unwrap_or_default(),
                    style::size(),
                ),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Fill(3),
                Constraint::Fill(1),
                Constraint::Length(10),
            ],
        )
        .row_highlight_style(style::selected())
        .block(Block::bordered().title(Span::styled(tr("Installed programs"), style::heading())));
        let mut table_state = TableState::default().with_selected(Some(self.cursor));
        list::render(frame, table, list_area, &mut table_state, 0);

        if let Some(p) = shown.get(self.cursor) {
            let mut lines = Vec::new();
            if let Some(publisher) = &p.publisher {
                lines.push(field(tr("Publisher"), publisher));
            }
            if let Some(version) = &p.version {
                lines.push(field(tr("Version"), version));
            }
            if let Some(date) = p.installed_on {
                lines.push(field(
                    tr("Installed on"),
                    &date.format("%Y-%m-%d").to_string(),
                ));
            }
            if let Some(manager) = p.manager {
                lines.push(field(tr("Installed with"), manager));
            }
            let scope = match p.scope {
                Scope::Machine => tr("Everyone on this computer"),
                Scope::User => tr("Only you"),
            };
            lines.push(field(tr("Installed for"), scope));
            if let Some(size) = p.estimated_bytes {
                lines.push(field(tr("Size the installer declared"), &bytes(size)));
            }
            if let Some(location) = &p.install_location {
                lines.push(field(tr("Folder"), &location.display().to_string()));
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

        if let Some(program) = &self.confirming {
            draw_confirm(frame, area, program);
        }
    }
}

/// What uninstalling does on this system, as the app says it.
fn consequence(program: &InstalledProgram) -> String {
    match Os::current() {
        Os::Windows => trf(
            "This starts the uninstaller of {name}. Follow its steps; afterwards, search for leftovers.",
            &[("name", &program.name)],
        ),
        Os::MacOs => trf(
            "{name} will be moved to the Trash, where you can put it back. Its settings in ~/Library stay.",
            &[("name", &program.name)],
        ),
        Os::Linux => trf(
            "{name} will be removed with the package manager, which asks for your password. Its settings in your home folder stay. If removing it would take other packages along, Rewind OS stops.",
            &[("name", &program.name)],
        ),
    }
}

fn draw_confirm(frame: &mut Frame, area: Rect, program: &InstalledProgram) {
    let mut lines = vec![Line::from(consequence(program)), Line::raw("")];
    if Os::current() == Os::Linux && !rewind_os_engine::system::is_elevated() {
        lines.push(Line::styled(
            tr("The password is asked in a window of the desktop. Without one, or over SSH, start Rewind OS with sudo."),
            style::muted(),
        ));
        lines.push(Line::raw(""));
    }
    lines.extend([Line::from(vec![
        Span::styled("[y] ", style::size()),
        Span::styled(tr("Uninstall"), style::heading()),
        Span::raw("    "),
        Span::styled("[n] ", style::muted()),
        Span::raw(tr("Cancel")),
    ])]);
    let [middle] = Layout::vertical([Constraint::Length(11)])
        .flex(Flex::Center)
        .areas(area);
    let [dialog] = Layout::horizontal([Constraint::Max(68)])
        .flex(Flex::Center)
        .areas(middle);
    frame.render_widget(Clear, dialog);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::bordered()
                .border_style(style::warn())
                .title(Span::styled(
                    tr("Uninstall this program?"),
                    style::heading(),
                )),
        ),
        dialog,
    );
}
