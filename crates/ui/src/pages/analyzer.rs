use crate::app::Shared;
use crate::charts;
use crate::jobs::{self, Job};
use crate::theme;
use crate::treemap;
use crate::widgets::{self, Choice, card, page_header};
use egui::{Color32, CornerRadius, RichText, Role, Sense, Stroke, WidgetInfo};
use rewind_os_engine::format::{bytes, count, local_date, signed_bytes};
use rewind_os_engine::history::Comparison;
use rewind_os_engine::scan::{self, FileEntry, NodeId, ScanTree};
use rewind_os_engine::{Request, Response};
use rewind_os_i18n::{tr, trf};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum View {
    #[default]
    Folders,
    LargestFiles,
    OldFiles,
    FileTypes,
    Changes,
}

/// Most tiles drawn in the treemap; the rest are too small to see.
const TREEMAP_TILES: usize = 80;

#[derive(Default)]
pub struct State {
    root: Option<PathBuf>,
    scan: Option<Job>,
    tree: Option<Arc<ScanTree>>,
    current: NodeId,
    view: View,
    files_job: Option<Job>,
    files: Option<(NodeId, Vec<FileEntry>)>,
    show_files: bool,
    /// Paths moved to the recycle bin since the last scan; shown struck through.
    trashed: HashSet<PathBuf>,
    pending_trash: Option<Vec<(PathBuf, u64)>>,
    trash_job: Option<Job>,
}

impl State {
    pub fn busy(&self) -> bool {
        self.scan.is_some() || self.files_job.is_some() || self.trash_job.is_some()
    }

    fn start_scan(&mut self, ctx: &egui::Context, shared: &Shared, root: PathBuf) {
        self.root = Some(root.clone());
        self.tree = None;
        self.files = None;
        self.trashed.clear();
        self.current = ScanTree::ROOT;
        self.scan = Some(Job::start(ctx, &shared.engine, Request::DiskScan { root }));
    }

    fn poll(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        match jobs::finished(&mut self.scan, ctx) {
            Some(Ok(Response::Scan(tree))) => {
                if let Some(root) = tree.root_node() {
                    shared.info(trf(
                        "Scan finished: {size} in {files} files.",
                        &[("size", &bytes(root.bytes)), ("files", &count(root.files))],
                    ));
                }
                self.tree = Some(tree);
                self.current = ScanTree::ROOT;
                match crate::devtools::analyzer_view().as_deref() {
                    Some("folders") => self.view = View::Folders,
                    Some("files") => self.view = View::LargestFiles,
                    Some("old") => self.view = View::OldFiles,
                    Some("types") => self.view = View::FileTypes,
                    Some("changes") => self.view = View::Changes,
                    _ => {}
                }
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        if let Some(outcome) = jobs::finished(&mut self.files_job, ctx) {
            match outcome {
                Ok(Response::Files(files)) => self.files = Some((self.current, files)),
                Ok(_) => {}
                Err(err) => shared.error(err),
            }
        }
        if let Some(outcome) = jobs::finished(&mut self.trash_job, ctx) {
            match outcome {
                Ok(Response::Trashed(results)) => {
                    let mut moved = 0;
                    let mut failed = 0;
                    for r in results {
                        if r.error.is_none() {
                            moved += r.bytes;
                            self.trashed.insert(r.path);
                        } else {
                            failed += 1;
                        }
                    }
                    let mut text = trf(
                        "Moved {size} to the recycle bin. Space is freed when the recycle bin is emptied.",
                        &[("size", &bytes(moved))],
                    );
                    if failed > 0 {
                        text.push(' ');
                        text.push_str(&trf(
                            "{n} items could not be moved.",
                            &[("n", &failed.to_string())],
                        ));
                        shared.error(text);
                    } else {
                        shared.info(text);
                    }
                }
                Ok(_) => {}
                Err(err) => shared.error(err),
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if let Some(root) = shared.scan_request.take() {
            self.start_scan(&ctx, shared, root);
        }
        self.poll(&ctx, shared);
        page_header(
            ui,
            tr("Disk analyzer"),
            tr("Find out what takes up space, folder by folder."),
        );
        self.controls(ui, shared);
        ui.add_space(t.gap);

        if let Some(job) = &self.scan {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::job_progress(ui, job, true);
            });
            return;
        }
        let Some(tree) = self.tree.clone() else {
            ui.label(
                RichText::new(tr("Choose a drive or folder and press Scan.")).color(t.text_muted),
            );
            return;
        };
        self.summary(ui, &tree);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.view, View::Folders, tr("Folders"));
            ui.selectable_value(&mut self.view, View::LargestFiles, tr("Largest files"));
            ui.selectable_value(&mut self.view, View::OldFiles, tr("Old large files"));
            ui.selectable_value(&mut self.view, View::FileTypes, tr("File types"));
            ui.selectable_value(&mut self.view, View::Changes, tr("What changed"));
        });
        ui.separator();
        match self.view {
            View::Folders => self.folders(ui, shared, &tree),
            View::LargestFiles => self.largest_files(ui, shared, &tree),
            View::OldFiles => self.old_files(ui, shared, &tree),
            View::FileTypes => file_types(ui, &tree),
            View::Changes => self.changes(ui, &tree),
        }
        self.confirm_trash(&ctx, shared);
    }

    fn controls(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let drives: Vec<PathBuf> = shared
            .overview
            .as_ref()
            .map(|o| o.drives.iter().map(|d| d.mount.clone()).collect())
            .unwrap_or_default();
        if self.root.is_none() {
            self.root = drives.first().cloned();
        }
        ui.horizontal(|ui| {
            let selected = self
                .root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("scan_root")
                .selected_text(selected)
                .width(220.0)
                .show_ui(ui, |ui| {
                    for drive in &drives {
                        ui.selectable_value(
                            &mut self.root,
                            Some(drive.clone()),
                            drive.display().to_string(),
                        );
                    }
                });
            let running = self.scan.is_some();
            if ui
                .add_enabled(!running, egui::Button::new(tr("Choose folder…")))
                .clicked()
                && let Some(folder) = rfd::FileDialog::new().pick_folder()
            {
                self.root = Some(folder);
            }
            let can_scan = !running && self.root.is_some();
            if widgets::primary_button(ui, tr("Scan"), can_scan).clicked()
                && let Some(root) = self.root.clone()
            {
                self.start_scan(&ctx, shared, root);
            }
        });
    }

    fn summary(&self, ui: &mut egui::Ui, tree: &ScanTree) {
        let t = theme::current(ui.ctx());
        let Some(root) = tree.root_node() else { return };
        let types = tree.extensions.len() as u64;
        charts::grid(ui, 4, 160.0, |ui, i| match i {
            0 => charts::stat_tile(
                ui,
                tr("Total size"),
                &bytes(root.bytes),
                &trf("{size} on disk", &[("size", &bytes(root.disk_bytes))]),
                None,
            ),
            1 => charts::stat_tile(ui, tr("Files"), &count(root.files), "", None),
            2 => charts::stat_tile(
                ui,
                tr("Folders"),
                &count(tree.dir_count() as u64),
                &trf("{n} file types", &[("n", &count(types))]),
                None,
            ),
            _ => charts::stat_tile(
                ui,
                tr("Scan time"),
                &format!("{:.1} s", tree.elapsed_ms as f64 / 1000.0),
                if tree.from_file_table {
                    tr("Read from the master file table")
                } else {
                    ""
                },
                None,
            ),
        });
        ui.add_space(4.0);
        if tree.cancelled {
            ui.label(
                RichText::new(tr("The scan was stopped; sizes are incomplete.")).color(t.warn),
            );
        }
        if tree.unreadable_dirs > 0 {
            ui.label(
                RichText::new(trf(
                    "{n} folders could not be read (administrator rights may help).",
                    &[("n", &count(tree.unreadable_dirs))],
                ))
                .color(t.warn),
            );
        }
        if tree.hard_linked_bytes > 0 {
            ui.label(
                RichText::new(trf(
                    "{size} of hard links (one file under several names) was counted once.",
                    &[("size", &bytes(tree.hard_linked_bytes))],
                ))
                .small()
                .color(t.text_muted),
            );
        }
        if let Some(Comparison::Changes {
            since,
            before,
            after,
            ..
        }) = &tree.history
        {
            ui.label(
                RichText::new(trf(
                    "{change} since the scan of {date} (see What changed).",
                    &[
                        (
                            "change",
                            &signed_bytes(i128::from(*after) - i128::from(*before)),
                        ),
                        ("date", &local_date(*since)),
                    ],
                ))
                .small()
                .color(t.text_muted),
            );
        }
    }

    fn breadcrumb(&mut self, ui: &mut egui::Ui, tree: &ScanTree) {
        ui.horizontal_wrapped(|ui| {
            for (i, id) in tree.ancestry(self.current).into_iter().enumerate() {
                if i > 0 {
                    ui.label("›");
                }
                let name = tree
                    .node(id)
                    .map(|n| n.name.to_string())
                    .unwrap_or_default();
                if ui.selectable_label(id == self.current, name).clicked() {
                    self.current = id;
                    self.show_files = false;
                }
            }
        });
    }

    fn folders(&mut self, ui: &mut egui::Ui, shared: &mut Shared, tree: &Arc<ScanTree>) {
        self.breadcrumb(ui, tree);
        let wide = ui.available_width() > 860.0;
        let navigate = if wide {
            let column = egui::Layout::top_down(egui::Align::Min);
            let gap = 16.0;
            let list_width = (ui.available_width() - gap) * 0.58;
            let map_width = ui.available_width() - gap - list_width;
            let height = (ui.available_height() - 8.0).max(200.0);
            ui.horizontal_top(|ui| {
                let from_list = ui
                    .allocate_ui_with_layout(egui::vec2(list_width, height), column, |ui| {
                        self.folder_list(ui, shared, tree)
                    })
                    .inner;
                ui.add_space(gap);
                let from_map = ui
                    .allocate_ui_with_layout(egui::vec2(map_width, height), column, |ui| {
                        treemap_view(ui, tree, self.current, &self.trashed, height - 8.0)
                    })
                    .inner;
                from_list.or(from_map)
            })
            .inner
        } else {
            let from_map = treemap_view(ui, tree, self.current, &self.trashed, 220.0);
            let from_list = self.folder_list(ui, shared, tree);
            from_list.or(from_map)
        };
        if self.show_files {
            self.own_files(ui, shared);
        }
        if let Some(id) = navigate {
            self.current = id;
            self.show_files = false;
        }
    }

    /// The subfolders of the current folder, largest first, plus one row for its own files.
    fn folder_list(
        &mut self,
        ui: &mut egui::Ui,
        shared: &mut Shared,
        tree: &Arc<ScanTree>,
    ) -> Option<NodeId> {
        let t = theme::current(ui.ctx());
        let node = tree.node(self.current)?.clone();
        let parent_bytes = node.bytes.max(1);
        let rows: Vec<(NodeId, String, u64, u64, bool, u64)> = tree
            .children(self.current)
            .map(|(id, n)| {
                (
                    id,
                    n.name.to_string(),
                    n.bytes,
                    n.files,
                    n.incomplete,
                    n.disk_bytes,
                )
            })
            .collect();
        let mut navigate = None;
        let row_height = 38.0;
        egui::ScrollArea::vertical()
            .id_salt("folder_rows")
            .max_height((ui.available_height() - 40.0).max(160.0))
            .auto_shrink([false, true])
            .show_rows(ui, row_height, rows.len() + 1, |ui, range| {
                for index in range {
                    let Some((id, name, size, files, incomplete, disk)) = rows.get(index) else {
                        // The files that sit directly in this folder, as one row.
                        widgets::list_row(ui, index, row_height, |ui| {
                            let label =
                                trf("{n} files in this folder", &[("n", &count(node.own_files))]);
                            if ui.selectable_label(self.show_files, label).clicked() {
                                self.show_files = !self.show_files;
                                if self.show_files {
                                    self.files = None;
                                    self.files_job = Some(Job::start(
                                        ui.ctx(),
                                        &shared.engine,
                                        Request::ListFiles {
                                            dir: tree.path(self.current),
                                        },
                                    ));
                                }
                            }
                            ui.label(RichText::new(bytes(node.own_bytes)).color(t.text_muted));
                        });
                        continue;
                    };
                    let path = tree.path(*id);
                    let gone = self.trashed.contains(&path);
                    widgets::list_row(ui, index, row_height, |ui| {
                        let fraction = *size as f32 / parent_bytes as f32;
                        ui.allocate_ui(egui::vec2(64.0, row_height), |ui| {
                            widgets::usage_bar(
                                ui,
                                fraction,
                                t.accent,
                                &format!("{name} {:.0}%", fraction * 100.0),
                            );
                        });
                        size_label(ui, row_height, *size, *disk);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (show, trash) = widgets::file_actions(ui, &path, gone);
                            if show {
                                reveal(shared, path.clone());
                            }
                            if trash {
                                self.pending_trash = Some(vec![(path.clone(), *size)]);
                            }
                            if *incomplete {
                                widgets::badge(ui, tr("partial"), t.warn)
                                    .on_hover_text(tr("Some folders inside could not be read."));
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let mut text = RichText::new(format!("{name}  ›"));
                                    if gone {
                                        text = text.strikethrough().color(t.text_muted);
                                    }
                                    let response = ui.add_enabled(
                                        !gone,
                                        egui::Button::new(text).frame(false).truncate(),
                                    );
                                    if response.clicked() {
                                        navigate = Some(*id);
                                    }
                                    response.on_hover_text(trf(
                                        "Open · {n} files",
                                        &[("n", &count(*files))],
                                    ));
                                },
                            );
                        });
                    });
                }
            });
        navigate
    }

    fn own_files(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        egui::Window::new(tr("Files in this folder"))
            .id(egui::Id::unique("own_files"))
            .collapsible(false)
            .default_size([640.0, 420.0])
            .open(&mut self.show_files)
            .show(ui.ctx(), |ui| match &self.files {
                Some((id, files)) if *id == self.current => {
                    let files = files.clone();
                    file_rows(ui, shared, &mut self.pending_trash, &self.trashed, &files);
                }
                _ => {
                    ui.spinner();
                }
            });
    }

    fn largest_files(&mut self, ui: &mut egui::Ui, shared: &mut Shared, tree: &ScanTree) {
        let t = theme::current(ui.ctx());
        ui.label(
            RichText::new(trf(
                "The {n} largest files found in the scan.",
                &[("n", &tree.top_files.len().to_string())],
            ))
            .color(t.text_muted),
        );
        file_rows(
            ui,
            shared,
            &mut self.pending_trash,
            &self.trashed,
            &tree.top_files,
        );
    }

    /// Large files nobody changed in a year: often forgotten videos, backups and disk images.
    fn old_files(&mut self, ui: &mut egui::Ui, shared: &mut Shared, tree: &ScanTree) {
        let t = theme::current(ui.ctx());
        let total: u64 = tree.old_files.iter().map(|f| f.bytes).sum();
        ui.label(
            RichText::new(trf(
                "Files of {size} or more that nobody changed in over a year: {n}, {total} in all. The system's and programs' own files are left out. Check each before moving it to the recycle bin.",
                &[
                    ("size", &bytes(scan::OLD_MIN_BYTES)),
                    ("n", &tree.old_files.len().to_string()),
                    ("total", &bytes(total)),
                ],
            ))
            .color(t.text_muted),
        );
        if tree.old_files.is_empty() {
            return;
        }
        file_rows(
            ui,
            shared,
            &mut self.pending_trash,
            &self.trashed,
            &tree.old_files,
        );
    }

    /// The folders that grew and shrank most since the last scan of the same folder.
    fn changes(&mut self, ui: &mut egui::Ui, tree: &ScanTree) {
        let t = theme::current(ui.ctx());
        let muted = |ui: &mut egui::Ui, text: &str| {
            ui.label(RichText::new(text).color(t.text_muted));
        };
        let Some(comparison) = &tree.history else {
            muted(ui, tr("Stopped scans are neither compared nor kept."));
            return;
        };
        let (since, before, after, min_bytes, grew, shrank) = match comparison {
            Comparison::Nothing { reason } => {
                muted(ui, tr(reason));
                return;
            }
            Comparison::Changes {
                since,
                before,
                after,
                min_bytes,
                grew,
                shrank,
            } => (since, before, after, *min_bytes, grew, shrank),
        };
        let total = i128::from(*after) - i128::from(*before);
        muted(
            ui,
            &trf(
                "Since the scan of {date}: {before} then, {after} now.",
                &[
                    ("date", &local_date(*since)),
                    ("before", &bytes(*before)),
                    ("after", &bytes(*after)),
                ],
            ),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(signed_bytes(total))
                .heading()
                .strong()
                .family(crate::theme::bold()),
        );
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("changes")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (title, list, empty) in [
                    (tr("Grew the most"), grew, tr("No folder grew noticeably.")),
                    (
                        tr("Shrank the most"),
                        shrank,
                        tr("No folder shrank noticeably."),
                    ),
                ] {
                    ui.add_space(t.gap);
                    charts::section_title(ui, title);
                    if list.is_empty() {
                        muted(ui, empty);
                    }
                    for (index, change) in list.iter().enumerate() {
                        let found = tree.find(&change.names);
                        widgets::list_row(ui, index, 38.0, |ui| {
                            ui.add_sized(
                                [96.0, 38.0],
                                egui::Label::new(
                                    RichText::new(signed_bytes(change.delta()))
                                        .strong()
                                        .family(crate::theme::bold()),
                                ),
                            );
                            ui.add_sized(
                                [130.0, 38.0],
                                egui::Label::new(
                                    RichText::new(if change.before == 0 {
                                        trf("was under {size}", &[("size", &bytes(min_bytes))])
                                    } else {
                                        trf("was {size}", &[("size", &bytes(change.before))])
                                    })
                                    .color(t.text_muted),
                                ),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let button =
                                        widgets::row_button(ui, tr("Show"), found.is_some());
                                    if button.clicked() {
                                        open = found;
                                    }
                                    if found.is_none() {
                                        button.on_disabled_hover_text(tr("It is no longer there."));
                                    }
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            let below =
                                                change.names.join(std::path::MAIN_SEPARATOR_STR);
                                            ui.add(egui::Label::new(below).truncate())
                                                .on_hover_text(change.path.display().to_string());
                                        },
                                    );
                                },
                            );
                        });
                    }
                }
            });
        if let Some(id) = open {
            self.current = id;
            self.view = View::Folders;
            self.show_files = false;
        }
    }

    fn confirm_trash(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        let Some(items) = &self.pending_trash else {
            return;
        };
        let total: u64 = items.iter().map(|(_, b)| b).sum();
        let body = if items.len() == 1 {
            trf(
                "{path} ({size}) will be moved to the recycle bin. You can restore it from there.",
                &[
                    ("path", &items[0].0.display().to_string()),
                    ("size", &bytes(total)),
                ],
            )
        } else {
            trf(
                "{n} items ({size}) will be moved to the recycle bin. You can restore them from there.",
                &[("n", &items.len().to_string()), ("size", &bytes(total))],
            )
        };
        match widgets::confirm(
            ctx,
            "confirm_trash_analyzer",
            tr("Move to the recycle bin?"),
            &body,
            tr("Move to recycle bin"),
        ) {
            Choice::Open => {}
            Choice::Cancelled => self.pending_trash = None,
            Choice::Confirmed => {
                let paths = items.iter().map(|(p, _)| p.clone()).collect();
                self.trash_job = Some(Job::start(
                    ctx,
                    &shared.engine,
                    Request::FilesTrash { paths },
                ));
                self.pending_trash = None;
            }
        }
    }
}

/// A size in a list, with the space it takes on the disk on hover.
fn size_label(ui: &mut egui::Ui, row_height: f32, size: u64, disk: u64) {
    ui.add_sized(
        [76.0, row_height],
        egui::Label::new(
            RichText::new(bytes(size))
                .strong()
                .family(crate::theme::bold()),
        ),
    )
    .on_hover_text(trf("{size} on disk", &[("size", &bytes(disk))]));
}

fn reveal(shared: &mut Shared, path: PathBuf) {
    shared.reveal(path);
}

fn file_rows(
    ui: &mut egui::Ui,
    shared: &mut Shared,
    pending: &mut Option<Vec<(PathBuf, u64)>>,
    trashed: &HashSet<PathBuf>,
    files: &[FileEntry],
) {
    let t = theme::current(ui.ctx());
    let row_height = 38.0;
    egui::ScrollArea::vertical()
        .id_salt(ui.next_auto_id())
        .auto_shrink([false, true])
        .show_rows(ui, row_height, files.len(), |ui, range| {
            for (index, file) in files.iter().enumerate().skip(range.start).take(range.len()) {
                let gone = trashed.contains(&file.path);
                widgets::list_row(ui, index, row_height, |ui| {
                    size_label(ui, row_height, file.bytes, file.disk_bytes);
                    ui.add_sized(
                        [86.0, row_height],
                        egui::Label::new(
                            RichText::new(widgets::date(file.modified)).color(t.text_muted),
                        ),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (show, trash) = widgets::file_actions(ui, &file.path, gone);
                        if show {
                            reveal(shared, file.path.clone());
                        }
                        if trash {
                            *pending = Some(vec![(file.path.clone(), file.bytes)]);
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let mut text = RichText::new(file.path.display().to_string());
                            if gone {
                                text = text.strikethrough().color(t.text_muted);
                            }
                            ui.add(egui::Label::new(text).truncate());
                        });
                    });
                });
            }
        });
}

/// Size per file type: a ring chart of the largest types (the rest grouped as "Other") with
/// its legend, then every type with its share as a bar.
fn file_types(ui: &mut egui::Ui, tree: &ScanTree) {
    let t = theme::current(ui.ctx());
    let total: u64 = tree.extensions.iter().map(|e| e.bytes).sum();
    let max = tree.extensions.first().map(|e| e.bytes).unwrap_or(1).max(1);
    let name_of = |ext: &rewind_os_engine::scan::ExtensionStat| {
        if ext.extension.is_empty() {
            tr("(no extension)").to_owned()
        } else {
            format!(".{}", ext.extension)
        }
    };
    let top = t.palette.len() - 1;
    let mut sizes: Vec<u64> = tree.extensions.iter().take(top).map(|e| e.bytes).collect();
    let mut names: Vec<String> = tree.extensions.iter().take(top).map(&name_of).collect();
    let rest: u64 = tree.extensions.iter().skip(top).map(|e| e.bytes).sum();
    if rest > 0 {
        sizes.push(rest);
        names.push(tr("Other").to_owned());
    }
    let parts = charts::shares(t, &sizes, total);
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.add_space(6.0);
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    let summary = names
                        .iter()
                        .zip(&sizes)
                        .map(|(n, b)| format!("{n} {}", bytes(*b)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    charts::donut(
                        ui,
                        168.0,
                        &parts,
                        &bytes(total),
                        &trf("{n} types", &[("n", &count(tree.extensions.len() as u64))]),
                        &format!("{}: {summary}", tr("File types")),
                    );
                    ui.add_space(t.gap);
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        for ((name, size), (fraction, color)) in
                            names.iter().zip(&sizes).zip(&parts)
                        {
                            charts::legend_row(ui, *color, name, &bytes(*size), Some(*fraction));
                        }
                    });
                });
            });
            ui.add_space(t.gap);
            egui::Grid::new("ext_grid")
                .num_columns(4)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    for (i, ext) in tree.extensions.iter().enumerate() {
                        let name = name_of(ext);
                        let color = if i < top {
                            t.palette[i % t.palette.len()]
                        } else {
                            t.control
                        };
                        ui.label(RichText::new(&name).strong().family(crate::theme::bold()));
                        ui.label(bytes(ext.bytes));
                        ui.label(
                            RichText::new(trf("{n} files", &[("n", &count(ext.files))]))
                                .color(t.text_muted),
                        );
                        ui.allocate_ui(egui::vec2(240.0, 12.0), |ui| {
                            widgets::usage_bar(ui, ext.bytes as f32 / max as f32, color, &name);
                        });
                        ui.end_row();
                    }
                });
        });
}

/// Draws the children of `current` as tiles. Returns the folder the user clicked.
fn treemap_view(
    ui: &mut egui::Ui,
    tree: &ScanTree,
    current: NodeId,
    trashed: &HashSet<PathBuf>,
    height: f32,
) -> Option<NodeId> {
    let t = theme::current(ui.ctx());
    let children: Vec<(NodeId, String, u64)> = tree
        .children(current)
        .take(TREEMAP_TILES)
        .map(|(id, n)| (id, n.name.to_string(), n.bytes))
        .collect();
    let (bounds, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    ui.painter()
        .rect_filled(bounds, CornerRadius::same(t.radius), t.surface);
    let values: Vec<f64> = children.iter().map(|(_, _, b)| *b as f64).collect();
    let tiles = treemap::layout(&values, bounds.shrink(2.0));
    let mut clicked = None;
    for (i, (rect, (id, name, size))) in tiles.iter().zip(&children).enumerate() {
        if !rect.is_positive() || rect.width() < 2.0 || rect.height() < 2.0 {
            continue;
        }
        let gone = trashed.contains(&tree.path(*id));
        let fill = if gone {
            t.track
        } else {
            t.palette[i % t.palette.len()]
        };
        let tile = rect.shrink(1.0);
        let response = ui.interact(tile, ui.scope_id().with(("tile", *id)), Sense::click());
        let hovered = response.hovered() || response.has_focus();
        ui.painter().rect_filled(
            tile,
            CornerRadius::same(3),
            if hovered {
                fill.gamma_multiply(1.25)
            } else {
                fill
            },
        );
        if response.has_focus() {
            ui.painter().rect_stroke(
                tile,
                CornerRadius::same(3),
                Stroke::new(2.0, t.text),
                egui::StrokeKind::Inside,
            );
        }
        if tile.width() > 64.0 && tile.height() > 34.0 {
            let ink = readable_on(fill);
            let galley = ui.painter().layout(
                format!("{name}\n{}", bytes(*size)),
                egui::FontId::proportional(12.0),
                ink,
                tile.width() - 8.0,
            );
            ui.painter()
                .with_clip_rect(tile)
                .galley(tile.min + egui::vec2(5.0, 4.0), galley, ink);
        }
        let label = format!("{name}, {}", bytes(*size));
        response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
        if response.on_hover_text(&label).clicked() {
            clicked = Some(*id);
        }
    }
    clicked
}

/// Black or white text, whichever contrasts more with `fill`.
fn readable_on(fill: Color32) -> Color32 {
    let lin = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let l = 0.2126 * lin(fill.r()) + 0.7152 * lin(fill.g()) + 0.0722 * lin(fill.b());
    if l > 0.18 {
        Color32::from_rgb(0x10, 0x12, 0x16)
    } else {
        Color32::WHITE
    }
}
