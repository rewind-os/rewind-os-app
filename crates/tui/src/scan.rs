//! Disk scan: folders by size, into and out of them, and the largest files.

use crate::app::Effect;
use crate::jobs::{Jobs, Purpose};
use crate::style;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState};
use rewind_os_engine::format::{bytes, count, duration_ms, local_date};
use rewind_os_engine::scan::{NodeId, ScanTree};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};
use std::path::PathBuf;
use std::sync::Arc;

pub struct State {
    /// The folder to scan, as typed.
    pub input: String,
    /// The folder field has the keys.
    pub editing: bool,
    pub tree: Option<Arc<ScanTree>>,
    /// The folder shown.
    pub current: NodeId,
    pub cursor: usize,
    /// Show the largest files instead of folders.
    pub files: bool,
    pub error: Option<String>,
}

impl State {
    pub fn new(root: PathBuf) -> Self {
        State {
            input: root.display().to_string(),
            editing: false,
            tree: None,
            current: ScanTree::ROOT,
            cursor: 0,
            files: false,
            error: None,
        }
    }

    /// The folders inside the one shown, largest first.
    pub fn rows(&self) -> Vec<NodeId> {
        self.tree
            .as_ref()
            .map(|t| t.children(self.current).map(|(id, _)| id).collect())
            .unwrap_or_default()
    }

    fn len(&self) -> usize {
        match (&self.tree, self.files) {
            (Some(t), true) => t.top_files.len(),
            (Some(_), false) => self.rows().len(),
            (None, _) => 0,
        }
    }

    fn start(&mut self) -> Vec<Effect> {
        let root = PathBuf::from(self.input.trim());
        self.error = None;
        vec![Effect::Run(Purpose::Scan, Request::DiskScan { root })]
    }

    /// The field takes every key while it is open.
    pub fn captures_keys(&self) -> bool {
        self.editing
    }

    pub fn key(&mut self, key: KeyEvent, jobs: &Jobs) -> Vec<Effect> {
        if self.editing {
            match key.code {
                KeyCode::Enter => {
                    self.editing = false;
                    return self.start();
                }
                KeyCode::Esc => self.editing = false,
                KeyCode::Backspace => {
                    self.input.pop();
                }
                KeyCode::Char(c) => self.input.push(c),
                _ => {}
            }
            return Vec::new();
        }
        if jobs.running(Purpose::Scan).is_some() {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('s') => vec![Effect::Stop(Purpose::Scan)],
                _ => Vec::new(),
            };
        }
        let last = self.len().saturating_sub(1);
        match key.code {
            KeyCode::Char('e') | KeyCode::Char('/') => self.editing = true,
            KeyCode::Char('r') => return self.start(),
            KeyCode::Enter if self.tree.is_none() => return self.start(),
            KeyCode::Char('f') => {
                self.files = !self.files;
                self.cursor = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => self.cursor = (self.cursor + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::PageDown => self.cursor = (self.cursor + 10).min(last),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(10),
            KeyCode::Home | KeyCode::Char('g') => self.cursor = 0,
            KeyCode::End | KeyCode::Char('G') => self.cursor = last,
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') if !self.files => self.enter(),
            KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') if !self.files => self.up(),
            _ => {}
        }
        Vec::new()
    }

    /// Into the folder under the cursor, if it has folders inside.
    fn enter(&mut self) {
        let Some(tree) = &self.tree else { return };
        if let Some(&id) = self.rows().get(self.cursor)
            && tree.children(id).next().is_some()
        {
            self.current = id;
            self.cursor = 0;
        }
    }

    /// Back to the folder above, with the cursor on the one left.
    fn up(&mut self) {
        let Some(tree) = &self.tree else { return };
        let Some(parent) = tree.node(self.current).and_then(|n| n.parent) else {
            return;
        };
        let left = self.current;
        self.current = parent;
        self.cursor = self.rows().iter().position(|&id| id == left).unwrap_or(0);
    }

    pub fn finished(&mut self, outcome: Result<Response, String>) {
        match outcome {
            Ok(Response::Scan(tree)) => {
                self.tree = Some(tree);
                self.current = ScanTree::ROOT;
                self.cursor = 0;
                self.files = false;
            }
            Ok(_) => {}
            Err(err) => self.error = Some(err),
        }
    }

    pub fn hints(&self, jobs: &Jobs) -> Vec<(&'static str, &'static str)> {
        if self.editing {
            return vec![("Enter", tr("Scan")), ("Esc", tr("Cancel"))];
        }
        if jobs.running(Purpose::Scan).is_some() {
            return vec![("Esc", tr("Stop"))];
        }
        if self.tree.is_none() {
            return vec![("Enter", tr("Scan")), ("e", tr("Choose a folder"))];
        }
        let mut hints = vec![("↑↓", tr("Move"))];
        if !self.files {
            hints.push(("Enter/←", tr("Open / back")));
        }
        hints.extend([
            (
                "f",
                if self.files {
                    tr("Folders")
                } else {
                    tr("Largest files")
                },
            ),
            ("e", tr("Choose a folder")),
            ("r", tr("Scan again")),
        ]);
        hints
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, jobs: &Jobs) {
        let [field, body] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).areas(area);
        let field_style = if self.editing {
            style::active()
        } else {
            ratatui::style::Style::new()
        };
        let mut input = vec![Span::styled(self.input.clone(), field_style)];
        if self.editing {
            input.push(Span::styled("▏", style::size()));
        }
        frame.render_widget(
            Paragraph::new(Line::from(input)).block(
                Block::bordered().title(Span::styled(tr("Folder to scan"), style::heading())),
            ),
            field,
        );

        if let Some(running) = jobs.running(Purpose::Scan) {
            let p = &running.progress;
            let elapsed = u64::try_from(running.elapsed.as_millis()).unwrap_or(u64::MAX);
            let mut lines = vec![Line::from(vec![
                Span::styled(
                    tr(if p.stage.is_empty() {
                        "Scanning"
                    } else {
                        p.stage
                    }),
                    style::heading(),
                ),
                Span::raw(" · "),
                Span::raw(trf("{n} items", &[("n", &count(p.items))])),
                Span::raw(" · "),
                Span::styled(bytes(p.bytes), style::size()),
                Span::raw(" · "),
                Span::styled(duration_ms(elapsed - elapsed % 1000), style::muted()),
            ])];
            if running.stopping {
                lines.push(Line::styled(tr("Stopping…"), style::warn()));
            }
            frame.render_widget(Paragraph::new(lines).block(Block::bordered()), body);
            return;
        }

        let Some(tree) = &self.tree else {
            let text = match &self.error {
                Some(err) => Line::styled(err.clone(), style::error()),
                None => Line::styled(
                    tr("Press Enter to measure every folder in it, or e to choose another folder."),
                    style::muted(),
                ),
            };
            frame.render_widget(Paragraph::new(text).block(Block::bordered()), body);
            return;
        };

        let notes = notes(tree);
        let [summary, list] = Layout::vertical([
            Constraint::Length(u16::try_from(notes.len()).unwrap_or(4).saturating_add(1)),
            Constraint::Min(3),
        ])
        .areas(body);
        let shown = tree.node(self.current);
        let mut lines = vec![Line::from(vec![
            Span::styled(
                tree.path(self.current).display().to_string(),
                style::heading(),
            ),
            Span::raw("  "),
            Span::styled(bytes(shown.map_or(0, |n| n.bytes)), style::size()),
            Span::raw(" · "),
            Span::raw(trf(
                "{n} files",
                &[("n", &count(shown.map_or(0, |n| n.files)))],
            )),
        ])];
        lines.extend(notes);
        frame.render_widget(Paragraph::new(lines), summary);

        let mut state = TableState::default().with_selected(Some(self.cursor));
        if self.files {
            let rows = tree.top_files.iter().map(|f| {
                Row::new(vec![
                    Line::styled(format!("{:>10}", bytes(f.bytes)), style::size()),
                    Line::from(f.path.display().to_string()),
                    Line::styled(
                        f.modified.map(local_date).unwrap_or_default(),
                        style::muted(),
                    ),
                ])
            });
            let table = Table::new(
                rows,
                [
                    Constraint::Length(10),
                    Constraint::Fill(1),
                    Constraint::Length(12),
                ],
            )
            .row_highlight_style(style::selected())
            .block(Block::bordered().title(Span::styled(
                tr("Largest files in the scan"),
                style::heading(),
            )));
            frame.render_stateful_widget(table, list, &mut state);
            return;
        }
        let total = shown.map_or(0, |n| n.bytes).max(1);
        let rows = self.rows().into_iter().filter_map(|id| {
            let n = tree.node(id)?;
            let has_folders = tree.children(id).next().is_some();
            let name = if has_folders {
                format!("{}/", n.name)
            } else {
                n.name.to_string()
            };
            let mut name_line = vec![Span::raw(name)];
            if n.incomplete {
                name_line.push(Span::styled(
                    format!("  {}", tr("incomplete")),
                    style::warn(),
                ));
            }
            Some(Row::new(vec![
                Line::styled(format!("{:>10}", bytes(n.bytes)), style::size()),
                Line::styled(style::bar(n.bytes as f64 / total as f64, 16), style::size()),
                Line::from(name_line),
                Line::styled(trf("{n} files", &[("n", &count(n.files))]), style::muted()),
                Line::styled(
                    n.modified.map(local_date).unwrap_or_default(),
                    style::muted(),
                ),
            ]))
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Length(16),
                Constraint::Fill(1),
                Constraint::Length(18),
                Constraint::Length(12),
            ],
        )
        .row_highlight_style(style::selected())
        .block(Block::bordered().title(Span::styled(tr("Folders by size"), style::heading())));
        frame.render_stateful_widget(table, list, &mut state);
    }
}

/// What the person should know about the scan: stopped, unreadable folders, repeated mounts.
fn notes(tree: &ScanTree) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if tree.cancelled {
        lines.push(Line::styled(
            tr("The scan was stopped; sizes are incomplete."),
            style::warn(),
        ));
    }
    if tree.unreadable_dirs > 0 {
        lines.push(Line::styled(
            trf(
                "{n} folders could not be read (administrator rights may help).",
                &[("n", &count(tree.unreadable_dirs))],
            ),
            style::warn(),
        ));
    }
    for (point, original) in &tree.repeated_mounts {
        lines.push(Line::styled(
            trf(
                "{folder} shows {original} again; its files were counted there.",
                &[
                    ("folder", &point.display().to_string()),
                    ("original", &original.display().to_string()),
                ],
            ),
            style::muted(),
        ));
    }
    lines
}
