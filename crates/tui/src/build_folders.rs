//! Build folders: what the build tools of your projects make (node_modules, target…), picked one
//! by one and deleted after a confirmation. Each project makes its own again.

use crate::app::Effect;
use crate::dialog::{self, FolderField};
use crate::jobs::{Jobs, Purpose};
use crate::list;
use crate::style;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState, Wrap};
use rewind_os_engine::cleanup::Risk;
use rewind_os_engine::format::{bytes, count, local_date};
use rewind_os_engine::projects::{BuildFolder, Cleaned, Report};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, tr_text, trf};
use std::collections::HashSet;
use std::path::PathBuf;

/// A project not changed for this long counts as not in use.
const UNUSED_DAYS: i64 = 90;

pub struct State {
    pub folder: FolderField,
    pub report: Option<Report>,
    pub selected: HashSet<PathBuf>,
    pub deleted: HashSet<PathBuf>,
    pub cursor: usize,
    pub confirming: bool,
    pub note: Option<(String, bool)>,
}

impl State {
    pub fn new(folder: PathBuf) -> Self {
        State {
            folder: FolderField::new(folder),
            report: None,
            selected: HashSet::new(),
            deleted: HashSet::new(),
            cursor: 0,
            confirming: false,
            note: None,
        }
    }

    fn search(&mut self) -> Vec<Effect> {
        self.note = None;
        vec![Effect::Run(
            Purpose::BuildFolders,
            Request::DevFind {
                root: Some(self.folder.path()),
            },
        )]
    }

    /// The folders found and not deleted yet.
    pub fn shown(&self) -> Vec<&BuildFolder> {
        self.report
            .iter()
            .flat_map(|r| r.folders.iter())
            .filter(|f| !self.deleted.contains(&f.path))
            .collect()
    }

    pub fn chosen(&self) -> (Vec<PathBuf>, u64) {
        let chosen: Vec<&BuildFolder> = self
            .shown()
            .into_iter()
            .filter(|f| self.selected.contains(&f.path))
            .collect();
        let size = chosen.iter().map(|f| f.bytes).sum();
        (chosen.iter().map(|f| f.path.clone()).collect(), size)
    }

    pub fn captures_keys(&self) -> bool {
        self.folder.editing || self.confirming
    }

    pub fn key(&mut self, key: KeyEvent, jobs: &Jobs) -> Vec<Effect> {
        if self.confirming {
            return match dialog::answer(key) {
                Some(true) => {
                    self.confirming = false;
                    let (paths, _) = self.chosen();
                    vec![Effect::Run(
                        Purpose::BuildClean,
                        Request::DevClean { paths },
                    )]
                }
                Some(false) => {
                    self.confirming = false;
                    Vec::new()
                }
                None => Vec::new(),
            };
        }
        if let Some(search) = self.folder.key(key) {
            return if search { self.search() } else { Vec::new() };
        }
        if let Some(purpose) = [Purpose::BuildFolders, Purpose::BuildClean]
            .into_iter()
            .find(|p| jobs.running(*p).is_some())
        {
            return match key.code {
                KeyCode::Esc => vec![Effect::Stop(purpose)],
                _ => Vec::new(),
            };
        }
        let len = self.shown().len();
        if let Some(cursor) = list::moved(key.code, self.cursor, len) {
            self.cursor = cursor;
            return Vec::new();
        }
        self.note = None;
        match key.code {
            KeyCode::Char('e') => self.folder.editing = true,
            KeyCode::Char('r') => return self.search(),
            KeyCode::Enter if self.report.is_none() => return self.search(),
            KeyCode::Char(' ') => {
                if let Some(path) = self.shown().get(self.cursor).map(|f| f.path.clone())
                    && !self.selected.remove(&path)
                {
                    self.selected.insert(path);
                }
            }
            KeyCode::Char('a') => {
                let before = chrono::Utc::now() - chrono::Duration::days(UNUSED_DAYS);
                self.selected = self
                    .shown()
                    .iter()
                    .filter(|f| f.last_used.is_some_and(|d| d < before))
                    .map(|f| f.path.clone())
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
            (Purpose::BuildFolders, Ok(Response::BuildFolders(report))) => {
                self.report = Some(report);
                self.selected.clear();
                self.deleted.clear();
                self.cursor = 0;
            }
            (Purpose::BuildClean, Ok(Response::BuildFoldersCleaned(results))) => {
                self.note = Some(self.cleaned(results));
                self.cursor = self.cursor.min(self.shown().len().saturating_sub(1));
            }
            (_, Err(err)) => self.note = Some((err, true)),
            _ => {}
        }
    }

    /// Takes what was deleted off the list and says what happened.
    fn cleaned(&mut self, results: Vec<Cleaned>) -> (String, bool) {
        let mut freed = 0;
        let mut problems = Vec::new();
        for r in results {
            freed += r.bytes;
            self.selected.remove(&r.path);
            match r.refused {
                Some(why) => problems.push(format!("{}: {}", r.path.display(), tr_text(why))),
                None => {
                    if r.failed > 0 {
                        problems.push(format!(
                            "{}: {}",
                            r.path.display(),
                            trf("{n} files in use were kept.", &[("n", &count(r.failed))])
                        ));
                    }
                    self.deleted.insert(r.path);
                }
            }
        }
        let mut text = trf("Deleted {size}.", &[("size", &bytes(freed))]);
        let error = !problems.is_empty();
        if error {
            text.push(' ');
            text.push_str(&problems.join("; "));
        }
        (text, error)
    }

    pub fn hints(&self, jobs: &Jobs) -> Vec<(&'static str, &'static str)> {
        if self.confirming {
            return vec![("y", tr("Delete")), ("n", tr("Cancel"))];
        }
        if self.folder.editing {
            return vec![("Enter", tr("Search")), ("Esc", tr("Cancel"))];
        }
        if jobs.running(Purpose::BuildFolders).is_some()
            || jobs.running(Purpose::BuildClean).is_some()
        {
            return vec![("Esc", tr("Stop"))];
        }
        if self.report.is_none() {
            return vec![("Enter", tr("Search")), ("e", tr("Choose a folder"))];
        }
        vec![
            ("Space", tr("Pick")),
            ("a", tr("Select projects not used for 3 months")),
            ("n", tr("None")),
            ("Enter", tr("Delete")),
            ("e", tr("Choose a folder")),
            ("r", tr("Search again")),
        ]
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, jobs: &Jobs) {
        let [field, body] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).areas(area);
        self.folder.draw(frame, field, tr("Folder to search"));
        let running = jobs
            .running(Purpose::BuildFolders)
            .map(|r| (r, "Searching"))
            .or_else(|| jobs.running(Purpose::BuildClean).map(|r| (r, "Deleting")));
        if let Some((running, what)) = running {
            frame.render_widget(
                Paragraph::new(dialog::progress(&running, what)).block(Block::bordered()),
                body,
            );
            return;
        }
        let Some(report) = &self.report else {
            let text = match &self.note {
                Some((err, true)) => Line::styled(err.clone(), style::error()),
                _ => Line::styled(
                    tr(
                        "Press Enter to look for the build folders of your projects in it, or e to choose another folder.",
                    ),
                    style::muted(),
                ),
            };
            frame.render_widget(Paragraph::new(text).block(Block::bordered()), body);
            return;
        };
        let shown = self.shown();
        let total: u64 = shown.iter().map(|f| f.bytes).sum();
        let (chosen, chosen_size) = self.chosen();
        let mut top = vec![Line::from(vec![
            Span::raw(tr("Build folders")),
            Span::raw(" "),
            Span::styled(bytes(total), style::size()),
            Span::styled(
                format!(
                    " · {}",
                    trf("{n} folders", &[("n", &count(shown.len() as u64))])
                ),
                style::muted(),
            ),
            Span::raw(" · "),
            Span::raw(trf("{n} picked", &[("n", &count(chosen.len() as u64))])),
            Span::raw(" "),
            Span::styled(bytes(chosen_size), style::size()),
        ])];
        if report.cancelled {
            top.push(Line::styled(
                tr("The search was stopped; some folders may be missing."),
                style::warn(),
            ));
        }
        if report.unreadable > 0 {
            top.push(Line::styled(
                trf(
                    "{n} folders could not be read.",
                    &[("n", &count(report.unreadable))],
                ),
                style::warn(),
            ));
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
        let [summary, list_area, detail] = Layout::vertical([
            Constraint::Length(u16::try_from(top.len()).unwrap_or(4)),
            Constraint::Min(3),
            Constraint::Length(4),
        ])
        .areas(body);
        frame.render_widget(Paragraph::new(top).wrap(Wrap { trim: true }), summary);
        if shown.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(tr("No build folders found."), style::good()))
                    .block(Block::bordered()),
                list_area,
            );
        } else {
            let rows = shown.iter().map(|f| {
                let mark = if self.selected.contains(&f.path) {
                    "[x]"
                } else {
                    "[ ]"
                };
                let mut kind = vec![Span::raw(tr(f.kind.label()))];
                if f.risk != Risk::Safe {
                    kind.push(Span::styled(
                        format!(" · {}", tr("Check first")),
                        style::warn(),
                    ));
                }
                Row::new(vec![
                    Line::from(mark),
                    Line::styled(format!("{:>10}", bytes(f.bytes)), style::size()),
                    Line::from(dialog::under(&f.path, &report.root)),
                    Line::from(kind),
                    Line::styled(
                        f.last_used.map(local_date).unwrap_or_default(),
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
                    Constraint::Length(30),
                    Constraint::Length(12),
                ],
            )
            .row_highlight_style(style::selected())
            .block(Block::bordered().title(Span::styled(tr("Build folders"), style::heading())));
            let mut state = TableState::default().with_selected(Some(self.cursor));
            list::render(frame, table, list_area, &mut state, 0);
        }
        if let Some(f) = shown.get(self.cursor) {
            let mut lines = vec![Line::from(trf(
                "Made again with: {command}",
                &[("command", f.kind.rebuild())],
            ))];
            if f.risk != Risk::Safe {
                lines.push(Line::styled(
                    tr("Packages installed by hand into this environment would be lost."),
                    style::warn(),
                ));
            }
            frame.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: true })
                    .block(Block::bordered().title(Span::styled(
                        dialog::under(&f.project, &report.root),
                        style::heading(),
                    ))),
                detail,
            );
        }
        if self.confirming {
            let body = vec![Line::from(trf(
                "{n} build folders ({size}) will be deleted permanently, not moved to the recycle bin. Each project makes its own again with its build tool (npm install, cargo build…).",
                &[
                    ("n", &count(chosen.len() as u64)),
                    ("size", &bytes(chosen_size)),
                ],
            ))];
            dialog::confirm(frame, area, tr("Delete build folders?"), body, tr("Delete"));
        }
    }
}
