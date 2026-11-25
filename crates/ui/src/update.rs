//! New versions: a button at the foot of the navigation when a newer release is out, and the
//! version number, whose menu turns the daily check on or off and checks at once. The check
//! itself is the engine's (`update.check`): at most once a day, and only the app's version goes
//! out. A failed daily check stays quiet; one asked for by hand says what went wrong.
//!
//! A copy that can update itself (the MSI, a portable copy) offers "Update to …": after a
//! confirmation the engine downloads the new version, checks its signature and installs it
//! (`update.install`), and the app closes for the new version to open. Other copies open the
//! version's page, and say why.

use crate::app::Shared;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets;
use egui::RichText;
use rewind_os_engine::update::{self, Release, Status, Way};
use rewind_os_engine::{Engine, Request, Response, Task};
use rewind_os_i18n::{tr, trf};
use std::sync::Arc;

pub struct Notice {
    job: Option<Job>,
    status: Option<Status>,
    /// The running check was asked for by hand, so its outcome is reported.
    asked: bool,
    /// The confirmation before installing is open.
    confirm: bool,
    install: Option<Job>,
}

impl Notice {
    /// Starts the daily check (it asks GitHub only when it is on and due).
    pub fn new(ctx: &egui::Context, engine: &Arc<Engine>) -> Self {
        // What the last update left behind (the replaced programs, the download).
        update::tidy();
        if let Some(version) = crate::devtools::newer_version() {
            return Self {
                job: None,
                status: Some(Status {
                    current: update::CURRENT,
                    latest: Some(Release {
                        url: format!("{}tag/v{version}", update::RELEASES_PAGE),
                        version,
                        published: None,
                    }),
                    newer: true,
                    checked: None,
                    auto: true,
                    way: Way::Portable,
                    manual: None,
                }),
                asked: false,
                confirm: false,
                install: None,
            };
        }
        Self {
            job: Some(Job::start(ctx, engine, Request::UpdateCheck { now: false })),
            status: None,
            asked: false,
            confirm: false,
            install: None,
        }
    }

    /// Takes in the outcome of a running check. Call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        match jobs::finished(&mut self.job, ctx) {
            Some(Ok(Response::Update(status))) => {
                if self.asked && !status.newer {
                    shared.info(trf(
                        "Rewind OS {current} is the newest version.",
                        &[("current", status.current)],
                    ));
                }
                self.status = Some(status);
                self.asked = false;
            }
            Some(Ok(_)) => self.asked = false,
            Some(Err(err)) => {
                if self.asked {
                    shared.error(trf(
                        "Could not check for a new version: {error}",
                        &[("error", &err)],
                    ));
                }
                self.asked = false;
            }
            None => {}
        }
        match jobs::finished(&mut self.install, ctx) {
            Some(Ok(Response::UpdateInstalled(done))) if done.restarting => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(trf(
                "The update was not installed: {error}",
                &[("error", &err)],
            )),
            None => {}
        }
    }

    /// When a newer version is out, a button that installs it (or opens its page, for copies
    /// that cannot update themselves).
    pub fn notice(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let Some(latest) = self
            .status
            .as_ref()
            .filter(|s| s.newer)
            .and_then(|s| s.latest.as_ref())
        else {
            return;
        };
        let t = theme::current(ui.ctx());
        let version = latest.version.clone();
        let manual = self
            .status
            .as_ref()
            .and_then(|s| (s.way == Way::Manual).then_some(s.manual))
            .map(|why| why.unwrap_or(update::UNKNOWN_PLACE));
        ui.add_space(8.0);
        if let Some(job) = &self.install {
            let stage = job.task.snapshot().stage;
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new(if stage.is_empty() {
                        tr("Updating")
                    } else {
                        tr(stage)
                    })
                    .small()
                    .color(t.text_muted),
                );
            });
            return;
        }
        let (text, hover) = match manual {
            Some(why) => (
                trf("Version {version} is out", &[("version", &version)]),
                format!(
                    "{} {}",
                    tr(why),
                    trf("You have {current}.", &[("current", update::CURRENT)])
                ),
            ),
            None => (
                trf("Update to {version}", &[("version", &version)]),
                trf(
                    "You have {current}. Downloads the new version, checks its signature and installs it.",
                    &[("current", update::CURRENT)],
                ),
            ),
        };
        let button = ui.add_sized(
            egui::vec2(ui.available_width(), 32.0),
            egui::Button::new(
                RichText::new(text)
                    .color(theme::current(ui.ctx()).on_accent)
                    .family(theme::bold()),
            )
            .fill(theme::current(ui.ctx()).accent_fill),
        );
        if button.on_hover_text(hover).clicked() {
            if manual.is_some() {
                run(shared, Request::UpdateNotes);
            } else {
                self.confirm = true;
            }
        }
        if self.confirm {
            self.confirm_install(ui.ctx(), shared, &version);
        }
    }

    fn confirm_install(&mut self, ctx: &egui::Context, shared: &mut Shared, version: &str) {
        let installer = self
            .status
            .as_ref()
            .is_some_and(|s| s.way == Way::Installer);
        let body = if installer {
            trf(
                "Rewind OS downloads version {version} from GitHub, checks that the Rewind OS project signed it, and starts its installer. Windows asks for permission; Rewind OS closes now and opens again when the installer finishes.",
                &[("version", version)],
            )
        } else {
            let folder = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|d| d.display().to_string()))
                .unwrap_or_default();
            trf(
                "Rewind OS downloads version {version} from GitHub, checks that the Rewind OS project signed it, replaces its programs in {folder} and opens again. If anything fails, this version stays.",
                &[("version", version), ("folder", &folder)],
            )
        };
        match widgets::confirm(
            ctx,
            "confirm_update",
            tr("Update Rewind OS?"),
            &body,
            tr("Update and restart"),
        ) {
            widgets::Choice::Open => {}
            widgets::Choice::Cancelled => self.confirm = false,
            widgets::Choice::Confirmed => {
                self.confirm = false;
                self.install = Some(Job::start(
                    ctx,
                    &shared.engine,
                    Request::UpdateInstall { restart: true },
                ));
            }
        }
    }

    /// The version number; its menu turns the daily check on or off and checks at once.
    pub fn version_menu(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let t = theme::current(ui.ctx());
        let label = RichText::new(format!("v{}", update::CURRENT))
            .small()
            .color(t.text_muted);
        let button = egui::Button::new(label).frame(false);
        let (response, _) = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
            ui.label(
                RichText::new(trf("Rewind OS {version}", &[("version", update::CURRENT)]))
                    .family(theme::bold()),
            );
            let mut auto = self.status.as_ref().is_none_or(|s| s.auto);
            if ui
                .checkbox(&mut auto, tr("Check for a new version once a day"))
                .changed()
            {
                run(shared, Request::UpdateAuto { enabled: auto });
                match &mut self.status {
                    Some(status) => status.auto = auto,
                    None if self.job.is_none() => {
                        self.job = Some(Job::start(
                            ui.ctx(),
                            &shared.engine,
                            Request::UpdateCheck { now: false },
                        ));
                    }
                    None => {}
                }
            }
            let newer = self
                .status
                .as_ref()
                .filter(|s| s.newer)
                .and_then(|s| s.latest.as_ref())
                .map(|l| l.version.clone());
            if let Some(version) = newer
                && ui
                    .button(trf("What's new in {version}", &[("version", &version)]))
                    .clicked()
            {
                run(shared, Request::UpdateNotes);
                ui.close();
            }
            if ui
                .add_enabled(self.job.is_none(), egui::Button::new(tr("Check now")))
                .clicked()
            {
                self.job = Some(Job::start(
                    ui.ctx(),
                    &shared.engine,
                    Request::UpdateCheck { now: true },
                ));
                self.asked = true;
                ui.close();
            }
        });
        response.on_hover_text(tr("Version and new versions"));
    }
}

/// Runs a quick request and reports its outcome in the status bar.
fn run(shared: &mut Shared, request: Request) {
    match shared.engine.run(request, &Task::new()) {
        Ok(Response::Done(message)) => shared.info(tr(message)),
        Ok(_) => {}
        Err(err) => shared.error(widgets::error_text(&err)),
    }
}
