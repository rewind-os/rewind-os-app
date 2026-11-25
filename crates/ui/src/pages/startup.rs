use crate::app::Shared;
use crate::charts;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, card, page_header};
use egui::RichText;
use rewind_os_engine::format::count;
use rewind_os_engine::startup::{Source, StartupEntry};
use rewind_os_engine::{Request, Response, Task};
use rewind_os_i18n::{tr, trf};

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
            Some(Ok(Response::Startup(list))) => {
                shared.icons.request(
                    &ctx,
                    list.iter()
                        .filter_map(|e| rewind_os_engine::icons::program_of_command(&e.command)),
                );
                self.entries = Some(list);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        page_header(
            ui,
            tr("Startup programs"),
            tr(
                "Programs that start when you sign in, and the tasks programs scheduled for themselves. Turning one off does not uninstall it, and you can turn it back on at any time.",
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
                summary(ui, entries);
                ui.add_space(t.gap * 1.5);
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (index, entry) in entries.iter().enumerate() {
                        let needs_rights = entry.needs_admin && !shared.elevated;
                        // Essential entries can be turned on but never off.
                        let essential = entry.essential && entry.enabled;
                        widgets::list_row(ui, index, 62.0, |ui| {
                            let program =
                                rewind_os_engine::icons::program_of_command(&entry.command);
                            shared.icons.show(ui, program.as_deref(), &entry.name);
                            ui.add_space(6.0);
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
                                                ui.label(
                                                    RichText::new(&entry.name)
                                                        .strong()
                                                        .family(crate::theme::bold()),
                                                );
                                                ui.label(
                                                    RichText::new(tr(entry
                                                        .when
                                                        .unwrap_or(entry.source.label())))
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

/// The kinds of entries, as people know them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    SignIn,
    Task,
    Service,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::SignIn, Kind::Task, Kind::Service];

    fn of(source: Source) -> Kind {
        match source {
            Source::ScheduledTask => Kind::Task,
            Source::Service => Kind::Service,
            _ => Kind::SignIn,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::SignIn => "Programs at sign-in",
            Kind::Task => "Scheduled tasks",
            Kind::Service => "Services",
        }
    }
}

/// Key figures and what starts, split by kind.
fn summary(ui: &mut egui::Ui, entries: &[StartupEntry]) {
    let t = theme::current(ui.ctx());
    let on = entries.iter().filter(|e| e.enabled).count();
    let essential = entries.iter().filter(|e| e.essential).count();
    let share = on as f32 / entries.len().max(1) as f32;
    charts::grid(ui, 3, 180.0, |ui, i| match i {
        0 => charts::stat_tile(
            ui,
            tr("Start with the system"),
            &count(on as u64),
            &trf("of {n} entries", &[("n", &count(entries.len() as u64))]),
            Some((share, t.accent)),
        ),
        1 => charts::stat_tile(
            ui,
            tr("Turned off"),
            &count((entries.len() - on) as u64),
            tr("You can turn them back on at any time"),
            None,
        ),
        _ => charts::stat_tile(
            ui,
            tr("Needed by the system"),
            &count(essential as u64),
            tr("Can be turned on, never off"),
            None,
        ),
    });
    let kinds: Vec<(Kind, usize)> = Kind::ALL
        .into_iter()
        .map(|k| {
            let n = entries
                .iter()
                .filter(|e| e.enabled && Kind::of(e.source) == k)
                .count();
            (k, n)
        })
        .filter(|(_, n)| *n > 0)
        .collect();
    if kinds.is_empty() {
        return;
    }
    ui.add_space(t.gap * 0.75);
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(tr("What starts with the system"))
                .strong()
                .family(crate::theme::bold()),
        );
        ui.add_space(8.0);
        let sizes: Vec<u64> = kinds.iter().map(|(_, n)| *n as u64).collect();
        let parts = charts::shares(t, &sizes, on as u64);
        let text = kinds
            .iter()
            .map(|(k, n)| format!("{} {n}", tr(k.label())))
            .collect::<Vec<_>>()
            .join(", ");
        charts::stacked_bar(ui, 12.0, &parts, &text);
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for ((kind, n), (fraction, color)) in kinds.iter().zip(&parts) {
                ui.allocate_ui(egui::vec2(230.0, 20.0), |ui| {
                    ui.set_width(230.0);
                    charts::legend_row(
                        ui,
                        *color,
                        tr(kind.label()),
                        &count(*n as u64),
                        Some(*fraction),
                    );
                });
                ui.add_space(t.gap);
            }
        });
    });
}
