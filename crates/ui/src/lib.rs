//! The Rewind OS desktop interface.

mod app;
mod devtools;
mod icons;
mod jobs;
mod pages;
mod theme;
mod treemap;
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
