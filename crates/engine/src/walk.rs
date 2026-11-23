//! Recursive measuring and deleting that never follows symbolic links or junctions, so a link
//! inside a cache folder can never lead the cleaner into the rest of the disk.

use crate::Task;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

/// Which files count. Directories always count as containers.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Only files last modified before this moment.
    pub modified_before: Option<SystemTime>,
    /// Only files with one of these extensions (lowercase, no dot). Empty means every file.
    pub extensions: &'static [&'static str],
}

impl Filter {
    fn accepts(&self, path: &Path, modified: Option<SystemTime>) -> bool {
        if let Some(limit) = self.modified_before {
            // A file whose time cannot be read is treated as new: keep it.
            match modified {
                Some(m) if m < limit => {}
                _ => return false,
            }
        }
        if !self.extensions.is_empty() {
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !self.extensions.contains(&ext.as_str()) {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Tally {
    pub files: u64,
    pub bytes: u64,
    /// Files that matched but could not be read or deleted (in use, access denied).
    pub failed: u64,
    /// Newest modification time among the files seen.
    pub newest: Option<SystemTime>,
}

impl Tally {
    fn merge(&mut self, other: Tally) {
        self.files += other.files;
        self.bytes += other.bytes;
        self.failed += other.failed;
        self.newest = newest(self.newest, other.newest);
    }
}

fn newest(a: Option<SystemTime>, b: Option<SystemTime>) -> Option<SystemTime> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Measure,
    Delete,
}

/// Counts the files under `path` (or `path` itself if it is a file) that pass `filter`.
pub fn measure(path: &Path, filter: &Filter, task: &Task) -> Tally {
    visit(path, filter, task, Mode::Measure)
}

/// Deletes the files under `path` (or `path` itself) that pass `filter`, then removes the
/// directories that became empty, including `path`. Returns what was deleted.
pub fn delete(path: &Path, filter: &Filter, task: &Task) -> Tally {
    visit(path, filter, task, Mode::Delete)
}

fn visit(path: &Path, filter: &Filter, task: &Task, mode: Mode) -> Tally {
    let mut tally = Tally::default();
    let Ok(meta) = fs::symlink_metadata(path) else {
        return tally;
    };
    let kind = meta.file_type();
    if kind.is_symlink() {
        return tally;
    }
    if kind.is_file() {
        let modified = meta.modified().ok();
        if filter.accepts(path, modified) {
            match mode {
                Mode::Measure => {
                    tally.files = 1;
                    tally.bytes = meta.len();
                    tally.newest = modified;
                    task.add(1, 0);
                }
                Mode::Delete => {
                    if remove_file(path) {
                        tally.files = 1;
                        tally.bytes = meta.len();
                        task.add(1, meta.len());
                    } else {
                        tally.failed = 1;
                    }
                }
            }
        }
        return tally;
    }
    if !kind.is_dir() {
        return tally;
    }
    let Ok(entries) = fs::read_dir(path) else {
        tally.failed += 1;
        return tally;
    };
    for entry in entries.flatten() {
        if task.is_cancelled() {
            break;
        }
        tally.merge(visit(&entry.path(), filter, task, mode));
    }
    if mode == Mode::Delete {
        // Fails harmlessly while the folder still holds something.
        let _ = fs::remove_dir(path);
    }
    tally
}

/// A file that is open in another program or read-only stays, and is counted as failed.
fn remove_file(path: &Path) -> bool {
    fs::remove_file(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn file(path: &Path, bytes: usize, age_days: u64) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, vec![1u8; bytes]).unwrap();
        let when = SystemTime::now() - std::time::Duration::from_secs(age_days * 86_400);
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn deleting_with_an_age_limit_keeps_recent_files_and_their_folders() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("cache");
        file(&target.join("old/a.bin"), 100, 10);
        file(&target.join("mixed/old.bin"), 100, 10);
        file(&target.join("mixed/new.bin"), 100, 0);
        let filter = Filter {
            modified_before: Some(SystemTime::now() - std::time::Duration::from_secs(86_400)),
            extensions: &[],
        };
        let measured = measure(&target, &filter, &Task::new());
        let deleted = delete(&target, &filter, &Task::new());
        assert_eq!(deleted.files, measured.files);
        assert!(
            !target.join("old").exists(),
            "emptied folder should be removed"
        );
        assert!(target.join("mixed/new.bin").exists());
        assert!(!target.join("mixed/old.bin").exists());
    }

    #[test]
    fn extension_filter_leaves_other_files() {
        let dir = tempfile::tempdir().unwrap();
        file(&dir.path().join("setup.exe"), 10, 0);
        file(&dir.path().join("photo.jpg"), 10, 0);
        let filter = Filter {
            modified_before: None,
            extensions: &["exe"],
        };
        delete(dir.path(), &filter, &Task::new());
        assert!(!dir.path().join("setup.exe").exists());
        assert!(dir.path().join("photo.jpg").exists());
    }

    #[cfg(unix)]
    #[test]
    fn deleting_never_reaches_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        file(&outside.path().join("precious.txt"), 10, 30);
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        delete(dir.path(), &Filter::default(), &Task::new());
        assert!(outside.path().join("precious.txt").exists());
    }
}
