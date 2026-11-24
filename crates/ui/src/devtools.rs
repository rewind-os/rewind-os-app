//! Hooks for checking the interface without a person at the screen. Only active when the
//! environment asks for them:
//!
//! - `REWIND_OS_PAGE=<overview|analyzer|duplicates|cleanup|leftovers|programs|startup|lighten|maintenance>`
//!   opens that page first.
//! - `REWIND_OS_SCAN=<path>` starts a disk scan of `path` (a duplicate search on the duplicates
//!   page).
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

pub fn scan_path() -> Option<PathBuf> {
    std::env::var_os("REWIND_OS_SCAN").map(PathBuf::from)
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
