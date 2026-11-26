//! Disk health: whether each drive reports problems, with its temperature, wear, hours and
//! errors (`disk.health`), and the system's own optimization of the drives (`disk.optimize`).

use crate::app::Shared;
use crate::charts;
use crate::glyphs::Glyph;
use crate::jobs::{self, Job};
use crate::theme::{self, Theme};
use crate::widgets::{self, Choice, card, page_header_with};
use egui::{Color32, RichText};
use rewind_os_engine::disks::{Disk, Health, Media, Report};
use rewind_os_engine::format::{bytes, count, local_date};
use rewind_os_engine::{Os, Request, Response};
use rewind_os_i18n::{tr, trf};

#[derive(Default)]
pub struct State {
    report_job: Option<Job>,
    report: Option<Result<Report, String>>,
    optimize_job: Option<Job>,
    output: Option<String>,
    confirm: bool,
}

/// The status colour of a health; the word next to it carries the meaning.
fn health_color(t: &Theme, health: Health) -> Color32 {
    match health {
        Health::Good => t.ok,
        Health::Unknown => t.text_muted,
        Health::Warning => t.warn,
        Health::Failing => t.danger,
    }
}

impl State {
    pub fn busy(&self) -> bool {
        self.report_job.is_some() || self.optimize_job.is_some()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if self.report.is_none() && self.report_job.is_none() {
            self.report_job = Some(Job::start(&ctx, &shared.engine, Request::DiskHealth));
        }
        match jobs::finished(&mut self.report_job, &ctx) {
            Some(Ok(Response::DiskHealth(report))) => self.report = Some(Ok(report)),
            Some(Ok(_)) => {}
            Some(Err(err)) => self.report = Some(Err(err)),
            None => {}
        }
        match jobs::finished(&mut self.optimize_job, &ctx) {
            Some(Ok(Response::ToolOutput(text))) => self.optimized(shared, text),
            // Run with administrator rights: the same answer, as JSON.
            Some(Ok(Response::Elevated(value))) => {
                let text = value["data"].as_str().unwrap_or_default().to_owned();
                self.optimized(shared, text);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }

        let checking = self.report_job.is_some();
        page_header_with(
            ui,
            tr("Disk health"),
            tr("Whether your drives report problems, and keeping them optimized."),
            |ui| {
                if ui
                    .add_enabled(!checking, egui::Button::new(tr("Check again")))
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
        let mut ask = false;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                if let Some(why) = report.missing {
                    ui.label(RichText::new(tr(why)).color(t.text_muted));
                    if Os::current() == Os::Windows && !shared.elevated {
                        shared.admin_hint(ui);
                    }
                    ui.add_space(t.gap);
                }
                if report.disks.is_empty() {
                    ui.label(tr("No drives were found."));
                }
                for disk in &report.disks {
                    drive(ui, disk, report.missing.is_none());
                    ui.add_space(t.gap);
                }
                ask = self.optimization(ui, shared, report);
            });
        if ask {
            self.confirm = true;
        }
        if self.confirm {
            let body = match Os::current() {
                Os::Windows => tr(
                    "Windows trims SSDs and defragments hard drives, as it does on its own. No file is deleted. On a hard drive this can take a long time; you can keep working meanwhile.",
                ),
                _ => tr(
                    "The system tells each SSD which space is free (TRIM), as it does on its own. No file is deleted.",
                ),
            };
            match widgets::confirm(
                &ctx,
                "confirm_optimize",
                tr("Optimize the drives now?"),
                body,
                tr("Optimize"),
            ) {
                Choice::Open => {}
                Choice::Cancelled => self.confirm = false,
                Choice::Confirmed => {
                    self.confirm = false;
                    self.output = None;
                    let request = shared.with_rights(Request::DiskOptimize);
                    self.optimize_job = Some(Job::start(&ctx, &shared.engine, request));
                }
            }
        }
    }

    fn optimized(&mut self, shared: &mut Shared, text: String) {
        shared.info(tr("The drives were optimized"));
        self.output = Some(text);
        // The last run changed.
        self.report = None;
    }

    /// The system's regular optimization and the button to run it now. Returns true when the
    /// user asked to run it.
    fn optimization(&self, ui: &mut egui::Ui, shared: &mut Shared, report: &Report) -> bool {
        let t = theme::current(ui.ctx());
        let mut ask = false;
        let allowed = shared.elevated || crate::app::can_elevate_actions();
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(tr("Optimization"))
                    .strong()
                    .family(theme::bold()),
            );
            let what = match Os::current() {
                Os::Windows => tr(
                    "SSDs work best when they are told which space is free (TRIM); hard drives, when their files are in one piece. Windows does both on its own.",
                ),
                _ => tr(
                    "SSDs work best when they are told which space is free (TRIM). The system does it on its own with fstrim.timer.",
                ),
            };
            ui.label(RichText::new(what).color(t.text_muted));
            let schedule = match (report.schedule.enabled, report.schedule.last) {
                (Some(true), Some(at)) => Some((
                    trf("On: it last ran on {date}.", &[("date", &local_date(at))]),
                    t.ok,
                )),
                (Some(true), None) => Some((tr("On.").to_owned(), t.ok)),
                (Some(false), _) => Some((
                    tr("Off: the drives are not optimized on their own.").to_owned(),
                    t.warn,
                )),
                (None, _) => None,
            };
            if let Some((line, color)) = schedule {
                ui.label(RichText::new(line).color(color));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let busy = self.optimize_job.is_some();
                if widgets::primary_button(ui, tr("Optimize now"), allowed && !busy).clicked() {
                    ask = true;
                }
                if !allowed {
                    shared.admin_hint(ui);
                }
            });
            if let Some(job) = &self.optimize_job {
                ui.add_space(6.0);
                widgets::job_progress(ui, job, false);
                ui.label(RichText::new(tr("This can take several minutes.")).color(t.text_muted));
            }
            if let Some(output) = &self.output {
                ui.add_space(6.0);
                ui.label(RichText::new(tr("Result")).strong().family(theme::bold()));
                ui.add(egui::Label::new(RichText::new(output).monospace().small()).wrap());
            }
        });
        ask
    }
}

/// One drive: its name, what it is, its health and its figures, and what is wrong.
/// `readable`: the drives' own reports could be read, so an unknown health is the drive's.
fn drive(ui: &mut egui::Ui, disk: &Disk, readable: bool) {
    let t = theme::current(ui.ctx());
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.set_height(40.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::badge(ui, tr(disk.health.label()), health_color(t, disk.health));
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    widgets::icon_tile(ui, Glyph::Drive, theme::TILES[0], 36.0);
                    let mut parts = vec![tr(disk.media.label()).to_owned()];
                    if !disk.bus.is_empty() {
                        parts.push(disk.bus.clone());
                    }
                    parts.push(bytes(disk.size));
                    if !disk.volumes.is_empty() {
                        parts.push(disk.volumes.join(" "));
                    }
                    widgets::card_text(ui, &disk.name, &parts.join(" · "));
                });
            });
        });
        let tiles = figures(t, disk);
        if !tiles.is_empty() {
            ui.add_space(10.0);
            charts::grid(ui, tiles.len().max(4), 140.0, |ui, i| {
                if let Some((label, value, detail, meter)) = tiles.get(i) {
                    charts::stat_tile(ui, label, value, detail, *meter);
                }
            });
        }
        if !disk.problems.is_empty() {
            ui.add_space(8.0);
            for problem in &disk.problems {
                ui.label(RichText::new(tr(problem.label())).color(health_color(t, disk.health)));
            }
        } else if disk.health == Health::Unknown && readable {
            ui.add_space(6.0);
            ui.label(
                RichText::new(tr("This drive does not say how it is doing.")).color(t.text_muted),
            );
        }
    });
}

/// A key figure: label, value, detail and an optional meter.
type Tile = (String, String, String, Option<(f32, Color32)>);

fn figures(t: &Theme, disk: &Disk) -> Vec<Tile> {
    let mut tiles: Vec<Tile> = Vec::new();
    if let Some(c) = disk.temperature_c {
        let hot = disk
            .problems
            .contains(&rewind_os_engine::disks::Problem::Hot);
        tiles.push((
            tr("Temperature").to_owned(),
            format!("{c} °C"),
            if hot { tr("Hot") } else { tr("Fine") }.to_owned(),
            None,
        ));
    }
    if let Some(used) = disk.used_life
        && disk.media == Media::Ssd
    {
        let fraction = used as f32 / 100.0;
        tiles.push((
            tr("Rated life used").to_owned(),
            format!("{used} %"),
            tr("Writes the SSD is rated for").to_owned(),
            Some((fraction, widgets::fill_color(t, fraction))),
        ));
    }
    if let Some(hours) = disk.power_on_hours {
        tiles.push((
            tr("Powered on").to_owned(),
            format!("{} h", count(hours)),
            trf("about {n} days", &[("n", &count(hours / 24))]),
            None,
        ));
    }
    if let Some(errors) = disk.errors {
        tiles.push((
            tr("Uncorrected errors").to_owned(),
            count(errors),
            if errors == 0 {
                tr("None")
            } else {
                tr("Back up your files")
            }
            .to_owned(),
            None,
        ));
    }
    if let Some(sectors) = disk.bad_sectors {
        tiles.push((
            tr("Damaged sectors").to_owned(),
            count(sectors),
            if sectors == 0 {
                tr("None")
            } else {
                tr("Back up your files")
            }
            .to_owned(),
            None,
        ));
    }
    tiles
}
