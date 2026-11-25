//! Duplicate files: files with the same size, then the same first block, then the same whole
//! contents (BLAKE3). Hard links to one file are one file, not duplicates: removing a link
//! frees nothing. Cloud placeholders are skipped, because reading them would download them, and
//! so are the operating system's folders, where nothing may be removed by hand, and the
//! folders of programs and projects (Program Files, AppData, `.git`, `node_modules`, `.app`
//! bundles), where each copy is needed where it is: removing one breaks that program.

use crate::{EngineError, Result, Task, safety};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use same_file::Handle;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Smaller files are not compared unless asked: they free little and are many.
pub const DEFAULT_MIN_BYTES: u64 = 1024 * 1024;
/// How much of each file the first comparison reads.
const FIRST_BLOCK: u64 = 16 * 1024;
const BUFFER: usize = 64 * 1024;

pub const SYSTEM_FOLDER: &str =
    "Duplicates are not searched inside the operating system's folders.";
pub const PROGRAM_FOLDER: &str = "Duplicates are not searched inside program folders: each program needs its own copy of its files.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Copy {
    pub path: PathBuf,
    pub modified: Option<DateTime<Utc>>,
}

/// Files with identical contents.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateSet {
    /// Size of each copy.
    pub bytes: u64,
    /// Oldest first, which is usually the original.
    pub copies: Vec<Copy>,
}

impl DuplicateSet {
    /// Space freed by keeping one copy.
    pub fn reclaimable(&self) -> u64 {
        self.bytes
            .saturating_mul(self.copies.len().saturating_sub(1) as u64)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicateReport {
    pub root: PathBuf,
    pub min_bytes: u64,
    /// Most space to free first.
    pub sets: Vec<DuplicateSet>,
    pub files_seen: u64,
    /// Files and folders that could not be read.
    pub unreadable: u64,
    /// Files only in the cloud, not compared.
    pub cloud_skipped: u64,
    /// Folders of programs and projects that were not searched.
    pub program_folders_skipped: u64,
    /// Stopped before the end: some duplicates may be missing.
    pub cancelled: bool,
}

impl DuplicateReport {
    pub fn reclaimable(&self) -> u64 {
        self.sets.iter().map(DuplicateSet::reclaimable).sum()
    }

    /// Every copy except the oldest of each set: a choice that always keeps one copy.
    pub fn all_but_oldest(&self) -> Vec<PathBuf> {
        self.sets
            .iter()
            .flat_map(|s| s.copies.iter().skip(1).map(|c| c.path.clone()))
            .collect()
    }
}

/// Where a search starts unless the person picks another folder: their home folder, where
/// their own copies live.
pub fn default_root() -> Option<PathBuf> {
    rewind_os_platform::paths::Known::Home.path()
}

/// True when `chosen` leaves at least one copy of every set it touches.
pub fn keeps_a_copy(sets: &[DuplicateSet], chosen: &HashSet<PathBuf>) -> bool {
    sets.iter()
        .all(|s| s.copies.iter().any(|c| !chosen.contains(&c.path)))
}

struct Candidate {
    path: PathBuf,
    bytes: u64,
    modified: Option<SystemTime>,
}

#[derive(Default)]
struct Collected {
    files: Vec<Candidate>,
    seen: u64,
    unreadable: u64,
    cloud: u64,
    programs: u64,
}

impl Collected {
    fn merge(&mut self, other: Collected) {
        self.files.extend(other.files);
        self.seen += other.seen;
        self.unreadable += other.unreadable;
        self.cloud += other.cloud;
        self.programs += other.programs;
    }
}

/// Finds files under `root` with identical contents, of at least `min_bytes` each.
pub fn find(root: &Path, min_bytes: u64, task: &Task) -> Result<DuplicateReport> {
    let meta = fs::symlink_metadata(root).map_err(|source| EngineError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    if !meta.is_dir() {
        return Err(EngineError::NotFound(root.display().to_string()));
    }
    if in_system_zone(root) {
        return Err(EngineError::Refused(SYSTEM_FOLDER));
    }
    if safety::Guard::current().is_program_folder(root) {
        return Err(EngineError::Refused(PROGRAM_FOLDER));
    }
    let min_bytes = min_bytes.max(1);
    task.set_stage("Looking for files");
    let collected = collect(root, min_bytes, task);

    let mut by_size: HashMap<u64, Vec<Candidate>> = HashMap::new();
    for c in collected.files {
        by_size.entry(c.bytes).or_default().push(c);
    }
    let groups: Vec<Vec<Candidate>> = by_size.into_values().filter(|g| g.len() > 1).collect();

    task.set_stage("Comparing files");
    let compared: Vec<(Vec<DuplicateSet>, u64)> = groups
        .into_par_iter()
        .map(|group| compare(group, task))
        .collect();
    let mut sets = Vec::new();
    let mut unreadable = collected.unreadable;
    for (found, failed) in compared {
        sets.extend(found);
        unreadable += failed;
    }
    sets.sort_by(|a, b| {
        b.reclaimable().cmp(&a.reclaimable()).then_with(|| {
            a.copies
                .first()
                .map(|c| &c.path)
                .cmp(&b.copies.first().map(|c| &c.path))
        })
    });
    Ok(DuplicateReport {
        root: root.to_path_buf(),
        min_bytes,
        sets,
        files_seen: collected.seen,
        unreadable,
        cloud_skipped: collected.cloud,
        program_folders_skipped: collected.programs,
        cancelled: task.is_cancelled(),
    })
}

/// Everything inside an operating system folder is refused for removal by hand, so there is no
/// point reading it.
fn in_system_zone(dir: &Path) -> bool {
    dir.parent().is_some() && safety::removal_refusal(dir) == Some(safety::SYSTEM)
}

fn collect(dir: &Path, min_bytes: u64, task: &Task) -> Collected {
    let mut out = Collected::default();
    if task.is_cancelled() {
        return out;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        out.unreadable += 1;
        return out;
    };
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            out.unreadable += 1;
            continue;
        };
        let kind = meta.file_type();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if safety::Guard::current().is_program_folder(&path) {
                out.programs += 1;
            } else {
                subdirs.push(path);
            }
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        out.seen += 1;
        if meta.len() < min_bytes {
            continue;
        }
        if rewind_os_platform::files::is_cloud_placeholder(&meta) {
            out.cloud += 1;
            continue;
        }
        out.files.push(Candidate {
            path,
            bytes: meta.len(),
            modified: meta.modified().ok(),
        });
    }
    task.add(out.seen, 0);
    let below: Vec<Collected> = subdirs
        .par_iter()
        .filter(|d| !in_system_zone(d))
        .map(|d| collect(d, min_bytes, task))
        .collect();
    for b in below {
        out.merge(b);
    }
    out
}

/// Splits files of one size into sets of identical files. Also returns how many could not be
/// read.
fn compare(group: Vec<Candidate>, task: &Task) -> (Vec<DuplicateSet>, u64) {
    let mut failed = 0;
    let bytes = group.first().map(|c| c.bytes).unwrap_or(0);

    // One entry per file, not per link.
    let mut seen = HashSet::new();
    let mut distinct = Vec::new();
    for c in group {
        match Handle::from_path(&c.path) {
            Ok(handle) => {
                if seen.insert(handle) {
                    distinct.push(c);
                }
            }
            Err(_) => failed += 1,
        }
    }
    drop(seen);
    if distinct.len() < 2 {
        return (Vec::new(), failed);
    }

    let mut sets = Vec::new();
    for first in split(distinct, Some(FIRST_BLOCK), task, &mut failed) {
        let whole = if bytes <= FIRST_BLOCK {
            vec![first]
        } else {
            split(first, None, task, &mut failed)
        };
        for same in whole {
            let mut copies: Vec<(Option<SystemTime>, Copy)> = same
                .into_iter()
                .map(|c| {
                    (
                        c.modified,
                        Copy {
                            path: c.path,
                            modified: c.modified.map(DateTime::<Utc>::from),
                        },
                    )
                })
                .collect();
            copies.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.path.cmp(&b.1.path)));
            sets.push(DuplicateSet {
                bytes,
                copies: copies.into_iter().map(|(_, c)| c).collect(),
            });
        }
    }
    (sets, failed)
}

/// Groups `files` by the hash of their first `limit` bytes (all of them when `None`), keeping
/// only groups of two or more.
fn split(
    files: Vec<Candidate>,
    limit: Option<u64>,
    task: &Task,
    failed: &mut u64,
) -> Vec<Vec<Candidate>> {
    let mut by_hash: HashMap<[u8; 32], Vec<Candidate>> = HashMap::new();
    for c in files {
        if task.is_cancelled() {
            return Vec::new();
        }
        match hash(&c.path, limit, task) {
            Some(h) => by_hash.entry(h).or_default().push(c),
            None if task.is_cancelled() => return Vec::new(),
            None => *failed += 1,
        }
    }
    by_hash.into_values().filter(|g| g.len() > 1).collect()
}

fn hash(path: &Path, limit: Option<u64>, task: &Task) -> Option<[u8; 32]> {
    let mut file = File::open(path).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; BUFFER];
    let mut left = limit.unwrap_or(u64::MAX);
    while left > 0 {
        if task.is_cancelled() {
            return None;
        }
        let want = usize::try_from(left).unwrap_or(BUFFER).min(BUFFER);
        let n = match file.read(buffer.get_mut(..want)?) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        if n == 0 {
            break;
        }
        hasher.update(buffer.get(..n)?);
        left -= n as u64;
        task.add(0, n as u64);
    }
    Some(*hasher.finalize().as_bytes())
}
