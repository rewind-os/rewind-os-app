//! Home: the computer, its drives and what is worth doing.

use crate::app::Effect;
use crate::jobs::{Jobs, Purpose};
use crate::style;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, Wrap};
use rewind_os_engine::format::bytes;
use rewind_os_engine::recommend::{Level, Recommendation};
use rewind_os_engine::system::Overview;
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};

#[derive(Default)]
pub struct State {
    pub overview: Option<Overview>,
    pub recommendations: Option<Vec<Recommendation>>,
    pub error: Option<String>,
}

impl State {
    pub fn load(&mut self) -> Vec<Effect> {
        vec![
            Effect::Run(Purpose::Overview, Request::SystemOverview),
            Effect::Run(Purpose::Recommendations, Request::Recommendations),
        ]
    }

    pub fn key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Char('r') => self.load(),
            _ => Vec::new(),
        }
    }

    pub fn finished(&mut self, purpose: Purpose, outcome: Result<Response, String>) {
        match (purpose, outcome) {
            (Purpose::Overview, Ok(Response::Overview(o))) => self.overview = Some(o),
            (Purpose::Recommendations, Ok(Response::Recommendations(r))) => {
                self.recommendations = Some(r)
            }
            (_, Err(err)) => self.error = Some(err),
            _ => {}
        }
    }

    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![("r", tr("Refresh"))]
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, jobs: &Jobs) {
        let drives = self.overview.as_ref().map_or(1, |o| o.drives.len().max(1));
        let [system, drives_area, rest] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(u16::try_from(drives).unwrap_or(8).saturating_add(3).min(12)),
            Constraint::Min(3),
        ])
        .areas(area);

        let block = |title: &'static str| {
            Block::bordered().title(Span::styled(tr(title), style::heading()))
        };
        match &self.overview {
            Some(o) => {
                let line = Line::from(vec![
                    Span::styled(o.host.clone(), style::heading()),
                    Span::raw(" · "),
                    Span::raw(o.os.clone()),
                    Span::raw(" · "),
                    Span::raw(trf(
                        "{used} of {total} memory in use",
                        &[
                            ("used", &bytes(o.memory_used)),
                            ("total", &bytes(o.memory_total)),
                        ],
                    )),
                    Span::raw(" · "),
                    Span::styled(
                        trf(
                            "On for {hours} h",
                            &[("hours", &(o.uptime_secs / 3600).to_string())],
                        ),
                        style::muted(),
                    ),
                ]);
                frame.render_widget(Paragraph::new(line).block(block("This computer")), system);
                let rows = o.drives.iter().map(|d| {
                    let used = d.total.saturating_sub(d.available);
                    let share = if d.total == 0 {
                        0.0
                    } else {
                        used as f64 / d.total as f64
                    };
                    let bar_style = if share >= 0.9 {
                        style::warn()
                    } else {
                        style::size()
                    };
                    Row::new(vec![
                        Line::from(d.mount.display().to_string()),
                        Line::from(Span::styled(d.file_system.clone(), style::muted())),
                        Line::from(Span::styled(style::bar(share, 20), bar_style)),
                        Line::from(Span::styled(format!("{:>3.0} %", share * 100.0), bar_style)),
                        Line::from(trf(
                            "{free} free of {total}",
                            &[("free", &bytes(d.available)), ("total", &bytes(d.total))],
                        )),
                    ])
                });
                let table = Table::new(
                    rows,
                    [
                        Constraint::Fill(2),
                        Constraint::Length(8),
                        Constraint::Length(20),
                        Constraint::Length(5),
                        Constraint::Fill(2),
                    ],
                )
                .header(
                    Row::new(vec![tr("Drive"), "", tr("Used"), "", tr("Space")])
                        .style(style::muted()),
                )
                .block(block("Drives"));
                frame.render_widget(table, drives_area);
            }
            None => {
                let text = match &self.error {
                    Some(err) => Line::styled(err.clone(), style::error()),
                    None => Line::styled(tr("Reading this computer…"), style::muted()),
                };
                frame.render_widget(Paragraph::new(text).block(block("This computer")), system);
                frame.render_widget(block("Drives"), drives_area);
            }
        }

        let lines: Vec<Line> = match &self.recommendations {
            Some(list) if list.is_empty() => vec![Line::styled(
                tr("Nothing to do: this computer is in good shape."),
                style::good(),
            )],
            Some(list) => list.iter().flat_map(recommendation).collect(),
            None => {
                let stage = jobs
                    .running(Purpose::Recommendations)
                    .map(|r| r.progress.stage)
                    .filter(|s| !s.is_empty())
                    .map(tr)
                    .unwrap_or_else(|| tr("Looking for what is worth doing…"));
                vec![Line::styled(stage, style::muted())]
            }
        };
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(block("Worth doing")),
            rest,
        );
    }
}

/// A recommendation as two lines: its level as a word, title and saving, then its detail.
fn recommendation(r: &Recommendation) -> Vec<Line<'static>> {
    let (mark, word, mark_style) = match r.level {
        Level::Important => ("!", tr("Important"), style::error()),
        Level::Suggested => ("*", tr("Suggested"), style::size()),
        Level::Tip => ("-", tr("Tip"), style::muted()),
    };
    let args: Vec<(&str, &str)> = r.args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut first = vec![
        Span::styled(format!("{mark} {word}: "), mark_style),
        Span::styled(trf(r.title, &args), style::heading()),
    ];
    if let Some(saving) = r.saving {
        first.push(Span::raw("  "));
        first.push(Span::styled(bytes(saving), style::size()));
    }
    vec![
        Line::from(first),
        Line::styled(format!("  {}", tr(r.detail)), style::muted()),
    ]
}
