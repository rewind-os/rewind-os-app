use crate::app::Shared;
use crate::charts;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, card, page_header};
use chrono::{Duration, Utc};
use egui::RichText;
use rewind_os_engine::cleanup::Risk;
use rewind_os_engine::format::{bytes, count};
use rewind_os_engine::projects::{self, Report};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};
use std::collections::HashSet;
use std::path::PathBuf;

/// Projects left alone this long are chosen by "Select unused".
const UNUSED_DAYS: i64 = 90;

pub struct State {
    find: Option<Job>,
    clean: Option<Job>,
    root: Option<PathBuf>,
    report: Option<Report>,
    selected: HashSet<PathBuf>,
    deleted: HashSet<PathBuf>,
    confirming: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            find: None,
            clean: None,
            root: projects::default_root(),
            report: None,
            selected: HashSet::new(),
            deleted: HashSet::new(),
            confirming: false,
        }
    }
}

impl State {
    pub fn busy(&self) -> bool {
        self.find.is_some() || self.clean.is_some()
    }

    fn start(&mut self, ctx: &egui::Context, shared: &Shared, root: PathBuf) {
        let request = Request::DevFind { root: Some(root) };
        self.find = Some(Job::start(ctx, &shared.engine, request));
    }

    fn poll(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        match jobs::finished(&mut self.find, ctx) {
            Some(Ok(Response::BuildFolders(report))) => {
                shared.info(trf(
                    "Search finished: {n} build folders, {size} in total.",
                    &[
                        ("n", &count(report.folders.len() as u64)),
                        ("size", &bytes(report.total())),
                    ],
                ));
                self.selected.clear();
                self.deleted.clear();
                self.report = Some(report);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        match jobs::finished(&mut self.clean, ctx) {
            Some(Ok(Response::BuildFoldersCleaned(results))) => {
                let mut freed = 0;
                let mut problems = Vec::new();
                for r in results {
                    freed += r.bytes;
                    self.selected.remove(&r.path);
                    match r.refused {
                        Some(why) => {
                            problems.push(format!("{}: {}", r.path.display(), tr(why)));
                        }
                        None => {
                            if r.failed > 0 {
                                problems.push(format!(
                                    "{}: {}",
                                    r.path.display(),
                                    trf("{n} files in use were kept.", &[("n", &count(r.failed))])
                                ));
                            }
                            self.deleted.insert(r.path);
                        }
                    }
                }
                let text = trf("Deleted {size}.", &[("size", &bytes(freed))]);
                if problems.is_empty() {
                    shared.info(text);
                } else {
                    shared.error(format!("{text} {}", problems.join("; ")));
                }
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if let Some(root) = shared.scan_request.take() {
            self.root = Some(root.clone());
            self.start(&ctx, shared, root);
        }
        self.poll(&ctx, shared);
        page_header(
            ui,
            tr("Build folders"),
            tr(
                "Downloaded packages and build output of your software projects. Each project makes them again with its own tool when you next build it.",
            ),
        );
        self.controls(ui, shared);
        ui.add_space(t.gap);
        let cancellable = self.clean.is_none();
        for job in [&self.find, &self.clean].into_iter().flatten() {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::job_progress(ui, job, cancellable);
            });
            ui.add_space(t.gap);
        }
        let Some(report) = self.report.clone() else {
            if self.find.is_none() {
                ui.label(
                    RichText::new(tr("Choose a folder and press Search.")).color(t.text_muted),
                );
            }
            return;
        };
        self.summary(ui, &report);
        ui.add_space(8.0);
        if report.folders.is_empty() {
            ui.label(tr("No build folders found."));
            return;
        }
        self.list(ui, shared, &report);
        self.confirm(&ctx, shared, &report);
    }

    fn controls(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let busy = self.busy();
        ui.horizontal(|ui| {
            // Buttons first, from the right, so a long path is cut short instead of pushing
            // them out of the window.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.report.is_some() {
                    tr("Search again")
                } else {
                    tr("Search")
                };
                if widgets::primary_button(ui, label, !busy && self.root.is_some()).clicked()
                    && let Some(root) = self.root.clone()
                {
                    self.start(&ctx, shared, root);
                }
                if ui
                    .add_enabled(!busy, egui::Button::new(tr("Choose folder…")))
                    .clicked()
                {
                    let mut dialog = rfd::FileDialog::new();
                    if let Some(root) = &self.root {
                        dialog = dialog.set_directory(root);
                    }
                    if let Some(folder) = dialog.pick_folder() {
                        self.root = Some(folder);
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let folder = self
                        .root
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| tr("No folder chosen").to_owned());
                    ui.add(
                        egui::Label::new(
                            RichText::new(folder).strong().family(crate::theme::bold()),
                        )
                        .truncate(),
                    );
                });
            });
        });
    }

    fn summary(&mut self, ui: &mut egui::Ui, report: &Report) {
        let t = theme::current(ui.ctx());
        let busy = self.busy();
        let left: Vec<&projects::BuildFolder> = report
            .folders
            .iter()
            .filter(|f| !self.deleted.contains(&f.path))
            .collect();
        let total: u64 = left.iter().map(|f| f.bytes).sum();
        let projects_count = left
            .iter()
            .map(|f| &f.project)
            .collect::<HashSet<_>>()
            .len();
        let unused_since = Utc::now() - Duration::days(UNUSED_DAYS);
        let unused: Vec<&&projects::BuildFolder> = left
            .iter()
            .filter(|f| f.risk == Risk::Safe && f.last_used.is_some_and(|d| d < unused_since))
            .collect();
        charts::grid(ui, 3, 180.0, |ui, i| match i {
            0 => charts::stat_tile(
                ui,
                tr("Build folders"),
                &bytes(total),
                &trf("{n} folders", &[("n", &count(left.len() as u64))]),
                None,
            ),
            1 => charts::stat_tile(
                ui,
                tr("Projects"),
                &count(projects_count as u64),
                &report.root.display().to_string(),
                None,
            ),
            _ => charts::stat_tile(
                ui,
                tr("Not used for 3 months"),
                &bytes(unused.iter().map(|f| f.bytes).sum()),
                &trf("{n} folders", &[("n", &count(unused.len() as u64))]),
                None,
            ),
        });
        ui.add_space(4.0);
        let mut notes = Vec::new();
        if report.cancelled {
            notes.push(tr("The search was stopped; some folders may be missing.").to_owned());
        }
        if report.unreadable > 0 {
            notes.push(trf(
                "{n} folders could not be read.",
                &[("n", &count(report.unreadable))],
            ));
        }
        if !notes.is_empty() {
            ui.label(RichText::new(notes.join(" ")).small().color(t.warn));
        }
        ui.add_space(8.0);
        let selected_bytes: u64 = left
            .iter()
            .filter(|f| self.selected.contains(&f.path))
            .map(|f| f.bytes)
            .sum();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !busy && !unused.is_empty(),
                    egui::Button::new(tr("Select projects not used for 3 months")),
                )
                .on_hover_text(tr(
                    "Python environments are left out: packages installed by hand would be lost.",
                ))
                .clicked()
            {
                self.selected = unused.iter().map(|f| f.path.clone()).collect();
            }
            if ui
                .add_enabled(
                    !busy && !self.selected.is_empty(),
                    egui::Button::new(tr("Clear")),
                )
                .clicked()
            {
                self.selected.clear();
            }
            let label = trf("Delete {size}", &[("size", &bytes(selected_bytes))]);
            if widgets::danger_button(ui, &label, !busy && !self.selected.is_empty()).clicked() {
                self.confirming = true;
            }
        });
    }

    fn list(&mut self, ui: &mut egui::Ui, shared: &mut Shared, report: &Report) {
        let t = theme::current(ui.ctx());
        let busy = self.busy();
        let row_height = 40.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(ui, row_height, report.folders.len(), |ui, range| {
                for (index, folder) in report
                    .folders
                    .iter()
                    .enumerate()
                    .skip(range.start)
                    .take(range.len())
                {
                    let gone = self.deleted.contains(&folder.path);
                    let mut checked = self.selected.contains(&folder.path);
                    widgets::list_row(ui, index, row_height, |ui| {
                        let response = ui.add_enabled(
                            !busy && !gone,
                            egui::Checkbox::without_text(&mut checked),
                        );
                        if response.changed() {
                            if checked {
                                self.selected.insert(folder.path.clone());
                            } else {
                                self.selected.remove(&folder.path);
                            }
                        }
                        let tile = if folder.risk == Risk::Safe {
                            crate::theme::TILES[0]
                        } else {
                            crate::theme::TILES[4]
                        };
                        widgets::icon_tile(ui, crate::glyphs::Glyph::BuildFolders, tile, 32.0);
                        ui.add_sized(
                            [76.0, 20.0],
                            egui::Label::new(RichText::new(bytes(folder.bytes)).strong().family(crate::theme::bold())),
                        );
                        let color = if folder.risk == Risk::Safe {
                            t.accent
                        } else {
                            t.warn
                        };
                        ui.allocate_ui(egui::vec2(250.0, 20.0), |ui| {
                            ui.set_min_width(250.0);
                            ui.horizontal(|ui| {
                                widgets::badge(ui, tr(folder.kind.label()), color).on_hover_text(
                                    trf(
                                        "Made again with: {command}",
                                        &[("command", folder.kind.rebuild())],
                                    ),
                                );
                                if folder.risk != Risk::Safe {
                                    widgets::badge(ui, tr("Check first"), t.warn).on_hover_text(
                                        tr(
                                            "Packages installed by hand into this environment would be lost.",
                                        ),
                                    );
                                }
                            });
                        });
                        ui.add_sized(
                            [90.0, 20.0],
                            egui::Label::new(
                                RichText::new(widgets::date(folder.last_used))
                                    .small()
                                    .color(t.text_muted),
                            ),
                        )
                        .on_hover_text(tr("When the project last changed"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::row_button(ui, tr("Show"), !gone).clicked() {
                                shared.reveal(folder.project.clone());
                            }
                            // The project's place below the searched folder says more than
                            // the whole path, which is on hover.
                            let below = folder
                                .path
                                .strip_prefix(&report.root)
                                .unwrap_or(&folder.path)
                                .display()
                                .to_string();
                            let mut name = RichText::new(below);
                            if gone {
                                name = name.strikethrough().color(t.text_muted);
                            }
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.add(egui::Label::new(name).truncate())
                                    .on_hover_text(folder.path.display().to_string());
                            });
                        });
                    });
                }
            });
    }

    fn confirm(&mut self, ctx: &egui::Context, shared: &Shared, report: &Report) {
        if !self.confirming {
            return;
        }
        let chosen: Vec<&projects::BuildFolder> = report
            .folders
            .iter()
            .filter(|f| self.selected.contains(&f.path) && !self.deleted.contains(&f.path))
            .collect();
        let size: u64 = chosen.iter().map(|f| f.bytes).sum();
        let body = trf(
            "{n} build folders ({size}) will be deleted permanently, not moved to the recycle bin. Each project makes its own again with its build tool (npm install, cargo build…).",
            &[("n", &count(chosen.len() as u64)), ("size", &bytes(size))],
        );
        match widgets::confirm(
            ctx,
            "confirm_build_folders",
            tr("Delete build folders?"),
            &body,
            tr("Delete"),
        ) {
            Choice::Open => {}
            Choice::Cancelled => self.confirming = false,
            Choice::Confirmed => {
                self.confirming = false;
                let paths = chosen.iter().map(|f| f.path.clone()).collect();
                self.clean = Some(Job::start(ctx, &shared.engine, Request::DevClean { paths }));
            }
        }
    }
}
