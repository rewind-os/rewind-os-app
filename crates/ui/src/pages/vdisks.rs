//! Virtual disks of WSL and Docker Desktop (`vdisk.list`): what each takes on Windows and, when
//! its distribution runs, what Linux uses inside. Compacting one (`vdisk.compact`) gives Windows
//! back the difference; it stops WSL and needs administrator rights, so it asks first.

use crate::app::Shared;
use crate::charts;
use crate::glyphs::Glyph;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, card, page_header_with};
use egui::RichText;
use rewind_os_engine::format::{bytes, count};
use rewind_os_engine::vdisks::{Owner, QUIT_DOCKER, Report, VirtualDisk};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};

#[derive(Default)]
pub struct State {
    job: Option<Job>,
    report: Option<Result<Report, String>>,
    compact: Option<Job>,
    confirm: Option<VirtualDisk>,
    output: Option<String>,
}

impl State {
    pub fn busy(&self) -> bool {
        self.job.is_some() || self.compact.is_some()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if self.report.is_none() && self.job.is_none() {
            self.job = Some(Job::start(&ctx, &shared.engine, Request::VdiskList));
        }
        match jobs::finished(&mut self.job, &ctx) {
            Some(Ok(Response::VirtualDisks(report))) => self.report = Some(Ok(report)),
            Some(Ok(_)) => {}
            Some(Err(err)) => self.report = Some(Err(err)),
            None => {}
        }
        match jobs::finished(&mut self.compact, &ctx) {
            Some(Ok(Response::Compacted(done))) => {
                shared.info(trf(
                    "{name}: {before} → {after}",
                    &[
                        ("name", &done.name),
                        ("before", &bytes(done.before)),
                        ("after", &bytes(done.after)),
                    ],
                ));
                self.output = Some(done.output);
                self.report = None;
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }

        let busy = self.busy();
        page_header_with(
            ui,
            tr("Virtual disks"),
            tr(
                "WSL and Docker Desktop keep Linux in disk files that grow and never shrink by themselves. Compacting one gives Windows back the space Linux no longer uses.",
            ),
            |ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new(tr("Check again")))
                    .clicked()
                {
                    self.report = None;
                }
            },
        );
        let report = match &self.report {
            None => {
                ui.spinner();
                return;
            }
            Some(Err(err)) => {
                ui.label(RichText::new(err).color(t.text_muted));
                return;
            }
            Some(Ok(report)) => report,
        };
        let mut ask = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                if report.disks.is_empty() {
                    ui.label(tr("No virtual disks of WSL or Docker Desktop were found."));
                    return;
                }
                figures(ui, report);
                ui.add_space(t.gap);
                if report.docker_running {
                    ui.label(RichText::new(tr(QUIT_DOCKER)).color(t.warn));
                    ui.add_space(t.gap * 0.5);
                }
                if !shared.elevated {
                    shared.admin_hint(ui);
                    ui.add_space(t.gap * 0.5);
                }
                let allowed = shared.elevated && !report.docker_running && self.compact.is_none();
                for (index, disk) in report.disks.iter().enumerate() {
                    widgets::list_row(ui, index, 60.0, |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let button = widgets::row_button(ui, tr("Compact"), allowed);
                            let button = if allowed {
                                button.on_hover_text(tr(
                                    "Gives Windows back the space Linux no longer uses. Stops WSL while it runs.",
                                ))
                            } else if report.docker_running {
                                button.on_disabled_hover_text(tr(QUIT_DOCKER))
                            } else {
                                button.on_disabled_hover_text(tr("This needs administrator rights."))
                            };
                            if button.clicked() {
                                ask = Some(disk.clone());
                            }
                            ui.allocate_ui_with_layout(
                                egui::vec2(200.0, 40.0),
                                egui::Layout::top_down(egui::Align::Max),
                                |ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.label(
                                        RichText::new(bytes(disk.bytes)).family(theme::bold()),
                                    );
                                    let line = match disk.reclaimable() {
                                        Some(free) => trf(
                                            "about {free} to give back",
                                            &[("free", &bytes(free))],
                                        ),
                                        None if disk.owner == Owner::Docker => {
                                            tr("what Docker uses inside is not known").to_owned()
                                        }
                                        None => tr("start it to see what it uses inside").to_owned(),
                                    };
                                    ui.label(RichText::new(line).small().color(t.text_muted));
                                },
                            );
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                let tile = match disk.owner {
                                    Owner::Wsl => theme::TILES[0],
                                    Owner::Docker => theme::TILES[4],
                                };
                                widgets::icon_tile(ui, Glyph::Drive, tile, 36.0);
                                widgets::card_text(ui, &disk.name, &disk.path.display().to_string());
                            });
                        });
                    });
                    ui.add_space(6.0);
                }
                if let Some(job) = &self.compact {
                    ui.add_space(t.gap * 0.5);
                    card(ui, |ui| {
                        ui.set_width(ui.available_width());
                        widgets::job_progress(ui, job, false);
                        ui.label(
                            RichText::new(tr("This can take several minutes.")).color(t.text_muted),
                        );
                    });
                }
                if let Some(output) = &self.output {
                    ui.add_space(t.gap * 0.5);
                    card(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(tr("Result")).strong().family(theme::bold()));
                        ui.add(egui::Label::new(RichText::new(output).monospace().small()).wrap());
                    });
                }
            });
        if ask.is_some() {
            self.confirm = ask;
        }
        if let Some(disk) = &self.confirm {
            let body = match disk.owner {
                Owner::Wsl => tr(
                    "Every Linux program and WSL window closes, Linux is told which space it freed, and the disk file is compacted. Nothing inside it is deleted. It can take several minutes.",
                ),
                Owner::Docker => tr(
                    "WSL stops while the disk file is compacted. Nothing inside it is deleted: images, containers and volumes stay. It can take several minutes.",
                ),
            };
            match widgets::confirm(
                &ctx,
                "confirm_compact",
                &trf("Compact the disk of {name}?", &[("name", &disk.name)]),
                body,
                tr("Compact"),
            ) {
                Choice::Open => {}
                Choice::Cancelled => self.confirm = None,
                Choice::Confirmed => {
                    let request = Request::VdiskCompact {
                        path: disk.path.clone(),
                    };
                    self.confirm = None;
                    self.output = None;
                    self.compact = Some(Job::start(&ctx, &shared.engine, request));
                }
            }
        }
    }
}

/// What the disks take on Windows, and what compacting would give back where it is known.
fn figures(ui: &mut egui::Ui, report: &Report) {
    let total: u64 = report.disks.iter().map(|d| d.bytes).sum();
    let known: Vec<u64> = report
        .disks
        .iter()
        .filter_map(VirtualDisk::reclaimable)
        .collect();
    charts::grid(ui, 3, 180.0, |ui, i| match i {
        0 => charts::stat_tile(
            ui,
            tr("Virtual disks"),
            &count(report.disks.len() as u64),
            tr("Of WSL and Docker Desktop"),
            None,
        ),
        1 => charts::stat_tile(
            ui,
            tr("Space they take"),
            &bytes(total),
            tr("On this computer's drive"),
            None,
        ),
        _ => charts::stat_tile(
            ui,
            tr("Space you can recover"),
            &if known.is_empty() {
                "-".to_owned()
            } else {
                bytes(known.iter().sum())
            },
            tr("About, where Linux is running"),
            None,
        ),
    });
}
