//! File manager and recycle bin.

use crate::{PlatformError, Result, process};
use std::path::{Path, PathBuf};

/// Opens the file manager with `path` selected (or opened, for a folder).
pub fn reveal(path: &Path) -> Result<()> {
    if cfg!(windows) {
        let arg = format!("/select,{}", path.display());
        // explorer.exe returns 1 even when it works, so only a failure to start counts.
        process::command("explorer.exe")
            .arg(arg)
            .spawn()
            .map(|_| ())
            .map_err(|source| PlatformError::Spawn {
                program: "explorer.exe".into(),
                source,
            })
    } else {
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        process::command("xdg-open")
            .arg(dir)
            .spawn()
            .map(|_| ())
            .map_err(|source| PlatformError::Spawn {
                program: "xdg-open".into(),
                source,
            })
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
    #[cfg(windows)]
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
    #[cfg(not(windows))]
    Err(PlatformError::Unsupported("recycle bin size"))
}

/// Permanently deletes everything in the recycle bin. Returns how many items were removed.
pub fn empty_recycle_bin() -> Result<u64> {
    #[cfg(windows)]
    {
        let items = trash::os_limited::list().map_err(|e| PlatformError::Trash(e.to_string()))?;
        let count = items.len() as u64;
        if count > 0 {
            trash::os_limited::purge_all(items).map_err(|e| PlatformError::Trash(e.to_string()))?;
        }
        Ok(count)
    }
    #[cfg(not(windows))]
    Err(PlatformError::Unsupported("empty recycle bin"))
}
