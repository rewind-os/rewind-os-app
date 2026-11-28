//! Running programs: what each takes of the processor and memory right now
//! (`processes.list`), closing one (`processes.close`), and keeping one from starting with the
//! system (`startup.set`), and the power plan (`power.list`, `power.set`).

use crate::app::Shared;
use crate::charts;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, page_header_with};
use egui::RichText;
use rewind_os_engine::format::{bytes, count};
use rewind_os_engine::power;
use rewind_os_engine::processes::{Program, Report};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};

/// Programs listed.
const SHOWN: usize = 30;

#[derive(Default)]
pub struct State {
    job: Option<Job>,
    report: Option<Result<Report, String>>,
    by_memory: bool,
    /// Closing a program or turning off its startup entry.
    action: Option<Job>,
    /// The program to close, and whether to force it, waiting for confirmation.
    confirm: Option<(Program, bool)>,
    /// A program that did not close when asked; forcing it is offered.
    stuck: Option<Program>,
    /// Startup entries turned off from here.
    turned_off: Vec<String>,
    power_job: Option<Job>,
    power: Option<Result<power::Report, String>>,
    /// Switching plan or opening the power settings.
    power_action: Option<Job>,
}

impl State {
    pub fn busy(&self) -> bool {
        self.job.is_some()
            || self.action.is_some()
            || self.power_job.is_some()
            || self.power_action.is_some()
    }

    fn measure(&mut self) {
        self.report = None;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if self.report.is_none() && self.job.is_none() {
            self.job = Some(Job::start(&ctx, &shared.engine, Request::ProcessesList));
        }
        match jobs::finished(&mut self.job, &ctx) {
            Some(Ok(Response::Processes(report))) => self.report = Some(Ok(report)),
            Some(Ok(_)) => {}
            Some(Err(err)) => self.report = Some(Err(err)),
            None => {}
        }
        match jobs::finished(&mut self.action, &ctx) {
            Some(Ok(Response::ProcessClosed(done))) => {
                let program = self.program(&done.name);
                if let Some(why) = done.refusal {
                    shared.error(tr(why));
                } else if done.running == 0 {
                    shared.info(trf("{name} was closed.", &[("name", &done.name)]));
                    self.stuck = None;
                } else {
                    shared.error(trf(
                        "{name} is still running: it may keep running in the background, or be asking to save your work.",
                        &[("name", &done.name)],
                    ));
                    self.stuck = program;
                }
                self.measure();
            }
            Some(Ok(Response::Done(message))) => shared.info(tr(message)),
            Some(Ok(_)) => {}
            Some(Err(err)) => {
                // A startup entry that was not turned off is shown as on again.
                self.turned_off.pop();
                shared.error(err);
            }
            None => {}
        }

        self.poll_power(&ctx, shared);
        let measuring = self.job.is_some();
        page_header_with(
            ui,
            tr("Running programs"),
            tr("What is using the processor and memory right now."),
            |ui| {
                if ui
                    .add_enabled(!measuring, egui::Button::new(tr("Measure again")))
                    .clicked()
                {
                    self.measure();
                }
            },
        );
        let report = match &self.report {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(tr("Watching what each program uses…")).color(t.text_muted),
                    );
                });
                return;
            }
            Some(Err(err)) => {
                ui.label(RichText::new(err).color(t.text_muted));
                return;
            }
            Some(Ok(report)) => report,
        };
        let mut close = None;
        let mut turn_off = None;
        let mut power_request = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                figures(ui, report);
                ui.add_space(t.gap);
                if rewind_os_engine::tool("power.list").is_some_and(|tool| tool.available()) {
                    power_request = power_card(ui, &self.power, self.power_action.is_none());
                    ui.add_space(t.gap);
                }
                if let Some(program) = &self.stuck {
                    widgets::card(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(trf(
                                "{name} is still running. Many programs only hide when closed and keep running in the background, next to the clock; others ask to save first. Look for its window, or force it to close.",
                                &[("name", &program.name)],
                            ));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if widgets::danger_button(ui, tr("Force close"), self.action.is_none()).clicked() {
                                    close = Some((program.clone(), true));
                                }
                            });
                        });
                    });
                    ui.add_space(t.gap);
                }
                ui.horizontal(|ui| {
                    charts::section_title(ui, tr("Heaviest programs"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.selectable_label(self.by_memory, tr("Memory")).clicked() {
                            self.by_memory = true;
                        }
                        if ui.selectable_label(!self.by_memory, tr("Processor")).clicked() {
                            self.by_memory = false;
                        }
                        ui.label(RichText::new(tr("Sort by")).small().color(t.text_muted));
                    });
                });
                let mut programs: Vec<&Program> = report.programs.iter().collect();
                if self.by_memory {
                    programs.sort_by_key(|p| std::cmp::Reverse(p.memory));
                }
                let idle = self.action.is_none();
                for (index, program) in programs.into_iter().take(SHOWN).enumerate() {
                    let startup = program
                        .startup
                        .as_ref()
                        .filter(|id| !self.turned_off.contains(id));
                    widgets::list_row(ui, index, 56.0, |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let button = widgets::row_button(ui, tr("Close"), idle && program.refusal.is_none());
                            let button = match program.refusal {
                                Some(why) => button.on_disabled_hover_text(tr(why)),
                                None => button.on_hover_text(tr("Asks it to close, as closing its window would.")),
                            };
                            if button.clicked() {
                                close = Some((program.clone(), false));
                            }
                            if let Some(id) = startup {
                                let off = widgets::row_button(ui, tr("Turn off at startup"), idle)
                                    .on_hover_text(tr("It stops starting with the system; you can turn it back on in the list of startup programs."));
                                if off.clicked() {
                                    turn_off = Some(id.clone());
                                }
                            }
                            ui.allocate_ui_with_layout(
                                egui::vec2(80.0, 20.0),
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| ui.label(RichText::new(bytes(program.memory)).color(t.text_muted)),
                            );
                            ui.allocate_ui_with_layout(
                                egui::vec2(64.0, 20.0),
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| ui.label(RichText::new(format!("{:.1} %", program.cpu)).family(theme::bold())),
                            );
                            if startup.is_some() {
                                widgets::badge(ui, tr("Starts with the system"), t.accent);
                            }
                            if let Some(why) = program.refusal {
                                widgets::badge(ui, tr(refusal_word(why)), t.text_muted);
                            }
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                let line = if program.pids.len() > 1 {
                                    trf("{n} processes", &[("n", &count(program.pids.len() as u64))])
                                } else {
                                    program
                                        .exe
                                        .as_ref()
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_default()
                                };
                                widgets::card_text(ui, &program.name, &line);
                            });
                        });
                    });
                    ui.add_space(6.0);
                }
            });
        if let Some(request) = power_request {
            self.power_action = Some(Job::start(&ctx, &shared.engine, request));
        }
        if let Some(id) = turn_off {
            self.turned_off.push(id.clone());
            let request = shared.with_rights(Request::StartupSet { id, enabled: false });
            self.action = Some(Job::start(&ctx, &shared.engine, request));
        }
        if close.is_some() {
            self.confirm = close;
        }
        if let Some((program, force)) = &self.confirm {
            let (title, body, button) = if *force {
                (
                    trf("Force {name} to close?", &[("name", &program.name)]),
                    tr("It ends at once, and any unsaved work in it is lost."),
                    tr("Force close"),
                )
            } else {
                (
                    trf("Close {name}?", &[("name", &program.name)]),
                    tr(
                        "It is asked to close as if you closed its window. If it has unsaved work, it may ask you to save it first.",
                    ),
                    tr("Close"),
                )
            };
            match widgets::confirm(&ctx, "confirm_close_program", &title, body, button) {
                Choice::Open => {}
                Choice::Cancelled => self.confirm = None,
                Choice::Confirmed => {
                    let request = Request::ProcessesClose {
                        name: program.name.clone(),
                        pids: program.pids.clone(),
                        force: *force,
                    };
                    self.confirm = None;
                    self.action = Some(Job::start(&ctx, &shared.engine, request));
                }
            }
        }
    }

    fn poll_power(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        let available = rewind_os_engine::tool("power.list").is_some_and(|t| t.available());
        if available && self.power.is_none() && self.power_job.is_none() {
            self.power_job = Some(Job::start(ctx, &shared.engine, Request::PowerList));
        }
        match jobs::finished(&mut self.power_job, ctx) {
            Some(Ok(Response::Power(report))) => self.power = Some(Ok(report)),
            Some(Ok(_)) => {}
            Some(Err(err)) => self.power = Some(Err(err)),
            None => {}
        }
        match jobs::finished(&mut self.power_action, ctx) {
            Some(Ok(Response::Done(message))) => {
                shared.info(tr(message));
                self.power = None;
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
    }

    /// The listed program with this name that may be closed.
    fn program(&self, name: &str) -> Option<Program> {
        let Some(Ok(report)) = &self.report else {
            return None;
        };
        report
            .programs
            .iter()
            .find(|p| p.name == name && p.refusal.is_none())
            .cloned()
    }
}

/// The power plans to pick from, and Windows' power mode with the way to change it. Returns
/// what the user asked for.
fn power_card(
    ui: &mut egui::Ui,
    power: &Option<Result<power::Report, String>>,
    idle: bool,
) -> Option<Request> {
    let t = theme::current(ui.ctx());
    let mut request = None;
    widgets::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(tr("Power plan"))
                .strong()
                .family(theme::bold()),
        );
        ui.label(
            RichText::new(tr("How much the system favours speed over saving energy."))
                .color(t.text_muted),
        );
        ui.add_space(6.0);
        let report = match power {
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
        ui.horizontal_wrapped(|ui| {
            for plan in &report.plans {
                let name = if plan.kind == power::Kind::Custom {
                    plan.name.clone()
                } else {
                    tr(plan.kind.label()).to_owned()
                };
                let button = ui.add_enabled(
                    idle || plan.active,
                    egui::Button::selectable(plan.active, name),
                );
                let button = match &plan.degraded {
                    Some(why) => button.on_hover_text(trf(
                        "The system holds this plan back right now: {reason}",
                        &[("reason", why)],
                    )),
                    None => button,
                };
                if button.clicked() && !plan.active {
                    request = Some(Request::PowerSet {
                        id: plan.id.clone(),
                    });
                }
            }
        });
        for plan in report.plans.iter().filter(|p| p.degraded.is_some()) {
            let name = if plan.kind == power::Kind::Custom {
                plan.name.clone()
            } else {
                tr(plan.kind.label()).to_owned()
            };
            ui.label(
                RichText::new(trf(
                    "{plan} is held back right now: {reason}",
                    &[
                        ("plan", &name),
                        ("reason", plan.degraded.as_deref().unwrap_or_default()),
                    ],
                ))
                .small()
                .color(t.text_muted),
            );
        }
        if let Some(plugged) = report.mode_plugged_in {
            ui.add_space(6.0);
            let mut line = trf(
                "Power mode: {mode} when plugged in",
                &[("mode", tr(plugged.label()))],
            );
            if let Some(battery) = report.mode_on_battery {
                line.push_str(&format!(
                    ", {}",
                    trf("{mode} on battery", &[("mode", tr(battery.label()))])
                ));
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(line);
                if ui
                    .add_enabled(idle, egui::Button::new(tr("Change in Settings")))
                    .clicked()
                {
                    request = Some(Request::PowerSettings);
                }
            });
            if report.plans.len() <= 1
                && rewind_os_engine::Os::current() == rewind_os_engine::Os::Windows
            {
                ui.label(
                    RichText::new(tr(
                        "This computer has one power plan; Windows' power mode decides how fast it runs.",
                    ))
                    .small()
                    .color(t.text_muted),
                );
            }
        }
    });
    request
}

/// A word for why a program is not closed, for its badge; hovering its button says more.
fn refusal_word(why: &str) -> &'static str {
    use rewind_os_engine::safety;
    if why == safety::OTHER_ACCOUNT {
        "Other account"
    } else if why == safety::THIS_APP || why == safety::HOLDS_APP {
        "Rewind OS"
    } else {
        "System"
    }
}

/// The whole processor and memory in use, and how many programs run.
fn figures(ui: &mut egui::Ui, report: &Report) {
    let t = theme::current(ui.ctx());
    let cpu = report.cpu / 100.0;
    let memory = if report.memory_total > 0 {
        report.memory_used as f32 / report.memory_total as f32
    } else {
        0.0
    };
    charts::grid(ui, 3, 180.0, |ui, i| match i {
        0 => charts::stat_tile(
            ui,
            tr("Processor in use"),
            &format!("{:.0} %", report.cpu),
            tr("Over the last second"),
            Some((cpu, widgets::fill_color(t, cpu))),
        ),
        1 => charts::stat_tile(
            ui,
            tr("Memory in use"),
            &bytes(report.memory_used),
            &trf("of {total}", &[("total", &bytes(report.memory_total))]),
            Some((memory, widgets::fill_color(t, memory))),
        ),
        _ => charts::stat_tile(
            ui,
            tr("Programs running"),
            &count(report.programs.len() as u64),
            &trf("{n} processes", &[("n", &count(report.processes as u64))]),
            None,
        ),
    });
}
