//! The window's own title bar, in the app's colours, as Termius draws it. On Windows and Linux
//! the system's bar is removed: this one moves the window when dragged, maximizes it on a double
//! click, has the minimize, maximize and close buttons, and the window's edges resize it. On
//! macOS the system keeps its three buttons and the bar only blends into the window.
//!
//! `REWIND_OS_NATIVE_TITLE_BAR=1` keeps the system's title bar, for desktops where a window
//! without one misbehaves.

use crate::theme;
use egui::{CornerRadius, Rect, ResizeDirection, Sense, Stroke, Ui, ViewportCommand, pos2, vec2};

/// Height of the bar.
pub const HEIGHT: f32 = 36.0;
/// Width of each window button, as on Windows.
const BUTTON_WIDTH: f32 = 46.0;
/// How far from the window's edge a press starts a resize.
const EDGE: f32 = 5.0;

/// Whether the system draws the title bar.
pub fn native() -> bool {
    std::env::var_os("REWIND_OS_NATIVE_TITLE_BAR").is_some_and(|v| !v.is_empty() && v != "0")
}

/// The window, with this title bar instead of the system's.
pub fn window(builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if native() {
        builder
    } else if cfg!(target_os = "macos") {
        builder
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
    } else {
        builder.with_decorations(false)
    }
}

/// Whether the app draws a bar at all (on macOS it is only room under the system's buttons).
pub fn shown() -> bool {
    !native()
}

fn maximized(ui: &Ui) -> bool {
    ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false))
}

/// Draws the bar across `ui`: the title in the middle and, except on macOS, the window buttons.
pub fn show(ui: &mut Ui, title: &str) {
    let t = theme::current(ui.ctx());
    let rect = ui.max_rect();
    // The empty bar moves the window; the buttons, drawn after, take their own clicks.
    let bar = ui.interact(
        rect,
        ui.scope_id().with("title_bar"),
        Sense::click_and_drag(),
    );
    if bar.double_clicked() {
        let next = !maximized(ui);
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(next));
    } else if bar.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(13.0),
        t.text_muted,
    );
    // On macOS the system's own buttons sit on the left of the bar.
    if cfg!(target_os = "macos") {
        return;
    }
    let is_maximized = maximized(ui);
    let toggle = if is_maximized {
        Button::Restore
    } else {
        Button::Maximize
    };
    let ctx = ui.ctx().clone();
    // From the right: close, then maximize or restore, then minimize.
    for (i, kind) in [Button::Close, toggle, Button::Minimize]
        .into_iter()
        .enumerate()
    {
        let right = rect.right() - BUTTON_WIDTH * i as f32;
        let area = Rect::from_min_max(
            pos2(right - BUTTON_WIDTH, rect.top()),
            pos2(right, rect.bottom()),
        );
        if window_button(ui, area, kind).clicked() {
            ctx.send_viewport_cmd(match kind {
                Button::Close => ViewportCommand::Close,
                Button::Minimize => ViewportCommand::Minimized(true),
                Button::Maximize | Button::Restore => ViewportCommand::Maximized(!is_maximized),
            });
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Button {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl Button {
    /// English label; translate with `rewind_os_i18n::tr`.
    fn label(self) -> &'static str {
        match self {
            Button::Minimize => "Minimize",
            Button::Maximize => "Maximize",
            Button::Restore => "Restore",
            Button::Close => "Close",
        }
    }
}

/// One window button: a line glyph on the bar, filled when hovered (red for close).
fn window_button(ui: &mut Ui, rect: Rect, kind: Button) -> egui::Response {
    let t = theme::current(ui.ctx());
    let label = rewind_os_i18n::tr(kind.label());
    let response = ui
        .interact(rect, ui.scope_id().with(kind.label()), Sense::click())
        .on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    let hovered = response.hovered() || response.has_focus();
    let (fill, ink) = match (kind, hovered) {
        (Button::Close, true) => (Some(t.danger), t.on_danger),
        (_, true) => (Some(t.surface_raised), t.text),
        (_, false) => (None, t.text_muted),
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
    }
    let stroke = Stroke::new(1.0, ink);
    let c = rect.center();
    let painter = ui.painter();
    match kind {
        Button::Minimize => {
            painter.line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], stroke);
        }
        Button::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(c, vec2(10.0, 10.0)),
                CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Button::Restore => {
            let back = Rect::from_center_size(c + vec2(2.0, -2.0), vec2(8.0, 8.0));
            let front = Rect::from_center_size(c + vec2(-1.0, 1.0), vec2(8.0, 8.0));
            painter.line(
                vec![
                    pos2(back.left(), front.top()),
                    back.left_top(),
                    back.right_top(),
                    back.right_bottom(),
                    pos2(front.right(), back.bottom()),
                ],
                stroke,
            );
            painter.rect_stroke(
                front,
                CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Button::Close => {
            painter.line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], stroke);
            painter.line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], stroke);
        }
    }
    response
}

/// Without the system's frame, the window's edges resize it: the pointer near an edge shows the
/// resize cursor, and pressing there starts a resize. Nothing on macOS, with the system's bar,
/// or while maximized.
pub fn resize_edges(ctx: &egui::Context) {
    if native() || cfg!(target_os = "macos") {
        return;
    }
    let (pos, pressed, size, is_maximized) = ctx.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.viewport().inner_rect.map(|r| r.size()),
            i.viewport().maximized.unwrap_or(false),
        )
    });
    let (Some(pos), Some(size)) = (pos, size) else {
        return;
    };
    if is_maximized {
        return;
    }
    let Some(direction) = edge(pos, size) else {
        return;
    };
    ctx.set_cursor_icon(match direction {
        ResizeDirection::North | ResizeDirection::South => egui::CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => egui::CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => egui::CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => egui::CursorIcon::ResizeNeSw,
    });
    if pressed {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}

/// Which edge or corner of a window of `size` the point `pos` (inside it) is on, if any.
pub fn edge(pos: egui::Pos2, size: egui::Vec2) -> Option<ResizeDirection> {
    let left = pos.x <= EDGE;
    let right = pos.x >= size.x - EDGE;
    let top = pos.y <= EDGE;
    let bottom = pos.y >= size.y - EDGE;
    Some(match (top, bottom, left, right) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (true, _, _, true) => ResizeDirection::NorthEast,
        (_, true, true, _) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, _, _, _) => ResizeDirection::North,
        (_, true, _, _) => ResizeDirection::South,
        (_, _, true, _) => ResizeDirection::West,
        (_, _, _, true) => ResizeDirection::East,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_border_of_the_window_resizes_it() {
        let size = vec2(800.0, 600.0);
        assert!(edge(pos2(400.0, 300.0), size).is_none());
        assert_eq!(edge(pos2(1.0, 1.0), size), Some(ResizeDirection::NorthWest));
        assert_eq!(
            edge(pos2(799.0, 599.0), size),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(edge(pos2(400.0, 598.0), size), Some(ResizeDirection::South));
        assert_eq!(edge(pos2(2.0, 300.0), size), Some(ResizeDirection::West));
        // Just inside the border is the window's own.
        assert!(edge(pos2(EDGE + 1.0, 300.0), size).is_none());
    }
}
