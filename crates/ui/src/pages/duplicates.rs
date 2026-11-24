use crate::app::Shared;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, card, page_header};
use egui::RichText;
use rewind_os_engine::duplicates::{self, DuplicateReport, DuplicateSet};
use rewind_os_engine::format::{bytes, count};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};
use std::collections::HashSet;
use std::path::PathBuf;

const MB: u64 = 1024 * 1024;
/// Smallest file sizes offered, in megabytes.
const MIN_SIZES: [u64; 4] = [1, 10, 100, 1024];

pub struct State {
    find: Option<Job>,
    trash: Option<Job>,
    root: Option<PathBuf>,
    min_mb: u64,
    report: Option<DuplicateReport>,
    selected: HashSet<PathBuf>,
    moved: HashSet<PathBuf>,
    confirming: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            find: None,
            trash: None,
            root: duplicates::default_root(),
            min_mb: 1,
            report: None,
            selected: HashSet::new(),
            moved: HashSet::new(),
            confirming: false,
        }
    }
}

/// One line of the list: a set's heading, or one of its copies.
enum Row {
    Set(usize),
    Copy(usize, usize),
}

fn min_label(mb: u64) -> String {
    let size = if mb >= 1024 {
        format!("{} GB", mb / 1024)
    } else {
        format!("{mb} MB")
    };
    trf("{size} or larger", &[("size", &size)])
}

impl State {
    fn start(&mut self, ctx: &egui::Context, shared: &Shared, root: PathBuf) {
        let request = Request::Duplicates {
            root,
            min_bytes: Some(self.min_mb * MB),
        };
        self.find = Some(Job::start(ctx, &shared.engine, request));
    }

    pub fn busy(&self) -> bool {
        self.find.is_some() || self.trash.is_some()
    }

    fn poll(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        match jobs::finished(&mut self.find, ctx) {
            Some(Ok(Response::Duplicates(report))) => {
                shared.info(trf(
                    "Search finished: {n} sets of identical files, {size} can be freed.",
                    &[
                        ("n", &count(report.sets.len() as u64)),
                        ("size", &bytes(report.reclaimable())),
                    ],
                ));
                self.selected.clear();
                self.moved.clear();
                self.report = Some(report);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        match jobs::finished(&mut self.trash, ctx) {
            Some(Ok(Response::Trashed(results))) => {
                let mut moved = 0;
                let mut failed = Vec::new();
                for r in results {
                    match r.error {
                        None => {
                            moved += r.bytes;
                            self.selected.remove(&r.path);
                            self.moved.insert(r.path);
                        }
                        Some(err) => {
                            failed.push(format!("{}: {}", r.path.display(), widgets::reason(&err)))
                        }
                    }
                }
                let text = trf(
                    "Moved {size} to the recycle bin. Space is freed when the recycle bin is emptied.",
                    &[("size", &bytes(moved))],
                );
                if failed.is_empty() {
                    shared.info(text);
                } else {
                    shared.error(format!("{text} {}", failed.join("; ")));
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
            tr("Duplicate files"),
            tr(
                "Files with exactly the same contents. Keep one copy and move the others to the recycle bin.",
            ),
        );
        self.controls(ui, shared);
        ui.add_space(t.gap);

        let cancellable = self.trash.is_none();
        for job in [&self.find, &self.trash].into_iter().flatten() {
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
        if report.sets.is_empty() {
            ui.label(tr("No duplicate files found."));
            return;
        }
        self.list(ui, shared, &report);
        self.confirm_trash(&ctx, shared, &report);
    }

    fn controls(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let busy = self.busy();
        ui.horizontal(|ui| {
            let folder = self
                .root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| tr("No folder chosen").to_owned());
            ui.add(egui::Label::new(RichText::new(folder).strong()).truncate());
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
            egui::ComboBox::from_id_salt("duplicates_min")
                .selected_text(min_label(self.min_mb))
                .show_ui(ui, |ui| {
                    for mb in MIN_SIZES {
                        ui.selectable_value(&mut self.min_mb, mb, min_label(mb));
                    }
                });
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
        });
    }

    /// The sets without the copies already moved to the recycle bin.
    fn remaining(&self, report: &DuplicateReport) -> Vec<DuplicateSet> {
        report
            .sets
            .iter()
            .map(|s| {
                let mut s = s.clone();
                s.copies.retain(|c| !self.moved.contains(&c.path));
                s
            })
            .collect()
    }

    fn summary(&mut self, ui: &mut egui::Ui, report: &DuplicateReport) {
        let t = theme::current(ui.ctx());
        let busy = self.busy();
        let remaining = self.remaining(report);
        let free: u64 = remaining.iter().map(|s| s.reclaimable()).sum();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(bytes(free)).strong().size(18.0));
            ui.label(
                RichText::new(trf(
                    "can be freed by keeping one copy of each of {n} files",
                    &[("n", &count(report.sets.len() as u64))],
                ))
                .color(t.text_muted),
            );
        });
        let mut notes = Vec::new();
        if report.cancelled {
            notes.push(tr("The search was stopped; some duplicates may be missing.").to_owned());
        }
        if report.unreadable > 0 {
            notes.push(trf(
                "{n} files or folders could not be read.",
                &[("n", &count(report.unreadable))],
            ));
        }
        if report.cloud_skipped > 0 {
            notes.push(trf(
                "{n} files that are only in the cloud were not compared.",
                &[("n", &count(report.cloud_skipped))],
            ));
        }
        if !notes.is_empty() {
            ui.label(RichText::new(notes.join(" ")).small().color(t.warn));
        }
        ui.add_space(8.0);
        let selected_bytes: u64 = report
            .sets
            .iter()
            .flat_map(|s| {
                s.copies
                    .iter()
                    .filter(|c| self.selected.contains(&c.path))
                    .map(|_| s.bytes)
            })
            .sum();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new(tr("Select all but the oldest")))
                .on_hover_text(tr(
                    "Chooses every copy except the oldest of each file, which is usually the original.",
                ))
                .clicked()
            {
                let keep = DuplicateReport {
                    sets: remaining,
                    ..report.clone()
                };
                self.selected = keep
                    .all_but_oldest()
                    .into_iter()
                    .filter(|p| rewind_os_engine::safety::removal_refusal(p).is_none())
                    .collect();
            }
            if ui
                .add_enabled(!busy && !self.selected.is_empty(), egui::Button::new(tr("Clear")))
                .clicked()
            {
                self.selected.clear();
            }
            let label = trf(
                "Move {size} to recycle bin",
                &[("size", &bytes(selected_bytes))],
            );
            let safe = duplicates::keeps_a_copy(&self.remaining(report), &self.selected);
            if widgets::danger_button(ui, &label, !busy && safe && !self.selected.is_empty())
                .clicked()
            {
                self.confirming = true;
            }
        });
    }

    fn list(&mut self, ui: &mut egui::Ui, shared: &mut Shared, report: &DuplicateReport) {
        let t = theme::current(ui.ctx());
        let busy = self.busy();
        let rows: Vec<Row> = report
            .sets
            .iter()
            .enumerate()
            .flat_map(|(si, s)| {
                std::iter::once(Row::Set(si))
                    .chain((0..s.copies.len()).map(move |ci| Row::Copy(si, ci)))
            })
            .collect();
        let row_height = 36.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(ui, row_height, rows.len(), |ui, range| {
                for (index, row) in rows.iter().enumerate().skip(range.start).take(range.len()) {
                    match *row {
                        Row::Set(si) => {
                            let Some(set) = report.sets.get(si) else {
                                continue;
                            };
                            ui.allocate_ui(egui::vec2(ui.available_width(), row_height), |ui| {
                                ui.horizontal_centered(|ui| {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(trf(
                                            "{n} copies of {size}",
                                            &[
                                                ("n", &count(set.copies.len() as u64)),
                                                ("size", &bytes(set.bytes)),
                                            ],
                                        ))
                                        .strong(),
                                    );
                                    ui.label(
                                        RichText::new(trf(
                                            "frees {size}",
                                            &[("size", &bytes(set.reclaimable()))],
                                        ))
                                        .small()
                                        .color(t.text_muted),
                                    );
                                });
                            });
                        }
                        Row::Copy(si, ci) => {
                            let Some(set) = report.sets.get(si) else {
                                continue;
                            };
                            let Some(copy) = set.copies.get(ci) else {
                                continue;
                            };
                            let gone = self.moved.contains(&copy.path);
                            let mut checked = self.selected.contains(&copy.path);
                            // Choosing this one too would leave no copy of the file.
                            let last = !checked
                                && set.copies.iter().filter(|c| c.path != copy.path).all(|c| {
                                    self.selected.contains(&c.path) || self.moved.contains(&c.path)
                                });
                            let refusal = rewind_os_engine::safety::removal_refusal(&copy.path);
                            widgets::list_row(ui, index, row_height, |ui| {
                                let response = ui.add_enabled(
                                    !busy && !gone && !last && refusal.is_none(),
                                    egui::Checkbox::without_text(&mut checked),
                                );
                                let response = match (refusal, last) {
                                    (Some(why), _) => response.on_disabled_hover_text(tr(why)),
                                    (None, true) if !gone => response
                                        .on_disabled_hover_text(tr("Keep at least one copy.")),
                                    _ => response,
                                };
                                if response.changed() {
                                    if checked {
                                        self.selected.insert(copy.path.clone());
                                    } else {
                                        self.selected.remove(&copy.path);
                                    }
                                }
                                ui.add_sized(
                                    [90.0, 20.0],
                                    egui::Label::new(
                                        RichText::new(widgets::date(copy.modified))
                                            .small()
                                            .color(t.text_muted),
                                    ),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let show = widgets::row_button(ui, tr("Show"), true)
                                            .on_hover_text(tr("Show in File Explorer"));
                                        if show.clicked()
                                            && let Err(err) = shared.engine.run(
                                                Request::FilesReveal {
                                                    path: copy.path.clone(),
                                                },
                                                &rewind_os_engine::Task::new(),
                                            )
                                        {
                                            shared.error(widgets::error_text(&err));
                                        }
                                        let mut name =
                                            RichText::new(copy.path.display().to_string());
                                        if gone {
                                            name = name.strikethrough().color(t.text_muted);
                                        }
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.add(egui::Label::new(name).truncate());
                                            },
                                        );
                                    },
                                );
                            });
                        }
                    }
                }
            });
    }

    fn confirm_trash(&mut self, ctx: &egui::Context, shared: &Shared, report: &DuplicateReport) {
        if !self.confirming {
            return;
        }
        let paths: Vec<PathBuf> = report
            .sets
            .iter()
            .flat_map(|s| s.copies.iter())
            .filter(|c| self.selected.contains(&c.path))
            .map(|c| c.path.clone())
            .collect();
        let size: u64 = report
            .sets
            .iter()
            .map(|s| {
                s.bytes
                    * s.copies
                        .iter()
                        .filter(|c| self.selected.contains(&c.path))
                        .count() as u64
            })
            .sum();
        let body = trf(
            "{n} copies ({size}) will be moved to the recycle bin. One copy of each file stays where it is.",
            &[("n", &paths.len().to_string()), ("size", &bytes(size))],
        );
        match widgets::confirm(
            ctx,
            "confirm_duplicates",
            tr("Move to the recycle bin?"),
            &body,
            tr("Move to recycle bin"),
        ) {
            Choice::Open => {}
            Choice::Cancelled => self.confirming = false,
            Choice::Confirmed => {
                self.confirming = false;
                // Checked again here: the dialog must never remove the last copy.
                if duplicates::keeps_a_copy(&self.remaining(report), &self.selected) {
                    self.trash = Some(Job::start(
                        ctx,
                        &shared.engine,
                        Request::FilesTrash { paths },
                    ));
                }
            }
        }
    }
}
