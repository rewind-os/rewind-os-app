//! The views, the keys every view shares and how they are drawn around the current view.
//! Keys and answers only change state and name requests (`Effect`); the event loop in
//! `lib.rs` runs them, so this is tested without a terminal or the engine.

use crate::jobs::{Jobs, Purpose};
use crate::{
    build_folders, cleanup, duplicates, health, home, leftovers, maintenance, processes, programs,
    scan, startup, style,
};
use ratatui::Frame;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};
use std::path::PathBuf;

/// What a key or an answer asks the event loop to do.
#[derive(Debug, Clone)]
pub enum Effect {
    Run(Purpose, Request),
    Stop(Purpose),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Home,
    Scan,
    Cleanup,
    Startup,
    Programs,
    Duplicates,
    Leftovers,
    BuildFolders,
    Processes,
    Maintenance,
    Health,
}

impl View {
    pub const ALL: [View; 11] = [
        View::Home,
        View::Scan,
        View::Cleanup,
        View::Startup,
        View::Programs,
        View::Duplicates,
        View::Leftovers,
        View::BuildFolders,
        View::Processes,
        View::Maintenance,
        View::Health,
    ];

    /// Whether this system has what the view shows: views of other systems are left out.
    fn available(self) -> bool {
        let tool = |id: &str| rewind_os_engine::tool(id).is_some_and(|t| t.available());
        match self {
            View::Health => tool("disk.health") || tool("battery.health"),
            _ => true,
        }
    }

    /// The views of this system, in order; the first ten have the keys 1 to 9 and 0.
    pub fn shown() -> Vec<View> {
        View::ALL.into_iter().filter(|v| v.available()).collect()
    }

    fn title(self) -> &'static str {
        match self {
            View::Home => tr("Home"),
            View::Scan => tr("Disk"),
            View::Cleanup => tr("Cleanup"),
            View::Startup => tr("Startup"),
            View::Programs => tr("Programs"),
            View::Duplicates => tr("Duplicates"),
            View::Leftovers => tr("Leftovers"),
            View::BuildFolders => tr("Builds"),
            View::Processes => tr("Processes"),
            View::Maintenance => tr("Maintenance"),
            View::Health => tr("Health"),
        }
    }

    /// The names `:` accepts, in English and in the current language.
    fn matches(self, typed: &str) -> bool {
        let typed = typed.trim().to_lowercase();
        if typed.is_empty() {
            return false;
        }
        let names: &[&str] = match self {
            View::Home => &["home"],
            View::Scan => &["scan", "disk"],
            View::Cleanup => &["cleanup", "clean"],
            View::Startup => &["startup"],
            View::Programs => &["programs", "apps", "uninstall"],
            View::Duplicates => &["duplicates", "dupes"],
            View::Leftovers => &["leftovers"],
            View::BuildFolders => &["builds", "dev", "projects"],
            View::Processes => &["processes", "ps", "top"],
            View::Maintenance => &["maintenance"],
            View::Health => &["health", "disks", "battery", "drives"],
        };
        names.iter().any(|n| n.starts_with(&typed))
            || self.title().to_lowercase().starts_with(&typed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Normal,
    /// The `:` line, with what was typed.
    Command(String),
    Help,
}

pub struct App {
    pub view: View,
    pub home: home::State,
    pub scan: scan::State,
    pub cleanup: cleanup::State,
    pub startup: startup::State,
    pub programs: programs::State,
    pub duplicates: duplicates::State,
    pub leftovers: leftovers::State,
    pub build_folders: build_folders::State,
    pub processes: processes::State,
    pub maintenance: maintenance::State,
    pub health: health::State,
    mode: Mode,
    /// A line for the bottom bar, and whether it is an error.
    message: Option<(String, bool)>,
    pub quit: bool,
    /// Where each view's tab was drawn on the top line: first and last column.
    tabs: std::cell::RefCell<Vec<(u16, u16, View)>>,
    /// The last click on a row: when, and which row, to tell a double click.
    last_click: Option<(std::time::Instant, usize)>,
}

impl App {
    pub fn new(scan_root: PathBuf) -> Self {
        App {
            view: View::Home,
            home: home::State::default(),
            scan: scan::State::new(scan_root.clone()),
            duplicates: duplicates::State::new(scan_root.clone()),
            leftovers: leftovers::State::default(),
            build_folders: build_folders::State::new(scan_root),
            processes: processes::State::default(),
            maintenance: maintenance::State::default(),
            health: health::State::default(),
            cleanup: cleanup::State::default(),
            startup: startup::State::default(),
            programs: programs::State::default(),
            mode: Mode::Normal,
            message: None,
            quit: false,
            tabs: std::cell::RefCell::new(Vec::new()),
            last_click: None,
        }
    }

    /// What the first screen needs.
    pub fn start(&mut self) -> Vec<Effect> {
        self.home.load()
    }

    fn show(&mut self, view: View) -> Vec<Effect> {
        self.view = view;
        match view {
            View::Cleanup => self.cleanup.open(),
            View::Startup => self.startup.open(),
            View::Programs => self.programs.open(),
            View::Leftovers => self.leftovers.open(),
            View::Processes => self.processes.open(),
            View::Maintenance => self.maintenance.open(),
            View::Health => self.health.open(),
            View::Home | View::Scan | View::Duplicates | View::BuildFolders => Vec::new(),
        }
    }

    fn captured(&self) -> bool {
        match self.view {
            View::Scan => self.scan.captures_keys(),
            View::Cleanup => self.cleanup.captures_keys(),
            View::Startup => self.startup.captures_keys(),
            View::Programs => self.programs.captures_keys(),
            View::Duplicates => self.duplicates.captures_keys(),
            View::Leftovers => self.leftovers.captures_keys(),
            View::BuildFolders => self.build_folders.captures_keys(),
            View::Processes => self.processes.captures_keys(),
            View::Maintenance => self.maintenance.captures_keys(),
            View::Health => self.health.captures_keys(),
            View::Home => false,
        }
    }

    pub fn key(&mut self, key: KeyEvent, jobs: &Jobs) -> Vec<Effect> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return Vec::new();
        }
        match &mut self.mode {
            Mode::Help => {
                self.mode = Mode::Normal;
                return Vec::new();
            }
            Mode::Command(typed) => {
                match key.code {
                    KeyCode::Esc => self.mode = Mode::Normal,
                    KeyCode::Backspace => {
                        if typed.pop().is_none() {
                            self.mode = Mode::Normal;
                        }
                    }
                    KeyCode::Char(c) => typed.push(c),
                    KeyCode::Enter => {
                        let typed = std::mem::take(typed);
                        self.mode = Mode::Normal;
                        return self.command(&typed);
                    }
                    _ => {}
                }
                return Vec::new();
            }
            Mode::Normal => {}
        }
        if !self.captured() {
            self.message = None;
            let shown = View::shown();
            let next = |step: isize| {
                let at = shown.iter().position(|v| *v == self.view).unwrap_or(0);
                let n = shown.len().max(1) as isize;
                let index = (at as isize + step).rem_euclid(n) as usize;
                shown.get(index).copied().unwrap_or(View::Home)
            };
            match key.code {
                KeyCode::Char('q') => {
                    self.quit = true;
                    return Vec::new();
                }
                KeyCode::Char(':') => {
                    self.mode = Mode::Command(String::new());
                    return Vec::new();
                }
                KeyCode::Char('?') => {
                    self.mode = Mode::Help;
                    return Vec::new();
                }
                KeyCode::Char(c @ '0'..='9') => {
                    // 1 to 9, then 0 for the tenth, as on the keyboard.
                    let index = if c == '0' {
                        9
                    } else {
                        (c as usize).saturating_sub('1' as usize)
                    };
                    if let Some(&view) = shown.get(index) {
                        return self.show(view);
                    }
                }
                KeyCode::Tab => return self.show(next(1)),
                KeyCode::BackTab => return self.show(next(-1)),
                _ => {}
            }
        }
        match self.view {
            View::Home => self.home.key(key),
            View::Scan => self.scan.key(key, jobs),
            View::Cleanup => self.cleanup.key(key, jobs),
            View::Startup => self.startup.key(key, jobs),
            View::Programs => self.programs.key(key, jobs),
            View::Duplicates => self.duplicates.key(key, jobs),
            View::Leftovers => self.leftovers.key(key, jobs),
            View::BuildFolders => self.build_folders.key(key, jobs),
            View::Processes => self.processes.key(key, jobs),
            View::Maintenance => self.maintenance.key(key, jobs),
            View::Health => self.health.key(key, jobs),
        }
    }

    /// The mouse: a click on a tab opens its view, a click on a row moves the cursor there, the
    /// wheel moves it like the arrows. A double click opens a folder in the disk view. Nothing
    /// happens while a field, a question or the help is open.
    pub fn mouse(&mut self, event: MouseEvent, jobs: &Jobs) -> Vec<Effect> {
        if self.mode != Mode::Normal || self.captured() {
            return Vec::new();
        }
        let arrow = |code| KeyEvent::new(code, KeyModifiers::NONE);
        match event.kind {
            MouseEventKind::ScrollDown => self.key(arrow(KeyCode::Down), jobs),
            MouseEventKind::ScrollUp => self.key(arrow(KeyCode::Up), jobs),
            MouseEventKind::Down(MouseButton::Left) => {
                let tab = self
                    .tabs
                    .borrow()
                    .iter()
                    .find(|(start, end, _)| {
                        event.row == 0 && event.column >= *start && event.column < *end
                    })
                    .map(|(_, _, view)| *view);
                if let Some(view) = tab {
                    return self.show(view);
                }
                let Some(row) =
                    crate::list::last_rows().and_then(|r| r.row_at(event.column, event.row))
                else {
                    return Vec::new();
                };
                let now = std::time::Instant::now();
                let double = self.last_click.is_some_and(|(at, last)| {
                    last == row && now.duration_since(at) < std::time::Duration::from_millis(400)
                });
                self.last_click = Some((now, row));
                // To the row: the first row, then down to it, as the keys would.
                let mut effects = self.key(arrow(KeyCode::Home), jobs);
                for _ in 0..row {
                    effects.extend(self.key(arrow(KeyCode::Down), jobs));
                }
                if double && self.view == View::Scan {
                    effects.extend(self.key(arrow(KeyCode::Enter), jobs));
                }
                effects
            }
            _ => Vec::new(),
        }
    }

    /// Called often while nothing happens: the running programs are measured again every few
    /// seconds while their view is open.
    pub fn tick(&mut self, jobs: &Jobs) -> Vec<Effect> {
        if self.view == View::Processes && self.mode == Mode::Normal {
            self.processes.tick(jobs)
        } else {
            Vec::new()
        }
    }

    fn command(&mut self, typed: &str) -> Vec<Effect> {
        let typed = typed.trim();
        if matches!(typed, "q" | "quit") {
            self.quit = true;
            return Vec::new();
        }
        match View::shown().into_iter().find(|v| v.matches(typed)) {
            Some(view) => self.show(view),
            None => {
                self.message = Some((trf("No view is called “{name}”.", &[("name", typed)]), true));
                Vec::new()
            }
        }
    }

    pub fn finished(&mut self, purpose: Purpose, outcome: Result<Response, String>) -> Vec<Effect> {
        match purpose {
            Purpose::Overview | Purpose::Recommendations => {
                self.home.finished(purpose, outcome);
                Vec::new()
            }
            Purpose::Scan => {
                self.scan.finished(outcome);
                Vec::new()
            }
            Purpose::Analyze | Purpose::Clean => self.cleanup.finished(purpose, outcome),
            Purpose::Startup | Purpose::StartupSet => self.startup.finished(purpose, outcome),
            Purpose::Programs | Purpose::Uninstall => self.programs.finished(purpose, outcome),
            Purpose::Duplicates | Purpose::DuplicatesTrash => {
                self.duplicates.finished(purpose, outcome);
                Vec::new()
            }
            Purpose::Leftovers | Purpose::LeftoversTrash => {
                self.leftovers.finished(purpose, outcome);
                Vec::new()
            }
            Purpose::BuildFolders | Purpose::BuildClean => {
                self.build_folders.finished(purpose, outcome);
                Vec::new()
            }
            Purpose::Processes | Purpose::Close | Purpose::StartupOff => {
                self.processes.finished(purpose, outcome)
            }
            Purpose::Maintenance => {
                self.maintenance.finished(outcome);
                Vec::new()
            }
            Purpose::Health | Purpose::Battery | Purpose::Optimize => {
                self.health.finished(purpose, outcome);
                Vec::new()
            }
        }
    }

    pub fn draw(&self, frame: &mut Frame, jobs: &Jobs) {
        let [top, body, bottom] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let mut tabs = vec![
            Span::styled(" Rewind OS ", style::heading()),
            Span::raw(" "),
        ];
        let shown = View::shown();
        // Each view's key: 1 to 9, then 0; later views have none (Tab or `:` reach them).
        let key = |i: usize| (i < 10).then(|| ((i + 1) % 10).to_string());
        let label = |i: usize, view: View| match key(i) {
            Some(k) => format!(" {k} {} ", view.title()),
            None => format!(" {} ", view.title()),
        };
        let full: usize = shown
            .iter()
            .enumerate()
            .map(|(i, v)| label(i, *v).chars().count())
            .sum::<usize>()
            + 12;
        // When every name does not fit, the other views show only their key, or their initial.
        let compact = full > usize::from(top.width);
        for (i, view) in shown.iter().enumerate() {
            if *view == self.view {
                tabs.push(Span::styled(label(i, *view), style::active()));
            } else if compact {
                let short = key(i).unwrap_or_else(|| view.title().chars().take(1).collect());
                tabs.push(Span::styled(format!(" {short} "), style::muted()));
            } else {
                tabs.push(Span::raw(label(i, *view)));
            }
        }
        // Each tab's columns, for the mouse.
        let mut column = top.x;
        let mut places = Vec::new();
        for (i, span) in tabs.iter().enumerate() {
            let width = u16::try_from(span.width()).unwrap_or(u16::MAX);
            // The first two spans are the name and a space; then one span per view.
            if let Some(&view) = i.checked_sub(2).and_then(|v| shown.get(v)) {
                places.push((column, column.saturating_add(width), view));
            }
            column = column.saturating_add(width);
        }
        *self.tabs.borrow_mut() = places;
        if let Some(o) = &self.home.overview {
            tabs.push(Span::styled(format!("  {}", o.host), style::muted()));
        }
        frame.render_widget(Paragraph::new(Line::from(tabs)), top);
        crate::list::forget_rows();

        match self.view {
            View::Home => self.home.draw(frame, body, jobs),
            View::Scan => self.scan.draw(frame, body, jobs),
            View::Cleanup => self.cleanup.draw(frame, body, jobs),
            View::Startup => self.startup.draw(frame, body, jobs),
            View::Programs => self.programs.draw(frame, body, jobs),
            View::Duplicates => self.duplicates.draw(frame, body, jobs),
            View::Leftovers => self.leftovers.draw(frame, body, jobs),
            View::BuildFolders => self.build_folders.draw(frame, body, jobs),
            View::Processes => self.processes.draw(frame, body, jobs),
            View::Maintenance => self.maintenance.draw(frame, body, jobs),
            View::Health => self.health.draw(frame, body, jobs),
        }

        let line = match (&self.mode, &self.message) {
            (Mode::Command(typed), _) => Line::from(vec![
                Span::styled(":", style::size()),
                Span::raw(typed.clone()),
                Span::styled("▏", style::size()),
            ]),
            (_, Some((text, true))) => Line::styled(text.clone(), style::error()),
            (_, Some((text, false))) => Line::raw(text.clone()),
            _ => {
                // Help and quit first, so a narrow terminal never cuts them off.
                let mut hints = Vec::new();
                if !self.captured() {
                    hints.extend([("?", tr("Help")), ("q", tr("Quit"))]);
                }
                hints.extend(match self.view {
                    View::Home => self.home.hints(),
                    View::Scan => self.scan.hints(jobs),
                    View::Cleanup => self.cleanup.hints(jobs),
                    View::Startup => self.startup.hints(),
                    View::Programs => self.programs.hints(),
                    View::Duplicates => self.duplicates.hints(jobs),
                    View::Leftovers => self.leftovers.hints(jobs),
                    View::BuildFolders => self.build_folders.hints(jobs),
                    View::Processes => self.processes.hints(jobs),
                    View::Maintenance => self.maintenance.hints(),
                    View::Health => self.health.hints(),
                });
                if !self.captured() {
                    hints.push(("1-0", tr("Views")));
                }
                let mut spans = Vec::new();
                for (key, what) in hints {
                    spans.push(Span::styled(format!(" {key} "), style::size()));
                    spans.push(Span::styled(format!("{what} "), style::muted()));
                }
                Line::from(spans)
            }
        };
        frame.render_widget(Paragraph::new(line), bottom);

        if self.mode == Mode::Help {
            let rows = [
                ("1 … 9  0", tr("The views, in the order of the tabs")),
                ("Tab", tr("Next view")),
                (":disk  :startup …", tr("Open a view by name")),
                ("/", tr("Filter the list")),
                ("↑ ↓  j k", tr("Move")),
                ("Enter  →  l", tr("Open")),
                ("Backspace  ←  h", tr("Back")),
                ("Space", tr("Pick, or turn on and off")),
                ("u", tr("Uninstall…")),
                ("Esc", tr("Stop what is running, or close")),
                ("q  Ctrl+C", tr("Quit")),
                (tr("Mouse"), tr("Click a tab or a row; the wheel moves")),
            ];
            let lines: Vec<Line> = rows
                .iter()
                .map(|(key, what)| {
                    Line::from(vec![
                        Span::styled(format!("{key:<20}"), style::size()),
                        Span::raw(*what),
                    ])
                })
                .collect();
            let [middle] = Layout::vertical([Constraint::Length(rows.len() as u16 + 2)])
                .flex(Flex::Center)
                .areas(body);
            let [dialog] = Layout::horizontal([Constraint::Max(76)])
                .flex(Flex::Center)
                .areas(middle);
            frame.render_widget(Clear, dialog);
            frame.render_widget(
                Paragraph::new(lines)
                    .block(Block::bordered().title(Span::styled(tr("Keys"), style::heading()))),
                dialog,
            );
        }
    }
}
