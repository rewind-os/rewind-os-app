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

// Deep navy surfaces in the spirit of Termius, one accent (the app's teal), no borders, muted
// secondary text, generous spacing. Status colours appear only as text or as a faint tint
// behind a word. `tiles` colour the initial shown for a program without an icon.
pub const DARK: Theme = Theme {
    background: Color32::from_rgb(0x14, 0x17, 0x29),
    sidebar: Color32::from_rgb(0x10, 0x12, 0x21),
    surface: Color32::from_rgb(0x1d, 0x20, 0x33),
    surface_raised: Color32::from_rgb(0x27, 0x2b, 0x42),
    border: Color32::from_rgb(0x2c, 0x31, 0x4a),
    text: Color32::from_rgb(0xf5, 0xf6, 0xff),
    text_muted: Color32::from_rgb(0x8d, 0x91, 0xa5),
    accent: Color32::from_rgb(0x2b, 0xc4, 0xc2),
    on_accent: Color32::from_rgb(0x05, 0x1a, 0x1d),
    ok: Color32::from_rgb(0x3d, 0xd0, 0x86),
    warn: Color32::from_rgb(0xff, 0xb3, 0x47),
    danger: Color32::from_rgb(0xff, 0x7a, 0x8a),
    on_danger: Color32::from_rgb(0x2a, 0x08, 0x0e),
    track: Color32::from_rgb(0x25, 0x29, 0x3f),
    control: Color32::from_rgb(0x6b, 0x70, 0x8a),
    // Tones of the accent, darkest last, for treemap tiles.
    palette: [
        Color32::from_rgb(0x2b, 0xc4, 0xc2),
        Color32::from_rgb(0x26, 0xa9, 0xab),
        Color32::from_rgb(0x22, 0x8f, 0x95),
        Color32::from_rgb(0x1f, 0x77, 0x80),
        Color32::from_rgb(0x1d, 0x62, 0x6e),
        Color32::from_rgb(0x1c, 0x50, 0x5f),
        Color32::from_rgb(0x1b, 0x41, 0x53),
        Color32::from_rgb(0x1a, 0x35, 0x49),
    ],
    tiles: TILES,
    radius: 10,
    gap: 16.0,
    card_padding: 18,
};

pub const LIGHT: Theme = Theme {
    background: Color32::from_rgb(0xf4, 0xf6, 0xf8),
    sidebar: Color32::from_rgb(0xe8, 0xec, 0xf0),
    surface: Color32::from_rgb(0xff, 0xff, 0xff),
    surface_raised: Color32::from_rgb(0xe6, 0xea, 0xef),
    border: Color32::from_rgb(0xdc, 0xe1, 0xe7),
    text: Color32::from_rgb(0x14, 0x17, 0x29),
    text_muted: Color32::from_rgb(0x55, 0x5a, 0x70),
    accent: Color32::from_rgb(0x0b, 0x7c, 0x7f),
    on_accent: Color32::from_rgb(0xff, 0xff, 0xff),
    ok: Color32::from_rgb(0x15, 0x7a, 0x47),
    warn: Color32::from_rgb(0x8f, 0x55, 0x00),
    danger: Color32::from_rgb(0xb8, 0x23, 0x3a),
    on_danger: Color32::from_rgb(0xff, 0xff, 0xff),
    track: Color32::from_rgb(0xe3, 0xe7, 0xec),
    control: Color32::from_rgb(0x84, 0x89, 0x9a),
    palette: [
        Color32::from_rgb(0x0b, 0x7c, 0x7f),
        Color32::from_rgb(0x2a, 0x8f, 0x92),
        Color32::from_rgb(0x48, 0xa2, 0xa4),
        Color32::from_rgb(0x67, 0xb4, 0xb6),
        Color32::from_rgb(0x86, 0xc5, 0xc7),
        Color32::from_rgb(0xa3, 0xd4, 0xd5),
        Color32::from_rgb(0xbe, 0xe1, 0xe2),
        Color32::from_rgb(0xd5, 0xec, 0xec),
    ],
    tiles: TILES,
    radius: 10,
    gap: 16.0,
    card_padding: 18,
};

/// Fills for the initial tile of a program without an icon; white text reads on each.
const TILES: [Color32; 8] = [
    Color32::from_rgb(0xd9, 0x4f, 0x1c),
    Color32::from_rgb(0xb8, 0x10, 0x5a),
    Color32::from_rgb(0x1f, 0x6f, 0xc2),
    Color32::from_rgb(0x6e, 0x45, 0xc4),
    Color32::from_rgb(0x15, 0x86, 0x5a),
    Color32::from_rgb(0x0f, 0x7f, 0x8a),
    Color32::from_rgb(0xb0, 0x6a, 0x00),
    Color32::from_rgb(0x4a, 0x55, 0x7a),
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
