//! Small building blocks shared by the pages. Drawn surfaces report their role, label and
//! value to assistive technology through `widget_info`.

use crate::jobs::Job;
use crate::theme::{self, Theme};
use egui::{
    Color32, CornerRadius, Margin, Response, RichText, Role, Sense, Stroke, Ui, WidgetInfo,
};
use rewindows_engine::format;
use rewindows_i18n::{tr, trf};

pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str) {
    let t = theme::current(ui.ctx());
    ui.add_space(4.0);
    ui.heading(RichText::new(title).strong());
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).color(t.text_muted));
    }
    ui.add_space(t.gap);
}

pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let t = theme::current(ui.ctx());
    egui::Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(CornerRadius::same(t.radius))
        .inner_margin(Margin::same(t.card_padding))
        .show(ui, add_contents)
        .inner
}

/// A horizontal bar showing `fraction` (0..=1). `label` is what a screen reader announces.
pub fn usage_bar(ui: &mut Ui, fraction: f32, color: Color32, label: &str) -> Response {
    let t = theme::current(ui.ctx());
    let fraction = fraction.clamp(0.0, 1.0);
    let width = ui.available_width().max(40.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 10.0), Sense::hover());
    let radius = CornerRadius::same(5);
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

/// A short status word with a coloured outline. The word carries the meaning; colour only
/// reinforces it.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let t = theme::current(ui.ctx());
    egui::Frame::new()
        .stroke(Stroke::new(1.0, color))
        .corner_radius(CornerRadius::same(t.radius))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| ui.label(RichText::new(text).color(color).small()))
        .inner
}

pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let t = theme::current(ui.ctx());
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(t.on_accent).strong()).fill(t.accent),
    )
}

pub fn danger_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let t = theme::current(ui.ctx());
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(t.on_danger).strong()).fill(t.danger),
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
        ui.label(RichText::new(stage).strong());
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

/// Date of a timestamp in the local time zone, `YYYY-MM-DD`.
pub fn date(ts: Option<chrono::DateTime<chrono::Utc>>) -> String {
    ts.map(|t| {
        t.with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string()
    })
    .unwrap_or_default()
}
