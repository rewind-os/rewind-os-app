use crate::app::{Page, Shared};
use crate::charts;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, page_header};
use egui::RichText;
use rewind_os_engine::format::{bytes, count};
use rewind_os_engine::programs::{InstalledProgram, Scope};
use rewind_os_engine::{Os, Request, Response, Task};
use rewind_os_i18n::{tr, trf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Sort {
    #[default]
    Name,
    Size,
    Date,
}

#[derive(Default)]
pub struct State {
    job: Option<Job>,
    uninstall_job: Option<Job>,
    programs: Option<Vec<InstalledProgram>>,
    search: String,
    sort: Sort,
    uninstall: Option<InstalledProgram>,
    started: bool,
}

impl State {
    pub fn busy(&self) -> bool {
        self.job.is_some() || self.uninstall_job.is_some()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if !self.started {
            self.started = true;
            self.job = Some(Job::start(&ctx, &shared.engine, Request::ProgramsList));
        }
        match jobs::finished(&mut self.job, &ctx) {
            Some(Ok(Response::Programs(list))) => {
                shared
                    .icons
                    .request(&ctx, list.iter().filter_map(|p| p.display_icon.clone()));
                self.programs = Some(list);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        match jobs::finished(&mut self.uninstall_job, &ctx) {
            Some(Ok(Response::Done(message))) => {
                shared.info(tr(message));
                // Package managers and the Trash change the list at once.
                // The list changes when a package manager removed the program.
                if Os::current() != Os::Windows {
                    self.job = Some(Job::start(&ctx, &shared.engine, Request::ProgramsList));
                }
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        page_header(
            ui,
            tr("Installed programs"),
            if Page::Leftovers.available() {
                tr("Uninstall what you no longer use, then look for its leftovers.")
            } else if Os::current() == Os::MacOs {
                tr("The apps in Applications. Uninstalling moves an app to the Trash.")
            } else {
                tr(
                    "Programs installed with the system's package managers, without libraries and the base system.",
                )
            },
        );
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text(tr("Search by name or publisher"))
                    .desired_width(280.0),
            );
            ui.label(tr("Sort by"));
            ui.selectable_value(&mut self.sort, Sort::Name, tr("Name"));
            ui.selectable_value(&mut self.sort, Sort::Size, tr("Size"));
            ui.selectable_value(&mut self.sort, Sort::Date, tr("Install date"));
            if ui
                .add_enabled(self.job.is_none(), egui::Button::new(tr("Refresh")))
                .clicked()
            {
                self.job = Some(Job::start(&ctx, &shared.engine, Request::ProgramsList));
            }
            if Page::Leftovers.available() && ui.button(tr("Find program leftovers")).clicked() {
                shared.goto = Some(Page::Leftovers);
            }
        });
        ui.add_space(t.gap);
        if self.job.is_some() {
            ui.spinner();
        }
        let Some(programs) = &self.programs else {
            return;
        };
        let needle = self.search.to_lowercase();
        let mut shown: Vec<&InstalledProgram> = programs
            .iter()
            .filter(|p| {
                needle.is_empty()
                    || p.name.to_lowercase().contains(&needle)
                    || p.publisher
                        .as_deref()
                        .is_some_and(|x| x.to_lowercase().contains(&needle))
            })
            .collect();
        match self.sort {
            Sort::Name => {}
            Sort::Size => shown.sort_by_key(|p| std::cmp::Reverse(p.estimated_bytes.unwrap_or(0))),
            Sort::Date => shown.sort_by_key(|p| std::cmp::Reverse(p.installed_on)),
        }
        let declared: u64 = shown.iter().filter_map(|p| p.estimated_bytes).sum();
        let recent_since = chrono::Local::now().date_naive() - chrono::Days::new(30);
        let recent = shown
            .iter()
            .filter(|p| p.installed_on.is_some_and(|d| d >= recent_since))
            .count();
        charts::grid(ui, 3, 180.0, |ui, i| match i {
            0 => charts::stat_tile(
                ui,
                tr("Programs"),
                &count(shown.len() as u64),
                &trf("{n} in total", &[("n", &count(programs.len() as u64))]),
                None,
            ),
            1 => charts::stat_tile(
                ui,
                tr("Size reported by installers"),
                &bytes(declared),
                tr("Often missing or out of date"),
                None,
            ),
            _ => charts::stat_tile(
                ui,
                tr("Installed in the last 30 days"),
                &count(recent as u64),
                "",
                None,
            ),
        });
        ui.add_space(t.gap * 0.75);
        let row_height = 52.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(ui, row_height, shown.len(), |ui, range| {
                for (index, program) in shown.iter().enumerate().skip(range.start).take(range.len())
                {
                    widgets::list_row(ui, index, row_height, |ui| {
                        shared
                            .icons
                            .show(ui, program.display_icon.as_deref(), &program.name);
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&program.name).strong());
                            let mut details = Vec::new();
                            if let Some(p) = &program.publisher {
                                details.push(p.clone());
                            }
                            if let Some(v) = &program.version {
                                details.push(v.clone());
                            }
                            if let Some(d) = program.installed_on {
                                details.push(d.format("%Y-%m-%d").to_string());
                            }
                            if let Some(manager) = program.manager {
                                details.push(manager.to_owned());
                            }
                            if program.scope == Scope::User {
                                details.push(tr("only for you").to_owned());
                            }
                            ui.label(
                                RichText::new(details.join(" · "))
                                    .small()
                                    .color(t.text_muted),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let can_uninstall = rewind_os_engine::tool("programs.uninstall")
                                .is_some_and(|t| t.available());
                            if can_uninstall
                                && ui
                                    .add_enabled(
                                        self.uninstall_job.is_none(),
                                        egui::Button::new(tr("Uninstall…")),
                                    )
                                    .clicked()
                            {
                                self.uninstall = Some((*program).clone());
                            }
                            if let Some(location) = &program.install_location
                                && ui.button(tr("Open")).clicked()
                                && let Err(err) = shared.engine.run(
                                    Request::FilesReveal {
                                        path: location.clone(),
                                    },
                                    &Task::new(),
                                )
                            {
                                shared.error(crate::widgets::error_text(&err));
                            }
                            if let Some(size) = program.estimated_bytes {
                                ui.label(bytes(size)).on_hover_text(tr(
                                    "Size the installer declared; it may be out of date.",
                                ));
                            }
                        });
                    });
                }
            });

        if let Some(program) = &self.uninstall {
            let body = if Os::current() == Os::Windows {
                trf(
                    "This starts the uninstaller of {name}. Follow its steps; afterwards, search for leftovers.",
                    &[("name", &program.name)],
                )
            } else if Os::current() == Os::MacOs {
                trf(
                    "{name} will be moved to the Trash, where you can put it back. Its settings in ~/Library stay.",
                    &[("name", &program.name)],
                )
            } else {
                trf(
                    "{name} will be removed with the package manager, which asks for your password. Its settings in your home folder stay. If removing it would take other packages along, Rewind OS stops.",
                    &[("name", &program.name)],
                )
            };
            match widgets::confirm(
                &ctx,
                "confirm_uninstall",
                tr("Uninstall this program?"),
                &body,
                tr("Start uninstaller"),
            ) {
                Choice::Open => {}
                Choice::Cancelled => self.uninstall = None,
                Choice::Confirmed => {
                    let key = program.key.clone();
                    // On Linux the package manager runs until it is done: off the window's
                    // thread.
                    self.uninstall_job = Some(Job::start(
                        &ctx,
                        &shared.engine,
                        Request::ProgramUninstall { key },
                    ));
                    self.uninstall = None;
                }
            }
        }
    }
}
