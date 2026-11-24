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
    /// Categorical fills for charts and treemap tiles, in the order series use them.
    pub palette: [Color32; 8],
    /// Fills for initial tiles (programs without an icon).
    pub tiles: [Color32; 8],
    pub radius: u8,
    pub gap: f32,
    pub card_padding: i8,
}

// A professional, data-first look: neutral graphite surfaces (white and cool greys in the light
// theme), one blue accent for actions and selection, no borders, muted secondary text. Charts
// use `palette`, a categorical set that stays distinct in both themes; status colours appear as
// text, a faint tint behind a word, or a gauge that also shows its number.
pub const DARK: Theme = Theme {
    background: Color32::from_rgb(0x0e, 0x11, 0x16),
    sidebar: Color32::from_rgb(0x0a, 0x0d, 0x12),
    surface: Color32::from_rgb(0x16, 0x1a, 0x22),
    surface_raised: Color32::from_rgb(0x1f, 0x24, 0x2e),
    border: Color32::from_rgb(0x2a, 0x30, 0x3c),
    text: Color32::from_rgb(0xe8, 0xeb, 0xf0),
    text_muted: Color32::from_rgb(0x8d, 0x96, 0xa6),
    accent: Color32::from_rgb(0x4c, 0x8d, 0xff),
    on_accent: Color32::from_rgb(0x06, 0x12, 0x2a),
    ok: Color32::from_rgb(0x3f, 0xb9, 0x6b),
    warn: Color32::from_rgb(0xe0, 0xa4, 0x2e),
    danger: Color32::from_rgb(0xf8, 0x5f, 0x5a),
    on_danger: Color32::from_rgb(0x1f, 0x06, 0x05),
    track: Color32::from_rgb(0x24, 0x2a, 0x35),
    control: Color32::from_rgb(0x5d, 0x67, 0x78),
    palette: [
        Color32::from_rgb(0x4c, 0x8d, 0xff),
        Color32::from_rgb(0x2d, 0xc0, 0xa8),
        Color32::from_rgb(0x9b, 0x7d, 0xff),
        Color32::from_rgb(0xf0, 0xa9, 0x3b),
        Color32::from_rgb(0xec, 0x6f, 0xa6),
        Color32::from_rgb(0x45, 0xbf, 0xe8),
        Color32::from_rgb(0x96, 0xc9, 0x5a),
        Color32::from_rgb(0xee, 0x7d, 0x52),
    ],
    tiles: TILES,
    radius: 8,
    gap: 16.0,
    card_padding: 18,
};

pub const LIGHT: Theme = Theme {
    background: Color32::from_rgb(0xf4, 0xf6, 0xf9),
    sidebar: Color32::from_rgb(0xeb, 0xee, 0xf3),
    surface: Color32::from_rgb(0xff, 0xff, 0xff),
    surface_raised: Color32::from_rgb(0xed, 0xf0, 0xf5),
    border: Color32::from_rgb(0xdc, 0xe1, 0xe8),
    text: Color32::from_rgb(0x13, 0x18, 0x21),
    text_muted: Color32::from_rgb(0x5a, 0x63, 0x72),
    accent: Color32::from_rgb(0x1f, 0x63, 0xe0),
    on_accent: Color32::from_rgb(0xff, 0xff, 0xff),
    ok: Color32::from_rgb(0x1a, 0x7f, 0x37),
    warn: Color32::from_rgb(0x9a, 0x63, 0x00),
    danger: Color32::from_rgb(0xcf, 0x22, 0x2e),
    on_danger: Color32::from_rgb(0xff, 0xff, 0xff),
    track: Color32::from_rgb(0xe4, 0xe8, 0xee),
    control: Color32::from_rgb(0x8a, 0x93, 0xa1),
    palette: [
        Color32::from_rgb(0x2f, 0x6f, 0xe4),
        Color32::from_rgb(0x13, 0x8f, 0x7f),
        Color32::from_rgb(0x74, 0x5c, 0xe6),
        Color32::from_rgb(0xc2, 0x7c, 0x0e),
        Color32::from_rgb(0xc7, 0x45, 0x83),
        Color32::from_rgb(0x1b, 0x8e, 0xc0),
        Color32::from_rgb(0x5c, 0x9a, 0x2a),
        Color32::from_rgb(0xcc, 0x58, 0x2c),
    ],
    tiles: TILES,
    radius: 8,
    gap: 16.0,
    card_padding: 18,
};

/// Fills for the initial tile of a program without an icon; white text reads on each.
const TILES: [Color32; 8] = [
    Color32::from_rgb(0x2f, 0x5f, 0xb8),
    Color32::from_rgb(0x1d, 0x7a, 0x6e),
    Color32::from_rgb(0x5b, 0x4b, 0xb0),
    Color32::from_rgb(0x8a, 0x5a, 0x12),
    Color32::from_rgb(0x9a, 0x3a, 0x68),
    Color32::from_rgb(0x1f, 0x6c, 0x93),
    Color32::from_rgb(0x4a, 0x6b, 0x24),
    Color32::from_rgb(0x4b, 0x55, 0x66),
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
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
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
