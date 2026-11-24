use crate::app::Shared;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, card, page_header};
use egui::RichText;
use rewind_os_engine::system::SystemFileKind;
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::tr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    ComponentCleanup,
    HibernationOff,
    HibernationOn,
    JournalVacuum,
}

fn available(id: &str) -> bool {
    rewind_os_engine::tool(id).is_some_and(|t| t.available())
}

#[derive(Default)]
pub struct State {
    job: Option<Job>,
    output: Option<String>,
    confirm: Option<Action>,
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
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            if journal {
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(tr("System journal")).strong());
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
            if windows {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(tr("Windows component store")).strong());
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
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(tr("Hibernation")).strong());
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
                    ui.label(RichText::new(tr("Result")).strong());
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
