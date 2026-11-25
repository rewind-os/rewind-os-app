//! The Rewind OS desktop interface.

mod app;
mod charts;
mod devtools;
mod glyphs;
mod icons;
mod jobs;
mod pages;
mod theme;
mod treemap;
mod update;
mod widgets;

/// Reverse-DNS id: the Wayland app id, the Linux `.desktop` file and the icon name.
pub const APP_ID: &str = "io.github.rewind_os.rewind_os_app";

/// Opens the main window and runs until it closes.
pub fn run() -> eframe::Result {
    rewind_os_i18n::init(None);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Rewind OS")
            .with_app_id(APP_ID)
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([760.0, 520.0])
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Rewind OS",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            devtools::apply_theme(&cc.egui_ctx);
            load_system_font(&cc.egui_ctx);
            Ok(Box::new(app::App::new(&cc.egui_ctx)))
        }),
    )
}

/// Uses the system's interface font when it is installed (read from the system, not shipped
/// with the app): Segoe UI on Windows. The semibold family (`theme::SEMIBOLD`) takes the
/// system's semibold face, Segoe UI Semibold on Windows or a common one on Linux, and falls back
/// to the regular font, so it always exists.
fn load_system_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut regular: Vec<std::path::PathBuf> = Vec::new();
    let mut semibold: Vec<std::path::PathBuf> = Vec::new();
    if let Some(windir) = std::env::var_os("WINDIR") {
        let dir = std::path::Path::new(&windir).join("Fonts");
        regular.push(dir.join("segoeui.ttf"));
        semibold.push(dir.join("seguisb.ttf"));
    } else {
        for path in [
            "/usr/share/fonts/truetype/ubuntu/Ubuntu-M.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf",
            "/usr/share/fonts/noto/NotoSans-SemiBold.ttf",
            "/usr/share/fonts/google-noto/NotoSans-SemiBold.ttf",
        ] {
            semibold.push(path.into());
        }
    }
    let mut family = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if let Some(data) = regular.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert(
            "system-ui".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(data)),
        );
        family.insert(0, "system-ui".to_owned());
        fonts
            .families
            .insert(egui::FontFamily::Proportional, family.clone());
    }
    let mut strong = family;
    if let Some(data) = semibold.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert(
            "system-ui-semibold".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(data)),
        );
        strong.insert(0, "system-ui-semibold".to_owned());
    }
    fonts
        .families
        .insert(egui::FontFamily::Name(theme::SEMIBOLD.into()), strong);
    ctx.set_fonts(fonts);
}

/// The window icon: the 256 px app icon on Windows and Linux, the one with Apple's margin on
/// macOS (see `assets/app-icon/README.md`).
fn icon() -> egui::IconData {
    let png: &[u8] = if cfg!(target_os = "macos") {
        include_bytes!("../../../assets/app-icon/rewind-os-macos-512.png")
    } else {
        include_bytes!(
            "../../../assets/app-icon/hicolor/256x256/apps/io.github.rewind_os.rewind_os_app.png"
        )
    };
    eframe::icon_data::from_png_bytes(png).unwrap_or_default()
}
