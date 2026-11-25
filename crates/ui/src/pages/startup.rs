use crate::app::Shared;
use crate::charts;
use crate::jobs::{self, Job};
use crate::theme;
use crate::widgets::{self, card, page_header};
use egui::RichText;
use rewind_os_engine::format::count;
use rewind_os_engine::startup::{Source, StartupEntry};
use rewind_os_engine::{Request, Response, Task};
use rewind_os_i18n::{tr, trf};

#[derive(Default)]
pub struct State {
    job: Option<Job>,
    entries: Option<Vec<StartupEntry>>,
    /// The id of the entry whose details are open.
    selected: Option<String>,
    started: bool,
}

const CARD_HEIGHT: f32 = 64.0;

/// Whether an entry starts, as a word and its colour (the word carries the meaning).
fn state(t: &theme::Theme, entry: &StartupEntry) -> (&'static str, egui::Color32) {
    if entry.enabled {
        (tr("On"), t.ok)
    } else {
        (tr("Off"), t.text_muted)
    }
}

/// When an entry runs, in English for `tr`.
fn when(entry: &StartupEntry) -> &'static str {
    entry.when.unwrap_or("At sign-in")
}

/// The details of one entry and its switch. Returns the state the user asked for.
fn details(
    ui: &mut egui::Ui,
    shared: &mut Shared,
    entry: &StartupEntry,
    elevated: bool,
) -> Option<bool> {
    let t = theme::current(ui.ctx());
    let mut asked = None;
    ui.horizontal(|ui| {
        let program = rewind_os_engine::icons::program_of_command(&entry.command);
        shared.icons.show(ui, program.as_deref(), &entry.name);
        ui.add(egui::Label::new(RichText::new(&entry.name).font(theme::semibold(16.0))).wrap());
    });
    ui.add_space(12.0);
    let (word, color) = state(t, entry);
    ui.label(RichText::new(tr("Status")).small().color(t.text_muted));
    ui.horizontal(|ui| {
        widgets::badge(ui, word, color);
        if entry.essential {
            widgets::badge(ui, tr("Needed by the system"), t.text_muted)
                .on_hover_text(tr(rewind_os_engine::startup::ESSENTIAL));
        }
    });
    ui.add_space(6.0);
    widgets::detail_field(ui, tr("When it runs"), tr(when(entry)));
    widgets::detail_field(ui, tr("Registered in"), tr(entry.source.label()));
    widgets::detail_field(ui, tr("Command"), RichText::new(&entry.command).monospace());
    ui.add_space(6.0);
    // Essential entries can be turned on but never off.
    if entry.essential && entry.enabled {
        ui.label(
            RichText::new(tr(rewind_os_engine::startup::ESSENTIAL))
                .small()
                .color(t.text_muted),
        );
    } else {
        let needs_rights = entry.needs_admin && !elevated;
        let label = if entry.enabled {
            tr("Turn off")
        } else {
            tr("Turn on")
        };
        let button = ui.add_enabled(!needs_rights, egui::Button::new(label));
        let button = if needs_rights {
            button.on_disabled_hover_text(tr("This needs administrator rights."))
        } else {
            button
        };
        if button.clicked() {
            asked = Some(!entry.enabled);
        }
    }
    asked
}

impl State {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn refresh(&mut self, ctx: &egui::Context, shared: &Shared) {
        self.started = true;
        self.job = Some(Job::start(ctx, &shared.engine, Request::StartupList));
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        if !self.started {
            self.refresh(&ctx, shared);
        }
        match jobs::finished(&mut self.job, &ctx) {
            Some(Ok(Response::Startup(list))) => {
                shared.icons.request(
                    &ctx,
                    list.iter()
                        .filter_map(|e| rewind_os_engine::icons::program_of_command(&e.command)),
                );
                if self.selected.is_none()
                    && let Some(index) = crate::devtools::selected_item()
                {
                    self.selected = list.get(index).map(|e| e.id.clone());
                }
                self.entries = Some(list);
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => shared.error(err),
            None => {}
        }
        page_header(
            ui,
            tr("Startup programs"),
            tr(
                "Programs that start when you sign in, and the tasks programs scheduled for themselves. Turning one off does not uninstall it, and you can turn it back on at any time.",
            ),
        );
        let Some(entries) = &self.entries else {
            ui.spinner();
            return;
        };
        if entries.is_empty() {
            ui.label(tr("No startup programs found."));
            return;
        }
        summary(ui, entries);
        ui.add_space(t.gap);
        if !shared.elevated && entries.iter().any(|e| e.needs_admin) {
            shared.admin_hint(ui);
            ui.add_space(t.gap * 0.75);
        }
        let mut changed = None;
        if let Some(id) = &self.selected {
            match entries.iter().find(|e| &e.id == id) {
                Some(entry) => {
                    let elevated = shared.elevated;
                    let closed =
                        widgets::detail_panel(ui, "startup_detail", tr("Startup entry"), |ui| {
                            if let Some(enabled) = details(ui, shared, entry, elevated) {
                                changed = Some((entry.id.clone(), enabled));
                            }
                        });
                    if closed {
                        self.selected = None;
                    }
                }
                None => self.selected = None,
            }
        }
        let mut clicked = None;
        let selected = self.selected.as_deref();
        widgets::card_grid(ui, entries.len(), 280.0, CARD_HEIGHT, |ui, index| {
            let Some(entry) = entries.get(index) else {
                return;
            };
            let card = widgets::item_card(
                ui,
                &entry.name,
                selected == Some(entry.id.as_str()),
                CARD_HEIGHT,
                |ui| {
                    let program = rewind_os_engine::icons::program_of_command(&entry.command);
                    shared.icons.show(ui, program.as_deref(), &entry.name);
                    ui.add_space(2.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (word, color) = state(t, entry);
                        ui.label(RichText::new(word).small().color(color));
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                            widgets::card_text(ui, &entry.name, tr(when(entry)));
                        });
                    });
                },
            );
            if card.clicked() {
                clicked = Some(entry.id.clone());
            }
        });
        if let Some(id) = clicked {
            self.selected = Some(id);
        }
        if let Some((id, enabled)) = changed {
            match shared
                .engine
                .run(Request::StartupSet { id, enabled }, &Task::new())
            {
                Ok(Response::Done(message)) => shared.info(tr(message)),
                Ok(_) => {}
                Err(err) => shared.error(crate::widgets::error_text(&err)),
            }
            self.refresh(&ctx, shared);
        }
    }
}

/// The kinds of entries, as people know them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    SignIn,
    Task,
    Service,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::SignIn, Kind::Task, Kind::Service];

    fn of(source: Source) -> Kind {
        match source {
            Source::ScheduledTask => Kind::Task,
            Source::Service => Kind::Service,
            _ => Kind::SignIn,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::SignIn => "Programs at sign-in",
            Kind::Task => "Scheduled tasks",
            Kind::Service => "Services",
        }
    }
}

/// Key figures and what starts, split by kind.
fn summary(ui: &mut egui::Ui, entries: &[StartupEntry]) {
    let t = theme::current(ui.ctx());
    let on = entries.iter().filter(|e| e.enabled).count();
    let essential = entries.iter().filter(|e| e.essential).count();
    let share = on as f32 / entries.len().max(1) as f32;
    charts::grid(ui, 3, 180.0, |ui, i| match i {
        0 => charts::stat_tile(
            ui,
            tr("Start with the system"),
            &count(on as u64),
            &trf("of {n} entries", &[("n", &count(entries.len() as u64))]),
            Some((share, t.accent)),
        ),
        1 => charts::stat_tile(
            ui,
            tr("Turned off"),
            &count((entries.len() - on) as u64),
            tr("You can turn them back on at any time"),
            None,
        ),
        _ => charts::stat_tile(
            ui,
            tr("Needed by the system"),
            &count(essential as u64),
            tr("Can be turned on, never off"),
            None,
        ),
    });
    let kinds: Vec<(Kind, usize)> = Kind::ALL
        .into_iter()
        .map(|k| {
            let n = entries
                .iter()
                .filter(|e| e.enabled && Kind::of(e.source) == k)
                .count();
            (k, n)
        })
        .filter(|(_, n)| *n > 0)
        .collect();
    if kinds.is_empty() {
        return;
    }
    ui.add_space(t.gap * 0.75);
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(tr("What starts with the system"))
                .strong()
                .family(crate::theme::bold()),
        );
        ui.add_space(8.0);
        let sizes: Vec<u64> = kinds.iter().map(|(_, n)| *n as u64).collect();
        let parts = charts::shares(t, &sizes, on as u64);
        let text = kinds
            .iter()
            .map(|(k, n)| format!("{} {n}", tr(k.label())))
            .collect::<Vec<_>>()
            .join(", ");
        charts::stacked_bar(ui, 12.0, &parts, &text);
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for ((kind, n), (fraction, color)) in kinds.iter().zip(&parts) {
                ui.allocate_ui(egui::vec2(230.0, 20.0), |ui| {
                    ui.set_width(230.0);
                    charts::legend_row(
                        ui,
                        *color,
                        tr(kind.label()),
                        &count(*n as u64),
                        Some(*fraction),
                    );
                });
                ui.add_space(t.gap);
            }
        });
    });
}
