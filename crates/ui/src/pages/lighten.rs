use crate::app::Shared;
use crate::charts;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, card, page_header};
use egui::RichText;
use rewind_os_engine::apps::StoreApp;
use rewind_os_engine::tweaks::{Group, State as TweakState, TweakStatus};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};
use std::collections::HashSet;

#[derive(Default)]
pub struct State {
    list_job: Option<Job>,
    set_job: Option<Job>,
    tweaks: Option<Vec<TweakStatus>>,
    /// Settings still to apply from "Apply recommended", one request at a time.
    queue: Vec<(String, bool)>,
    failures: Vec<String>,
    confirming: bool,
    started: bool,
    apps_job: Option<Job>,
    remove_job: Option<Job>,
    apps: Option<Vec<StoreApp>>,
    apps_selected: HashSet<String>,
    confirming_apps: bool,
}

impl State {
    pub fn busy(&self) -> bool {
        self.list_job.is_some()
            || self.set_job.is_some()
            || self.apps_job.is_some()
            || self.remove_job.is_some()
    }

    fn refresh(&mut self, ctx: &egui::Context, shared: &Shared) {
        self.started = true;
        self.list_job = Some(Job::start(ctx, &shared.engine, Request::TweaksList));
        if rewind_os_engine::tool("apps.list").is_some_and(|t| t.available()) {
            self.apps_job = Some(Job::start(ctx, &shared.engine, Request::AppsList));
        }
    }

    fn poll(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        match jobs::finished(&mut self.list_job, ctx) {
            Some(Ok(Response::Tweaks(list))) => self.tweaks = Some(list),
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        match jobs::finished(&mut self.apps_job, ctx) {
            Some(Ok(Response::Apps(apps))) => {
                self.apps_selected
                    .retain(|id| apps.iter().any(|a| &a.id == id));
                self.apps = Some(apps);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        match jobs::finished(&mut self.remove_job, ctx) {
            Some(Ok(Response::AppsRemoved(results))) => {
                let failed: Vec<String> = results
                    .iter()
                    .filter_map(|r| r.error.as_ref().map(|e| format!("{}: {e}", r.id)))
                    .collect();
                let removed = results.iter().filter(|r| r.error.is_none()).count();
                let text = trf(
                    "Removed {n} apps. They can be installed again from the Microsoft Store.",
                    &[("n", &removed.to_string())],
                );
                if failed.is_empty() {
                    shared.info(text);
                } else {
                    shared.error(format!("{text} {}", failed.join("; ")));
                }
                self.apps_selected.clear();
                self.apps_job = Some(Job::start(ctx, &shared.engine, Request::AppsList));
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        if let Some(outcome) = jobs::finished(&mut self.set_job, ctx)
            && let Err(err) = outcome
        {
            self.failures.push(err);
        }
        if self.set_job.is_none() {
            if let Some((id, on)) = self.queue.pop() {
                let request = Request::TweakSet { id, applied: on };
                self.set_job = Some(Job::start(ctx, &shared.engine, request));
            } else if !self.failures.is_empty() {
                shared.error(self.failures.join(" · "));
                self.failures.clear();
            }
        }
    }

    fn start(&mut self, ctx: &egui::Context, shared: &Shared, changes: Vec<(String, bool)>) {
        self.queue = changes;
        self.queue.reverse();
        if let Some((id, on)) = self.queue.pop() {
            let request = Request::TweakSet { id, applied: on };
            self.set_job = Some(Job::start(ctx, &shared.engine, request));
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if !self.started {
            self.refresh(&ctx, shared);
        }
        let was_setting = self.set_job.is_some() || !self.queue.is_empty();
        self.poll(&ctx, shared);
        if was_setting && self.set_job.is_none() && self.queue.is_empty() {
            self.refresh(&ctx, shared);
        }
        page_header(
            ui,
            tr("Make Windows lighter"),
            tr(
                "Turn off ads, tracking and extras you do not use. Every setting can be undone here, and removed apps can be installed again from the Microsoft Store.",
            ),
        );
        let Some(tweaks) = self.tweaks.clone() else {
            ui.spinner();
            return;
        };
        let elevated = shared.elevated;
        let usable = move |s: &TweakStatus| !s.needs_admin || elevated;
        let pending: Vec<&TweakStatus> = tweaks
            .iter()
            .filter(|s| s.tweak.recommended && s.state != TweakState::Applied && usable(s))
            .collect();
        let busy = self.busy() || !self.queue.is_empty();
        ui.horizontal(|ui| {
            let label = trf(
                "Apply {n} recommended",
                &[("n", &pending.len().to_string())],
            );
            if widgets::primary_button(ui, &label, !busy && !pending.is_empty()).clicked() {
                self.confirming = true;
            }
            if busy {
                ui.spinner();
            }
        });
        if !shared.elevated && tweaks.iter().any(|s| s.needs_admin) {
            ui.add_space(4.0);
            shared.admin_hint(ui);
        }
        ui.add_space(t.gap);

        let mut change = None;
        let removable = self.apps.as_ref().map(Vec::len);
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                progress(ui, &tweaks, pending.len(), removable);
                ui.add_space(t.gap * 1.5);
                for group in Group::ALL {
                    let rows: Vec<&TweakStatus> =
                        tweaks.iter().filter(|s| s.tweak.group == group).collect();
                    if rows.is_empty() {
                        continue;
                    }
                    charts::section_title(ui, tr(group.label()));
                    card(ui, |ui| {
                        ui.set_width(ui.available_width());
                        for (index, status) in rows.iter().enumerate() {
                            widgets::list_row(ui, index, 60.0, |ui| {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let enabled = !busy && usable(status);
                                        let (label, on) = match status.state {
                                            TweakState::Applied => (tr("Undo"), false),
                                            _ => (tr("Turn off"), true),
                                        };
                                        let button = widgets::row_button(ui, label, enabled);
                                        let button = if usable(status) {
                                            button
                                        } else {
                                            button.on_disabled_hover_text(tr(
                                                "This needs administrator rights.",
                                            ))
                                        };
                                        if button.clicked() {
                                            change = Some((status.tweak.id.to_owned(), on));
                                        }
                                        let state = match status.state {
                                            TweakState::Applied => {
                                                RichText::new(tr("Turned off")).color(t.ok)
                                            }
                                            TweakState::Partial => {
                                                RichText::new(tr("Partly")).color(t.warn)
                                            }
                                            TweakState::Default => {
                                                RichText::new(tr("On")).color(t.text_muted)
                                            }
                                        };
                                        ui.label(state.small());
                                        ui.with_layout(
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                ui.add_space(8.0);
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        RichText::new(tr(status.tweak.name))
                                                            .strong(),
                                                    );
                                                    if status.tweak.recommended {
                                                        widgets::badge(
                                                            ui,
                                                            tr("Recommended"),
                                                            t.accent,
                                                        );
                                                    }
                                                });
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(format!(
                                                            "{} {}",
                                                            tr(status.tweak.description),
                                                            tr(status.tweak.takes.label())
                                                        ))
                                                        .small()
                                                        .color(t.text_muted),
                                                    )
                                                    .truncate(),
                                                )
                                                .on_hover_text(tr(status.tweak.description));
                                            },
                                        );
                                    },
                                );
                            });
                        }
                    });
                    ui.add_space(t.gap);
                }
                self.apps_section(ui, busy);
            });
        self.confirm_apps(&ctx, shared);
        if let Some(one) = change {
            self.start(&ctx, shared, vec![one]);
        }

        if self.confirming {
            let names: Vec<&str> = pending.iter().map(|s| tr(s.tweak.name)).collect();
            let body = format!(
                "{}\n\n• {}",
                tr("These settings will be turned off. You can undo each one here at any time."),
                names.join("\n• ")
            );
            match widgets::confirm(
                &ctx,
                "confirm_lighten",
                tr("Apply the recommended settings?"),
                &body,
                tr("Apply"),
            ) {
                Choice::Open => {}
                Choice::Cancelled => self.confirming = false,
                Choice::Confirmed => {
                    self.confirming = false;
                    let changes = pending
                        .iter()
                        .map(|s| (s.tweak.id.to_owned(), true))
                        .collect();
                    self.start(&ctx, shared, changes);
                }
            }
        }
    }

    /// Preinstalled Store apps that can go, with a confirmation: removing one is not undone
    /// here, but the Store can install it again.
    fn apps_section(&mut self, ui: &mut egui::Ui, busy: bool) {
        let Some(apps) = self.apps.clone() else {
            return;
        };
        if apps.is_empty() {
            return;
        }
        let t = theme::current(ui.ctx());
        ui.label(RichText::new(tr("Preinstalled apps")).strong());
        ui.label(
            RichText::new(tr(
                "Removed for your account only. Any of them can be installed again from the Microsoft Store.",
            ))
            .small()
            .color(t.text_muted),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new(tr("Select suggested")))
                .clicked()
            {
                self.apps_selected = apps
                    .iter()
                    .filter(|a| a.recommended)
                    .map(|a| a.id.clone())
                    .collect();
            }
            let label = trf(
                "Remove {n} apps…",
                &[("n", &self.apps_selected.len().to_string())],
            );
            if widgets::danger_button(ui, &label, !busy && !self.apps_selected.is_empty()).clicked()
            {
                self.confirming_apps = true;
            }
        });
        ui.add_space(4.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            for (index, app) in apps.iter().enumerate() {
                widgets::list_row(ui, index, 40.0, |ui| {
                    let mut checked = self.apps_selected.contains(&app.id);
                    if ui
                        .add_enabled(!busy, egui::Checkbox::without_text(&mut checked))
                        .changed()
                    {
                        if checked {
                            self.apps_selected.insert(app.id.clone());
                        } else {
                            self.apps_selected.remove(&app.id);
                        }
                    }
                    ui.label(RichText::new(tr(app.name)).strong());
                    if app.recommended {
                        widgets::badge(ui, tr("Suggested"), t.accent);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(&app.version).small().color(t.text_muted));
                    });
                });
            }
        });
        ui.add_space(t.gap);
    }

    fn confirm_apps(&mut self, ctx: &egui::Context, shared: &Shared) {
        if !self.confirming_apps {
            return;
        }
        let names: Vec<&str> = self
            .apps
            .iter()
            .flatten()
            .filter(|a| self.apps_selected.contains(&a.id))
            .map(|a| tr(a.name))
            .collect();
        let body = format!(
            "{}\n\n• {}",
            tr(
                "These apps will be removed for your account. You can install any of them again from the Microsoft Store."
            ),
            names.join("\n• ")
        );
        match widgets::confirm(
            ctx,
            "confirm_apps",
            tr("Remove these apps?"),
            &body,
            tr("Remove"),
        ) {
            Choice::Open => {}
            Choice::Cancelled => self.confirming_apps = false,
            Choice::Confirmed => {
                self.confirming_apps = false;
                let ids: Vec<String> = self.apps_selected.iter().cloned().collect();
                self.remove_job =
                    Some(Job::start(ctx, &shared.engine, Request::AppsRemove { ids }));
            }
        }
    }
}

/// Key figures, and how far each group of settings is turned off.
fn progress(ui: &mut egui::Ui, tweaks: &[TweakStatus], pending: usize, removable: Option<usize>) {
    let t = theme::current(ui.ctx());
    let applied = tweaks
        .iter()
        .filter(|s| s.state == TweakState::Applied)
        .count();
    let share = applied as f32 / tweaks.len().max(1) as f32;
    let tiles = if removable.is_some() { 3 } else { 2 };
    charts::grid(ui, tiles, 180.0, |ui, i| match i {
        0 => charts::stat_tile(
            ui,
            tr("Settings turned off"),
            &format!("{applied} / {}", tweaks.len()),
            &format!("{:.0}%", share * 100.0),
            Some((share, t.ok)),
        ),
        1 => charts::stat_tile(
            ui,
            tr("Recommended still on"),
            &pending.to_string(),
            tr("Apply them all with one button"),
            None,
        ),
        _ => charts::stat_tile(
            ui,
            tr("Preinstalled apps"),
            &removable.unwrap_or(0).to_string(),
            tr("Can be removed for your account"),
            None,
        ),
    });
    ui.add_space(t.gap * 0.75);
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr("Progress by group")).strong());
        ui.add_space(8.0);
        egui::Grid::new("tweak_progress")
            .num_columns(3)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                for group in Group::ALL {
                    let rows: Vec<&TweakStatus> =
                        tweaks.iter().filter(|s| s.tweak.group == group).collect();
                    if rows.is_empty() {
                        continue;
                    }
                    let done = rows
                        .iter()
                        .filter(|s| s.state == TweakState::Applied)
                        .count();
                    let fraction = done as f32 / rows.len() as f32;
                    let name = tr(group.label());
                    ui.label(name);
                    ui.allocate_ui(egui::vec2(260.0, 12.0), |ui| {
                        widgets::usage_bar(ui, fraction, t.ok, name);
                    });
                    ui.label(
                        RichText::new(trf(
                            "{done} of {n} turned off",
                            &[("done", &done.to_string()), ("n", &rows.len().to_string())],
                        ))
                        .small()
                        .color(t.text_muted),
                    );
                    ui.end_row();
                }
            });
    });
}
