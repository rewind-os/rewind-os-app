use crate::app::{Page, Shared};
use crate::charts;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, card, fill_color};
use egui::RichText;
use rewind_os_engine::format::bytes;
use rewind_os_engine::recommend::{Action, Level, Recommendation};
use rewind_os_engine::system::{DriveKind, Overview, SystemFileKind};
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
            charts::section_title(ui, tr("Recommended for you"));
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
                            Action::Duplicates { .. } | Action::Leftovers => tr("Review"),
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
                            egui::vec2(76.0, 20.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if let Some(saving) = r.saving.filter(|b| *b > 0) {
                                    ui.label(
                                        RichText::new(bytes(saving))
                                            .strong()
                                            .family(crate::theme::bold()),
                                    );
                                }
                            },
                        );
                        ui.allocate_ui_with_layout(
                            egui::vec2(84.0, 20.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| widgets::badge(ui, word, color),
                        );
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.add_space(9.0);
                            let args: Vec<(&str, &str)> =
                                r.args.iter().map(|(k, v)| (*k, v.as_str())).collect();
                            ui.label(
                                RichText::new(trf(r.title, &args))
                                    .strong()
                                    .family(crate::theme::bold()),
                            );
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
                Action::Duplicates { path } => {
                    shared.scan_request = Some(path);
                    shared.goto = Some(Page::Duplicates);
                }
                Action::Leftovers => {
                    shared.leftovers_request = true;
                    shared.goto = Some(Page::Leftovers);
                }
                Action::ReviewStartup => shared.goto = Some(Page::Startup),
                Action::Lighten => shared.goto = Some(Page::Lighten),
                Action::Hibernation => shared.goto = Some(Page::Maintenance),
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let t = theme::current(ui.ctx());
        let mut refresh = false;
        let busy = self.busy();
        widgets::page_header_with(
            ui,
            tr("Home"),
            tr("What is worth doing on this computer, and how your drives are."),
            |ui| {
                refresh = ui
                    .add_enabled(!busy, egui::Button::new(tr("Refresh")))
                    .clicked();
            },
        );
        if refresh {
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
        let Some(overview) = shared.overview.clone() else {
            ui.spinner();
            return;
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                self.key_figures(ui, &overview);
                ui.add_space(t.gap * 1.5);
                charts::section_title(ui, tr("Drives"));
                drives(ui, shared, &overview);
                ui.add_space(t.gap * 1.5);
                self.recommendations(ui, shared);
                ui.add_space(t.gap * 1.5);
                charts::section_title(ui, tr("System"));
                system(ui, &overview);
                ui.add_space(t.gap);
            });
    }

    /// The row of key figures at the top: storage, free space, what can be recovered, memory.
    fn key_figures(&self, ui: &mut egui::Ui, overview: &Overview) {
        let t = theme::current(ui.ctx());
        let total: u64 = overview.drives.iter().map(|d| d.total).sum();
        let used: u64 = overview.drives.iter().map(|d| d.used()).sum();
        let free: u64 = overview.drives.iter().map(|d| d.available).sum();
        let used_share = share(used, total);
        let memory = share(overview.memory_used, overview.memory_total);
        let drives = overview.drives.len().to_string();
        let (recoverable, advice) = match &self.advice {
            Some(list) => (
                bytes(list.iter().filter_map(|r| r.saving).sum()),
                if list.len() == 1 {
                    tr("1 recommendation").to_owned()
                } else {
                    trf("{n} recommendations", &[("n", &list.len().to_string())])
                },
            ),
            None => ("…".to_owned(), tr("Looking at this computer…").to_owned()),
        };
        charts::grid(ui, 4, 190.0, |ui, i| match i {
            0 => charts::stat_tile(
                ui,
                tr("Storage used"),
                &format!("{:.0}%", used_share * 100.0),
                &trf(
                    "{used} of {total}",
                    &[("used", &bytes(used)), ("total", &bytes(total))],
                ),
                Some((used_share, fill_color(t, used_share))),
            ),
            1 => charts::stat_tile(
                ui,
                tr("Free space"),
                &bytes(free),
                &trf("on {n} drives", &[("n", &drives)]),
                None,
            ),
            2 => charts::stat_tile(ui, tr("Space you can recover"), &recoverable, &advice, None),
            _ => charts::stat_tile(
                ui,
                tr("Memory in use"),
                &format!("{:.0}%", memory * 100.0),
                &trf(
                    "{used} in use of {total}",
                    &[
                        ("used", &bytes(overview.memory_used)),
                        ("total", &bytes(overview.memory_total)),
                    ],
                ),
                Some((memory, fill_color(t, memory))),
            ),
        });
    }
}

fn share(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64) as f32
    }
}

/// One card per drive: a usage gauge, the figures next to it, and Analyze.
fn drives(ui: &mut egui::Ui, shared: &mut Shared, overview: &Overview) {
    let t = theme::current(ui.ctx());
    charts::grid(ui, overview.drives.len(), 330.0, |ui, i| {
        let Some(drive) = overview.drives.get(i) else {
            return;
        };
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            let fraction = share(drive.used(), drive.total);
            let summary = trf(
                "{free} free of {total}",
                &[
                    ("free", &bytes(drive.available)),
                    ("total", &bytes(drive.total)),
                ],
            );
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::row_button(ui, tr("Analyze"), true).clicked() {
                        shared.scan_request = Some(drive.mount.clone());
                        shared.goto = Some(Page::Analyzer);
                    }
                    if drive.removable {
                        widgets::badge(ui, tr("Removable"), t.text_muted);
                    }
                    let kind = match drive.kind {
                        DriveKind::Ssd => "SSD",
                        DriveKind::Hdd => "HDD",
                        DriveKind::Unknown => "",
                    };
                    if !kind.is_empty() {
                        widgets::badge(ui, kind, t.text_muted);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        widgets::icon_tile(
                            ui,
                            crate::glyphs::Glyph::Drive,
                            crate::theme::TILES[0],
                            36.0,
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(drive.mount.display().to_string())
                                    .strong()
                                    .family(crate::theme::bold())
                                    .size(15.0),
                            )
                            .truncate(),
                        );
                    });
                });
            });
            let name = [drive.label.as_str(), drive.file_system.as_str()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            ui.add(egui::Label::new(RichText::new(name).small().color(t.text_muted)).truncate());
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                charts::gauge(
                    ui,
                    92.0,
                    fraction,
                    fill_color(t, fraction),
                    tr("used"),
                    &format!("{} {summary}", drive.mount.display()),
                );
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.add_space(10.0);
                    egui::Grid::new(("drive_figures", i))
                        .num_columns(2)
                        .spacing([18.0, 6.0])
                        .show(ui, |ui| {
                            for (word, value) in [
                                (tr("Used"), drive.used()),
                                (tr("Free"), drive.available),
                                (tr("Total"), drive.total),
                            ] {
                                ui.label(RichText::new(word).color(t.text_muted));
                                ui.label(
                                    RichText::new(bytes(value))
                                        .strong()
                                        .family(crate::theme::bold()),
                                );
                                ui.end_row();
                            }
                        });
                });
            });
        });
    });
}

/// The computer, its memory as a gauge, and Windows' own large files as shares of their total.
fn system(ui: &mut egui::Ui, overview: &Overview) {
    let t = theme::current(ui.ctx());
    let panels = if overview.system_files.is_empty() {
        1
    } else {
        2
    };
    charts::grid(ui, panels, 340.0, |ui, i| {
        if i == 0 {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let memory = share(overview.memory_used, overview.memory_total);
                    charts::gauge(
                        ui,
                        96.0,
                        memory,
                        fill_color(t, memory),
                        tr("memory"),
                        &trf(
                            "{used} in use of {total}",
                            &[
                                ("used", &bytes(overview.memory_used)),
                                ("total", &bytes(overview.memory_total)),
                            ],
                        ),
                    );
                    ui.add_space(8.0);
                    egui::Grid::new("system_grid")
                        .num_columns(2)
                        .spacing([14.0, 4.0])
                        .show(ui, |ui| {
                            let hours = overview.uptime_secs / 3600;
                            let rows = [
                                (tr("Operating system"), overview.os.clone()),
                                (tr("Computer"), overview.host.clone()),
                                (
                                    tr("Processor"),
                                    format!(
                                        "{} · {} {}",
                                        overview.cpu,
                                        overview.cpu_threads,
                                        tr("threads")
                                    ),
                                ),
                                (
                                    tr("Running for"),
                                    trf(
                                        "{d} days {h} h",
                                        &[
                                            ("d", &(hours / 24).to_string()),
                                            ("h", &(hours % 24).to_string()),
                                        ],
                                    ),
                                ),
                            ];
                            for (word, value) in rows {
                                ui.label(RichText::new(word).color(t.text_muted));
                                ui.add(egui::Label::new(value).truncate());
                                ui.end_row();
                            }
                        });
                });
            });
        } else {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(tr("Large system files"))
                        .strong()
                        .family(crate::theme::bold()),
                );
                ui.label(
                    RichText::new(tr(
                        "Windows manages these files itself. Never delete them by hand.",
                    ))
                    .small()
                    .color(t.text_muted),
                );
                ui.add_space(8.0);
                let sizes: Vec<u64> = overview.system_files.iter().map(|f| f.bytes).collect();
                let total: u64 = sizes.iter().sum();
                let parts = charts::shares(t, &sizes, total);
                charts::stacked_bar(
                    ui,
                    10.0,
                    &parts,
                    &format!("{} {}", tr("Large system files"), bytes(total)),
                );
                ui.add_space(8.0);
                for (file, (fraction, color)) in overview.system_files.iter().zip(&parts) {
                    let name = file
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| file.path.display().to_string());
                    charts::legend_row(ui, *color, &name, &bytes(file.bytes), Some(*fraction));
                    let what = match file.kind {
                        SystemFileKind::Hibernation => {
                            tr("Hibernation and fast startup. You can turn it off in Maintenance.")
                        }
                        SystemFileKind::PageFile => {
                            tr("Virtual memory. Windows sizes it automatically.")
                        }
                        SystemFileKind::SwapFile => tr("Virtual memory for Store apps."),
                    };
                    ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        ui.label(RichText::new(what).small().color(t.text_muted));
                    });
                }
            });
        }
    });
}
