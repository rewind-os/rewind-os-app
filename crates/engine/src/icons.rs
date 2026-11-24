//! Program icons, read from the file Windows names in a program's `DisplayIcon` (an `.ico` file,
//! or an `.exe`/`.dll` with icon resources). Only reads files; never runs them.

use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Larger files are not read for an icon.
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// An icon as straight RGBA pixels, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Splits a `DisplayIcon` value (`"C:\App\app.exe",0`, `C:\App\app.ico`, `%SystemRoot%\x.dll,-101`)
/// into the file and the icon index. A negative index names a resource id.
pub fn parse_spec(spec: &str) -> (PathBuf, i32) {
    let spec = spec.trim();
    let (file, index) = match spec.rsplit_once(',') {
        Some((file, index)) => match index.trim().parse::<i32>() {
            Ok(i) => (file, i),
            Err(_) => (spec, 0),
        },
        None => (spec, 0),
    };
    (PathBuf::from(expand(file.trim().trim_matches('"'))), index)
}

/// The program a command line starts (`"C:\App\app.exe" --hidden` -> `C:\App\app.exe`), as a
/// value for [`load`].
pub fn program_of_command(command: &str) -> Option<String> {
    let command = command.trim();
    let program = if let Some(rest) = command.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        let lower = command.to_ascii_lowercase();
        match lower.find(".exe") {
            Some(end) => command.get(..end + 4)?,
            None => command.split_whitespace().next()?,
        }
    };
    let program = program.trim();
    (!program.is_empty()).then(|| program.to_owned())
}

/// Replaces `%NAME%` with the environment variable, leaving unknown names as they are.
fn expand(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        let (before, after) = rest.split_at(start);
        out.push_str(before);
        let after = after.get(1..).unwrap_or("");
        match after.find('%') {
            Some(end) => {
                let name = after.get(..end).unwrap_or("");
                match std::env::var(name) {
                    Ok(value) if !name.is_empty() => out.push_str(&value),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = after.get(end + 1..).unwrap_or("");
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The icon a `DisplayIcon` value names, at the size closest to `size` pixels (or larger).
pub fn load(spec: &str, size: u32) -> Option<Icon> {
    let (path, index) = parse_spec(spec);
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "ico" {
        return from_ico(&read(&path)?, size);
    }
    // Mapped, not read: executables can be hundreds of megabytes and only the resources are
    // needed.
    let map = pelite::FileMap::open(&path).ok()?;
    from_pe(map.as_ref(), index, size)
        .or_else(|| {
            // Since Windows 10 the icons of many system libraries live in a resource-only
            // copy: `C:\Windows\System32\shell32.dll` -> `C:\Windows\SystemResources\shell32.dll.mun`.
            let windows = path.parent()?.parent()?;
            let mut name = path.file_name()?.to_os_string();
            name.push(".mun");
            let mun = pelite::FileMap::open(&windows.join("SystemResources").join(name)).ok()?;
            from_pe(mun.as_ref(), index, size)
        })
        .or_else(|| from_ico(map.as_ref(), size))
}

fn read(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read(path).ok()
}

/// The image of an `.ico` file closest to `size`: the smallest at least that large, else the
/// largest.
pub fn from_ico(bytes: &[u8], size: u32) -> Option<Icon> {
    let dir = ico::IconDir::read(Cursor::new(bytes)).ok()?;
    let entries = dir.entries();
    let entry = entries
        .iter()
        .filter(|e| e.width() >= size)
        .min_by_key(|e| (e.width(), std::cmp::Reverse(e.bits_per_pixel())))
        .or_else(|| {
            entries
                .iter()
                .max_by_key(|e| (e.width(), e.bits_per_pixel()))
        })?;
    let image = entry.decode().ok()?;
    Some(Icon {
        width: image.width(),
        height: image.height(),
        rgba: image.into_rgba_data(),
    })
}

/// The icon group of an executable or library: the `index`-th one, or the one with resource
/// id `-index` when negative. Falls back to the first group.
fn from_pe(bytes: &[u8], index: i32, size: u32) -> Option<Icon> {
    use pelite::resources::Name;
    let file = pelite::PeFile::from_bytes(bytes).ok()?;
    let resources = file.resources().ok()?;
    let groups: Vec<_> = resources.icons().filter_map(Result::ok).collect();
    let chosen = if index < 0 {
        let id = index.unsigned_abs();
        groups.iter().find(|(name, _)| *name == Name::Id(id))
    } else {
        groups.get(usize::try_from(index).unwrap_or(0))
    }
    .or_else(|| groups.first())?;
    let mut ico = Vec::new();
    chosen.1.write(&mut ico).ok()?;
    from_ico(&ico, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ico_with(sizes: &[u32]) -> Vec<u8> {
        let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
        for &s in sizes {
            let image = ico::IconImage::from_rgba_data(s, s, vec![200; (s * s * 4) as usize]);
            dir.add_entry(ico::IconDirEntry::encode(&image).unwrap());
        }
        let mut out = Vec::new();
        dir.write(&mut out).unwrap();
        out
    }

    #[test]
    fn picks_the_smallest_image_not_smaller_than_asked() {
        let icon = from_ico(&ico_with(&[16, 32, 48, 256]), 24).unwrap();
        assert!(icon.width >= 24 && icon.width < 48);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
    }

    #[test]
    fn falls_back_to_the_largest_when_all_are_small() {
        let icon = from_ico(&ico_with(&[16, 32]), 64).unwrap();
        assert!(icon.width > 16);
    }

    #[test]
    fn display_icon_values_are_split_into_file_and_index() {
        let (file, index) = parse_spec(r#""C:\Program Files\App, Inc\app.exe",-101"#);
        assert!(file.to_string_lossy().ends_with("app.exe"));
        assert!(!file.to_string_lossy().contains('"'));
        assert!(index < 0);
        let (file, index) = parse_spec(r"C:\Program Files\App, Inc\app.ico");
        assert!(file.to_string_lossy().contains("App, Inc"));
        assert_eq!(index, 0);
    }

    #[test]
    fn the_program_of_a_command_line_keeps_spaces_and_drops_arguments() {
        let quoted = program_of_command(r#""C:\Program Files\App\app.exe" --minimized"#);
        assert_eq!(quoted.as_deref(), Some(r"C:\Program Files\App\app.exe"));
        let bare = program_of_command(r"C:\Program Files\App\App.EXE /background");
        assert_eq!(bare.as_deref(), Some(r"C:\Program Files\App\App.EXE"));
        assert!(program_of_command("   ").is_none());
    }

    #[test]
    fn unreadable_or_foreign_files_give_no_icon() {
        assert!(load("/no/such/file.exe,0", 32).is_none());
        assert!(from_ico(b"not an icon", 32).is_none());
        assert!(from_pe(b"MZ but not really", 0, 32).is_none());
    }
}
