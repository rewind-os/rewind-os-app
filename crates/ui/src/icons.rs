//! Program icons, read on a background thread and kept as textures for the whole session. A
//! program without a readable icon shows its initial instead.

use crate::theme;
use egui::{CornerRadius, Sense, TextureHandle, TextureOptions, Ui};
use rewind_os_engine::icons::{self, Icon};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};

/// Side of the drawn icon, in points.
pub const SIZE: f32 = 28.0;

pub struct IconCache {
    textures: HashMap<String, Option<TextureHandle>>,
    asked: HashSet<String>,
    tx: Sender<(String, Option<Icon>)>,
    rx: Receiver<(String, Option<Icon>)>,
}

impl Default for IconCache {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            textures: HashMap::new(),
            asked: HashSet::new(),
            tx,
            rx,
        }
    }
}

impl IconCache {
    /// Starts loading the icons named by `specs` (`DisplayIcon` values or program paths) that
    /// were not asked for yet.
    pub fn request(&mut self, ctx: &egui::Context, specs: impl IntoIterator<Item = String>) {
        let new: Vec<String> = specs
            .into_iter()
            .filter(|s| self.asked.insert(s.clone()))
            .collect();
        if new.is_empty() {
            return;
        }
        let pixels = (SIZE * ctx.pixels_per_point()).ceil() as u32;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for spec in new {
                let icon = icons::load(&spec, pixels);
                if tx.send((spec, icon)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok((spec, icon)) = self.rx.try_recv() {
            let texture = icon.and_then(|i| {
                let size = [
                    usize::try_from(i.width).ok()?,
                    usize::try_from(i.height).ok()?,
                ];
                if size[0] * size[1] * 4 != i.rgba.len() {
                    return None;
                }
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &i.rgba);
                Some(ctx.load_texture(format!("icon:{spec}"), image, TextureOptions::LINEAR))
            });
            self.textures.insert(spec, texture);
        }
    }

    /// Draws the icon of `spec`, or the first letter of `name` while it loads or when there is
    /// none. Decorative: the name next to it is what screen readers read.
    pub fn show(&mut self, ui: &mut Ui, spec: Option<&str>, name: &str) {
        self.receive(ui.ctx());
        let texture = spec
            .and_then(|s| self.textures.get(s))
            .and_then(Option::as_ref);
        match texture {
            Some(texture) => {
                ui.add(
                    egui::Image::new((texture.id(), egui::vec2(SIZE, SIZE)))
                        .corner_radius(CornerRadius::same(7)),
                );
            }
            None => initial(ui, name),
        }
    }
}

fn initial(ui: &mut Ui, name: &str) {
    let t = theme::current(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), Sense::hover());
    // The same name always gets the same colour.
    let hash = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    let fill = t
        .tiles
        .get(hash as usize % t.tiles.len())
        .copied()
        .unwrap_or(t.surface_raised);
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    let letter: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(14.0),
        egui::Color32::WHITE,
    );
}
