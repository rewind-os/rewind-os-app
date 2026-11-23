use crate::app::{Page, Shared};
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, Choice, card, page_header};
use egui::RichText;
use rewindows_engine::format::{bytes, count};
use rewindows_engine::leftovers::{Confidence, Leftover};
use rewindows_engine::{Request, Response};
use rewindows_i18n::{tr, trf};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Default)]
pub struct State {
    find: Option<Job>,
    trash: Option<Job>,
    found: Option<Vec<Leftover>>,
    selected: HashSet<PathBuf>,
    moved: HashSet<PathBuf>,
    min_confidence: Option<Confidence>,
    confirming: bool,
}

fn confidence_label(c: Confidence) -> &'static str {
    match c {
        Confidence::High => "Likely unused",
        Confidence::Medium => "Possibly unused",
        Confidence::Low => "Recently used",
    }
}

impl State {
    pub fn busy(&self) -> bool {
        self.find.is_some() || self.trash.is_some()
    }

    fn poll(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        match jobs::finished(&mut self.find, ctx) {
            Some(Ok(Response::Leftovers(found))) => {
                self.selected.clear();
                self.moved.clear();
                self.found = Some(found);
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
                        Some(err) => failed.push(format!("{}: {err}", r.path.display())),
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
        self.poll(&ctx, shared);
        page_header(
            ui,
            tr("Program leftovers"),
            tr(
                "Folders in Program Files, ProgramData and AppData that no installed program, service, running program or startup entry refers to.",
            ),
        );
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(tr(
                    "This is an estimate. Portable programs and games installed outside Windows' list can show up here. Check each folder before moving it; everything goes to the recycle bin so you can restore it.",
                ))
                .color(t.warn),
            );
        });
        ui.add_space(t.gap);

        let busy = self.find.is_some() || self.trash.is_some();
        let visible: Vec<Leftover> = self
            .found
            .iter()
            .flatten()
            .filter(|l| self.min_confidence.is_none_or(|m| l.confidence >= m))
            .cloned()
            .collect();
        let selected_bytes: u64 = visible
            .iter()
            .filter(|l| self.selected.contains(&l.path))
            .map(|l| l.bytes)
            .sum();
        ui.horizontal(|ui| {
            let label = if self.found.is_some() {
                tr("Search again")
            } else {
                tr("Search")
            };
            if widgets::primary_button(ui, label, !busy).clicked() {
                self.find = Some(Job::start(&ctx, &shared.engine, Request::LeftoversFind));
            }
            egui::ComboBox::from_id_salt("leftover_filter")
                .selected_text(match self.min_confidence {
                    None => tr("Show all"),
                    Some(c) => tr(confidence_label(c)),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.min_confidence, None, tr("Show all"));
                    ui.selectable_value(
                        &mut self.min_confidence,
                        Some(Confidence::Medium),
                        tr("Possibly unused"),
                    );
                    ui.selectable_value(
                        &mut self.min_confidence,
                        Some(Confidence::High),
                        tr("Likely unused"),
                    );
                });
            let label = trf(
                "Move {size} to recycle bin",
                &[("size", &bytes(selected_bytes))],
            );
            if widgets::danger_button(ui, &label, !busy && !self.selected.is_empty()).clicked() {
                self.confirming = true;
            }
            if ui.button(tr("Installed programs")).clicked() {
                shared.goto = Some(Page::Programs);
            }
        });
        ui.add_space(t.gap);
        for job in [&self.find, &self.trash].into_iter().flatten() {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::job_progress(ui, job, false);
            });
            ui.add_space(t.gap);
        }
        let Some(found) = &self.found else { return };
        if found.is_empty() {
            ui.label(tr("No leftover folders found."));
            return;
        }
        let total: u64 = visible.iter().map(|l| l.bytes).sum();
        ui.label(
            RichText::new(trf(
                "{n} folders, {size} in total",
                &[("n", &visible.len().to_string()), ("size", &bytes(total))],
            ))
            .color(t.text_muted),
        );
        let row_height = 44.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_rows(ui, row_height, visible.len(), |ui, range| {
                for item in &visible[range] {
                    let gone = self.moved.contains(&item.path);
                    ui.horizontal(|ui| {
                        ui.set_height(row_height);
                        let mut checked = self.selected.contains(&item.path);
                        if ui
                            .add_enabled(!gone, egui::Checkbox::without_text(&mut checked))
                            .changed()
                        {
                            if checked {
                                self.selected.insert(item.path.clone());
                            } else {
                                self.selected.remove(&item.path);
                            }
                        }
                        ui.add_sized(
                            [80.0, 20.0],
                            egui::Label::new(RichText::new(bytes(item.bytes)).strong()),
                        );
                        let color = match item.confidence {
                            Confidence::High => t.ok,
                            Confidence::Medium => t.warn,
                            Confidence::Low => t.text_muted,
                        };
                        ui.allocate_ui(egui::vec2(130.0, 20.0), |ui| {
                            widgets::badge(ui, tr(confidence_label(item.confidence)), color);
                        });
                        ui.vertical(|ui| {
                            let mut name = RichText::new(item.path.display().to_string());
                            if gone {
                                name = name.strikethrough().color(t.text_muted);
                            }
                            ui.add(egui::Label::new(name).truncate());
                            ui.label(
                                RichText::new(trf(
                                    "{location} · {n} files · last change {date}",
                                    &[
                                        ("location", tr(item.location.label())),
                                        ("n", &count(item.files)),
                                        ("date", &widgets::date(item.last_change)),
                                    ],
                                ))
                                .small()
                                .color(t.text_muted),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(tr("Open")).clicked()
                                && let Err(err) = shared.engine.run(
                                    Request::FilesReveal {
                                        path: item.path.clone(),
                                    },
                                    &rewindows_engine::Task::new(),
                                )
                            {
                                shared.error(err.to_string());
                            }
                        });
                    });
                }
            });

        if self.confirming {
            let paths: Vec<PathBuf> = visible
                .iter()
                .filter(|l| self.selected.contains(&l.path))
                .map(|l| l.path.clone())
                .collect();
            let body = trf(
                "{n} items ({size}) will be moved to the recycle bin. You can restore them from there.",
                &[
                    ("n", &paths.len().to_string()),
                    ("size", &bytes(selected_bytes)),
                ],
            );
            match widgets::confirm(
                &ctx,
                "confirm_leftovers",
                tr("Move to the recycle bin?"),
                &body,
                tr("Move to recycle bin"),
            ) {
                Choice::Open => {}
                Choice::Cancelled => self.confirming = false,
                Choice::Confirmed => {
                    self.confirming = false;
                    self.trash = Some(Job::start(
                        &ctx,
                        &shared.engine,
                        Request::FilesTrash { paths },
                    ));
                }
            }
        }
    }
}
