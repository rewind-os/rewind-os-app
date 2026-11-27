use crate::app::Shared;
use crate::charts;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, card, page_header};
use egui::RichText;
use rewind_os_engine::format::{bytes, count, local_date};
use rewind_os_engine::restore;
use rewind_os_engine::system::{Overview, SystemFileKind};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    ComponentCleanup,
    HibernationOff,
    HibernationOn,
    JournalVacuum,
    RestoreClean,
    DriversClean,
    KernelsClean,
}

fn available(id: &str) -> bool {
    rewind_os_engine::tool(id).is_some_and(|t| t.available())
}

#[derive(Default)]
pub struct State {
    job: Option<Job>,
    output: Option<String>,
    confirm: Option<Action>,
    /// The restore points, once checked.
    restore: Option<restore::Report>,
    /// The old driver packages, once checked.
    drivers: Option<rewind_os_engine::drivers::Report>,
    /// The installed kernels, once checked.
    kernels: Option<rewind_os_engine::kernels::Report>,
}

impl State {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        match jobs::finished(&mut self.job, &ctx) {
            Some(Ok(Response::ToolOutput(text))) => self.output = Some(text),
            // Run with administrator rights: the same answer, as JSON.
            Some(Ok(Response::Elevated(value))) => match value["kind"].as_str() {
                Some("tool_output") => {
                    self.output = value["data"].as_str().map(str::to_owned);
                }
                Some("done") => {
                    if let Some(message) = value["data"].as_str() {
                        shared.info(rewind_os_i18n::tr_text(message));
                    }
                }
                _ => {}
            },
            Some(Ok(Response::RestorePoints(report))) => self.restore = Some(report),
            Some(Ok(Response::Drivers(report))) => self.drivers = Some(report),
            Some(Ok(Response::DriversRemoved(done))) => {
                shared.info(trf(
                    "Removed {n} old driver packages, {size}.",
                    &[
                        ("n", &count(done.removed.len() as u64)),
                        ("size", &bytes(done.bytes)),
                    ],
                ));
                if !done.failed.is_empty() {
                    shared.error(trf(
                        "Windows kept {n} driver packages.",
                        &[("n", &count(done.failed.len() as u64))],
                    ));
                }
                self.output = (!done.failed.is_empty()).then(|| {
                    done.failed
                        .iter()
                        .map(|(name, why)| format!("{name}: {}", rewind_os_i18n::tr_text(why)))
                        .collect::<Vec<_>>()
                        .join("\n")
                });
                // Checked again, so the card shows what is left.
                self.job = Some(Job::start(&ctx, &shared.engine, Request::DriversList));
            }
            Some(Ok(Response::Kernels(report))) => self.kernels = Some(report),
            Some(Ok(Response::KernelsRemoved(done))) => {
                shared.info(trf(
                    "Removed {n} old kernels, {size}.",
                    &[
                        ("n", &count(done.removed.len() as u64)),
                        ("size", &bytes(done.bytes)),
                    ],
                ));
                self.output = (!done.failed.is_empty()).then(|| {
                    done.failed
                        .iter()
                        .map(|(release, why)| {
                            format!("{release}: {}", rewind_os_i18n::tr_text(why))
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                });
                // Checked again, so the card shows what is left.
                self.job = Some(Job::start(&ctx, &shared.engine, Request::KernelsList));
            }
            Some(Ok(Response::RestoreCleaned(done))) => {
                shared.info(trf(
                    "{count} restore points deleted; they took {before}, now {after}",
                    &[
                        ("count", &count(done.deleted as u64)),
                        ("before", &bytes(done.before)),
                        ("after", &bytes(done.after)),
                    ],
                ));
                if !done.failed.is_empty() {
                    shared.error(trf(
                        "Windows did not delete {count} restore points.",
                        &[("count", &count(done.failed.len() as u64))],
                    ));
                }
                self.output = (!done.failed.is_empty()).then(|| {
                    done.failed
                        .iter()
                        .map(|(id, why)| format!("{id}: {why}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                });
                // Read them again, so the card shows what is left.
                self.job = Some(Job::start(&ctx, &shared.engine, Request::RestoreList));
            }
            Some(Ok(Response::Done(message))) => {
                shared.info(tr(message));
                if let Some(o) = shared.overview.as_mut() {
                    // The hibernation file changes; ask for a fresh overview next time.
                    o.system_files
                        .retain(|f| f.kind != SystemFileKind::Hibernation);
                }
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        page_header(
            ui,
            tr("Maintenance"),
            tr("The system's own maintenance tools, run when you ask for them."),
        );
        if !shared.elevated {
            shared.admin_hint(ui);
            ui.add_space(t.gap);
        }
        let ready = shared.elevated && self.job.is_none();
        // The journal can ask for the password for one action instead.
        let journal_ready =
            self.job.is_none() && (shared.elevated || crate::app::can_elevate_actions());
        let windows = available("maintenance.component_cleanup");
        let journal = available("maintenance.journal_vacuum");
        let restore_points = available("restore.list");
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            if let Some(overview) = &shared.overview {
                figures(ui, overview);
                ui.add_space(t.gap * 1.5);
            }
            if journal {
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(tr("System journal")).strong().family(crate::theme::bold()));
                    ui.label(RichText::new(tr("systemd keeps a log of everything the system does. Old entries are rarely needed and can take gigabytes.")).color(t.text_muted));
                    ui.horizontal(|ui| {
                        if ui.add_enabled(self.job.is_none(), egui::Button::new(tr("Measure"))).clicked() {
                            let request = shared.with_rights(Request::JournalUsage);
                            self.job = Some(Job::start(&ctx, &shared.engine, request));
                        }
                        if widgets::primary_button(ui, tr("Keep only the last two weeks"), journal_ready).clicked() {
                            self.confirm = Some(Action::JournalVacuum);
                        }
                    });
                });
            }
            if available("kernels.list") {
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.kernels_card(ui, &ctx, shared);
                });
                ui.add_space(t.gap);
            }
            if windows {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(tr("Windows component store")).strong().family(crate::theme::bold()));
                ui.label(RichText::new(tr("The WinSxS folder looks huge in a disk scan because most of its files are shared with Windows itself. This measures its real size and removes old versions of updated components.")).color(t.text_muted));
                ui.horizontal(|ui| {
                    if ui.add_enabled(ready, egui::Button::new(tr("Measure real size"))).clicked() {
                        self.job = Some(Job::start(&ctx, &shared.engine, Request::AnalyzeComponentStore));
                    }
                    if widgets::primary_button(ui, tr("Remove old components"), ready).clicked() {
                        self.confirm = Some(Action::ComponentCleanup);
                    }
                });
            });
            ui.add_space(t.gap);
            if restore_points {
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.restore_card(ui, &ctx, shared, ready);
                });
                ui.add_space(t.gap);
            }
            if available("drivers.list") {
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.drivers_card(ui, &ctx, shared, ready);
                });
                ui.add_space(t.gap);
            }
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(tr("Hibernation")).strong().family(crate::theme::bold()));
                ui.label(RichText::new(tr("Turning hibernation off deletes hiberfil.sys, which is often as large as your memory. Fast startup also stops working.")).color(t.text_muted));
                ui.horizontal(|ui| {
                    if ui.add_enabled(ready, egui::Button::new(tr("Turn off"))).clicked() {
                        self.confirm = Some(Action::HibernationOff);
                    }
                    if ui.add_enabled(ready, egui::Button::new(tr("Turn on"))).clicked() {
                        self.confirm = Some(Action::HibernationOn);
                    }
                });
            });
            }
            if let Some(job) = &self.job {
                ui.add_space(t.gap);
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    widgets::job_progress(ui, job, false);
                    ui.label(RichText::new(tr("This can take several minutes.")).color(t.text_muted));
                });
            }
            if let Some(output) = &self.output {
                ui.add_space(t.gap);
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(tr("Result")).strong().family(crate::theme::bold()));
                    ui.add(egui::Label::new(RichText::new(output).monospace().small()).wrap());
                });
            }
        });

        if let Some(action) = self.confirm {
            let (title, body, button, request) = match action {
                Action::ComponentCleanup => (
                    tr("Remove old components?"),
                    tr(
                        "Old versions of updated Windows components are removed. Installed updates can no longer be uninstalled afterwards.",
                    ),
                    tr("Remove"),
                    Request::ComponentCleanup,
                ),
                Action::HibernationOff => (
                    tr("Turn hibernation off?"),
                    tr(
                        "hiberfil.sys is deleted and fast startup stops working. You can turn it back on here.",
                    ),
                    tr("Turn off"),
                    Request::Hibernation { enabled: false },
                ),
                Action::JournalVacuum => (
                    tr("Delete old journal entries?"),
                    tr("Entries older than two weeks are deleted for good."),
                    tr("Delete"),
                    Request::JournalVacuum,
                ),
                Action::RestoreClean => (
                    tr("Delete old restore points?"),
                    tr(
                        "Every restore point and shadow copy except the newest on each drive is deleted, as Windows' Disk Cleanup does. You can no longer go back to those moments. This cannot be undone.",
                    ),
                    tr("Delete"),
                    Request::RestoreClean,
                ),
                Action::DriversClean => (
                    tr("Remove old driver packages?"),
                    tr(
                        "The older versions of each driver that no device uses are removed from the driver store. The newest of each driver and every one in use stay. They cannot be put back here; Windows Update or the maker's installer brings a driver again if one is needed.",
                    ),
                    tr("Remove"),
                    Request::DriversClean {
                        packages: Vec::new(),
                    },
                ),
                Action::KernelsClean => (
                    tr("Remove old kernels?"),
                    tr(KERNELS_QUESTION),
                    tr("Remove"),
                    Request::KernelsClean {
                        releases: Vec::new(),
                    },
                ),
                Action::HibernationOn => (
                    tr("Turn hibernation on?"),
                    tr("Windows creates hiberfil.sys again."),
                    tr("Turn on"),
                    Request::Hibernation { enabled: true },
                ),
            };
            match widgets::confirm(&ctx, "confirm_maintenance", title, body, button) {
                Choice::Open => {}
                Choice::Cancelled => self.confirm = None,
                Choice::Confirmed => {
                    self.confirm = None;
                    self.output = None;
                    let request = shared.with_rights(request);
                    self.job = Some(Job::start(&ctx, &shared.engine, request));
                }
            }
        }
    }
}

impl State {
    /// Restore points: how many, what they take, and deleting all but the newest.
    fn restore_card(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        shared: &mut Shared,
        ready: bool,
    ) {
        let t = theme::current(ctx);
        ui.label(
            RichText::new(tr("Restore points"))
                .strong()
                .family(crate::theme::bold()),
        );
        ui.label(
            RichText::new(tr(
                "Windows keeps restore points and other copies of your drives to undo changes. Deleting all but the newest frees their space; the newest one on each drive is always kept.",
            ))
            .color(t.text_muted),
        );
        let deletable = self
            .restore
            .as_ref()
            .map_or(0, |r| restore::all_but_newest(&r.copies).len());
        if let Some(report) = &self.restore {
            ui.add_space(4.0);
            if report.copies.is_empty() {
                ui.label(tr("There are no restore points."));
            } else {
                ui.label(
                    RichText::new(trf(
                        "{count} restore points take {size}",
                        &[
                            ("count", &count(report.copies.len() as u64)),
                            ("size", &bytes(report.used())),
                        ],
                    ))
                    .family(crate::theme::bold()),
                );
                if let Some(newest) = report.copies.last() {
                    let line = match &newest.description {
                        Some(what) => trf(
                            "Newest: {date}, {what}",
                            &[("date", &local_date(newest.created)), ("what", what)],
                        ),
                        None => trf("Newest: {date}", &[("date", &local_date(newest.created))]),
                    };
                    ui.label(RichText::new(line).color(t.text_muted));
                }
                for storage in &report.storage {
                    if let Some(max) = storage.max {
                        ui.label(
                            RichText::new(trf(
                                "{drive}: Windows lets them take up to {max}",
                                &[("drive", &storage.drive), ("max", &bytes(max))],
                            ))
                            .small()
                            .color(t.text_muted),
                        );
                    }
                }
            }
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ready, egui::Button::new(tr("Check")))
                .clicked()
            {
                self.job = Some(Job::start(ctx, &shared.engine, Request::RestoreList));
            }
            let label = if deletable > 0 {
                trf(
                    "Delete {count} old ones",
                    &[("count", &count(deletable as u64))],
                )
            } else {
                tr("Delete all but the newest").to_owned()
            };
            let button = widgets::primary_button(ui, &label, ready && deletable > 0);
            if self.restore.is_none() {
                button.on_disabled_hover_text(tr("Check first how many there are."));
            } else if button.clicked() {
                self.confirm = Some(Action::RestoreClean);
            }
        });
    }

    /// Old driver packages: how many, what they take, and removing them.
    fn drivers_card(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        shared: &mut Shared,
        ready: bool,
    ) {
        let t = theme::current(ctx);
        ui.label(
            RichText::new(tr("Old driver packages"))
                .strong()
                .family(crate::theme::bold()),
        );
        ui.label(
            RichText::new(tr(
                "Every driver update leaves the previous version in the driver store. The newest of each driver and every one a device uses always stay.",
            ))
            .color(t.text_muted),
        );
        let old = self.drivers.as_ref().map_or(0, |r| r.old.len());
        if let Some(report) = &self.drivers {
            ui.add_space(4.0);
            if report.old.is_empty() {
                ui.label(tr(
                    "No old driver packages: each driver has only the version in use.",
                ));
            } else {
                ui.label(
                    RichText::new(trf(
                        "{n} old driver packages take {size}.",
                        &[
                            ("n", &count(report.old.len() as u64)),
                            ("size", &bytes(report.reclaimable())),
                        ],
                    ))
                    .family(crate::theme::bold()),
                );
                for p in report.old.iter().take(5) {
                    ui.label(
                        RichText::new(format!(
                            "{}  ·  {} {} {}  ({})",
                            bytes(p.bytes),
                            p.provider,
                            p.original,
                            p.version,
                            trf("newest: {version}", &[("version", &p.newest)])
                        ))
                        .small()
                        .color(t.text_muted),
                    );
                }
            }
            if report.old_in_use > 0 {
                ui.label(
                    RichText::new(trf(
                        "{n} older packages stay because a device uses them.",
                        &[("n", &count(report.old_in_use as u64))],
                    ))
                    .small()
                    .color(t.text_muted),
                );
            }
        }
        ui.horizontal(|ui| {
            // Reading needs no administrator rights.
            if ui
                .add_enabled(self.job.is_none(), egui::Button::new(tr("Check")))
                .clicked()
            {
                self.job = Some(Job::start(ctx, &shared.engine, Request::DriversList));
            }
            let label = if old > 0 {
                trf("Remove {count} old ones", &[("count", &count(old as u64))])
            } else {
                tr("Remove old packages").to_owned()
            };
            let button = widgets::primary_button(ui, &label, ready && old > 0);
            if self.drivers.is_none() {
                button.on_disabled_hover_text(tr("Check first how many there are."));
            } else if button.clicked() {
                self.confirm = Some(Action::DriversClean);
            }
        });
    }
}

impl State {
    /// Old kernels: which are installed, what the old ones take, and removing them.
    fn kernels_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, shared: &mut Shared) {
        let t = theme::current(ctx);
        ui.label(
            RichText::new(tr("Old kernels"))
                .strong()
                .family(crate::theme::bold()),
        );
        ui.label(RichText::new(tr(KERNELS_ABOUT)).color(t.text_muted));
        let old = self.kernels.as_ref().map_or(0, |r| r.old().count());
        if let Some(report) = &self.kernels {
            ui.add_space(4.0);
            if report.kernels.is_empty() {
                ui.label(tr(rewind_os_engine::kernels::NO_PACKAGES));
            } else if old == 0 {
                ui.label(tr(
                    "No old kernels: only the running one and the newest are installed.",
                ));
            } else {
                ui.label(
                    RichText::new(trf(
                        "{n} old kernels take {size}.",
                        &[
                            ("n", &count(old as u64)),
                            ("size", &bytes(report.reclaimable())),
                        ],
                    ))
                    .family(crate::theme::bold()),
                );
            }
            for k in &report.kernels {
                let why = if k.running {
                    format!("  ({})", tr("running now, kept"))
                } else if k.newest {
                    format!("  ({})", tr("newest, kept"))
                } else {
                    String::new()
                };
                ui.label(
                    RichText::new(format!("{}  ·  {}{why}", bytes(k.bytes), k.release))
                        .small()
                        .color(t.text_muted),
                );
            }
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.job.is_none(), egui::Button::new(tr("Check")))
                .clicked()
            {
                self.job = Some(Job::start(ctx, &shared.engine, Request::KernelsList));
            }
            let label = if old > 0 {
                trf("Remove {count} old ones", &[("count", &count(old as u64))])
            } else {
                tr("Remove old kernels").to_owned()
            };
            // The package manager asks for the password itself.
            let button = widgets::primary_button(ui, &label, self.job.is_none() && old > 0);
            if self.kernels.is_none() {
                button.on_disabled_hover_text(tr("Check first how many there are."));
            } else if button.clicked() {
                self.confirm = Some(Action::KernelsClean);
            }
        });
    }
}

const KERNELS_ABOUT: &str = "Each kernel update installs the new kernel next to the old ones, in /boot and /lib/modules. The kernel running now and the newest always stay.";
const KERNELS_QUESTION: &str = "The old kernels are removed with the package manager, which asks for your password. The running kernel and the newest stay, so the computer keeps starting. If removing them would take other packages along, Rewind OS stops.";

/// A key figure: label, value, detail and an optional meter.
struct Tile(String, String, String, Option<(f32, egui::Color32)>);

/// The system drive, and the files the system manages itself, as key figures.
fn figures(ui: &mut egui::Ui, overview: &Overview) {
    let t = theme::current(ui.ctx());
    let system = overview.drives.iter().find(|d| {
        let mount = d.mount.to_string_lossy();
        mount == "/" || mount.eq_ignore_ascii_case(r"C:\")
    });
    let mut tiles: Vec<Tile> = Vec::new();
    if let Some(drive) = system {
        let fraction = if drive.total > 0 {
            drive.used() as f32 / drive.total as f32
        } else {
            0.0
        };
        tiles.push(Tile(
            tr("System drive").to_owned(),
            bytes(drive.available),
            trf("free of {total}", &[("total", &bytes(drive.total))]),
            Some((fraction, widgets::fill_color(t, fraction))),
        ));
    }
    for file in &overview.system_files {
        let name = match file.kind {
            SystemFileKind::Hibernation => tr("Hibernation file"),
            SystemFileKind::PageFile => tr("Page file"),
            SystemFileKind::SwapFile => tr("Swap file"),
        };
        tiles.push(Tile(
            name.to_owned(),
            bytes(file.bytes),
            file.path.display().to_string(),
            None,
        ));
    }
    // At least three columns, so a lone tile keeps the size it has next to others.
    charts::grid(ui, tiles.len().max(3), 180.0, |ui, i| {
        if let Some(Tile(label, value, detail, meter)) = tiles.get(i) {
            charts::stat_tile(ui, label, value, detail, *meter);
        }
    });
}
