use crate::app::Shared;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, card, page_header};
use egui::RichText;
use rewind_os_engine::startup::StartupEntry;
use rewind_os_engine::{Request, Response, Task};
use rewind_os_i18n::tr;

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
                    for (index, entry) in entries.iter().enumerate() {
                        let needs_rights = entry.needs_admin && !shared.elevated;
                        // Essential entries can be turned on but never off.
                        let essential = entry.essential && entry.enabled;
                        widgets::list_row(ui, index, 62.0, |ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if essential {
                                        ui.label(
                                            RichText::new(tr("Needed by the system"))
                                                .small()
                                                .color(t.text_muted),
                                        )
                                        .on_hover_text(tr(rewind_os_engine::startup::ESSENTIAL));
                                    } else {
                                        let label = if entry.enabled {
                                            tr("Turn off")
                                        } else {
                                            tr("Turn on")
                                        };
                                        let button = widgets::row_button(ui, label, !needs_rights);
                                        let button = if needs_rights {
                                            button.on_disabled_hover_text(tr(
                                                "This needs administrator rights.",
                                            ))
                                        } else {
                                            button
                                        };
                                        if button.clicked() {
                                            changed = Some((entry.id.clone(), !entry.enabled));
                                        }
                                    }
                                    let (word, color) = if entry.enabled {
                                        (tr("On"), t.ok)
                                    } else {
                                        (tr("Off"), t.text_muted)
                                    };
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(90.0, 20.0),
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| ui.label(RichText::new(word).small().color(color)),
                                    );
                                    ui.with_layout(
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.add_space(8.0);
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new(&entry.name).strong());
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
                                        },
                                    );
                                },
                            );
                        });
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
                Err(err) => shared.error(crate::widgets::error_text(&err)),
            }
            self.refresh(&ctx, shared);
        }
    }
}
