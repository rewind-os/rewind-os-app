use crate::app::{Page, Shared};
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, card, fill_color, page_header, usage_bar};
use egui::RichText;
use rewind_os_engine::format::bytes;
use rewind_os_engine::recommend::{Action, Level, Recommendation};
use rewind_os_engine::system::{DriveKind, SystemFileKind};
use rewind_os_engine::{Engine, Request, Response};
use rewind_os_i18n::{tr, trf};
use std::sync::Arc;

pub struct State {
    job: Option<Job>,
    advice_job: Option<Job>,
    advice: Option<Vec<Recommendation>>,
}

impl State {
    pub fn busy(&self) -> bool {
        self.job.is_some() || self.advice_job.is_some()
    }

    pub fn new(ctx: &egui::Context, engine: &Arc<Engine>) -> Self {
        Self {
            job: Some(Job::start(ctx, engine, Request::SystemOverview)),
            advice_job: Some(Job::start(ctx, engine, Request::Recommendations)),
            advice: None,
        }
    }

    /// Runs every frame, whatever page is shown, because other pages use the drive list.
    pub fn poll(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        match jobs::finished(&mut self.job, ctx) {
            Some(Ok(Response::Overview(overview))) => {
                shared.elevated = overview.elevated;
                shared.overview = Some(overview);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        match jobs::finished(&mut self.advice_job, ctx) {
            Some(Ok(Response::Recommendations(list))) => self.advice = Some(list),
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
    }

    /// What the computer could use, most important first, each with a button that leads there.
    fn recommendations(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let t = theme::current(ui.ctx());
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Recommended for you")).strong());
            if self.advice_job.is_some() {
                ui.spinner();
                ui.label(
                    RichText::new(tr("Looking at this computer…"))
                        .small()
                        .color(t.text_muted),
                );
            }
        });
        ui.add_space(4.0);
        let Some(advice) = &self.advice else { return };
        if advice.is_empty() {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(tr("Nothing to do: this computer is in good shape.")).color(t.ok),
                );
            });
            return;
        }
        let mut chosen = None;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            for (index, r) in advice.iter().enumerate() {
                widgets::list_row(ui, index, 58.0, |ui| {
                    let (word, color) = match r.level {
                        Level::Important => (tr("Important"), t.danger),
                        Level::Suggested => (tr("Suggested"), t.accent),
                        Level::Tip => (tr("Tip"), t.text_muted),
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = match r.action {
                            Action::Clean { .. } => tr("Clean"),
                            Action::Analyze { .. } => tr("Analyze"),
                            Action::Hibernation => tr("Open"),
                            Action::ReviewStartup | Action::Lighten => tr("Review"),
                        };
                        let button = if r.level == Level::Tip {
                            widgets::row_button(ui, label, true)
                        } else {
                            widgets::primary_button(ui, label, true)
                        };
                        if button.clicked() {
                            chosen = Some(r.action.clone());
                        }
                        ui.allocate_ui_with_layout(
                            egui::vec2(84.0, 20.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| widgets::badge(ui, word, color),
                        );
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.add_space(9.0);
                            let args: Vec<(&str, &str)> =
                                r.args.iter().map(|(k, v)| (*k, v.as_str())).collect();
                            ui.label(RichText::new(trf(r.title, &args)).strong());
                            ui.add(
                                egui::Label::new(
                                    RichText::new(tr(r.detail)).small().color(t.text_muted),
                                )
                                .truncate(),
                            );
                        });
                    });
                });
            }
        });
        if let Some(action) = chosen {
            match action {
                Action::Clean { categories } => {
                    shared.cleanup_request = Some(categories);
                    shared.goto = Some(Page::Cleanup);
                }
                Action::Analyze { path } => {
                    shared.scan_request = Some(path);
                    shared.goto = Some(Page::Analyzer);
                }
                Action::ReviewStartup => shared.goto = Some(Page::Startup),
                Action::Lighten => shared.goto = Some(Page::Lighten),
                Action::Hibernation => shared.goto = Some(Page::Maintenance),
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let t = theme::current(ui.ctx());
        page_header(
            ui,
            tr("Home"),
            tr("What is worth doing on this computer, and how your drives are."),
        );
        if ui
            .add_enabled(!self.busy(), egui::Button::new(tr("Refresh")))
            .clicked()
        {
            self.job = Some(Job::start(
                ui.ctx(),
                &shared.engine,
                Request::SystemOverview,
            ));
            self.advice_job = Some(Job::start(
                ui.ctx(),
                &shared.engine,
                Request::Recommendations,
            ));
        }
        ui.add_space(t.gap);
        let Some(overview) = shared.overview.clone() else {
            ui.spinner();
            return;
        };
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            self.recommendations(ui, shared);
            ui.add_space(t.gap * 1.5);
            ui.label(RichText::new(tr("Drives")).strong());
            ui.add_space(4.0);
            let columns = ((ui.available_width() / 320.0).floor() as usize).max(1);
            ui.columns(columns, |cols| {
                for (i, drive) in overview.drives.iter().enumerate() {
                    let ui = &mut cols[i % columns];
                    card(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(drive.mount.display().to_string()).strong().size(16.0));
                            if !drive.label.is_empty() {
                                ui.label(RichText::new(&drive.label).color(t.text_muted));
                            }
                            let kind = match drive.kind {
                                DriveKind::Ssd => "SSD",
                                DriveKind::Hdd => "HDD",
                                DriveKind::Unknown => "",
                            };
                            if !kind.is_empty() {
                                widgets::badge(ui, kind, t.text_muted);
                            }
                            if drive.removable {
                                widgets::badge(ui, tr("Removable"), t.text_muted);
                            }
                        });
                        let fraction = if drive.total > 0 { drive.used() as f32 / drive.total as f32 } else { 0.0 };
                        let summary = trf("{free} free of {total}", &[
                            ("free", &bytes(drive.available)),
                            ("total", &bytes(drive.total)),
                        ]);
                        usage_bar(ui, fraction, fill_color(t, fraction), &format!("{} {summary}", drive.mount.display()));
                        ui.horizontal(|ui| {
                            ui.label(summary);
                            ui.label(RichText::new(format!("{:.0}% {}", fraction * 100.0, tr("used"))).color(t.text_muted));
                        });
                        ui.horizontal(|ui| {
                            if ui.button(tr("Analyze")).clicked() {
                                shared.scan_request = Some(drive.mount.clone());
                                shared.goto = Some(Page::Analyzer);
                            }
                            ui.label(RichText::new(&drive.file_system).small().color(t.text_muted));
                        });
                    });
                    ui.add_space(t.gap);
                }
            });

            ui.add_space(t.gap);
            ui.columns(2, |cols| {
                card(&mut cols[0], |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(tr("Memory")).strong());
                    let fraction = if overview.memory_total > 0 {
                        overview.memory_used as f32 / overview.memory_total as f32
                    } else {
                        0.0
                    };
                    let text = trf("{used} in use of {total}", &[
                        ("used", &bytes(overview.memory_used)),
                        ("total", &bytes(overview.memory_total)),
                    ]);
                    usage_bar(ui, fraction, fill_color(t, fraction), &text);
                    ui.label(text);
                });
                card(&mut cols[1], |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(tr("System")).strong());
                    egui::Grid::new("system_grid").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                        ui.label(RichText::new(tr("Operating system")).color(t.text_muted));
                        ui.label(&overview.os);
                        ui.end_row();
                        ui.label(RichText::new(tr("Computer")).color(t.text_muted));
                        ui.label(&overview.host);
                        ui.end_row();
                        ui.label(RichText::new(tr("Processor")).color(t.text_muted));
                        ui.label(format!("{} · {} {}", overview.cpu, overview.cpu_threads, tr("threads")));
                        ui.end_row();
                        ui.label(RichText::new(tr("Running for")).color(t.text_muted));
                        let hours = overview.uptime_secs / 3600;
                        ui.label(trf("{d} days {h} h", &[("d", &(hours / 24).to_string()), ("h", &(hours % 24).to_string())]));
                        ui.end_row();
                    });
                });
            });

            if !overview.system_files.is_empty() {
                ui.add_space(t.gap);
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(tr("Large system files")).strong());
                    ui.label(RichText::new(tr("Windows manages these files itself. Never delete them by hand.")).color(t.text_muted));
                    ui.add_space(4.0);
                    egui::Grid::new("system_files").num_columns(3).spacing([16.0, 6.0]).show(ui, |ui| {
                        for file in &overview.system_files {
                            ui.label(file.path.display().to_string());
                            ui.label(RichText::new(bytes(file.bytes)).strong());
                            let what = match file.kind {
                                SystemFileKind::Hibernation => tr("Hibernation and fast startup. You can turn it off in Maintenance."),
                                SystemFileKind::PageFile => tr("Virtual memory. Windows sizes it automatically."),
                                SystemFileKind::SwapFile => tr("Virtual memory for Store apps."),
                            };
                            ui.label(RichText::new(what).color(t.text_muted));
                            ui.end_row();
                        }
                    });
                });
            }

            ui.add_space(t.gap);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, tr("Find junk files"), true).clicked() {
                    shared.goto = Some(Page::Cleanup);
                }
                if ui.button(tr("Find program leftovers")).clicked() {
                    shared.goto = Some(Page::Leftovers);
                }
            });
        });
    }
}
