use crate::app::{Page, Shared};
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, page_header};
use egui::RichText;
use rewind_os_engine::format::bytes;
use rewind_os_engine::programs::{InstalledProgram, Scope};
use rewind_os_engine::{Request, Response, Task};
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
    programs: Option<Vec<InstalledProgram>>,
    search: String,
    sort: Sort,
    uninstall: Option<InstalledProgram>,
    started: bool,
}

impl State {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if !self.started {
            self.started = true;
            self.job = Some(Job::start(&ctx, &shared.engine, Request::ProgramsList));
        }
        match jobs::finished(&mut self.job, &ctx) {
            Some(Ok(Response::Programs(list))) => self.programs = Some(list),
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        page_header(
            ui,
            tr("Installed programs"),
            tr("Uninstall what you no longer use, then look for its leftovers."),
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
            if ui.button(tr("Find program leftovers")).clicked() {
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
        ui.label(
            RichText::new(trf("{n} programs", &[("n", &shown.len().to_string())]))
                .color(t.text_muted),
        );
        let row_height = 42.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(ui, row_height, shown.len(), |ui, range| {
                for program in &shown[range] {
                    ui.horizontal(|ui| {
                        ui.set_height(row_height);
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
                            if ui.button(tr("Uninstall…")).clicked() {
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
                                shared.error(err.to_string());
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
            let body = trf(
                "This starts the uninstaller of {name}. Follow its steps; afterwards, search for leftovers.",
                &[("name", &program.name)],
            );
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
                    match shared
                        .engine
                        .run(Request::ProgramUninstall { key }, &Task::new())
                    {
                        Ok(Response::Done(message)) => shared.info(tr(message)),
                        Ok(_) => {}
                        Err(err) => shared.error(err.to_string()),
                    }
                    self.uninstall = None;
                }
            }
        }
    }
}
