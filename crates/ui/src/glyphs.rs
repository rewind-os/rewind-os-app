//! Small line icons drawn with the painter, so they look the same on every system and need no
//! icon font (fonts without the glyphs showed empty boxes). Each is drawn in a 16×16 box.

use egui::{Color32, Painter, Pos2, Rect, Stroke, vec2};
use std::f32::consts::{PI, TAU};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Home,
    Analyzer,
    Duplicates,
    Cleanup,
    Leftovers,
    BuildFolders,
    Programs,
    Startup,
    Lighten,
    Maintenance,
    Drive,
    Health,
    Gauge,
    Download,
    Stack,
    Battery,
}

/// Draws `glyph` centred in `rect` (any size; drawn at 16 points).
pub fn paint(painter: &Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let origin = rect.center() - vec2(8.0, 8.0);
    let p = |x: f32, y: f32| origin + vec2(x, y);
    let stroke = Stroke::new(1.5, color);
    let line = |points: Vec<Pos2>| painter.line(points, stroke);
    match glyph {
        Glyph::Home => {
            line(vec![p(2.0, 8.0), p(8.0, 2.5), p(14.0, 8.0)]);
            line(vec![p(4.0, 6.5), p(4.0, 14.0), p(12.0, 14.0), p(12.0, 6.5)]);
            line(vec![
                p(6.75, 14.0),
                p(6.75, 10.0),
                p(9.25, 10.0),
                p(9.25, 14.0),
            ]);
        }
        Glyph::Analyzer => {
            painter.circle_stroke(p(8.0, 8.0), 6.0, stroke);
            line(vec![p(8.0, 2.0), p(8.0, 8.0), p(14.0, 8.0)]);
        }
        Glyph::Duplicates => {
            rounded(
                painter,
                Rect::from_min_max(p(2.0, 5.5), p(10.5, 14.0)),
                stroke,
            );
            line(vec![
                p(5.5, 5.5),
                p(5.5, 2.0),
                p(14.0, 2.0),
                p(14.0, 10.5),
                p(10.5, 10.5),
            ]);
        }
        Glyph::Cleanup => {
            line(vec![p(2.5, 4.5), p(13.5, 4.5)]);
            line(vec![p(6.0, 4.5), p(6.0, 2.5), p(10.0, 2.5), p(10.0, 4.5)]);
            line(vec![p(4.0, 4.5), p(5.0, 14.0), p(11.0, 14.0), p(12.0, 4.5)]);
            line(vec![p(7.0, 7.0), p(7.0, 11.5)]);
            line(vec![p(9.0, 7.0), p(9.0, 11.5)]);
        }
        Glyph::Leftovers => {
            line(vec![
                p(2.0, 13.5),
                p(2.0, 3.0),
                p(6.0, 3.0),
                p(7.5, 4.75),
                p(14.0, 4.75),
                p(14.0, 13.5),
                p(2.0, 13.5),
            ]);
        }
        Glyph::BuildFolders => {
            // A package: a box seen from above, with its lid's seam.
            line(vec![
                p(8.0, 2.0),
                p(14.0, 5.0),
                p(14.0, 11.0),
                p(8.0, 14.0),
                p(2.0, 11.0),
                p(2.0, 5.0),
                p(8.0, 2.0),
            ]);
            line(vec![p(2.0, 5.0), p(8.0, 8.0), p(14.0, 5.0)]);
            line(vec![p(8.0, 8.0), p(8.0, 14.0)]);
        }
        Glyph::Programs => {
            for (x, y) in [(2.0, 2.0), (9.0, 2.0), (2.0, 9.0), (9.0, 9.0)] {
                rounded(
                    painter,
                    Rect::from_min_size(p(x, y), vec2(5.0, 5.0)),
                    stroke,
                );
            }
        }
        Glyph::Startup => {
            // A power symbol: a ring open at the top and a bar through the gap.
            let centre = p(8.0, 8.75);
            let points: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = -PI / 2.0 + 0.55 + (TAU - 1.1) * i as f32 / 24.0;
                    centre + 5.5 * vec2(a.cos(), a.sin())
                })
                .collect();
            line(points);
            line(vec![p(8.0, 1.75), p(8.0, 8.0)]);
        }
        Glyph::Lighten => {
            for (y, knob) in [(4.0, 10.5), (8.0, 5.0), (12.0, 9.0)] {
                line(vec![p(2.0, y), p(14.0, y)]);
                painter.circle_filled(p(knob, y), 2.0, color);
            }
        }
        Glyph::Drive => {
            // A drive seen from the front, with its activity light.
            rounded(
                painter,
                Rect::from_min_max(p(1.5, 4.5), p(14.5, 11.5)),
                stroke,
            );
            painter.circle_filled(p(11.5, 8.0), 1.1, color);
            line(vec![p(4.0, 8.0), p(8.0, 8.0)]);
        }
        Glyph::Health => {
            // A drive with a pulse across it.
            rounded(
                painter,
                Rect::from_min_max(p(1.5, 3.5), p(14.5, 12.5)),
                stroke,
            );
            line(vec![
                p(3.5, 8.0),
                p(6.0, 8.0),
                p(7.25, 5.5),
                p(8.75, 10.5),
                p(10.0, 8.0),
                p(12.5, 8.0),
            ]);
        }
        Glyph::Battery => {
            // A battery standing up, with its terminal on top and two of three bars charged.
            rounded(
                painter,
                Rect::from_min_max(p(4.5, 3.0), p(11.5, 15.0)),
                stroke,
            );
            painter.rect_filled(Rect::from_min_max(p(6.5, 1.0), p(9.5, 3.0)), 0.5, color);
            for top in [9.0, 11.75] {
                painter.rect_filled(
                    Rect::from_min_max(p(6.5, top), p(9.5, top + 1.75)),
                    0.3,
                    color,
                );
            }
        }
        Glyph::Stack => {
            // Two disks in a stack: a drive file holding a drive.
            rounded(
                painter,
                Rect::from_min_max(p(4.5, 2.0), p(14.0, 9.5)),
                stroke,
            );
            line(vec![
                p(2.0, 6.5),
                p(2.0, 14.0),
                p(11.5, 14.0),
                p(11.5, 12.0),
            ]);
            painter.circle_filled(p(11.25, 5.75), 1.0, color);
        }
        Glyph::Download => {
            // An arrow down into a tray.
            line(vec![p(8.0, 2.0), p(8.0, 10.0)]);
            line(vec![p(4.75, 6.75), p(8.0, 10.0), p(11.25, 6.75)]);
            line(vec![
                p(2.5, 10.5),
                p(2.5, 13.5),
                p(13.5, 13.5),
                p(13.5, 10.5),
            ]);
        }
        Glyph::Gauge => {
            // A speedometer: an open arc with its needle.
            let center = p(8.0, 9.5);
            let arc: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = PI * (0.85 + 1.3 * i as f32 / 16.0);
                    center + vec2(a.cos(), a.sin()) * 6.5
                })
                .collect();
            line(arc);
            line(vec![center, center + vec2(0.6, -1.0).normalized() * 4.5]);
            painter.circle_filled(center, 1.2, color);
        }
        Glyph::Maintenance => {
            painter.circle_stroke(p(8.0, 8.0), 2.5, stroke);
            for i in 0..8 {
                let a = TAU * i as f32 / 8.0;
                let dir = vec2(a.cos(), a.sin());
                line(vec![p(8.0, 8.0) + dir * 4.5, p(8.0, 8.0) + dir * 6.5]);
            }
            painter.circle_stroke(p(8.0, 8.0), 4.75, Stroke::new(1.0, color));
        }
    }
}

fn rounded(painter: &Painter, rect: Rect, stroke: Stroke) {
    painter.rect_stroke(rect, 1.5, stroke, egui::StrokeKind::Middle);
}
