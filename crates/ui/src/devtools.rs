//! Hooks for checking the interface without a person at the screen. Only active when the
//! environment asks for them:
//!
//! - `REWIND_OS_PAGE=<overview|analyzer|duplicates|cleanup|leftovers|buildfolders|virtualdisks|programs|startup|lighten|maintenance|disks|processes>`
//!   opens that page first.
//! - `REWIND_OS_THEME=<light|dark>` shows that theme instead of following the system.
//! - `REWIND_OS_SCAN=<path>` starts a disk scan of `path` (a search on the duplicates and build
//!   folders pages).
//! - `REWIND_OS_VIEW=<folders|files|types|changes>` opens that view of the analyzer after a scan;
//!   `REWIND_OS_VIEW=speed` opens the startup page's startup speed.
//! - `REWIND_OS_SELECT=<n>` selects the n-th item (from 0) on the programs and startup pages.
//! - `REWIND_OS_NEWER=<version>` shows the notice of a newer version without asking GitHub.
//! - `REWIND_OS_SCREENSHOT=<file.ppm>` saves the window as a PPM image once background work has
//!   settled (or after 20 seconds) and closes the app.

use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct Devtools {
    screenshot: Option<PathBuf>,
    requested: bool,
    started: Instant,
    quiet_since: Option<Instant>,
}

pub fn start_page() -> Option<String> {
    std::env::var("REWIND_OS_PAGE").ok()
}

pub fn apply_theme(ctx: &egui::Context) {
    match std::env::var("REWIND_OS_THEME").as_deref() {
        Ok("light") => ctx.set_theme(egui::ThemePreference::Light),
        Ok("dark") => ctx.set_theme(egui::ThemePreference::Dark),
        _ => {}
    }
}

pub fn scan_path() -> Option<PathBuf> {
    std::env::var_os("REWIND_OS_SCAN").map(PathBuf::from)
}

/// The item to select on the programs and startup pages, counting from 0, so the screenshot
/// shows its detail panel.
pub fn selected_item() -> Option<usize> {
    std::env::var("REWIND_OS_SELECT").ok()?.parse().ok()
}

/// A version to announce as newer, instead of asking GitHub.
pub fn newer_version() -> Option<String> {
    std::env::var("REWIND_OS_NEWER").ok()
}

/// The view asked for (of the analyzer, or `speed` on the startup page), as the variable spells
/// it.
pub fn analyzer_view() -> Option<String> {
    std::env::var("REWIND_OS_VIEW").ok()
}

impl Devtools {
    pub fn from_env() -> Self {
        Self {
            screenshot: std::env::var_os("REWIND_OS_SCREENSHOT").map(PathBuf::from),
            requested: false,
            started: Instant::now(),
            quiet_since: None,
        }
    }

    /// Call once per frame with whether any background job is still running.
    pub fn frame(&mut self, ctx: &egui::Context, busy: bool) {
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        if self.requested {
            return;
        }
        let now = Instant::now();
        let quiet = if busy {
            None
        } else {
            Some(*self.quiet_since.get_or_insert(now))
        };
        if busy {
            self.quiet_since = None;
        }
        let settled =
            quiet.is_some_and(|since| now.duration_since(since) > Duration::from_millis(1200));
        if settled || now.duration_since(self.started) > Duration::from_secs(20) {
            self.requested = true;
            let done = ctx.clone();
            let callback = egui::ScreenshotCallback::new(move |image| {
                if let Err(err) = write_ppm(&path, &image) {
                    eprintln!("screenshot: {err}");
                }
                done.send_viewport_cmd(egui::ViewportCommand::Close);
                done.request_repaint();
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(callback));
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

fn write_ppm(path: &std::path::Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let [w, h] = image.size;
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    out.reserve(w * h * 3);
    for p in &image.pixels {
        out.extend_from_slice(&[p.r(), p.g(), p.b()]);
    }
    std::fs::write(path, out)
}
