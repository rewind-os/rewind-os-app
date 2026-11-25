//! New versions: a notice at the foot of the navigation when a newer release is out, and the
//! version number, whose menu turns the daily check on or off and checks at once. The check
//! itself is the engine's (`update.check`): at most once a day, and only the app's version goes
//! out. A failed daily check stays quiet; one asked for by hand says what went wrong.

use crate::app::Shared;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets;
use egui::RichText;
use rewind_os_engine::update::{self, Release, Status};
use rewind_os_engine::{Engine, Request, Response, Task};
use rewind_os_i18n::{tr, trf};
use std::sync::Arc;

pub struct Notice {
    job: Option<Job>,
    status: Option<Status>,
    /// The running check was asked for by hand, so its outcome is reported.
    asked: bool,
}

impl Notice {
    /// Starts the daily check (it asks GitHub only when it is on and due).
    pub fn new(ctx: &egui::Context, engine: &Arc<Engine>) -> Self {
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
                }),
                asked: false,
            };
        }
        Self {
            job: Some(Job::start(ctx, engine, Request::UpdateCheck { now: false })),
            status: None,
            asked: false,
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
    }

    /// When a newer version is out, a button that says so and opens its page.
    pub fn notice(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let Some(latest) = self
            .status
            .as_ref()
            .filter(|s| s.newer)
            .and_then(|s| s.latest.as_ref())
        else {
            return;
        };
        let text = trf("Version {version} is out", &[("version", &latest.version)]);
        let hover = trf(
            "You have {current}. Opens the new version's page, with its notes and downloads.",
            &[("current", update::CURRENT)],
        );
        ui.add_space(8.0);
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
            run(shared, Request::UpdateNotes);
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
