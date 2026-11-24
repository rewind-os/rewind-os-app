//! Theme tokens. Every colour, radius and spacing the UI uses comes from here, so a theme
//! change reaches every panel. Text colours meet 4.5:1 against their surfaces and control
//! colours 3:1, in both themes.

use egui::{Color32, CornerRadius, Stroke, Visuals};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub background: Color32,
    /// The navigation column, a step darker than the page.
    pub sidebar: Color32,
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
    /// Fills for initial tiles (programs without an icon).
    pub tiles: [Color32; 8],
    pub radius: u8,
    pub gap: f32,
    pub card_padding: i8,
}

// Warm and retro, like the cassette in the app icon: espresso surfaces (cream in the light
// theme), one mustard accent, no borders, muted secondary text, generous spacing. The layout
// follows the spirit of Termius. Status colours appear only as text or as a faint tint behind a
// word. `tiles` colour the initial shown for a program without an icon.
pub const DARK: Theme = Theme {
    background: Color32::from_rgb(0x17, 0x12, 0x0e),
    sidebar: Color32::from_rgb(0x12, 0x0e, 0x0b),
    surface: Color32::from_rgb(0x21, 0x1a, 0x15),
    surface_raised: Color32::from_rgb(0x2d, 0x24, 0x1d),
    border: Color32::from_rgb(0x38, 0x2e, 0x26),
    text: Color32::from_rgb(0xf6, 0xef, 0xe2),
    text_muted: Color32::from_rgb(0xa8, 0x99, 0x8a),
    accent: Color32::from_rgb(0xe0, 0xa4, 0x3c),
    on_accent: Color32::from_rgb(0x24, 0x17, 0x08),
    ok: Color32::from_rgb(0x86, 0xc9, 0x8f),
    warn: Color32::from_rgb(0xf0, 0x90, 0x4f),
    danger: Color32::from_rgb(0xf0, 0x7b, 0x6c),
    on_danger: Color32::from_rgb(0x2a, 0x0b, 0x07),
    track: Color32::from_rgb(0x2e, 0x26, 0x1f),
    control: Color32::from_rgb(0x7d, 0x6f, 0x62),
    // Tones of the accent, darkest last, for treemap tiles.
    palette: [
        Color32::from_rgb(0xe0, 0xa4, 0x3c),
        Color32::from_rgb(0xc8, 0x8f, 0x33),
        Color32::from_rgb(0xaf, 0x7a, 0x2c),
        Color32::from_rgb(0x95, 0x66, 0x26),
        Color32::from_rgb(0x7c, 0x53, 0x21),
        Color32::from_rgb(0x64, 0x43, 0x1d),
        Color32::from_rgb(0x4f, 0x35, 0x19),
        Color32::from_rgb(0x3d, 0x2a, 0x16),
    ],
    tiles: TILES,
    radius: 10,
    gap: 16.0,
    card_padding: 18,
};

pub const LIGHT: Theme = Theme {
    background: Color32::from_rgb(0xf7, 0xf2, 0xea),
    sidebar: Color32::from_rgb(0xef, 0xe7, 0xda),
    surface: Color32::from_rgb(0xff, 0xfc, 0xf7),
    surface_raised: Color32::from_rgb(0xec, 0xe3, 0xd5),
    border: Color32::from_rgb(0xe2, 0xd7, 0xc6),
    text: Color32::from_rgb(0x2b, 0x1a, 0x14),
    text_muted: Color32::from_rgb(0x6b, 0x5a, 0x4a),
    accent: Color32::from_rgb(0x85, 0x56, 0x0c),
    on_accent: Color32::from_rgb(0xff, 0xff, 0xff),
    ok: Color32::from_rgb(0x2a, 0x6f, 0x38),
    warn: Color32::from_rgb(0x96, 0x4a, 0x0e),
    danger: Color32::from_rgb(0xb2, 0x38, 0x2a),
    on_danger: Color32::from_rgb(0xff, 0xff, 0xff),
    track: Color32::from_rgb(0xea, 0xe1, 0xd3),
    control: Color32::from_rgb(0x9a, 0x8b, 0x7b),
    palette: [
        Color32::from_rgb(0x85, 0x56, 0x0c),
        Color32::from_rgb(0x9c, 0x6a, 0x1f),
        Color32::from_rgb(0xb3, 0x80, 0x36),
        Color32::from_rgb(0xc6, 0x97, 0x50),
        Color32::from_rgb(0xd6, 0xad, 0x6e),
        Color32::from_rgb(0xe3, 0xc2, 0x8e),
        Color32::from_rgb(0xec, 0xd5, 0xad),
        Color32::from_rgb(0xf3, 0xe6, 0xcc),
    ],
    tiles: TILES,
    radius: 10,
    gap: 16.0,
    card_padding: 18,
};

/// Fills for the initial tile of a program without an icon; white text reads on each.
const TILES: [Color32; 8] = [
    Color32::from_rgb(0xb4, 0x57, 0x3a),
    Color32::from_rgb(0xa8, 0x72, 0x1c),
    Color32::from_rgb(0x6f, 0x7a, 0x2e),
    Color32::from_rgb(0x2f, 0x6f, 0x6a),
    Color32::from_rgb(0x7a, 0x4a, 0x6e),
    Color32::from_rgb(0x8a, 0x4b, 0x2f),
    Color32::from_rgb(0x4f, 0x5d, 0x75),
    Color32::from_rgb(0x5e, 0x48, 0x3a),
];

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
