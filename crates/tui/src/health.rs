//! Health: how much charge the battery still holds, how each drive is doing (its health,
//! temperature, wear, hours and errors), and optimizing the drives the way the system does on
//! its own, after a confirmation.

use crate::app::Effect;
use crate::dialog;
use crate::jobs::{Jobs, Purpose};
use crate::list;
use crate::startup::field;
use crate::style;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState, Wrap};
use rewind_os_engine::battery::Battery;
use rewind_os_engine::disks::{Disk, Health, Report};
use rewind_os_engine::format::{bytes, count, local_date};
use rewind_os_engine::{Os, Request, Response};
use rewind_os_i18n::{tr, trf};

#[derive(Default)]
pub struct State {
    pub report: Option<Report>,
    pub batteries: Option<Vec<Battery>>,
    pub cursor: usize,
    /// Optimizing the drives is waiting for an answer.
    pub confirming: bool,
    /// What the last action said, and whether it is an error.
    pub note: Option<(Vec<String>, bool)>,
    pub elevated: bool,
    started: bool,
}

fn health_style(health: Health) -> Style {
    match health {
        Health::Good => style::good(),
        Health::Unknown => style::muted(),
        Health::Warning => style::warn(),
        Health::Failing => style::error(),
    }
}

impl State {
    pub fn open(&mut self) -> Vec<Effect> {
        if std::mem::replace(&mut self.started, true) {
            return Vec::new();
        }
        self.elevated = rewind_os_engine::system::is_elevated();
        self.load()
    }

    fn load(&mut self) -> Vec<Effect> {
        vec![
            Effect::Run(Purpose::Health, Request::DiskHealth),
            Effect::Run(Purpose::Battery, Request::BatteryHealth),
        ]
    }

    pub fn captures_keys(&self) -> bool {
        self.confirming
    }

    pub fn key(&mut self, key: KeyEvent, jobs: &Jobs) -> Vec<Effect> {
        if self.confirming {
            return match dialog::answer(key) {
                Some(true) => {
                    self.confirming = false;
                    self.note = None;
                    vec![Effect::Run(Purpose::Optimize, Request::DiskOptimize)]
                }
                Some(false) => {
                    self.confirming = false;
                    Vec::new()
                }
                None => Vec::new(),
            };
        }
        if jobs.running(Purpose::Optimize).is_some() {
            return Vec::new();
        }
        let drives = self.report.as_ref().map_or(0, |r| r.disks.len());
        if let Some(cursor) = list::moved(key.code, self.cursor, drives) {
            self.cursor = cursor;
            return Vec::new();
        }
        match key.code {
            KeyCode::Char('r') => {
                self.note = None;
                self.load()
            }
            KeyCode::Char('o') => {
                if !self.elevated {
                    self.note = Some((
                        vec![tr("This needs administrator rights. Start Rewind OS as administrator (with sudo on Linux).").to_owned()],
                        true,
                    ));
                } else {
                    self.confirming = true;
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub fn finished(&mut self, purpose: Purpose, outcome: Result<Response, String>) {
        match (purpose, outcome) {
            (Purpose::Health, Ok(Response::DiskHealth(report))) => {
                self.cursor = self.cursor.min(report.disks.len().saturating_sub(1));
                self.report = Some(report);
            }
            (Purpose::Battery, Ok(Response::Batteries(list))) => self.batteries = Some(list),
            // Without a battery report, the view shows the drives only.
            (Purpose::Battery, _) => self.batteries = Some(Vec::new()),
            (Purpose::Optimize, Ok(Response::ToolOutput(text))) => {
                let mut lines = vec![tr("The drives were optimized").to_owned()];
                lines.extend(text.lines().map(str::to_owned));
                self.note = Some((lines, false));
            }
            (_, Err(err)) => self.note = Some((vec![err], true)),
            _ => {}
        }
    }

    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.confirming {
            return vec![("y", tr("Optimize")), ("n", tr("Cancel"))];
        }
        vec![
            ("↑↓", tr("Move")),
            ("o", tr("Optimize now")),
            ("r", tr("Check again")),
        ]
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, jobs: &Jobs) {
        let batteries = self.batteries.as_deref().unwrap_or_default();
        let battery_rows = u16::try_from(batteries.len() * 2).unwrap_or(4);
        let [battery_area, drives_area, bottom] = Layout::vertical([
            Constraint::Length(if batteries.is_empty() {
                0
            } else {
                battery_rows + 2
            }),
            Constraint::Min(6),
            Constraint::Length(6),
        ])
        .areas(area);
        if !batteries.is_empty() {
            frame.render_widget(
                Paragraph::new(batteries.iter().flat_map(battery_lines).collect::<Vec<_>>())
                    .wrap(Wrap { trim: true })
                    .block(Block::bordered().title(Span::styled(tr("Battery"), style::heading()))),
                battery_area,
            );
        }

        let [list_area, detail] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                .areas(drives_area);
        let disks = self
            .report
            .as_ref()
            .map(|r| r.disks.as_slice())
            .unwrap_or_default();
        let rows = disks.iter().map(|d| {
            Row::new(vec![
                Line::styled(tr(d.health.label()), health_style(d.health)),
                Line::from(d.name.clone()),
                Line::styled(
                    format!("{} · {}", tr(d.media.label()), bytes(d.size)),
                    style::muted(),
                ),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(16),
                Constraint::Fill(3),
                Constraint::Fill(2),
            ],
        )
        .row_highlight_style(style::selected())
        .block(Block::bordered().title(Span::styled(tr("Drives"), style::heading())));
        let mut state = TableState::default().with_selected(Some(self.cursor));
        list::render(frame, table, list_area, &mut state, 0);

        let mut lines: Vec<Line> = Vec::new();
        match (disks.get(self.cursor), &self.report) {
            (Some(d), Some(report)) => lines.extend(drive_lines(d, report.missing.is_none())),
            (None, None) if jobs.running(Purpose::Health).is_some() => {
                lines.push(Line::styled(tr("Reading the drives…"), style::muted()));
            }
            (None, Some(_)) => lines.push(Line::raw(tr("No drives were found."))),
            _ => {}
        }
        if let Some(why) = self.report.as_ref().and_then(|r| r.missing) {
            lines.push(Line::raw(""));
            lines.push(Line::styled(tr(why), style::muted()));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::bordered()),
            detail,
        );

        let mut out: Vec<Line> = Vec::new();
        if let Some(running) = jobs.running(Purpose::Optimize) {
            out.push(dialog::progress(&running, "Optimizing the drives"));
            out.push(Line::styled(
                tr("This can take several minutes."),
                style::muted(),
            ));
        } else if let Some((lines, error)) = &self.note {
            let s = if *error { style::error() } else { Style::new() };
            out.extend(lines.iter().map(|l| Line::styled(l.clone(), s)));
        } else if let Some(report) = &self.report {
            let line = match (report.schedule.enabled, report.schedule.last) {
                (Some(true), Some(at)) => Line::styled(
                    trf("On: it last ran on {date}.", &[("date", &local_date(at))]),
                    style::good(),
                ),
                (Some(true), None) => Line::styled(tr("On."), style::good()),
                (Some(false), _) => Line::styled(
                    tr("Off: the drives are not optimized on their own."),
                    style::warn(),
                ),
                (None, _) => Line::raw(""),
            };
            out.push(line);
        }
        frame.render_widget(
            Paragraph::new(out)
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(Span::styled(tr("Optimization"), style::heading()))),
            bottom,
        );

        if self.confirming {
            let body = match Os::current() {
                Os::Windows => tr(
                    "Windows trims SSDs and defragments hard drives, as it does on its own. No file is deleted. On a hard drive this can take a long time; you can keep working meanwhile.",
                ),
                _ => tr(
                    "The system tells each SSD which space is free (TRIM), as it does on its own. No file is deleted.",
                ),
            };
            dialog::confirm(
                frame,
                area,
                tr("Optimize the drives now?"),
                vec![Line::from(body)],
                tr("Optimize"),
            );
        }
    }
}

/// A battery in two lines: what it holds against what it was made to hold, and what it means.
fn battery_lines(b: &Battery) -> Vec<Line<'static>> {
    let mut first = vec![Span::styled(format!("{}: ", b.name), style::heading())];
    match b.percent() {
        Some(p) => {
            first.push(Span::styled(
                format!("{p} %"),
                if b.worn() {
                    style::warn()
                } else {
                    style::good()
                },
            ));
            if let (Some(full), Some(design)) = (b.full_mwh, b.design_mwh) {
                first.push(Span::styled(
                    format!(
                        "  {}",
                        trf(
                            "{now} of {design} when new",
                            &[
                                ("now", &format!("{:.1}", full as f64 / 1000.0)),
                                ("design", &format!("{:.1} Wh", design as f64 / 1000.0)),
                            ],
                        )
                    ),
                    style::muted(),
                ));
            }
        }
        None => first.push(Span::styled(
            tr("This battery does not say how much it holds."),
            style::muted(),
        )),
    }
    if let Some(cycles) = b.cycles {
        first.push(Span::styled(
            format!(
                " · {}",
                trf("{n} charge cycles", &[("n", &count(u64::from(cycles)))])
            ),
            style::muted(),
        ));
    }
    let second = if b.worn() {
        tr(
            "It lasts about that share of its first time on a charge. Only a new battery brings that back; settings and calibration do not.",
        )
    } else {
        tr("Batteries hold a little less every year; below 80 % they count as worn.")
    };
    vec![Line::from(first), Line::styled(second, style::muted())]
}

/// The selected drive: where it is, its figures and what is wrong.
fn drive_lines(d: &Disk, readable: bool) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(d.name.clone(), style::heading()),
        field(tr("Health"), tr(d.health.label())),
    ];
    if !d.bus.is_empty() {
        lines.push(field(tr("Connected by"), &d.bus));
    }
    if !d.volumes.is_empty() {
        lines.push(field(tr("Volumes"), &d.volumes.join(" ")));
    }
    if let Some(c) = d.temperature_c {
        lines.push(field(tr("Temperature"), &format!("{c} °C")));
    }
    if let Some(used) = d.used_life {
        lines.push(field(tr("Rated life used"), &format!("{used} %")));
    }
    if let Some(hours) = d.power_on_hours {
        lines.push(field(tr("Powered on"), &format!("{} h", count(hours))));
    }
    if let Some(errors) = d.errors {
        lines.push(field(tr("Uncorrected errors"), &count(errors)));
    }
    if let Some(sectors) = d.bad_sectors {
        lines.push(field(tr("Damaged sectors"), &count(sectors)));
    }
    if !d.problems.is_empty() {
        lines.push(Line::raw(""));
        for p in &d.problems {
            lines.push(Line::styled(tr(p.label()), health_style(d.health)));
        }
    } else if d.health == Health::Unknown && readable {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            tr("This drive does not say how it is doing."),
            style::muted(),
        ));
    }
    lines
}
