//! The startup page's second view: how long the computer takes to start, and what slowed it
//! down (`boot.report`). A program that slowed starts and is a startup entry the app can turn
//! off gets its switch right there.

use crate::app::Shared;
use crate::charts;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, card};
use egui::RichText;
use rewind_os_engine::boot::{Kind, Report, Slow};
use rewind_os_engine::format::{count, duration_ms, local_date};
use rewind_os_engine::startup::{Source, StartupEntry};
use rewind_os_engine::{Os, Request, Response};
use rewind_os_i18n::{tr, trf};

/// Starts shown as bars.
const SHOWN_BOOTS: usize = 10;

#[derive(Default)]
pub struct Speed {
    job: Option<Job>,
    report: Option<Result<Report, String>>,
}

impl Speed {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Draws the view. Returns the startup entry the user asked to turn off.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        shared: &mut Shared,
        entries: &[StartupEntry],
    ) -> Option<String> {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        // Windows keeps its record of starts where only administrators can read it.
        if Os::current() == Os::Windows && !shared.elevated {
            ui.label(tr(
                "Windows records how long each start took and what slowed it down, but only administrators can read that record.",
            ));
            ui.add_space(t.gap * 0.5);
            shared.admin_hint(ui);
            return None;
        }
        if self.report.is_none() && self.job.is_none() {
            self.job = Some(Job::start(&ctx, &shared.engine, Request::BootReport));
        }
        match jobs::finished(&mut self.job, &ctx) {
            Some(Ok(Response::Boot(report))) => self.report = Some(Ok(report)),
            Some(Ok(_)) => {}
            Some(Err(err)) => self.report = Some(Err(err)),
            None => {}
        }
        let report = match &self.report {
            None => {
                ui.spinner();
                return None;
            }
            Some(Err(err)) => {
                ui.label(RichText::new(err).color(t.text_muted));
                return None;
            }
            Some(Ok(report)) => report,
        };
        let Some(last) = report.boots.first() else {
            ui.label(tr("The system has no record of its starts yet."));
            return None;
        };
        let mut turn_off = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                figures(ui, report, last.total_ms, last.desktop_ms);
                if report.boots.len() > 1 {
                    ui.add_space(t.gap);
                    recent(ui, report);
                }
                if !report.slow.is_empty() {
                    ui.add_space(t.gap);
                    turn_off = slowest(ui, report, entries, shared.elevated);
                }
            });
        turn_off
    }
}

/// The last start, until the desktop, and the average or the slowest step.
fn figures(ui: &mut egui::Ui, report: &Report, total: u64, desktop: Option<u64>) {
    let date = report
        .boots
        .first()
        .and_then(|b| b.at)
        .map(local_date)
        .unwrap_or_default();
    let average = (report.boots.len() > 1)
        .then(|| report.boots.iter().map(|b| b.total_ms).sum::<u64>() / report.boots.len() as u64);
    charts::grid(ui, 3, 180.0, |ui, i| match i {
        0 => charts::stat_tile(ui, tr("Last start"), &duration_ms(total), &date, None),
        1 => charts::stat_tile(
            ui,
            tr("Until the desktop was ready"),
            &desktop.map(duration_ms).unwrap_or_else(|| "-".to_owned()),
            tr("The rest happens while you can already work"),
            None,
        ),
        _ => match average {
            Some(avg) => charts::stat_tile(
                ui,
                tr("Average start"),
                &duration_ms(avg),
                &trf(
                    "of the last {n} starts",
                    &[("n", &count(report.boots.len() as u64))],
                ),
                None,
            ),
            None => {
                let slowest = report.slow.first();
                charts::stat_tile(
                    ui,
                    tr("Slowest step"),
                    &slowest.map(|s| duration_ms(s.delay_ms)).unwrap_or_default(),
                    slowest.map(|s| s.name.as_str()).unwrap_or(""),
                    None,
                )
            }
        },
    });
}

/// The last starts as bars, newest first, each with its date and duration.
fn recent(ui: &mut egui::Ui, report: &Report) {
    let t = theme::current(ui.ctx());
    let longest = report
        .boots
        .iter()
        .map(|b| b.total_ms)
        .max()
        .unwrap_or(1)
        .max(1);
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr("Recent starts")).family(theme::bold()));
        ui.add_space(8.0);
        for boot in report.boots.iter().take(SHOWN_BOOTS) {
            let date = boot.at.map(local_date).unwrap_or_default();
            let time = duration_ms(boot.total_ms);
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(96.0, 20.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| ui.label(RichText::new(&date).small().color(t.text_muted)),
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(84.0, 20.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| ui.label(RichText::new(&time).family(theme::bold())),
                );
                let fraction = boot.total_ms as f32 / longest as f32;
                widgets::usage_bar(
                    ui,
                    fraction,
                    t.accent,
                    &trf(
                        "Start on {date}: {time}",
                        &[("date", &date), ("time", &time)],
                    ),
                );
            });
        }
    });
}

/// The startup entry behind a slow program or service, when the app can turn it off.
fn entry_for<'a>(slow: &Slow, entries: &'a [StartupEntry]) -> Option<&'a StartupEntry> {
    let file = slow.file.to_ascii_lowercase();
    entries.iter().find(|e| {
        e.enabled
            && !e.essential
            && match slow.kind {
                Kind::Program => e.command.to_ascii_lowercase().contains(&file),
                Kind::Service | Kind::Unit => {
                    let name = file.strip_suffix(".service").unwrap_or(&file);
                    matches!(e.source, Source::Service)
                        && (e.name.eq_ignore_ascii_case(name)
                            || e.id.to_ascii_lowercase().ends_with(&format!("|{name}")))
                }
                _ => false,
            }
    })
}

/// What made starts slower, largest delay first. Returns the entry the user asked to turn off.
fn slowest(
    ui: &mut egui::Ui,
    report: &Report,
    entries: &[StartupEntry],
    elevated: bool,
) -> Option<String> {
    let t = theme::current(ui.ctx());
    let mut turn_off = None;
    charts::section_title(
        ui,
        if report.history {
            tr("What made starts slower than usual")
        } else {
            tr("What took longest to start since the last start")
        },
    );
    ui.add_space(6.0);
    for (index, slow) in report.slow.iter().enumerate() {
        let entry = entry_for(slow, entries);
        widgets::list_row(ui, index, 56.0, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(entry) = entry {
                    let allowed = !entry.needs_admin || elevated;
                    let button = widgets::row_button(ui, tr("Turn off at startup"), allowed);
                    let button = if allowed {
                        button.on_hover_text(tr(
                            "It stops starting with the system; you can turn it back on in the list of startup programs.",
                        ))
                    } else {
                        button.on_disabled_hover_text(tr("This needs administrator rights."))
                    };
                    if button.clicked() {
                        turn_off = Some(entry.id.clone());
                    }
                }
                let delay = if report.history {
                    trf("+{time}", &[("time", &duration_ms(slow.delay_ms))])
                } else {
                    duration_ms(slow.delay_ms)
                };
                ui.label(RichText::new(delay).family(theme::bold()));
                widgets::badge(ui, tr(slow.kind.label()), t.text_muted);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let mut parts = Vec::new();
                    if slow.file != slow.name {
                        parts.push(slow.file.clone());
                    }
                    if report.history {
                        parts.push(trf(
                            "slowed {n} starts",
                            &[("n", &count(u64::from(slow.times)))],
                        ));
                    }
                    let line = parts.join(" · ");
                    widgets::card_text(ui, &slow.name, &line);
                });
            });
        });
        ui.add_space(6.0);
    }
    turn_off
}
