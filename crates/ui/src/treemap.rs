//! Squarified treemap layout: splits a rectangle into tiles whose areas are proportional to
//! the values and whose shapes stay close to squares, so small tiles remain clickable.

use egui::{Pos2, Rect, pos2};

/// Lays out `values` (largest first works best) inside `bounds`. The result has one rectangle
/// per value, in the same order; values of zero get an empty rectangle.
pub fn layout(values: &[f64], bounds: Rect) -> Vec<Rect> {
    let mut out = vec![Rect::NOTHING; values.len()];
    let total: f64 = values.iter().filter(|v| **v > 0.0).sum();
    let area = f64::from(bounds.width()) * f64::from(bounds.height());
    if total <= 0.0 || area <= 0.0 {
        return out;
    }
    let scale = area / total;
    let items: Vec<(usize, f64)> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > 0.0)
        .map(|(i, v)| (i, v * scale))
        .collect();

    let (mut x, mut y) = (f64::from(bounds.min.x), f64::from(bounds.min.y));
    let (mut w, mut h) = (f64::from(bounds.width()), f64::from(bounds.height()));
    let mut start = 0;
    while start < items.len() {
        let side = w.min(h);
        let mut end = start + 1;
        let mut best = worst(&items[start..end], side);
        while end < items.len() {
            let next = worst(&items[start..=end], side);
            if next > best {
                break;
            }
            best = next;
            end += 1;
        }
        let row = &items[start..end];
        let row_area: f64 = row.iter().map(|(_, a)| a).sum();
        let last = end == items.len();
        if w >= h {
            // A column along the left edge.
            let col_w = if last { w } else { row_area / h };
            let mut cy = y;
            for (k, (index, a)) in row.iter().enumerate() {
                let th = if k + 1 == row.len() {
                    y + h - cy
                } else {
                    a / col_w
                };
                out[*index] = rect(x, cy, col_w, th);
                cy += th;
            }
            x += col_w;
            w -= col_w;
        } else {
            // A row along the top edge.
            let row_h = if last { h } else { row_area / w };
            let mut cx = x;
            for (k, (index, a)) in row.iter().enumerate() {
                let tw = if k + 1 == row.len() {
                    x + w - cx
                } else {
                    a / row_h
                };
                out[*index] = rect(cx, y, tw, row_h);
                cx += tw;
            }
            y += row_h;
            h -= row_h;
        }
        start = end;
    }
    out
}

/// The worst aspect ratio in a row laid along a side of length `side`.
fn worst(row: &[(usize, f64)], side: f64) -> f64 {
    let sum: f64 = row.iter().map(|(_, a)| a).sum();
    let max = row.iter().map(|(_, a)| *a).fold(f64::MIN, f64::max);
    let min = row.iter().map(|(_, a)| *a).fold(f64::MAX, f64::min);
    if sum <= 0.0 || min <= 0.0 {
        return f64::MAX;
    }
    let s2 = side * side;
    (s2 * max / (sum * sum)).max(sum * sum / (s2 * min))
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::from_min_max(
        Pos2::new(x as f32, y as f32),
        pos2((x + w) as f32, (y + h) as f32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(400.0, 250.0))
    }

    fn area(r: &Rect) -> f32 {
        if r.is_positive() { r.area() } else { 0.0 }
    }

    const VALUES: [f64; 9] = [500.0, 260.0, 120.0, 120.0, 60.0, 30.0, 8.0, 3.0, 1.0];

    #[test]
    fn tiles_stay_inside_and_fill_the_bounds() {
        let tiles = layout(&VALUES, bounds());
        let outer = bounds().expand(0.01);
        for t in &tiles {
            assert!(outer.contains_rect(*t), "{t:?} outside");
        }
        let covered: f32 = tiles.iter().map(area).sum();
        assert!((covered - bounds().area()).abs() < bounds().area() * 0.001);
    }

    #[test]
    fn tiles_do_not_overlap() {
        let tiles = layout(&VALUES, bounds());
        for (i, a) in tiles.iter().enumerate() {
            for b in &tiles[i + 1..] {
                let overlap = a.intersect(*b);
                assert!(area(&overlap) < 0.5, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn bigger_values_get_bigger_tiles() {
        let tiles = layout(&VALUES, bounds());
        for w in tiles.windows(2) {
            assert!(area(&w[0]) + 0.5 >= area(&w[1]));
        }
    }

    #[test]
    fn zero_values_get_no_space() {
        let tiles = layout(&[10.0, 0.0, 5.0], bounds());
        assert_eq!(area(&tiles[1]), 0.0);
        assert!(area(&tiles[0]) > area(&tiles[2]));
    }
}
