//! Theme tokens. Every colour, radius and spacing the UI uses comes from here, so a theme
//! change reaches every panel. Text colours meet 4.5:1 against their surfaces and control
//! colours 3:1, in both themes.

use egui::{Color32, CornerRadius, Stroke, Visuals};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub background: Color32,
    pub surface: Color32,
    pub surface_raised: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub danger: Color32,
    pub on_danger: Color32,
    pub track: Color32,
    /// Fill of check boxes and radio buttons: 3:1 against the surfaces, so they are visible
    /// without outlines.
    pub control: Color32,
    /// Distinct fills for treemap tiles and charts.
    pub palette: [Color32; 8],
    pub radius: u8,
    pub gap: f32,
    pub card_padding: i8,
}

// Minimal by design: one accent (the app's teal), near-flat surfaces without borders, muted
// secondary text, generous spacing. Status colours are only used for text, next to a word.
pub const DARK: Theme = Theme {
    background: Color32::from_rgb(0x12, 0x13, 0x16),
    surface: Color32::from_rgb(0x19, 0x1a, 0x1e),
    surface_raised: Color32::from_rgb(0x22, 0x24, 0x28),
    border: Color32::from_rgb(0x2a, 0x2c, 0x31),
    text: Color32::from_rgb(0xec, 0xec, 0xee),
    text_muted: Color32::from_rgb(0x9a, 0x9c, 0xa3),
    accent: Color32::from_rgb(0x3c, 0xc2, 0xad),
    on_accent: Color32::from_rgb(0x06, 0x1f, 0x1b),
    ok: Color32::from_rgb(0x6f, 0xcf, 0x97),
    warn: Color32::from_rgb(0xe2, 0xb7, 0x5a),
    danger: Color32::from_rgb(0xf2, 0x8b, 0x82),
    on_danger: Color32::from_rgb(0x2a, 0x0b, 0x09),
    track: Color32::from_rgb(0x26, 0x28, 0x2d),
    control: Color32::from_rgb(0x6e, 0x72, 0x79),
    // Tones of the accent, darkest last, for treemap tiles.
    palette: [
        Color32::from_rgb(0x3c, 0xc2, 0xad),
        Color32::from_rgb(0x34, 0xa6, 0x95),
        Color32::from_rgb(0x2d, 0x8c, 0x7e),
        Color32::from_rgb(0x27, 0x74, 0x69),
        Color32::from_rgb(0x22, 0x5f, 0x57),
        Color32::from_rgb(0x1e, 0x4d, 0x47),
        Color32::from_rgb(0x1b, 0x3e, 0x3a),
        Color32::from_rgb(0x19, 0x33, 0x30),
    ],
    radius: 8,
    gap: 16.0,
    card_padding: 18,
};

pub const LIGHT: Theme = Theme {
    background: Color32::from_rgb(0xfb, 0xfb, 0xfa),
    surface: Color32::from_rgb(0xf3, 0xf3, 0xf1),
    surface_raised: Color32::from_rgb(0xea, 0xea, 0xe7),
    border: Color32::from_rgb(0xe2, 0xe2, 0xde),
    text: Color32::from_rgb(0x17, 0x18, 0x1a),
    text_muted: Color32::from_rgb(0x5c, 0x5f, 0x66),
    accent: Color32::from_rgb(0x0e, 0x7a, 0x6c),
    on_accent: Color32::from_rgb(0xff, 0xff, 0xff),
    ok: Color32::from_rgb(0x1d, 0x7a, 0x45),
    warn: Color32::from_rgb(0x8a, 0x5a, 0x00),
    danger: Color32::from_rgb(0xb4, 0x2b, 0x22),
    on_danger: Color32::from_rgb(0xff, 0xff, 0xff),
    track: Color32::from_rgb(0xe6, 0xe6, 0xe3),
    control: Color32::from_rgb(0x86, 0x89, 0x8f),
    palette: [
        Color32::from_rgb(0x0e, 0x7a, 0x6c),
        Color32::from_rgb(0x2a, 0x8e, 0x81),
        Color32::from_rgb(0x48, 0xa1, 0x95),
        Color32::from_rgb(0x67, 0xb3, 0xa9),
        Color32::from_rgb(0x86, 0xc4, 0xbc),
        Color32::from_rgb(0xa3, 0xd3, 0xcd),
        Color32::from_rgb(0xbe, 0xe0, 0xdb),
        Color32::from_rgb(0xd5, 0xeb, 0xe8),
    ],
    radius: 8,
    gap: 16.0,
    card_padding: 18,
};

/// The tokens for the theme egui is currently showing.
pub fn current(ctx: &egui::Context) -> &'static Theme {
    match ctx.theme() {
        egui::Theme::Dark => &DARK,
        egui::Theme::Light => &LIGHT,
    }
}

fn visuals(theme: &Theme, base: Visuals) -> Visuals {
    let mut v = base;
    let radius = CornerRadius::same(theme.radius);
    v.panel_fill = theme.background;
    v.window_fill = theme.surface;
    v.extreme_bg_color = theme.surface_raised;
    v.faint_bg_color = theme.surface_raised;
    v.window_stroke = Stroke::new(1.0, theme.border);
    v.window_corner_radius = radius;
    v.override_text_color = Some(theme.text);
    v.hyperlink_color = theme.accent;
    v.selection.bg_fill = theme.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, theme.text);
    v.warn_fg_color = theme.warn;
    v.error_fg_color = theme.danger;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = radius;
    }
    // Flat widgets: no outlines, a soft fill that darkens on hover.
    v.widgets.noninteractive.bg_stroke = Stroke::NONE;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, theme.text);
    v.widgets.inactive.weak_bg_fill = theme.surface_raised;
    v.widgets.inactive.bg_fill = theme.control;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, theme.text);
    v.widgets.hovered.weak_bg_fill = theme.border;
    v.widgets.hovered.bg_fill = theme.text_muted;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.hovered.expansion = 0.0;
    v.widgets.active.weak_bg_fill = theme.border;
    v.widgets.active.bg_stroke = Stroke::new(1.0, theme.accent);
    v.widgets.active.expansion = 0.0;
    v.widgets.open.bg_stroke = Stroke::NONE;
    v.window_stroke = Stroke::NONE;
    v.window_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(60),
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(50),
    };
    // Keyboard focus stays clearly visible.
    v.selection.stroke = Stroke::new(1.5, theme.accent);
    v
}

/// Installs both themes; egui follows the system setting.
pub fn install(ctx: &egui::Context) {
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&DARK, Visuals::dark()));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&LIGHT, Visuals::light()));
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(14.0, 7.0);
        style.spacing.interact_size.y = 30.0;
        for (text_style, size) in [
            (egui::TextStyle::Heading, 26.0),
            (egui::TextStyle::Body, 14.0),
            (egui::TextStyle::Button, 14.0),
            (egui::TextStyle::Small, 12.0),
            (egui::TextStyle::Monospace, 13.0),
        ] {
            if let Some(font) = style.text_styles.get_mut(&text_style) {
                font.size = size;
            }
        }
    });
}
