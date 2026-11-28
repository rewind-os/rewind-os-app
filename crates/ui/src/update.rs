//! New versions: a button at the foot of the navigation when a newer release is out, and the
//! version number next to the app's name. The check itself is the engine's (`update.check`):
//! each time the app starts, and only the app's version goes out; a failed check stays quiet.
//! `rewind-os-cli update auto off` turns it off.
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
    /// The confirmation before installing is open.
    confirm: bool,
    install: Option<Job>,
}

impl Notice {
    /// Starts the check at start (it asks GitHub only when it is on).
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
                confirm: false,
                install: None,
            };
        }
        Self {
            job: Some(Job::start(ctx, engine, Request::UpdateCheck { now: false })),
            status: None,
            confirm: false,
            install: None,
        }
    }

    /// Takes in the outcome of a running check. Call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        // A check at start that failed (offline) stays quiet.
        if let Some(Ok(Response::Update(status))) = jobs::finished(&mut self.job, ctx) {
            self.status = Some(status);
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
        let (button_text, hover) = match manual {
            Some(why) => (
                tr("See what's new"),
                format!(
                    "{} {}",
                    tr(why),
                    trf("You have {current}.", &[("current", update::CURRENT)])
                ),
            ),
            None => (
                tr("Update"),
                trf(
                    "You have {current}. Downloads the new version, checks its signature and installs it.",
                    &[("current", update::CURRENT)],
                ),
            ),
        };
        let mut clicked = false;
        ui.add_space(8.0);
        // A card in the colour of the current page's pill, as wide as the navigation's items:
        // what is new, then the one thing to do about it.
        egui::Frame::new()
            .fill(t.surface_raised)
            .corner_radius(egui::CornerRadius::same(t.radius))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    widgets::icon_tile(ui, crate::glyphs::Glyph::Download, t.accent_fill, 28.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new(tr("New version")).family(theme::bold()));
                        ui.label(
                            RichText::new(trf(
                                "{version} · you have {current}",
                                &[("version", &version), ("current", update::CURRENT)],
                            ))
                            .small()
                            .color(t.text_muted),
                        );
                    });
                });
                ui.add_space(10.0);
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
                let button = ui.add_sized(
                    egui::vec2(ui.available_width(), 30.0),
                    egui::Button::new(
                        RichText::new(button_text)
                            .color(t.on_accent)
                            .family(theme::bold()),
                    )
                    .fill(t.accent_fill),
                );
                clicked = button.on_hover_text(hover).clicked();
            });
        if clicked {
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

    /// The version number, as plain text.
    pub fn version(&self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        ui.label(
            RichText::new(format!("v{}", update::CURRENT))
                .small()
                .color(t.text_muted),
        );
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
