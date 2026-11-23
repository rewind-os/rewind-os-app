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
    /// Distinct fills for treemap tiles and charts.
    pub palette: [Color32; 8],
    pub radius: u8,
    pub gap: f32,
    pub card_padding: i8,
}

pub const DARK: Theme = Theme {
    background: Color32::from_rgb(0x17, 0x19, 0x1d),
    surface: Color32::from_rgb(0x21, 0x24, 0x2a),
    surface_raised: Color32::from_rgb(0x2b, 0x2f, 0x37),
    border: Color32::from_rgb(0x3a, 0x3f, 0x49),
    text: Color32::from_rgb(0xe8, 0xea, 0xed),
    text_muted: Color32::from_rgb(0xa9, 0xaf, 0xb9),
    accent: Color32::from_rgb(0x4e, 0xa1, 0xff),
    on_accent: Color32::from_rgb(0x08, 0x16, 0x26),
    ok: Color32::from_rgb(0x56, 0xd3, 0x64),
    warn: Color32::from_rgb(0xe3, 0xb3, 0x41),
    danger: Color32::from_rgb(0xff, 0x7b, 0x72),
    on_danger: Color32::from_rgb(0x26, 0x08, 0x08),
    track: Color32::from_rgb(0x34, 0x39, 0x42),
    palette: [
        Color32::from_rgb(0x3b, 0x82, 0xd6),
        Color32::from_rgb(0x2f, 0xa5, 0x8c),
        Color32::from_rgb(0xc0, 0x7a, 0x2c),
        Color32::from_rgb(0x8b, 0x6b, 0xd6),
        Color32::from_rgb(0xc2, 0x56, 0x7a),
        Color32::from_rgb(0x5f, 0x9e, 0x3a),
        Color32::from_rgb(0x3a, 0x9c, 0xb8),
        Color32::from_rgb(0xa0, 0x8a, 0x3c),
    ],
    radius: 6,
    gap: 12.0,
    card_padding: 14,
};

pub const LIGHT: Theme = Theme {
    background: Color32::from_rgb(0xf3, 0xf4, 0xf6),
    surface: Color32::from_rgb(0xff, 0xff, 0xff),
    surface_raised: Color32::from_rgb(0xf7, 0xf8, 0xfa),
    border: Color32::from_rgb(0xd5, 0xd9, 0xe0),
    text: Color32::from_rgb(0x1b, 0x1d, 0x22),
    text_muted: Color32::from_rgb(0x55, 0x5c, 0x67),
    accent: Color32::from_rgb(0x0a, 0x62, 0xc4),
    on_accent: Color32::from_rgb(0xff, 0xff, 0xff),
    ok: Color32::from_rgb(0x1a, 0x7f, 0x37),
    warn: Color32::from_rgb(0x8f, 0x5f, 0x00),
    danger: Color32::from_rgb(0xc4, 0x26, 0x26),
    on_danger: Color32::from_rgb(0xff, 0xff, 0xff),
    track: Color32::from_rgb(0xe3, 0xe6, 0xeb),
    palette: [
        Color32::from_rgb(0x5b, 0x9b, 0xe6),
        Color32::from_rgb(0x45, 0xb8, 0x9f),
        Color32::from_rgb(0xe0, 0x9a, 0x4c),
        Color32::from_rgb(0xa5, 0x8b, 0xe8),
        Color32::from_rgb(0xe0, 0x7b, 0x9c),
        Color32::from_rgb(0x82, 0xbd, 0x5c),
        Color32::from_rgb(0x5c, 0xb9, 0xd4),
        Color32::from_rgb(0xc4, 0xab, 0x5c),
    ],
    radius: 6,
    gap: 12.0,
    card_padding: 14,
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
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, theme.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, theme.text);
    v.widgets.inactive.weak_bg_fill = theme.surface_raised;
    v.widgets.inactive.bg_fill = theme.surface_raised;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, theme.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, theme.text);
    v.widgets.hovered.weak_bg_fill = theme.track;
    v.widgets.hovered.bg_fill = theme.track;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, theme.accent);
    v.widgets.active.weak_bg_fill = theme.accent.gamma_multiply(0.5);
    v.widgets.active.bg_stroke = Stroke::new(1.0, theme.accent);
    v
}

/// Installs both themes; egui follows the system setting.
pub fn install(ctx: &egui::Context) {
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&DARK, Visuals::dark()));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&LIGHT, Visuals::light()));
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        for (text_style, size) in [
            (egui::TextStyle::Heading, 22.0),
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
