//! Small building blocks shared by the pages. Drawn surfaces report their role, label and
//! value to assistive technology through `widget_info`.

use crate::jobs::Job;
use crate::theme::{self, Theme};
use egui::{
    Color32, CornerRadius, Margin, Response, RichText, Role, Sense, Stroke, Ui, WidgetInfo,
};
use rewind_os_engine::format;
use rewind_os_i18n::{tr, trf};

pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str) {
    page_header_with(ui, title, subtitle, |_| {});
}

/// The page's tool band, as in Termius: the title in semibold and its one-line description on
/// a band a step lighter than the page, running across the page's whole width, with `actions`
/// laid out from the right edge.
pub fn page_header_with(ui: &mut Ui, title: &str, subtitle: &str, actions: impl FnOnce(&mut Ui)) {
    let t = theme::current(ui.ctx());
    // The band is painted under the header once its height is known.
    let band = ui.painter().add(egui::Shape::Noop);
    let header = ui
        .horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(title)
                        .font(theme::semibold(18.0))
                        .color(t.text),
                );
                if !subtitle.is_empty() {
                    ui.label(RichText::new(subtitle).small().color(t.text_muted));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
        })
        .response
        .rect;
    let clip = ui.clip_rect();
    let rect = egui::Rect::from_min_max(
        egui::pos2(clip.left() + 1.0, clip.top()),
        egui::pos2(clip.right(), header.bottom() + 14.0),
    );
    ui.painter().set(
        band,
        egui::Shape::rect_filled(rect, CornerRadius::ZERO, t.surface),
    );
    ui.add_space(14.0 + t.gap);
}

pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let t = theme::current(ui.ctx());
    egui::Frame::new()
        .fill(t.surface)
        .corner_radius(CornerRadius::same(t.radius))
        .inner_margin(Margin::same(t.card_padding))
        .show(ui, add_contents)
        .inner
}

/// One row of a list, on its own card as Termius lists its hosts and logs. The row under the
/// pointer is lighter across its whole width, so the buttons at the end of a row clearly belong
/// to it. `add` lays the row out from left to right, vertically centred.
pub fn list_row<R>(ui: &mut Ui, _index: usize, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = theme::current(ui.ctx());
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let fill = if ui.rect_contains_pointer(rect) {
        t.surface_raised
    } else {
        t.surface
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(t.radius), fill);
    let inner = rect.shrink2(egui::vec2(12.0, 0.0));
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    add(&mut row)
}

/// Items as a grid of cards, as Termius shows its hosts: as many columns of at least
/// `min_width` as fit, rows of `height`, in a scroll area that only lays out the visible rows.
/// `add` draws item `index` into its cell, usually with [`item_card`].
pub fn card_grid(
    ui: &mut Ui,
    n: usize,
    min_width: f32,
    height: f32,
    mut add: impl FnMut(&mut Ui, usize),
) {
    let t = theme::current(ui.ctx());
    let width = ui.available_width();
    let per_row = (((width + t.gap) / (min_width + t.gap)).floor() as usize).max(1);
    let cell = (width - t.gap * (per_row - 1) as f32) / per_row as f32;
    let rows = n.div_ceil(per_row);
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show_rows(ui, height, rows, |ui, range| {
            for row in range {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap;
                    let first = row * per_row;
                    for index in first..(first + per_row).min(n) {
                        ui.allocate_ui_with_layout(
                            egui::vec2(cell, height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(cell);
                                add(ui, index);
                            },
                        );
                    }
                });
            }
        });
}

/// One item of a [`card_grid`]: a card that selects the item when clicked (or chosen with Tab
/// and Enter). The selected card is lighter and ringed in the accent colour; screen readers
/// hear `label` as a list option and whether it is selected. `add` lays the card out from left
/// to right, vertically centred.
pub fn item_card(
    ui: &mut Ui,
    label: &str,
    selected: bool,
    height: f32,
    add: impl FnOnce(&mut Ui),
) -> Response {
    let t = theme::current(ui.ctx());
    let size = egui::vec2(ui.available_width(), height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let radius = CornerRadius::same(t.radius);
    let fill = if selected || response.hovered() {
        t.surface_raised
    } else {
        t.surface
    };
    ui.painter().rect_filled(rect, radius, fill);
    if selected || response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.5, t.accent),
            egui::StrokeKind::Inside,
        );
    }
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(14.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    add(&mut inner);
    let label = label.to_owned();
    response.widget_info(move || WidgetInfo::selected(Role::ListBoxOption, true, selected, &label));
    response
}

/// The selected item's details on a card along the right edge, as Termius opens a host's
/// settings beside the list. Call it before laying out the list, so the list takes the space
/// that is left. `add` draws the details under a row with a Close button; returns true when
/// the user closed the panel with that button or Escape.
pub fn detail_panel(ui: &mut Ui, id: &str, title: &str, add: impl FnOnce(&mut Ui)) -> bool {
    let t = theme::current(ui.ctx());
    let mut closed = ui.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Panel::right(id)
        .exact_size(320.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::new().outer_margin(Margin {
            left: t.gap as i8,
            ..Margin::ZERO
        }))
        .show(ui, |ui| {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).small().color(t.text_muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if row_button(ui, tr("Close"), true)
                            .on_hover_text(tr("Close the details (Escape)"))
                            .clicked()
                        {
                            closed = true;
                        }
                    });
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, add);
            });
        });
    closed
}

/// One fact in a [`detail_panel`]: a muted caption over its value, which wraps.
pub fn detail_field(ui: &mut Ui, caption: &str, value: impl Into<egui::WidgetText>) {
    let t = theme::current(ui.ctx());
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(RichText::new(caption).small().color(t.text_muted));
        ui.add(egui::Label::new(value).wrap());
    });
    ui.add_space(6.0);
}

/// A card's name in semibold over one muted line, both cut short with "…" when the card is
/// narrow, centred in the card's height. Hovering a cut name shows it whole.
pub fn card_text(ui: &mut Ui, name: &str, line: &str) {
    let t = theme::current(ui.ctx());
    let gap = 2.0;
    let height = ui.text_style_height(&egui::TextStyle::Body)
        + gap
        + ui.text_style_height(&egui::TextStyle::Small);
    let top = ((ui.available_height() - height) / 2.0).max(0.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = gap;
        ui.add_space(top);
        ui.add(egui::Label::new(RichText::new(name).family(theme::bold())).truncate());
        ui.add(egui::Label::new(RichText::new(line).small().color(t.text_muted)).truncate());
    });
}

/// A Termius-style icon: a line glyph in white on a rounded square of `fill` (one of
/// `theme::TILES`). Decorative: the name next to it is what screen readers read.
pub fn icon_tile(ui: &mut Ui, glyph: crate::glyphs::Glyph, fill: Color32, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    crate::glyphs::paint(ui.painter(), rect, glyph, Color32::WHITE);
    response
}

/// A small button at the end of a list row.
pub fn row_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(label).small()))
}

/// The usual end of a file or folder row: "Show" in the file manager and "Move to recycle
/// bin", greyed out with the reason for protected paths. Returns true when the user asked to
/// move it. Lays out right to left, so call it inside a right-to-left layout.
pub fn file_actions(ui: &mut Ui, path: &std::path::Path, gone: bool) -> (bool, bool) {
    let refusal = rewind_os_engine::safety::removal_refusal(path);
    let trash = row_button(ui, tr("Bin"), !gone && refusal.is_none());
    let trash = match refusal {
        Some(why) => trash.on_disabled_hover_text(tr(why)),
        None => trash.on_hover_text(tr("Move to recycle bin")),
    };
    let show = row_button(ui, tr("Show"), true).on_hover_text(tr("Show in File Explorer"));
    (show.clicked(), trash.clicked())
}

/// A horizontal bar showing `fraction` (0..=1). `label` is what a screen reader announces.
pub fn usage_bar(ui: &mut Ui, fraction: f32, color: Color32, label: &str) -> Response {
    let t = theme::current(ui.ctx());
    let fraction = fraction.clamp(0.0, 1.0);
    let width = ui.available_width().max(40.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 6.0), Sense::hover());
    let radius = CornerRadius::same(3);
    ui.painter().rect_filled(rect, radius, t.track);
    let mut fill = rect;
    fill.set_width(rect.width() * fraction);
    if fraction > 0.0 {
        ui.painter().rect_filled(fill, radius, color);
    }
    let label = label.to_owned();
    response.widget_info(move || {
        let mut info = WidgetInfo::labeled(Role::ProgressIndicator, true, &label);
        info.value = Some(f64::from(fraction) * 100.0);
        info
    });
    response
}

/// Colour for how full something is; the number is always shown next to it as well.
pub fn fill_color(t: &Theme, fraction: f32) -> Color32 {
    if fraction >= 0.9 {
        t.danger
    } else if fraction >= 0.75 {
        t.warn
    } else {
        t.accent
    }
}

/// A short status word in a status colour. The word carries the meaning; colour only
/// reinforces it.
///
/// Sized to its word, whatever the layout around it: a frame would grow to the height of a row
/// laid out with its items centred.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let padding = egui::vec2(7.0, 2.0);
    let (rect, response) = ui.allocate_exact_size(galley.size() + padding * 2.0, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(6), color.gamma_multiply(0.14));
    ui.painter().galley(rect.min + padding, galley, color);
    let text = text.to_owned();
    response.widget_info(move || WidgetInfo::labeled(Role::Label, true, &text));
    response
}

/// One entry of the navigation list: an icon and a label; the current page sits on a filled
/// pill. Works with the keyboard (Tab, Enter) and reports itself as a tab to assistive
/// technology.
pub fn nav_item(
    ui: &mut Ui,
    glyph: crate::glyphs::Glyph,
    label: &str,
    selected: bool,
    enabled: bool,
) -> Response {
    let t = theme::current(ui.ctx());
    let size = egui::vec2(ui.available_width(), 36.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let hovered = enabled && (response.hovered() || response.has_focus());
    // The current page sits on a lighter pill, as in Termius; hover shows a fainter one.
    let fill = if selected {
        Some(t.surface_raised)
    } else if hovered {
        Some(t.surface_raised.gamma_multiply(0.5))
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(t.radius), fill);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(t.radius),
            Stroke::new(1.5, t.accent),
            egui::StrokeKind::Inside,
        );
    }
    let color = if enabled {
        t.text
    } else {
        t.text_muted.gamma_multiply(0.6)
    };
    let icon = egui::Rect::from_center_size(
        rect.left_center() + egui::vec2(20.0, 0.0),
        egui::vec2(16.0, 16.0),
    );
    crate::glyphs::paint(ui.painter(), icon, glyph, color);
    ui.painter().text(
        rect.left_center() + egui::vec2(40.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        theme::semibold(14.0),
        color,
    );
    let label = label.to_owned();
    response.widget_info(move || WidgetInfo::selected(Role::Tab, enabled, selected, &label));
    response
}

pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let t = theme::current(ui.ctx());
    ui.add_enabled(
        enabled,
        egui::Button::new(
            RichText::new(text)
                .color(t.on_accent)
                .strong()
                .family(crate::theme::bold()),
        )
        .fill(t.accent_fill),
    )
}

pub fn danger_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let t = theme::current(ui.ctx());
    ui.add_enabled(
        enabled,
        egui::Button::new(
            RichText::new(text)
                .color(t.on_danger)
                .strong()
                .family(crate::theme::bold()),
        )
        .fill(t.danger),
    )
}

/// Spinner, stage, counters and a Stop button for a running job. Returns true when the user
/// asked to stop.
pub fn job_progress(ui: &mut Ui, job: &Job, cancellable: bool) -> bool {
    let t = theme::current(ui.ctx());
    let p = job.task.snapshot();
    let mut stop = false;
    ui.horizontal(|ui| {
        ui.spinner();
        let stage = if p.stage.is_empty() {
            tr("Working")
        } else {
            tr(p.stage)
        };
        ui.label(RichText::new(stage).strong().family(crate::theme::bold()));
        if p.items > 0 {
            ui.label(
                RichText::new(trf("{n} items", &[("n", &format::count(p.items))]))
                    .color(t.text_muted),
            );
        }
        if p.bytes > 0 {
            ui.label(RichText::new(format::bytes(p.bytes)).color(t.text_muted));
        }
        ui.label(
            RichText::new(format!("{:.1} s", job.started.elapsed().as_secs_f32()))
                .color(t.text_muted),
        );
        if cancellable && !job.task.is_cancelled() && ui.button(tr("Stop")).clicked() {
            job.task.cancel();
            stop = true;
        }
    });
    if !p.detail.is_empty() {
        ui.add(egui::Label::new(RichText::new(&p.detail).small().color(t.text_muted)).truncate());
    }
    stop
}

/// What the user chose in a confirmation dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Open,
    Confirmed,
    Cancelled,
}

/// A modal confirmation. Escape or clicking outside cancels.
pub fn confirm(ctx: &egui::Context, id: &str, title: &str, body: &str, action: &str) -> Choice {
    let mut choice = Choice::Open;
    let modal = egui::Modal::new(egui::Id::unique(id)).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.heading(title);
        ui.add_space(6.0);
        ui.label(body);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let go = danger_button(ui, action, true);
            if go.clicked() {
                choice = Choice::Confirmed;
            }
            if ui.button(tr("Cancel")).clicked() {
                choice = Choice::Cancelled;
            }
        });
    });
    if choice == Choice::Open && modal.should_close() {
        choice = Choice::Cancelled;
    }
    choice
}

/// Translates a reason the engine gives (English) when the catalog knows it.
pub fn reason(text: &str) -> String {
    rewind_os_i18n::tr_text(text)
}

/// An engine error as text for people: refusals are translated.
pub fn error_text(err: &rewind_os_engine::EngineError) -> String {
    match err {
        rewind_os_engine::EngineError::Refused(why) => reason(why),
        other => other.to_string(),
    }
}

/// Date of a timestamp in the local time zone, `YYYY-MM-DD`.
pub fn date(ts: Option<chrono::DateTime<chrono::Utc>>) -> String {
    ts.map(|t| {
        t.with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string()
    })
    .unwrap_or_default()
}
