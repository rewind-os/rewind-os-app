//! Maintenance: the system's own tools, as a list of actions. Measuring runs at once; what
//! deletes or changes the system asks first. Only the actions of this system are listed.

use crate::app::Effect;
use crate::dialog;
use crate::jobs::{Jobs, Purpose};
use crate::list;
use crate::style;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState, Wrap};
use rewind_os_engine::format::{bytes, count, local_date};
use rewind_os_engine::{AdminNeed, Request, Response};
use rewind_os_i18n::{tr, trf};

/// One action of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    JournalUsage,
    JournalVacuum,
    ComponentStore,
    ComponentCleanup,
    RestoreList,
    RestoreClean,
    DriversList,
    DriversClean,
    HibernationOff,
    HibernationOn,
}

impl Action {
    const ALL: [Action; 10] = [
        Action::JournalUsage,
        Action::JournalVacuum,
        Action::ComponentStore,
        Action::ComponentCleanup,
        Action::RestoreList,
        Action::RestoreClean,
        Action::DriversList,
        Action::DriversClean,
        Action::HibernationOff,
        Action::HibernationOn,
    ];

    pub fn request(self) -> Request {
        match self {
            Action::JournalUsage => Request::JournalUsage,
            Action::JournalVacuum => Request::JournalVacuum,
            Action::ComponentStore => Request::AnalyzeComponentStore,
            Action::ComponentCleanup => Request::ComponentCleanup,
            Action::RestoreList => Request::RestoreList,
            Action::RestoreClean => Request::RestoreClean,
            Action::DriversList => Request::DriversList,
            Action::DriversClean => Request::DriversClean {
                packages: Vec::new(),
            },
            Action::HibernationOff => Request::Hibernation { enabled: false },
            Action::HibernationOn => Request::Hibernation { enabled: true },
        }
    }

    /// The tool it belongs to and what it does.
    fn group(self) -> &'static str {
        match self {
            Action::JournalUsage | Action::JournalVacuum => tr("System journal"),
            Action::ComponentStore | Action::ComponentCleanup => tr("Windows component store"),
            Action::RestoreList | Action::RestoreClean => tr("Restore points"),
            Action::DriversList | Action::DriversClean => tr("Old driver packages"),
            Action::HibernationOff | Action::HibernationOn => tr("Hibernation"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Action::JournalUsage => tr("Measure"),
            Action::JournalVacuum => tr("Keep only the last two weeks"),
            Action::ComponentStore => tr("Measure real size"),
            Action::ComponentCleanup => tr("Remove old components"),
            Action::RestoreList => tr("Check"),
            Action::RestoreClean => tr("Delete all but the newest"),
            Action::DriversList => tr("Check"),
            Action::DriversClean => tr("Remove old packages"),
            Action::HibernationOff => tr("Turn off"),
            Action::HibernationOn => tr("Turn on"),
        }
    }

    fn about(self) -> &'static str {
        match self {
            Action::JournalUsage | Action::JournalVacuum => tr(
                "systemd keeps a log of everything the system does. Old entries are rarely needed and can take gigabytes.",
            ),
            Action::ComponentStore | Action::ComponentCleanup => tr(
                "The WinSxS folder looks huge in a disk scan because most of its files are shared with Windows itself. This measures its real size and removes old versions of updated components.",
            ),
            Action::RestoreList | Action::RestoreClean => tr(
                "Windows keeps restore points and other copies of your drives to undo changes. Deleting all but the newest frees their space; the newest one on each drive is always kept.",
            ),
            Action::DriversList | Action::DriversClean => tr(
                "Every driver update leaves the previous version in the driver store. The newest of each driver and every one a device uses always stay.",
            ),
            Action::HibernationOff | Action::HibernationOn => tr(
                "Turning hibernation off deletes hiberfil.sys, which is often as large as your memory. Fast startup also stops working.",
            ),
        }
    }

    /// The question asked first, for what changes the system: title, text and the button.
    pub fn question(self) -> Option<(&'static str, &'static str, &'static str)> {
        Some(match self {
            Action::ComponentCleanup => (
                tr("Remove old components?"),
                tr(
                    "Old versions of updated Windows components are removed. Installed updates can no longer be uninstalled afterwards.",
                ),
                tr("Remove"),
            ),
            Action::HibernationOff => (
                tr("Turn hibernation off?"),
                tr(
                    "hiberfil.sys is deleted and fast startup stops working. You can turn it back on here.",
                ),
                tr("Turn off"),
            ),
            Action::HibernationOn => (
                tr("Turn hibernation on?"),
                tr("Windows creates hiberfil.sys again."),
                tr("Turn on"),
            ),
            Action::JournalVacuum => (
                tr("Delete old journal entries?"),
                tr("Entries older than two weeks are deleted for good."),
                tr("Delete"),
            ),
            Action::RestoreClean => (
                tr("Delete old restore points?"),
                tr(
                    "Every restore point and shadow copy except the newest on each drive is deleted, as Windows' Disk Cleanup does. You can no longer go back to those moments. This cannot be undone.",
                ),
                tr("Delete"),
            ),
            Action::DriversClean => (
                tr("Remove old driver packages?"),
                tr(
                    "The older versions of each driver that no device uses are removed from the driver store. The newest of each driver and every one in use stay. They cannot be put back here; Windows Update or the maker's installer brings a driver again if one is needed.",
                ),
                tr("Remove"),
            ),
            Action::JournalUsage
            | Action::ComponentStore
            | Action::RestoreList
            | Action::DriversList => return None,
        })
    }

    fn available(self) -> bool {
        rewind_os_engine::tool(self.request().id()).is_some_and(|t| t.available())
    }

    fn needs_admin(self) -> bool {
        rewind_os_engine::tool(self.request().id()).is_some_and(|t| t.admin == AdminNeed::Yes)
    }
}

#[derive(Default)]
pub struct State {
    pub cursor: usize,
    /// The action waiting for an answer.
    pub confirming: Option<Action>,
    /// What the last action said.
    pub output: Option<(Vec<String>, bool)>,
    pub elevated: bool,
    started: bool,
}

impl State {
    pub fn open(&mut self) -> Vec<Effect> {
        if !std::mem::replace(&mut self.started, true) {
            self.elevated = rewind_os_engine::system::is_elevated();
        }
        Vec::new()
    }

    /// The actions of this system.
    pub fn actions(&self) -> Vec<Action> {
        Action::ALL.into_iter().filter(|a| a.available()).collect()
    }

    pub fn captures_keys(&self) -> bool {
        self.confirming.is_some()
    }

    fn run(&mut self, action: Action) -> Vec<Effect> {
        self.output = None;
        vec![Effect::Run(Purpose::Maintenance, action.request())]
    }

    pub fn key(&mut self, key: KeyEvent, jobs: &Jobs) -> Vec<Effect> {
        if let Some(action) = self.confirming {
            return match dialog::answer(key) {
                Some(true) => {
                    self.confirming = None;
                    self.run(action)
                }
                Some(false) => {
                    self.confirming = None;
                    Vec::new()
                }
                None => Vec::new(),
            };
        }
        if jobs.running(Purpose::Maintenance).is_some() {
            return Vec::new();
        }
        let actions = self.actions();
        if let Some(cursor) = list::moved(key.code, self.cursor, actions.len()) {
            self.cursor = cursor;
            return Vec::new();
        }
        if !matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
            return Vec::new();
        }
        let Some(&action) = actions.get(self.cursor) else {
            return Vec::new();
        };
        if action.needs_admin() && !self.elevated {
            self.output = Some((
                vec![tr("This needs administrator rights. Start Rewind OS as administrator (with sudo on Linux).").to_owned()],
                true,
            ));
            return Vec::new();
        }
        if action.question().is_some() {
            self.confirming = Some(action);
            return Vec::new();
        }
        self.run(action)
    }

    pub fn finished(&mut self, outcome: Result<Response, String>) {
        self.output = Some(match outcome {
            Ok(Response::ToolOutput(text)) => (text.lines().map(str::to_owned).collect(), false),
            Ok(Response::Done(message)) => (vec![tr(message).to_owned()], false),
            Ok(Response::RestorePoints(report)) => (restore_lines(&report), false),
            Ok(Response::Drivers(report)) => (driver_lines(&report), false),
            Ok(Response::DriversRemoved(done)) => {
                let mut lines = vec![trf(
                    "Removed {n} old driver packages, {size}.",
                    &[
                        ("n", &count(done.removed.len() as u64)),
                        ("size", &bytes(done.bytes)),
                    ],
                )];
                lines.extend(
                    done.failed
                        .iter()
                        .map(|(name, why)| format!("{name}: {}", rewind_os_i18n::tr_text(why))),
                );
                (lines, !done.failed.is_empty())
            }
            Ok(Response::RestoreCleaned(done)) => {
                let mut lines = vec![trf(
                    "{count} restore points deleted; they took {before}, now {after}",
                    &[
                        ("count", &count(done.deleted as u64)),
                        ("before", &bytes(done.before)),
                        ("after", &bytes(done.after)),
                    ],
                )];
                lines.extend(done.failed.iter().map(|(id, why)| format!("{id}: {why}")));
                (lines, !done.failed.is_empty())
            }
            Ok(_) => return,
            Err(err) => (vec![err], true),
        });
    }

    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.confirming.is_some() {
            return vec![("y", tr("Yes")), ("n", tr("Cancel"))];
        }
        vec![("↑↓", tr("Move")), ("Enter", tr("Run"))]
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, jobs: &Jobs) {
        let actions = self.actions();
        if actions.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    tr("This system has no maintenance tools Rewind OS can run yet."),
                    style::muted(),
                ))
                .block(Block::bordered()),
                area,
            );
            return;
        }
        let [list_area, about, out] = Layout::vertical([
            Constraint::Length(u16::try_from(actions.len()).unwrap_or(8).saturating_add(2)),
            Constraint::Length(4),
            Constraint::Min(3),
        ])
        .areas(area);
        let rows = actions.iter().map(|a| {
            let mut tags = Vec::new();
            if a.question().is_some() {
                tags.push(Span::styled(tr("Asks first"), style::warn()));
            }
            if a.needs_admin() && !self.elevated {
                if !tags.is_empty() {
                    tags.push(Span::raw(" · "));
                }
                tags.push(Span::styled(tr("Needs administrator"), style::warn()));
            }
            Row::new(vec![
                Line::styled(a.group(), style::muted()),
                Line::from(a.label()),
                Line::from(tags),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Fill(2),
                Constraint::Fill(3),
                Constraint::Length(40),
            ],
        )
        .row_highlight_style(style::selected())
        .block(Block::bordered().title(Span::styled(tr("Maintenance"), style::heading())));
        let mut state = TableState::default().with_selected(Some(self.cursor));
        list::render(frame, table, list_area, &mut state, 0);

        if let Some(a) = actions.get(self.cursor) {
            frame.render_widget(
                Paragraph::new(a.about())
                    .wrap(Wrap { trim: true })
                    .block(Block::bordered().title(Span::styled(a.group(), style::heading()))),
                about,
            );
        }
        let lines: Vec<Line> = match (&self.output, jobs.running(Purpose::Maintenance)) {
            (_, Some(running)) => vec![
                dialog::progress(&running, "Working"),
                Line::styled(tr("This can take several minutes."), style::muted()),
            ],
            (Some((lines, error)), None) => lines
                .iter()
                .map(|l| {
                    Line::styled(
                        l.clone(),
                        if *error {
                            style::error()
                        } else {
                            ratatui::style::Style::new()
                        },
                    )
                })
                .collect(),
            (None, None) => Vec::new(),
        };
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(Span::styled(tr("Result"), style::heading()))),
            out,
        );
        if let Some(action) = self.confirming
            && let Some((title, body, yes)) = action.question()
        {
            dialog::confirm(frame, area, title, vec![Line::from(body)], yes);
        }
    }
}

/// How many restore points there are and what they take.
fn restore_lines(report: &rewind_os_engine::restore::Report) -> Vec<String> {
    if report.copies.is_empty() {
        return vec![tr("There are no restore points.").to_owned()];
    }
    let mut lines = vec![trf(
        "{count} restore points take {size}",
        &[
            ("count", &count(report.copies.len() as u64)),
            ("size", &bytes(report.used())),
        ],
    )];
    if let Some(newest) = report.copies.last() {
        lines.push(match &newest.description {
            Some(what) => trf(
                "Newest: {date}, {what}",
                &[("date", &local_date(newest.created)), ("what", what)],
            ),
            None => trf("Newest: {date}", &[("date", &local_date(newest.created))]),
        });
    }
    for storage in &report.storage {
        if let Some(max) = storage.max {
            lines.push(trf(
                "{drive}: Windows lets them take up to {max}",
                &[("drive", &storage.drive), ("max", &bytes(max))],
            ));
        }
    }
    lines
}

/// How many old driver packages there are, what they take, and the largest.
fn driver_lines(report: &rewind_os_engine::drivers::Report) -> Vec<String> {
    if report.old.is_empty() {
        return vec![
            tr("No old driver packages: each driver has only the version in use.").to_owned(),
        ];
    }
    let mut lines = vec![trf(
        "{n} old driver packages take {size}.",
        &[
            ("n", &count(report.old.len() as u64)),
            ("size", &bytes(report.reclaimable())),
        ],
    )];
    lines.extend(report.old.iter().take(8).map(|p| {
        format!(
            "{:>10}  {} {} {}  ({})",
            bytes(p.bytes),
            p.provider,
            p.original,
            p.version,
            trf("newest: {version}", &[("version", &p.newest)])
        )
    }));
    if report.old_in_use > 0 {
        lines.push(trf(
            "{n} older packages stay because a device uses them.",
            &[("n", &count(report.old_in_use as u64))],
        ));
    }
    lines
}
