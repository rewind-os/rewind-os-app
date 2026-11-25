//! Dashboard pieces: key figures, gauges, ring charts and stacked bars. Every chart is drawn
//! with the painter in the theme's colours, reports a role, a label and a value to assistive
//! technology, and is shown next to its numbers, so colour is never the only signal.

use crate::theme;
use egui::{
    Color32, CornerRadius, Margin, Response, RichText, Role, Sense, Stroke, Ui, WidgetInfo,
};
use std::f32::consts::{FRAC_PI_2, TAU};

/// A semibold heading above a group of cards ("Drives", "Hosts" in Termius).
pub fn section_title(ui: &mut Ui, text: &str) {
    let t = theme::current(ui.ctx());
    ui.label(
        RichText::new(text)
            .font(theme::semibold(14.0))
            .color(t.text),
    );
    ui.add_space(6.0);
}

/// A key figure: a muted label, a large value, an optional line under it and an optional
/// meter (fraction, colour) along the bottom.
pub fn stat_tile(
    ui: &mut Ui,
    label: &str,
    value: &str,
    detail: &str,
    meter: Option<(f32, Color32)>,
) {
    let t = theme::current(ui.ctx());
    egui::Frame::new()
        .fill(t.surface)
        .corner_radius(CornerRadius::same(t.radius))
        .inner_margin(Margin::symmetric(t.card_padding, 16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(label).small().color(t.text_muted));
            ui.add_space(2.0);
            ui.label(
                RichText::new(value)
                    .size(24.0)
                    .strong()
                    .family(crate::theme::bold())
                    .color(t.text),
            );
            // An empty line still takes its place, so tiles in a row keep one height.
            let detail = if detail.is_empty() { " " } else { detail };
            ui.add(egui::Label::new(RichText::new(detail).small().color(t.text_muted)).truncate());
            // Tiles with and without a meter keep the same height, so a row lines up.
            ui.add_space(8.0);
            match meter {
                Some((fraction, color)) => {
                    crate::widgets::usage_bar(ui, fraction, color, &format!("{label} {value}"));
                }
                None => ui.add_space(6.0),
            }
        });
}

/// Lays out `n` equal columns that wrap to fewer on narrow windows (at least `min_width` each)
/// and calls `add` for each index.
pub fn grid(ui: &mut Ui, n: usize, min_width: f32, mut add: impl FnMut(&mut Ui, usize)) {
    if n == 0 {
        return;
    }
    let t = theme::current(ui.ctx());
    let per_row = ((ui.available_width() + t.gap) / (min_width + t.gap))
        .floor()
        .clamp(1.0, n as f32) as usize;
    let cell = (ui.available_width() - t.gap * (per_row - 1) as f32) / per_row as f32;
    let mut index = 0;
    while index < n {
        let in_row = per_row.min(n - index);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap;
            for offset in 0..in_row {
                ui.allocate_ui_with_layout(
                    egui::vec2(cell, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(cell);
                        add(ui, index + offset);
                    },
                );
            }
        });
        index += in_row;
        if index < n {
            ui.add_space(t.gap * 0.75);
        }
    }
}

/// Points around a full ring.
const RING_STEPS: usize = 96;

/// Draws a ring of `segments` (fraction of the circle, colour), clockwise from the top, on a
/// track. Fractions above 1 in total are cut at a full circle.
fn paint_ring(ui: &Ui, center: egui::Pos2, radius: f32, width: f32, segments: &[(f32, Color32)]) {
    let t = theme::current(ui.ctx());
    let painter = ui.painter();
    // The track is drawn like the segments, centred on `radius`: egui's own circle stroke lies
    // outside the radius, which left the segments inside the track instead of on it.
    let point = |turn: f32| {
        let a = turn * TAU - FRAC_PI_2;
        center + radius * egui::vec2(a.cos(), a.sin())
    };
    let track: Vec<egui::Pos2> = (0..RING_STEPS)
        .map(|i| point(i as f32 / RING_STEPS as f32))
        .collect();
    painter.add(egui::Shape::closed_line(track, Stroke::new(width, t.track)));
    let mut start = 0.0_f32;
    for &(fraction, color) in segments {
        let fraction = fraction.clamp(0.0, 1.0 - start);
        if fraction <= 0.0005 {
            continue;
        }
        let end = start + fraction;
        let steps = ((fraction * RING_STEPS as f32).ceil() as usize).max(2);
        let points: Vec<egui::Pos2> = (0..=steps)
            .map(|i| point(start + fraction * i as f32 / steps as f32))
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(width, color)));
        start = end;
    }
}

/// A gauge: one value as a ring with its percentage in the middle and a caption under it.
pub fn gauge(
    ui: &mut Ui,
    size: f32,
    fraction: f32,
    color: Color32,
    caption: &str,
    label: &str,
) -> Response {
    let t = theme::current(ui.ctx());
    let fraction = fraction.clamp(0.0, 1.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    let width = (size * 0.11).max(6.0);
    let radius = size / 2.0 - width / 2.0 - 1.0;
    paint_ring(ui, rect.center(), radius, width, &[(fraction, color)]);
    ui.painter().text(
        rect.center() - egui::vec2(0.0, if caption.is_empty() { 0.0 } else { 7.0 }),
        egui::Align2::CENTER_CENTER,
        format!("{:.0}%", fraction * 100.0),
        egui::FontId::proportional(size * 0.2),
        t.text,
    );
    if !caption.is_empty() {
        ui.painter().text(
            rect.center() + egui::vec2(0.0, size * 0.13),
            egui::Align2::CENTER_CENTER,
            caption,
            egui::FontId::proportional(11.0),
            t.text_muted,
        );
    }
    let label = label.to_owned();
    response.widget_info(move || {
        let mut info = WidgetInfo::labeled(Role::ProgressIndicator, true, &label);
        info.value = Some(f64::from(fraction) * 100.0);
        info
    });
    response
}

/// A ring chart of shares (fraction of the whole, colour) with `center` written inside.
/// `label` describes the chart for screen readers; the legend next to it carries the numbers.
pub fn donut(
    ui: &mut Ui,
    size: f32,
    segments: &[(f32, Color32)],
    center: &str,
    caption: &str,
    label: &str,
) -> Response {
    let t = theme::current(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    let width = (size * 0.13).max(8.0);
    let radius = size / 2.0 - width / 2.0 - 1.0;
    paint_ring(ui, rect.center(), radius, width, segments);
    ui.painter().text(
        rect.center() - egui::vec2(0.0, 7.0),
        egui::Align2::CENTER_CENTER,
        center,
        egui::FontId::proportional((size * 0.12).max(14.0)),
        t.text,
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, size * 0.11),
        egui::Align2::CENTER_CENTER,
        caption,
        egui::FontId::proportional(11.0),
        t.text_muted,
    );
    let label = label.to_owned();
    response.widget_info(move || WidgetInfo::labeled(Role::Image, true, &label));
    response
}

/// One horizontal bar split into shares (fraction of the whole, colour), on a track.
pub fn stacked_bar(ui: &mut Ui, height: f32, segments: &[(f32, Color32)], label: &str) -> Response {
    let t = theme::current(ui.ctx());
    let width = ui.available_width().max(40.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let radius = CornerRadius::same((height / 2.0).min(4.0) as u8);
    ui.painter().rect_filled(rect, radius, t.track);
    let painter = ui.painter().with_clip_rect(rect);
    let mut x = rect.left();
    for &(fraction, color) in segments {
        let w = rect.width() * fraction.clamp(0.0, 1.0);
        if w < 0.5 {
            continue;
        }
        let w = w.min(rect.right() - x);
        let seg = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, height));
        // A hairline gap between shares keeps neighbours apart without outlines.
        painter.rect_filled(seg.shrink2(egui::vec2(0.5, 0.0)), CornerRadius::ZERO, color);
        x += w;
    }
    let label = label.to_owned();
    response.widget_info(move || WidgetInfo::labeled(Role::Image, true, &label));
    response
}

/// A legend line: a colour square, a name, and its value on the right.
pub fn legend_row(ui: &mut Ui, color: Color32, name: &str, value: &str, share: Option<f32>) {
    let t = theme::current(ui.ctx());
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(2), color);
        ui.add(egui::Label::new(name).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(share) = share {
                ui.label(
                    RichText::new(format!("{:.0}%", share * 100.0))
                        .small()
                        .color(t.text_muted),
                );
            }
            ui.label(RichText::new(value).strong().family(crate::theme::bold()));
        });
    });
}

/// Shares of a whole from sizes: each size over `total`, coloured from the theme palette in
/// order. Sizes past the palette's length are grouped by the caller as "other".
pub fn shares(t: &theme::Theme, sizes: &[u64], total: u64) -> Vec<(f32, Color32)> {
    let total = total.max(1) as f64;
    sizes
        .iter()
        .enumerate()
        .map(|(i, &s)| ((s as f64 / total) as f32, t.palette[i % t.palette.len()]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::shares;

    #[test]
    fn shares_never_add_up_to_more_than_the_whole() {
        let sizes = [500, 300, 200];
        let parts = shares(&crate::theme::DARK, &sizes, 1000);
        let sum: f32 = parts.iter().map(|(f, _)| f).sum();
        assert!(sum <= 1.0001);
        assert!(parts[0].0 > parts[1].0 && parts[1].0 > parts[2].0);
    }

    #[test]
    fn neighbouring_shares_get_different_colours() {
        let parts = shares(&crate::theme::DARK, &[1, 1, 1, 1], 4);
        for pair in parts.windows(2) {
            assert_ne!(pair[0].1, pair[1].1);
        }
    }
}
