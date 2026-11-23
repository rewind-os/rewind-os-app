//! The ReWindows desktop interface.

mod app;
mod devtools;
mod jobs;
mod pages;
mod theme;
mod treemap;
mod widgets;

/// Opens the main window and runs until it closes.
pub fn run() -> eframe::Result {
    rewindows_i18n::init(None);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ReWindows")
            .with_app_id("rewindows")
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([760.0, 520.0])
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "ReWindows",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            load_system_font(&cc.egui_ctx);
            Ok(Box::new(app::App::new(&cc.egui_ctx)))
        }),
    )
}

/// Uses the Windows interface font when it is installed (it is read from the system, not
/// shipped with the app). Elsewhere egui's bundled font stays.
fn load_system_font(ctx: &egui::Context) {
    let Some(windir) = std::env::var_os("WINDIR") else {
        return;
    };
    let path = std::path::Path::new(&windir)
        .join("Fonts")
        .join("segoeui.ttf");
    let Ok(data) = std::fs::read(path) else {
        return;
    };
    ctx.add_font(egui::epaint::text::FontInsert::new(
        "system-ui",
        egui::FontData::from_owned(data),
        vec![egui::epaint::text::InsertFontFamily {
            family: egui::FontFamily::Proportional,
            priority: egui::epaint::text::FontPriority::Highest,
        }],
    ));
}

/// The window icon.
fn icon() -> egui::IconData {
    const SIZE: u32 = 64;
    egui::IconData {
        rgba: icon_rgba(SIZE),
        width: SIZE,
        height: SIZE,
    }
}

/// The app icon as RGBA pixels, drawn in code at any size: a rounded blue tile with three
/// white bars of rising height. Also used to write `assets/rewindows.ico`.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let tile = [0x0a, 0x62, 0xc4];
    let radius = s * 0.19;
    // (left, top) of each bar as fractions of the size; bars share width and bottom.
    let bars = [(0.22, 0.56), (0.44, 0.38), (0.66, 0.20)];
    let (bar_w, bar_bottom) = (0.14, 0.80);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let cx = fx.clamp(radius, s - radius);
            let cy = fy.clamp(radius, s - radius);
            if (fx - cx).powi(2) + (fy - cy).powi(2) > radius * radius {
                continue;
            }
            let (u, v) = (fx / s, fy / s);
            let on_bar = bars
                .iter()
                .any(|(left, top)| u >= *left && u < left + bar_w && v >= *top && v < bar_bottom);
            let i = ((y * size + x) * 4) as usize;
            let color = if on_bar { [0xff, 0xff, 0xff] } else { tile };
            rgba[i..i + 3].copy_from_slice(&color);
            rgba[i + 3] = 0xff;
        }
    }
    rgba
}
