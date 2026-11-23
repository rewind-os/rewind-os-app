use crate::app::Shared;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{card, page_header};
use egui::RichText;
use rewindows_engine::startup::StartupEntry;
use rewindows_engine::{Request, Response, Task};
use rewindows_i18n::tr;

#[derive(Default)]
pub struct State {
    job: Option<Job>,
    entries: Option<Vec<StartupEntry>>,
    started: bool,
}

impl State {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn refresh(&mut self, ctx: &egui::Context, shared: &Shared) {
        self.started = true;
        self.job = Some(Job::start(ctx, &shared.engine, Request::StartupList));
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if !self.started {
            self.refresh(&ctx, shared);
        }
        match jobs::finished(&mut self.job, &ctx) {
            Some(Ok(Response::Startup(list))) => self.entries = Some(list),
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        page_header(
            ui,
            tr("Startup programs"),
            tr(
                "Programs that start when you sign in. Turning one off does not uninstall it, and you can turn it back on at any time.",
            ),
        );
        let Some(entries) = &self.entries else {
            ui.spinner();
            return;
        };
        if entries.is_empty() {
            ui.label(tr("No startup programs found."));
            return;
        }
        let mut changed = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (i, entry) in entries.iter().enumerate() {
                        if i > 0 {
                            ui.separator();
                        }
                        let locked = entry.needs_admin && !shared.elevated;
                        ui.horizontal(|ui| {
                            let mut on = entry.enabled;
                            let label = RichText::new(&entry.name).strong();
                            let response =
                                ui.add_enabled(!locked, egui::Checkbox::new(&mut on, label));
                            let response = if locked {
                                response
                                    .on_disabled_hover_text(tr("This needs administrator rights."))
                            } else {
                                response
                            };
                            if response.changed() {
                                changed = Some((entry.id.clone(), on));
                            }
                            ui.label(
                                RichText::new(if entry.enabled { tr("On") } else { tr("Off") })
                                    .color(if entry.enabled { t.ok } else { t.text_muted }),
                            );
                            ui.label(
                                RichText::new(tr(entry.source.label()))
                                    .small()
                                    .color(t.text_muted),
                            );
                        });
                        ui.add(
                            egui::Label::new(
                                RichText::new(&entry.command)
                                    .small()
                                    .monospace()
                                    .color(t.text_muted),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&entry.command);
                    }
                });
                if !shared.elevated && entries.iter().any(|e| e.needs_admin) {
                    ui.add_space(t.gap);
                    shared.admin_hint(ui);
                }
            });
        if let Some((id, enabled)) = changed {
            match shared
                .engine
                .run(Request::StartupSet { id, enabled }, &Task::new())
            {
                Ok(Response::Done(message)) => shared.info(tr(message)),
                Ok(_) => {}
                Err(err) => shared.error(err.to_string()),
            }
            self.refresh(&ctx, shared);
        }
    }
}
