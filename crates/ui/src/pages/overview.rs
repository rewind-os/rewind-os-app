use crate::app::{Page, Shared};
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, card, fill_color, page_header, usage_bar};
use egui::RichText;
use rewind_os_engine::format::bytes;
use rewind_os_engine::system::{DriveKind, SystemFileKind};
use rewind_os_engine::{Engine, Request, Response};
use rewind_os_i18n::{tr, trf};
use std::sync::Arc;

pub struct State {
    job: Option<Job>,
}

impl State {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub fn new(ctx: &egui::Context, engine: &Arc<Engine>) -> Self {
        Self {
            job: Some(Job::start(ctx, engine, Request::SystemOverview)),
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
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let t = theme::current(ui.ctx());
        page_header(
            ui,
            tr("Overview"),
            tr("Your drives, memory and the large files Windows keeps for itself."),
        );
        if ui
            .add_enabled(self.job.is_none(), egui::Button::new(tr("Refresh")))
            .clicked()
        {
            self.job = Some(Job::start(
                ui.ctx(),
                &shared.engine,
                Request::SystemOverview,
            ));
        }
        ui.add_space(t.gap);
        let Some(overview) = shared.overview.clone() else {
            ui.spinner();
            return;
        };
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
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
