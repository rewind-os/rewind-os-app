use crate::app::Shared;
use crate::charts;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, card, page_header};
use egui::RichText;
use rewind_os_engine::cleanup::{CategoryReport, CategoryResult, Disposal, Group, Risk};
use rewind_os_engine::format::{bytes, count, local_date};
use rewind_os_engine::schedule::{Frequency, Status};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};
use std::collections::HashSet;

#[derive(Default)]
pub struct State {
    analyze: Option<Job>,
    run: Option<Job>,
    reports: Option<Vec<CategoryReport>>,
    selected: HashSet<&'static str>,
    results: Option<Vec<CategoryResult>>,
    confirming: bool,
    /// Asking before cleaning the categories that need administrator rights.
    confirming_system: bool,
    started: bool,
    /// The scheduled cleanup: reading it, changing it, and the frequency waiting for an answer.
    schedule_job: Option<Job>,
    schedule: Option<Status>,
    confirming_schedule: Option<Frequency>,
}

impl State {
    pub fn busy(&self) -> bool {
        self.analyze.is_some() || self.run.is_some() || self.schedule_job.is_some()
    }

    /// Reads or changes the schedule; the answer comes back through `poll_schedule`.
    fn schedule_request(&mut self, ctx: &egui::Context, shared: &Shared, request: Request) {
        self.schedule_job = Some(Job::start(ctx, &shared.engine, request));
    }

    fn poll_schedule(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        match jobs::finished(&mut self.schedule_job, ctx) {
            Some(Ok(Response::Schedule(status))) => self.schedule = Some(status),
            Some(Ok(Response::Done(message))) => {
                shared.info(tr(message));
                self.schedule_request(ctx, shared, Request::ScheduleStatus);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => {
                shared.error(err);
                self.schedule_request(ctx, shared, Request::ScheduleStatus);
            }
            None => {}
        }
    }
}

/// The card that turns the scheduled cleanup on and off, and says how it last went. Returns
/// the frequency picked (`Some(None)` for off).
fn schedule_card(
    ui: &mut egui::Ui,
    status: Option<&Status>,
    busy: bool,
) -> Option<Option<Frequency>> {
    let t = theme::current(ui.ctx());
    let status = status?;
    let names: Vec<&str> = status
        .categories
        .iter()
        .filter_map(|id| {
            rewind_os_engine::cleanup::categories()
                .iter()
                .find(|c| c.id == *id)
                .map(|c| tr(c.name))
        })
        .collect();
    let current = status.frequency;
    let last = status.runs.first().cloned();
    let mut pick: Option<Option<Frequency>> = None;
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(tr("Clean automatically"))
                .strong()
                .family(theme::bold()),
        );
        ui.label(
            RichText::new(trf(
                "The system's scheduler cleans these on its own: {list}. Nothing that needs a look or administrator rights is included.",
                &[("list", &names.join(", "))],
            ))
            .color(t.text_muted),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (choice, label) in [
                (None, tr("Off")),
                (Some(Frequency::Weekly), tr(Frequency::Weekly.label())),
                (Some(Frequency::FourWeeks), tr(Frequency::FourWeeks.label())),
            ] {
                let response =
                    ui.add_enabled(!busy, egui::Button::selectable(current == choice, label));
                if response.clicked() && current != choice {
                    pick = Some(choice);
                }
            }
            if busy {
                ui.spinner();
            }
        });
        if let Some(run) = &last {
            ui.add_space(4.0);
            ui.label(
                RichText::new(trf(
                    "Last run {date}: {size} freed, {files} files.",
                    &[
                        ("date", &local_date(run.at)),
                        ("size", &bytes(run.bytes)),
                        ("files", &count(run.files)),
                    ],
                ))
                .color(t.text_muted),
            );
            if run.failed > 0 {
                ui.label(
                    RichText::new(trf(
                        "{n} files were in use and stayed.",
                        &[("n", &count(run.failed))],
                    ))
                    .color(t.warn),
                );
            }
            for (id, why) in &run.skipped {
                ui.label(
                    RichText::new(format!("{id}: {}", rewind_os_i18n::tr_text(why))).color(t.warn),
                );
            }
        }
    });
    pick
}

impl State {
    fn start_analyze(&mut self, ctx: &egui::Context, shared: &Shared) {
        self.started = true;
        self.analyze = Some(Job::start(ctx, &shared.engine, Request::CleanupAnalyze));
    }

    fn poll(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        match jobs::finished(&mut self.analyze, ctx) {
            Some(Ok(Response::CleanupAnalysis(reports))) => {
                let total: u64 = reports.iter().filter(|r| !r.blocked).map(|r| r.bytes).sum();
                shared.info(rewind_os_i18n::trf(
                    "Analysis finished: {size} of junk found.",
                    &[("size", &rewind_os_engine::format::bytes(total))],
                ));
                if let Some(wanted) = shared.cleanup_request.take() {
                    // A recommendation chose these categories.
                    self.selected = reports
                        .iter()
                        .filter(|r| wanted.iter().any(|w| w == r.category.id) && !r.blocked)
                        .map(|r| r.category.id)
                        .collect();
                } else if self.reports.is_none() {
                    self.selected = reports
                        .iter()
                        .filter(|r| r.category.selected_by_default && r.present && !r.blocked)
                        .map(|r| r.category.id)
                        .collect();
                }
                self.reports = Some(reports);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        match jobs::finished(&mut self.run, ctx) {
            Some(Ok(Response::CleanupDone(results))) => {
                let freed: u64 = results.iter().map(|r| r.bytes).sum();
                let kept: u64 = results.iter().map(|r| r.failed).sum();
                let mut text = trf("Freed {size}.", &[("size", &bytes(freed))]);
                if kept > 0 {
                    text.push(' ');
                    text.push_str(&trf("{n} files in use were kept", &[("n", &count(kept))]));
                }
                shared.info(text);
                self.results = Some(results);
                self.start_analyze(ctx, shared);
            }
            // The categories cleaned with administrator rights answer as JSON.
            Some(Ok(Response::Elevated(value))) => {
                let results = value["data"].as_array().cloned().unwrap_or_default();
                let sum =
                    |key: &str| -> u64 { results.iter().filter_map(|r| r[key].as_u64()).sum() };
                let mut text = trf("Freed {size}.", &[("size", &bytes(sum("bytes")))]);
                let kept = sum("failed");
                if kept > 0 {
                    text.push(' ');
                    text.push_str(&trf("{n} files in use were kept", &[("n", &count(kept))]));
                }
                shared.info(text);
                self.start_analyze(ctx, shared);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
    }

    /// Safe categories that need administrator rights to read and clean.
    fn system_categories(&self) -> Vec<&'static str> {
        self.reports
            .iter()
            .flatten()
            .filter(|r| r.blocked && r.present && r.category.selected_by_default)
            .map(|r| r.category.id)
            .collect()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        {
            let ctx = ui.ctx().clone();
            if self.schedule.is_none() && self.schedule_job.is_none() {
                self.schedule_request(&ctx, shared, Request::ScheduleStatus);
            }
            self.poll_schedule(&ctx, shared);
        }
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if !self.started || (shared.cleanup_request.is_some() && self.analyze.is_none()) {
            self.start_analyze(&ctx, shared);
        }
        self.poll(&ctx, shared);
        page_header(
            ui,
            tr("Junk cleanup"),
            tr(
                "Temporary files and caches that the system and your programs recreate when they need them.",
            ),
        );

        let selected_bytes: u64 = self
            .reports
            .iter()
            .flatten()
            .filter(|r| self.selected.contains(r.category.id))
            .map(|r| r.bytes)
            .sum();
        let busy = self.analyze.is_some() || self.run.is_some();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new(tr("Analyze again")))
                .clicked()
            {
                self.start_analyze(&ctx, shared);
            }
            let label = trf("Clean {size}", &[("size", &bytes(selected_bytes))]);
            if widgets::primary_button(ui, &label, !busy && !self.selected.is_empty()).clicked() {
                self.confirming = true;
            }
        });
        if !shared.elevated {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(tr("Some categories need administrator rights."))
                        .small()
                        .color(t.text_muted),
                );
                if rewind_os_engine::tool("system.relaunch_elevated").is_some_and(|t| t.available())
                {
                    let restart = egui::Button::new(
                        RichText::new(tr("Restart as administrator"))
                            .small()
                            .color(t.accent),
                    )
                    .frame(false);
                    if ui.add(restart).clicked() {
                        shared.relaunch_elevated(&ctx);
                    }
                } else if crate::app::can_elevate_actions() {
                    let system = self.system_categories();
                    let label = trf(
                        "Clean {n} system categories (asks for your password)…",
                        &[("n", &system.len().to_string())],
                    );
                    let button = egui::Button::new(RichText::new(label).small().color(t.accent))
                        .frame(false);
                    if ui
                        .add_enabled(!busy && !system.is_empty(), button)
                        .clicked()
                    {
                        self.confirming_system = true;
                    }
                } else {
                    ui.label(
                        RichText::new("sudo rewind-os-cli clean run --defaults --yes")
                            .small()
                            .monospace()
                            .color(t.text_muted),
                    );
                }
            });
        }
        ui.add_space(t.gap);
        for job in [&self.analyze, &self.run].into_iter().flatten() {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::job_progress(ui, job, false);
            });
            ui.add_space(t.gap);
        }
        if let Some(results) = &self.results {
            results_card(ui, results, self.reports.as_deref());
            ui.add_space(t.gap);
        }

        let Some(reports) = &self.reports else { return };
        if !reports.iter().any(|r| r.present || r.bytes > 0) {
            ui.label(tr(
                "None of the known junk locations exist on this computer.",
            ));
            return;
        }
        let mut schedule_pick = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                breakdown(ui, reports, selected_bytes, self.selected.len());
                ui.add_space(t.gap);
                schedule_pick =
                    schedule_card(ui, self.schedule.as_ref(), self.schedule_job.is_some());
                ui.add_space(t.gap * 1.5);
                for group in Group::ALL {
                    let items: Vec<&CategoryReport> = reports
                        .iter()
                        .filter(|r| r.category.group == group && (r.present || r.bytes > 0))
                        .collect();
                    if items.is_empty() {
                        continue;
                    }
                    charts::section_title(ui, tr(group.label()));
                    card(ui, |ui| {
                        ui.set_width(ui.available_width());
                        for (i, report) in items.iter().enumerate() {
                            if i > 0 {
                                ui.separator();
                            }
                            category_row(ui, report, &mut self.selected);
                        }
                    });
                    ui.add_space(t.gap);
                }
            });
        match schedule_pick {
            // Turning it off deletes nothing: at once.
            Some(None) => {
                self.schedule_job = Some(Job::start(
                    ui.ctx(),
                    &shared.engine,
                    Request::ScheduleSet { frequency: None },
                ));
            }
            Some(Some(f)) => self.confirming_schedule = Some(f),
            None => {}
        }

        if let Some(frequency) = self.confirming_schedule {
            match widgets::confirm(
                ui.ctx(),
                "confirm_schedule",
                tr("Clean automatically?"),
                tr(
                    "Each run deletes these caches without asking, as the safe cleanup here does. You can turn it off here at any time.",
                ),
                tr("Turn on"),
            ) {
                Choice::Open => {}
                Choice::Cancelled => self.confirming_schedule = None,
                Choice::Confirmed => {
                    self.confirming_schedule = None;
                    self.schedule_job = Some(Job::start(
                        ui.ctx(),
                        &shared.engine,
                        Request::ScheduleSet {
                            frequency: Some(frequency),
                        },
                    ));
                }
            }
        }

        if self.confirming_system {
            let system = self.system_categories();
            let names: Vec<&str> = self
                .reports
                .iter()
                .flatten()
                .filter(|r| system.contains(&r.category.id))
                .map(|r| tr(r.category.name))
                .collect();
            let body = format!(
                "{}\n\n• {}",
                tr(
                    "These system categories are deleted permanently. The system asks for your password for this one action; the app itself keeps running without administrator rights."
                ),
                names.join("\n• ")
            );
            match widgets::confirm(
                &ctx,
                "confirm_system_cleanup",
                tr("Clean the system categories?"),
                &body,
                tr("Clean"),
            ) {
                Choice::Open => {}
                Choice::Cancelled => self.confirming_system = false,
                Choice::Confirmed => {
                    self.confirming_system = false;
                    let categories = system.iter().map(|s| (*s).to_owned()).collect();
                    let request = shared.with_rights(Request::CleanupRun { categories });
                    self.run = Some(Job::start(&ctx, &shared.engine, request));
                }
            }
        }
        if self.confirming {
            let chosen: Vec<&CategoryReport> = reports
                .iter()
                .filter(|r| self.selected.contains(r.category.id))
                .collect();
            let to_bin = chosen
                .iter()
                .any(|r| r.category.disposal == Disposal::RecycleBin);
            let mut body = trf(
                "{n} categories, about {size}, will be deleted permanently.",
                &[
                    ("n", &chosen.len().to_string()),
                    ("size", &bytes(selected_bytes)),
                ],
            );
            if to_bin {
                body.push(' ');
                body.push_str(tr("Old installers go to the recycle bin instead."));
            }
            body.push(' ');
            body.push_str(tr("Files a program has open are skipped."));
            match widgets::confirm(
                &ctx,
                "confirm_cleanup",
                tr("Clean the selected categories?"),
                &body,
                tr("Clean"),
            ) {
                Choice::Open => {}
                Choice::Cancelled => self.confirming = false,
                Choice::Confirmed => {
                    self.confirming = false;
                    let categories = self.selected.iter().map(|s| (*s).to_owned()).collect();
                    self.run = Some(Job::start(
                        &ctx,
                        &shared.engine,
                        Request::CleanupRun { categories },
                    ));
                }
            }
        }
    }
}

/// Key figures and where the junk is: one bar split by group, with its legend.
fn breakdown(ui: &mut egui::Ui, reports: &[CategoryReport], selected_bytes: u64, selected: usize) {
    let t = theme::current(ui.ctx());
    let found: u64 = reports.iter().map(|r| r.bytes).sum();
    let with_junk = reports.iter().filter(|r| r.bytes > 0).count();
    let files: u64 = reports.iter().map(|r| r.files).sum();
    charts::grid(ui, 3, 180.0, |ui, i| match i {
        0 => charts::stat_tile(
            ui,
            tr("Junk found"),
            &bytes(found),
            &trf("{n} files", &[("n", &count(files))]),
            None,
        ),
        1 => charts::stat_tile(
            ui,
            tr("Selected to clean"),
            &bytes(selected_bytes),
            &trf("{n} categories", &[("n", &selected.to_string())]),
            Some((
                if found > 0 {
                    (selected_bytes as f64 / found as f64) as f32
                } else {
                    0.0
                },
                t.accent,
            )),
        ),
        _ => charts::stat_tile(
            ui,
            tr("Categories with junk"),
            &with_junk.to_string(),
            &trf("of {n} checked", &[("n", &reports.len().to_string())]),
            None,
        ),
    });
    let groups: Vec<(Group, u64)> = Group::ALL
        .into_iter()
        .map(|g| {
            let size = reports
                .iter()
                .filter(|r| r.category.group == g)
                .map(|r| r.bytes)
                .sum();
            (g, size)
        })
        .filter(|(_, size)| *size > 0)
        .collect();
    if groups.is_empty() {
        return;
    }
    ui.add_space(t.gap * 0.75);
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(tr("Where the junk is"))
                .strong()
                .family(crate::theme::bold()),
        );
        ui.add_space(8.0);
        let sizes: Vec<u64> = groups.iter().map(|(_, s)| *s).collect();
        let parts = charts::shares(t, &sizes, found);
        let summary = groups
            .iter()
            .map(|(g, s)| format!("{} {}", tr(g.label()), bytes(*s)))
            .collect::<Vec<_>>()
            .join(", ");
        charts::stacked_bar(ui, 12.0, &parts, &summary);
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for ((group, size), (fraction, color)) in groups.iter().zip(&parts) {
                ui.allocate_ui(egui::vec2(230.0, 20.0), |ui| {
                    ui.set_width(230.0);
                    charts::legend_row(
                        ui,
                        *color,
                        tr(group.label()),
                        &bytes(*size),
                        Some(*fraction),
                    );
                });
                ui.add_space(t.gap);
            }
        });
    });
}

fn category_row(ui: &mut egui::Ui, report: &CategoryReport, selected: &mut HashSet<&'static str>) {
    let t = theme::current(ui.ctx());
    let c = &report.category;
    let enabled = !report.blocked && (report.bytes > 0 || report.files > 0);
    ui.horizontal(|ui| {
        let mut checked = selected.contains(c.id);
        let response = ui.add_enabled(
            enabled,
            egui::Checkbox::new(
                &mut checked,
                RichText::new(tr(c.name))
                    .strong()
                    .family(crate::theme::bold()),
            ),
        );
        if response.changed() {
            if checked {
                selected.insert(c.id);
            } else {
                selected.remove(c.id);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if report.blocked {
                widgets::badge(ui, tr("Needs administrator"), t.warn);
            } else if let Some(note) = &report.note {
                ui.label(RichText::new(note).color(t.text_muted))
                    .on_hover_text(note);
            } else {
                ui.label(
                    RichText::new(bytes(report.bytes))
                        .strong()
                        .family(crate::theme::bold()),
                );
                ui.label(
                    RichText::new(trf("{n} files", &[("n", &count(report.files))]))
                        .color(t.text_muted),
                );
            }
            match c.risk {
                Risk::Safe => widgets::badge(ui, tr("Safe"), t.ok),
                Risk::Moderate => widgets::badge(ui, tr("Rebuilt later"), t.warn).on_hover_text(
                    tr("Safe to delete, but something gets slower until it is rebuilt."),
                ),
                Risk::Review => widgets::badge(ui, tr("Review"), t.danger)
                    .on_hover_text(tr("Your own files. Check them before cleaning.")),
            };
        });
    });
    let mut description = tr(c.description).to_owned();
    if c.min_age_days > 1 {
        description.push(' ');
        description.push_str(&trf(
            "Only files older than {n} days.",
            &[("n", &c.min_age_days.to_string())],
        ));
    }
    ui.label(RichText::new(description).color(t.text_muted).small());
}

fn results_card(ui: &mut egui::Ui, results: &[CategoryResult], reports: Option<&[CategoryReport]>) {
    let t = theme::current(ui.ctx());
    let name = |id: &str| {
        reports
            .and_then(|r| r.iter().find(|r| r.category.id == id))
            .map(|r| tr(r.category.name))
            .unwrap_or("")
    };
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(tr("Last cleanup"))
                .strong()
                .family(crate::theme::bold()),
        );
        egui::Grid::new("cleanup_results")
            .num_columns(3)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                for r in results {
                    ui.label(name(r.id));
                    ui.label(
                        RichText::new(bytes(r.bytes))
                            .strong()
                            .family(crate::theme::bold()),
                    );
                    let mut notes = Vec::new();
                    if r.failed > 0 {
                        notes.push(trf(
                            "{n} files in use were kept",
                            &[("n", &count(r.failed))],
                        ));
                    }
                    if let Some(reason) = r.skipped {
                        notes.push(tr(reason).to_owned());
                    }
                    if let Some(err) = &r.error {
                        notes.push(format!("{}: {err}", tr("Error")));
                    }
                    let color = if r.error.is_some() {
                        t.danger
                    } else {
                        t.text_muted
                    };
                    ui.label(RichText::new(notes.join(" · ")).color(color));
                    ui.end_row();
                }
            });
    });
}
