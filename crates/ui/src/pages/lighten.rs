use crate::app::Shared;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, card, page_header};
use egui::RichText;
use rewind_os_engine::tweaks::{Group, State as TweakState, TweakStatus};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};

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
}

impl State {
    pub fn busy(&self) -> bool {
        self.list_job.is_some() || self.set_job.is_some()
    }

    fn refresh(&mut self, ctx: &egui::Context, shared: &Shared) {
        self.started = true;
        self.list_job = Some(Job::start(ctx, &shared.engine, Request::TweaksList));
    }

    fn poll(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        match jobs::finished(&mut self.list_job, ctx) {
            Some(Ok(Response::Tweaks(list))) => self.tweaks = Some(list),
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
                "Turn off ads, tracking and extras you do not use. Nothing is uninstalled, and every change can be undone here.",
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
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for group in Group::ALL {
                    let rows: Vec<&TweakStatus> =
                        tweaks.iter().filter(|s| s.tweak.group == group).collect();
                    if rows.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(tr(group.label())).strong());
                    ui.add_space(4.0);
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
            });
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
}
