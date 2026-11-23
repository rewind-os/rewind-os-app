use crate::app::Shared;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, card, page_header};
use egui::RichText;
use rewind_os_engine::cleanup::{CategoryReport, CategoryResult, Disposal, Group, Risk};
use rewind_os_engine::format::{bytes, count};
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
    started: bool,
}

impl State {
    pub fn busy(&self) -> bool {
        self.analyze.is_some() || self.run.is_some()
    }

    fn start_analyze(&mut self, ctx: &egui::Context, shared: &Shared) {
        self.started = true;
        self.analyze = Some(Job::start(ctx, &shared.engine, Request::CleanupAnalyze));
    }

    fn poll(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        match jobs::finished(&mut self.analyze, ctx) {
            Some(Ok(Response::CleanupAnalysis(reports))) => {
                if self.reports.is_none() {
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
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if !self.started {
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
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for group in Group::ALL {
                    let items: Vec<&CategoryReport> = reports
                        .iter()
                        .filter(|r| r.category.group == group && (r.present || r.bytes > 0))
                        .collect();
                    if items.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(tr(group.label())).strong());
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

fn category_row(ui: &mut egui::Ui, report: &CategoryReport, selected: &mut HashSet<&'static str>) {
    let t = theme::current(ui.ctx());
    let c = &report.category;
    let enabled = !report.blocked && (report.bytes > 0 || report.files > 0);
    ui.horizontal(|ui| {
        let mut checked = selected.contains(c.id);
        let response = ui.add_enabled(
            enabled,
            egui::Checkbox::new(&mut checked, RichText::new(tr(c.name)).strong()),
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
                ui.label(RichText::new(bytes(report.bytes)).strong());
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
        ui.label(RichText::new(tr("Last cleanup")).strong());
        egui::Grid::new("cleanup_results")
            .num_columns(3)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                for r in results {
                    ui.label(name(r.id));
                    ui.label(RichText::new(bytes(r.bytes)).strong());
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
