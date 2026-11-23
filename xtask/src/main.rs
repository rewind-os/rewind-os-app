//! `cargo xtask <command>`:
//!
//! - `icons`: renders every app icon file from `assets/app-icon/rewind-os.svg` (PNG sizes for
//!   Linux and the window icon, the Windows `.ico`, the macOS 512 px icon with Apple's margin).
//!   The outputs are committed, so building never needs this.
//! - `render <svg> <size> <png>`: renders one SVG to a square PNG, for previews.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const APP_ID: &str = "io.github.r_dvl.rewind_os";
const HICOLOR: [u32; 8] = [16, 24, 32, 48, 64, 128, 256, 512];
const ICO: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];

type Result<T> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["icons"] => icons(),
        ["render", svg, size, out] => size
            .parse::<u32>()
            .map_err(|e| format!("size: {e}"))
            .and_then(|s| render_file(Path::new(svg), s, Path::new(out))),
        _ => Err("usage: cargo xtask icons | cargo xtask render <svg> <size> <png>".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xtask: {err}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn load(svg: &str) -> Result<resvg::usvg::Tree> {
    resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).map_err(|e| e.to_string())
}

/// Renders `tree` into a `size`×`size` PNG.
fn render(tree: &resvg::usvg::Tree, size: u32) -> Result<Vec<u8>> {
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(size, size).ok_or_else(|| format!("bad size {size}"))?;
    let view = tree.size();
    let scale = size as f32 / view.width().max(view.height());
    resvg::render(
        tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| e.to_string())
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn render_file(svg: &Path, size: u32, out: &Path) -> Result<()> {
    let text = std::fs::read_to_string(svg).map_err(|e| format!("{}: {e}", svg.display()))?;
    write(out, &render(&load(&text)?, size)?)
}

fn icons() -> Result<()> {
    let dir = root().join("assets/app-icon");
    let source = dir.join("rewind-os.svg");
    let text =
        std::fs::read_to_string(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    if !text.contains(r#"viewBox="0 0 512 512""#) {
        return Err(format!(
            "{}: expected viewBox=\"0 0 512 512\"",
            source.display()
        ));
    }
    let tile = load(&text)?;
    // macOS icons follow Apple's grid: an 824/1024 body with a transparent margin, made by
    // widening the viewBox (512 / 0.805 = 636, so 62 units each side).
    let mac = load(&text.replacen(
        r#"viewBox="0 0 512 512""#,
        r#"viewBox="-62 -62 636 636""#,
        1,
    ))?;

    write(&dir.join("rewind-os-1024.png"), &render(&tile, 1024)?)?;
    write(&dir.join("rewind-os-macos-512.png"), &render(&mac, 512)?)?;
    for size in HICOLOR {
        let path = dir.join(format!("hicolor/{size}x{size}/apps/{APP_ID}.png"));
        write(&path, &render(&tile, size)?)?;
    }
    write(
        &dir.join(format!("hicolor/scalable/apps/{APP_ID}.svg")),
        text.as_bytes(),
    )?;

    let images: Vec<(u32, Vec<u8>)> = ICO
        .iter()
        .map(|&s| render(&tile, s).map(|png| (s, png)))
        .collect::<Result<_>>()?;
    write(&dir.join("rewind-os.ico"), &ico(&images))?;
    println!("icons written to {}", dir.display());
    Ok(())
}

/// A Windows icon file whose images are PNG-compressed (supported since Windows Vista).
fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, png) in images {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in images {
        out.extend_from_slice(png);
    }
    out
}
