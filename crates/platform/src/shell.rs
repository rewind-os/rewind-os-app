//! File manager and recycle bin.

use crate::{PlatformError, Result, process};
use std::path::{Path, PathBuf};

/// What the file manager was opened on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revealed {
    /// The path itself, selected (or opened, for a folder).
    Exact,
    /// The path no longer exists; its nearest existing folder was opened instead.
    Nearest(PathBuf),
}

/// Opens the file manager with `path` selected (or opened, for a folder). When `path` no longer
/// exists, opens the nearest folder above it that does, instead of the file manager's default
/// place (explorer.exe falls back to Documents for a path it cannot find).
pub fn reveal(path: &Path) -> Result<Revealed> {
    if std::fs::symlink_metadata(path).is_ok() {
        select(path)?;
        return Ok(Revealed::Exact);
    }
    let folder = nearest_existing(path, |p| p.is_dir()).ok_or_else(|| PlatformError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::from(std::io::ErrorKind::NotFound),
    })?;
    open_folder(&folder)?;
    Ok(Revealed::Nearest(folder))
}

/// The closest folder above `path` for which `is_dir` holds.
fn nearest_existing(path: &Path, is_dir: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|p| !p.as_os_str().is_empty() && is_dir(p))
        .map(Path::to_path_buf)
}

fn spawned(program: &str, result: std::io::Result<std::process::Child>) -> Result<()> {
    result.map(|_| ()).map_err(|source| PlatformError::Spawn {
        program: program.to_owned(),
        source,
    })
}

/// Shows `path`, which exists, selected in the file manager.
fn select(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // explorer.exe parses its own command line and wants `/select,"C:\a b\c"`. Rust's usual
        // quoting wraps the whole argument (`"/select,C:\a b\c"`), which explorer does not
        // understand: it opens Documents instead. Paths cannot contain `"` on Windows.
        // explorer.exe returns 1 even when it works, so only a failure to start counts.
        let arg = format!("/select,\"{}\"", explorer_path(path));
        spawned(
            "explorer.exe",
            process::command("explorer.exe").raw_arg(arg).spawn(),
        )
    }
    #[cfg(not(windows))]
    if cfg!(target_os = "macos") {
        spawned("open", process::command("open").arg("-R").arg(path).spawn())
    } else {
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        open_folder(dir)
    }
}

/// Opens the folder `dir` in the file manager.
fn open_folder(dir: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let arg = format!("\"{}\"", explorer_path(dir));
        spawned(
            "explorer.exe",
            process::command("explorer.exe").raw_arg(arg).spawn(),
        )
    }
    #[cfg(not(windows))]
    {
        let program = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        spawned(program, process::command(program).arg(dir).spawn())
    }
}

/// Opens `url` in the default web browser. Only `https` addresses are opened.
pub fn open_web(url: &str) -> Result<()> {
    if !url.starts_with("https://") {
        return Err(PlatformError::Unsupported("web addresses other than https"));
    }
    #[cfg(windows)]
    {
        spawned(
            "explorer.exe",
            process::command("explorer.exe").arg(url).spawn(),
        )
    }
    #[cfg(not(windows))]
    {
        let program = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        spawned(program, process::command(program).arg(url).spawn())
    }
}

/// The path as explorer.exe understands it: backslashes, without the `\\?\` prefix.
#[cfg(any(windows, test))]
fn explorer_path(path: &Path) -> String {
    let text = path.display().to_string().replace('/', "\\");
    match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned(),
    }
}

/// Result of moving one path to the recycle bin.
#[derive(Debug)]
pub struct TrashOutcome {
    pub path: PathBuf,
    pub error: Option<String>,
}

/// Moves each path to the recycle bin and reports every path separately, so one locked file
/// does not hide what did move.
pub fn move_to_trash(paths: &[PathBuf]) -> Vec<TrashOutcome> {
    paths
        .iter()
        .map(|path| TrashOutcome {
            path: path.clone(),
            error: trash::delete(path).err().map(|e| e.to_string()),
        })
        .collect()
}

/// What the recycle bin holds.
#[derive(Debug, Clone, Copy, Default)]
pub struct BinSummary {
    pub items: u64,
    pub bytes: u64,
}

/// Counts the recycle bin's items and their size.
pub fn recycle_bin_summary() -> Result<BinSummary> {
    #[cfg(any(windows, target_os = "linux"))]
    {
        let items = trash::os_limited::list().map_err(|e| PlatformError::Trash(e.to_string()))?;
        let mut summary = BinSummary {
            items: items.len() as u64,
            bytes: 0,
        };
        for item in &items {
            if let Ok(meta) = trash::os_limited::metadata(item)
                && let trash::TrashItemSize::Bytes(b) = meta.size
            {
                summary.bytes += b;
            }
        }
        Ok(summary)
    }
    #[cfg(target_os = "macos")]
    {
        let bin = mac_trash()?;
        let entries =
            std::fs::read_dir(&bin).map_err(|source| PlatformError::Io { path: bin, source })?;
        let mut summary = BinSummary::default();
        for entry in entries.flatten() {
            summary.items += 1;
            summary.bytes += size_of(&entry.path());
        }
        Ok(summary)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    Err(PlatformError::Unsupported("recycle bin size"))
}

/// Permanently deletes everything in the recycle bin. Returns how many items were removed.
pub fn empty_recycle_bin() -> Result<u64> {
    #[cfg(any(windows, target_os = "linux"))]
    {
        let items = trash::os_limited::list().map_err(|e| PlatformError::Trash(e.to_string()))?;
        let count = items.len() as u64;
        if count > 0 {
            trash::os_limited::purge_all(items).map_err(|e| PlatformError::Trash(e.to_string()))?;
        }
        Ok(count)
    }
    #[cfg(target_os = "macos")]
    {
        let bin = mac_trash()?;
        let entries =
            std::fs::read_dir(&bin).map_err(|source| PlatformError::Io { path: bin, source })?;
        let mut removed = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            let done = match entry.file_type() {
                Ok(t) if t.is_dir() => std::fs::remove_dir_all(&path),
                _ => std::fs::remove_file(&path),
            };
            if done.is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    Err(PlatformError::Unsupported("empty recycle bin"))
}

/// The user's Trash on macOS. Reading it may need Full Disk Access; the error says so.
#[cfg(target_os = "macos")]
fn mac_trash() -> Result<PathBuf> {
    crate::paths::Known::Home
        .path()
        .map(|h| h.join(".Trash"))
        .ok_or(PlatformError::Unsupported("home folder not set"))
}

/// Total size below `path`, never following links.
#[cfg(target_os = "macos")]
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    std::fs::read_dir(path)
        .map(|rd| rd.flatten().map(|e| size_of(&e.path())).sum())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{explorer_path, nearest_existing};
    use std::path::Path;

    #[test]
    fn explorer_gets_a_plain_path() {
        assert_eq!(
            explorer_path(Path::new(r"\\?\C:\Program Files\App")),
            r"C:\Program Files\App"
        );
        assert_eq!(
            explorer_path(Path::new(r"\\?\UNC\server\share\f")),
            r"\\server\share\f"
        );
        assert_eq!(
            explorer_path(Path::new("C:/Users/me/a b")),
            r"C:\Users\me\a b"
        );
    }

    #[test]
    fn a_missing_path_leads_to_its_nearest_existing_folder() {
        let existing = [Path::new("/home/ana"), Path::new("/home"), Path::new("/")];
        let is_dir = |p: &Path| existing.contains(&p);
        assert_eq!(
            nearest_existing(Path::new("/home/ana/gone/deeper/file.txt"), is_dir),
            Some(Path::new("/home/ana").to_path_buf())
        );
        // Never the path itself, even when it is a folder: it is the one that went missing.
        assert_eq!(
            nearest_existing(Path::new("/home/ana"), is_dir),
            Some(Path::new("/home").to_path_buf())
        );
        assert_eq!(nearest_existing(Path::new("gone"), |_| false), None);
    }
}
