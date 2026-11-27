//! The views, the keys every view shares and how they are drawn around the current view.
//! Keys and answers only change state and name requests (`Effect`); the event loop in
//! `lib.rs` runs them, so this is tested without a terminal or the engine.

use crate::jobs::{Jobs, Purpose};
use crate::{cleanup, home, scan, style};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
}

impl View {
    pub const ALL: [View; 3] = [View::Home, View::Scan, View::Cleanup];

    fn title(self) -> &'static str {
        match self {
            View::Home => tr("Home"),
            View::Scan => tr("Disk"),
            View::Cleanup => tr("Cleanup"),
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
    mode: Mode,
    /// A line for the bottom bar, and whether it is an error.
    message: Option<(String, bool)>,
    pub quit: bool,
}

impl App {
    pub fn new(scan_root: PathBuf) -> Self {
        App {
            view: View::Home,
            home: home::State::default(),
            scan: scan::State::new(scan_root),
            cleanup: cleanup::State::default(),
            mode: Mode::Normal,
            message: None,
            quit: false,
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
            View::Home | View::Scan => Vec::new(),
        }
    }

    fn captured(&self) -> bool {
        match self.view {
            View::Scan => self.scan.captures_keys(),
            View::Cleanup => self.cleanup.captures_keys(),
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
            let next = |step: isize| {
                let at = View::ALL.iter().position(|v| *v == self.view).unwrap_or(0);
                let n = View::ALL.len() as isize;
                let index = (at as isize + step).rem_euclid(n) as usize;
                View::ALL.get(index).copied().unwrap_or(View::Home)
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
                KeyCode::Char(c @ '1'..='3') => {
                    let index = (c as usize).saturating_sub('1' as usize);
                    if let Some(&view) = View::ALL.get(index) {
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
        }
    }

    fn command(&mut self, typed: &str) -> Vec<Effect> {
        let typed = typed.trim();
        if matches!(typed, "q" | "quit") {
            self.quit = true;
            return Vec::new();
        }
        match View::ALL.iter().find(|v| v.matches(typed)) {
            Some(&view) => self.show(view),
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
        for (i, view) in View::ALL.iter().enumerate() {
            let label = format!(" {} {} ", i + 1, view.title());
            if *view == self.view {
                tabs.push(Span::styled(label, style::active()));
            } else {
                tabs.push(Span::raw(label));
            }
            tabs.push(Span::raw(" "));
        }
        if let Some(o) = &self.home.overview {
            tabs.push(Span::styled(format!("  {}", o.host), style::muted()));
        }
        frame.render_widget(Paragraph::new(Line::from(tabs)), top);

        match self.view {
            View::Home => self.home.draw(frame, body, jobs),
            View::Scan => self.scan.draw(frame, body, jobs),
            View::Cleanup => self.cleanup.draw(frame, body, jobs),
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
                });
                if !self.captured() {
                    hints.push(("1-3", tr("Views")));
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
                ("1  2  3", tr("Home, disk, cleanup")),
                ("Tab", tr("Next view")),
                (":home  :disk  :cleanup", tr("Open a view by name")),
                ("↑ ↓  j k", tr("Move")),
                ("Enter  →  l", tr("Open")),
                ("Backspace  ←  h", tr("Back")),
                ("Space", tr("Pick a category")),
                ("Esc", tr("Stop what is running, or close")),
                ("q  Ctrl+C", tr("Quit")),
            ];
            let lines: Vec<Line> = rows
                .iter()
                .map(|(key, what)| {
                    Line::from(vec![
                        Span::styled(format!("{key:<24}"), style::size()),
                        Span::raw(*what),
                    ])
                })
                .collect();
            let [middle] = Layout::vertical([Constraint::Length(rows.len() as u16 + 2)])
                .flex(Flex::Center)
                .areas(body);
            let [dialog] = Layout::horizontal([Constraint::Max(64)])
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
