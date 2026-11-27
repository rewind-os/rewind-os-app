//! Duplicate files: copies with identical contents under a folder, picked one by one and moved to
//! the recycle bin after a confirmation. One copy of each file always stays.

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
use rewind_os_engine::duplicates::{self, DuplicateReport, DuplicateSet};
use rewind_os_engine::format::{bytes, count, local_date};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, tr_text, trf};
use std::collections::HashSet;
use std::path::PathBuf;

pub struct State {
    pub folder: FolderField,
    pub report: Option<DuplicateReport>,
    /// Copies picked to move.
    pub selected: HashSet<PathBuf>,
    /// Copies already moved to the recycle bin.
    pub moved: HashSet<PathBuf>,
    pub cursor: usize,
    pub confirming: bool,
    pub note: Option<(String, bool)>,
}

/// One copy in the list, with its set.
pub struct Item<'a> {
    pub set: &'a DuplicateSet,
    pub path: &'a PathBuf,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
    /// The first copy of its set in the list (the oldest).
    pub first: bool,
}

impl State {
    pub fn new(folder: PathBuf) -> Self {
        State {
            folder: FolderField::new(folder),
            report: None,
            selected: HashSet::new(),
            moved: HashSet::new(),
            cursor: 0,
            confirming: false,
            note: None,
        }
    }

    /// The sets as they are now: without the copies already moved, and only those that still
    /// have more than one copy.
    pub fn remaining(&self) -> Vec<DuplicateSet> {
        self.report
            .iter()
            .flat_map(|r| r.sets.iter())
            .map(|s| {
                let mut s = s.clone();
                s.copies.retain(|c| !self.moved.contains(&c.path));
                s
            })
            .filter(|s| s.copies.len() > 1)
            .collect()
    }

    fn search(&mut self) -> Vec<Effect> {
        self.note = None;
        vec![Effect::Run(
            Purpose::Duplicates,
            Request::Duplicates {
                root: self.folder.path(),
                min_bytes: None,
            },
        )]
    }

    pub fn captures_keys(&self) -> bool {
        self.folder.editing || self.confirming
    }

    /// The paths picked that are still there, and the space they take.
    pub fn chosen(&self) -> (Vec<PathBuf>, u64) {
        let mut paths = Vec::new();
        let mut size = 0;
        for set in self.remaining() {
            for copy in &set.copies {
                if self.selected.contains(&copy.path) {
                    paths.push(copy.path.clone());
                    size += set.bytes;
                }
            }
        }
        (paths, size)
    }

    pub fn key(&mut self, key: KeyEvent, jobs: &Jobs) -> Vec<Effect> {
        if self.confirming {
            return match dialog::answer(key) {
                Some(true) => {
                    self.confirming = false;
                    // Checked again: the last copy of a file is never moved.
                    if !duplicates::keeps_a_copy(&self.remaining(), &self.selected) {
                        self.note = Some((tr("Keep at least one copy.").to_owned(), true));
                        return Vec::new();
                    }
                    let (paths, _) = self.chosen();
                    vec![Effect::Run(
                        Purpose::DuplicatesTrash,
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
        if let Some(search) = self.folder.key(key) {
            return if search { self.search() } else { Vec::new() };
        }
        if jobs.running(Purpose::Duplicates).is_some() {
            return match key.code {
                KeyCode::Esc => vec![Effect::Stop(Purpose::Duplicates)],
                _ => Vec::new(),
            };
        }
        if jobs.running(Purpose::DuplicatesTrash).is_some() {
            return Vec::new();
        }
        let sets = self.remaining();
        let rows: Vec<(usize, PathBuf)> = sets
            .iter()
            .enumerate()
            .flat_map(|(i, s)| s.copies.iter().map(move |c| (i, c.path.clone())))
            .collect();
        if let Some(cursor) = list::moved(key.code, self.cursor, rows.len()) {
            self.cursor = cursor;
            return Vec::new();
        }
        self.note = None;
        match key.code {
            KeyCode::Char('e') => self.folder.editing = true,
            KeyCode::Char('r') => return self.search(),
            KeyCode::Enter if self.report.is_none() => return self.search(),
            KeyCode::Char(' ') => {
                if let Some((_, path)) = rows.get(self.cursor)
                    && !self.selected.remove(path)
                {
                    self.selected.insert(path.clone());
                    if !duplicates::keeps_a_copy(&sets, &self.selected) {
                        self.selected.remove(path);
                        self.note = Some((tr("Keep at least one copy.").to_owned(), true));
                    }
                }
            }
            KeyCode::Char('a') => {
                // Every copy but the oldest, which is usually the original.
                self.selected = sets
                    .iter()
                    .flat_map(|s| s.copies.iter().skip(1).map(|c| c.path.clone()))
                    .collect();
            }
            KeyCode::Char('n') => self.selected.clear(),
            KeyCode::Enter => {
                if self.chosen().0.is_empty() {
                    self.note = Some((tr("Pick at least one copy with Space.").to_owned(), true));
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
            (Purpose::Duplicates, Ok(Response::Duplicates(report))) => {
                self.report = Some(report);
                self.selected.clear();
                self.moved.clear();
                self.cursor = 0;
            }
            (Purpose::DuplicatesTrash, Ok(Response::Trashed(results))) => {
                let (note, moved) = trashed(results);
                self.moved.extend(moved);
                self.selected.retain(|p| !self.moved.contains(p));
                let rows: usize = self.remaining().iter().map(|s| s.copies.len()).sum();
                self.cursor = self.cursor.min(rows.saturating_sub(1));
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
        if self.folder.editing {
            return vec![("Enter", tr("Search")), ("Esc", tr("Cancel"))];
        }
        if jobs.running(Purpose::Duplicates).is_some() {
            return vec![("Esc", tr("Stop"))];
        }
        if self.report.is_none() {
            return vec![("Enter", tr("Search")), ("e", tr("Choose a folder"))];
        }
        vec![
            ("Space", tr("Pick")),
            ("a", tr("Select all but the oldest")),
            ("n", tr("None")),
            ("Enter", tr("Move to recycle bin")),
            ("e", tr("Choose a folder")),
            ("r", tr("Search again")),
        ]
    }

    /// The copies in order, set by set.
    pub fn items<'a>(&self, sets: &'a [DuplicateSet]) -> Vec<Item<'a>> {
        sets.iter()
            .flat_map(|set| {
                set.copies.iter().enumerate().map(move |(i, c)| Item {
                    set,
                    path: &c.path,
                    modified: c.modified,
                    first: i == 0,
                })
            })
            .collect()
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, jobs: &Jobs) {
        let [field, body] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).areas(area);
        self.folder.draw(frame, field, tr("Folder to search"));
        if let Some(running) = jobs.running(Purpose::Duplicates) {
            frame.render_widget(
                Paragraph::new(dialog::progress(&running, "Searching")).block(Block::bordered()),
                body,
            );
            return;
        }
        let Some(report) = &self.report else {
            let text = match &self.note {
                Some((err, true)) => Line::styled(err.clone(), style::error()),
                _ => Line::styled(
                    tr(
                        "Press Enter to look for files with identical contents in it, or e to choose another folder.",
                    ),
                    style::muted(),
                ),
            };
            frame.render_widget(Paragraph::new(text).block(Block::bordered()), body);
            return;
        };
        let sets = self.remaining();
        let items = self.items(&sets);
        let reclaimable: u64 = sets.iter().map(|s| s.reclaimable()).sum();
        let (chosen, chosen_size) = self.chosen();
        let mut top = vec![Line::from(vec![
            Span::raw(tr("Space you can free")),
            Span::raw(" "),
            Span::styled(bytes(reclaimable), style::size()),
            Span::styled(
                format!(
                    " · {} · {}",
                    trf("{n} copies in total", &[("n", &count(items.len() as u64))]),
                    trf("{n} files", &[("n", &count(report.files_seen))])
                ),
                style::muted(),
            ),
            Span::raw(" · "),
            Span::raw(trf("{n} picked", &[("n", &count(chosen.len() as u64))])),
            Span::raw(" "),
            Span::styled(bytes(chosen_size), style::size()),
        ])];
        top.extend(
            notes(report)
                .into_iter()
                .map(|n| Line::styled(n, style::warn())),
        );
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
        let [summary, list_area] = Layout::vertical([
            Constraint::Length(u16::try_from(top.len()).unwrap_or(4)),
            Constraint::Min(3),
        ])
        .areas(body);
        frame.render_widget(Paragraph::new(top).wrap(Wrap { trim: true }), summary);
        if items.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(tr("No duplicate files found."), style::good()))
                    .block(Block::bordered()),
                list_area,
            );
        } else {
            let rows = items.iter().map(|item| {
                let mark = if self.selected.contains(item.path) {
                    "[x]"
                } else {
                    "[ ]"
                };
                let size = if item.first {
                    format!("{:>10}", bytes(item.set.bytes))
                } else {
                    String::new()
                };
                let shown = dialog::under(item.path, &report.root);
                let path = if item.first {
                    shown
                } else {
                    format!("  {shown}")
                };
                Row::new(vec![
                    Line::from(mark),
                    Line::styled(size, style::size()),
                    Line::from(path),
                    Line::styled(
                        item.modified.map(local_date).unwrap_or_default(),
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
                    Constraint::Length(12),
                ],
            )
            .row_highlight_style(style::selected())
            .block(Block::bordered().title(Span::styled(tr("Duplicate files"), style::heading())));
            let mut state = TableState::default().with_selected(Some(self.cursor));
            frame.render_stateful_widget(table, list_area, &mut state);
        }
        if self.confirming {
            let body = vec![Line::from(trf(
                "{n} copies ({size}) will be moved to the recycle bin. One copy of each file stays where it is.",
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

fn notes(report: &DuplicateReport) -> Vec<String> {
    let mut notes = Vec::new();
    if report.cancelled {
        notes.push(tr("The search was stopped; some duplicates may be missing.").to_owned());
    }
    if report.unreadable > 0 {
        notes.push(trf(
            "{n} files or folders could not be read.",
            &[("n", &count(report.unreadable))],
        ));
    }
    if report.cloud_skipped > 0 {
        notes.push(trf(
            "{n} files that are only in the cloud were not compared.",
            &[("n", &count(report.cloud_skipped))],
        ));
    }
    if report.program_folders_skipped > 0 {
        notes.push(trf(
            "{n} program folders were not searched: each program needs its own copy of its files.",
            &[("n", &count(report.program_folders_skipped))],
        ));
    }
    notes
}

/// What moving to the recycle bin did, as a note, and the paths it moved.
pub fn trashed(results: Vec<rewind_os_engine::TrashResult>) -> ((String, bool), Vec<PathBuf>) {
    let mut freed = 0;
    let mut moved = Vec::new();
    let mut failed = Vec::new();
    for r in results {
        match r.error {
            None => {
                freed += r.bytes;
                moved.push(r.path);
            }
            Some(err) => failed.push(format!("{}: {}", r.path.display(), tr_text(&err))),
        }
    }
    let mut text = trf(
        "Moved {size} to the recycle bin. Space is freed when the recycle bin is emptied.",
        &[("size", &bytes(freed))],
    );
    let error = !failed.is_empty();
    if error {
        text.push(' ');
        text.push_str(&failed.join("; "));
    }
    ((text, error), moved)
}
