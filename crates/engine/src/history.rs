//! What changed since the last scan of the same folder.
//!
//! After every complete scan a small summary is kept: the size of each folder large enough to
//! matter, nothing about files. The next scan of the same folder is compared with it and names
//! the folders that grew or shrank the most, as deep as the change goes (the cache folder that
//! grew, not every folder above it). Summaries stay on this computer, in the app's own data
//! folder (`rewind-os/scans`), the last few per scanned folder.

use crate::scan::{NodeId, ScanTree};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The app's own folder inside the system's data folder.
pub const APP_FOLDER: &str = "rewind-os";
/// Summaries kept per scanned folder.
const KEEP: usize = 12;
/// Folders kept per summary, largest first.
const MAX_FOLDERS: usize = 50_000;
/// Folders listed as grown, and as shrunk.
const LISTED: usize = 10;
/// A child that explains this many tenths of its parent's change stands for it.
const EXPLAINS_TENTHS: i128 = 7;

/// English reasons; translate with `rewind_os_i18n::tr`.
pub const FIRST_SCAN: &str =
    "This is the first scan of this folder; what changed shows from the next one.";
pub const MEASURED_OTHERWISE: &str = "The last scan of this folder was measured another way (from the master file table or folder by folder); what changed shows from the next scan.";

/// Where summaries are kept: `<data>/rewind-os/scans`.
pub fn folder() -> Option<PathBuf> {
    rewind_os_platform::paths::Known::Data
        .path()
        .map(|data| data.join(APP_FOLDER).join("scans"))
}

/// One scan, in short.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub taken: DateTime<Utc>,
    pub bytes: u64,
    pub from_file_table: bool,
    /// Folders kept: below this size a folder is left out, and counts as "less than this".
    pub min_bytes: u64,
    /// Path below the root, names joined with `/` (which no system allows in a name), and size.
    pub folders: Vec<(String, u64)>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Stored {
    root: PathBuf,
    snapshots: Vec<Snapshot>,
}

/// A folder whose size changed.
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub path: PathBuf,
    /// Names below the scanned root, to find it in the tree.
    #[serde(skip)]
    pub names: Vec<String>,
    pub before: u64,
    pub after: u64,
}

impl Change {
    pub fn delta(&self) -> i128 {
        i128::from(self.after) - i128::from(self.before)
    }
}

/// How this scan compares with the last one.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Comparison {
    /// Nothing to compare with; the reason is English text.
    Nothing { reason: &'static str },
    Changes {
        since: DateTime<Utc>,
        before: u64,
        after: u64,
        /// Folders below this size were not kept: a `before` or `after` of 0 means "less".
        min_bytes: u64,
        grew: Vec<Change>,
        shrank: Vec<Change>,
    },
}

impl ScanTree {
    /// The folder at `names` below the root.
    pub fn find(&self, names: &[String]) -> Option<NodeId> {
        let mut id = ScanTree::ROOT;
        for name in names {
            id = self
                .children(id)
                .find(|(_, n)| &*n.name == name.as_str())
                .map(|(cid, _)| cid)?;
        }
        Some(id)
    }
}

/// Summarizes `tree`.
pub fn snapshot(tree: &ScanTree, taken: DateTime<Utc>) -> Snapshot {
    let total = tree.root_node().map(|r| r.bytes).unwrap_or(0);
    let min_bytes = (total / 10_000).max(1 << 20);
    let mut folders = Vec::new();
    let mut stack: Vec<(NodeId, String)> = vec![(ScanTree::ROOT, String::new())];
    while let Some((id, path)) = stack.pop() {
        for (cid, child) in tree.children(id) {
            // Children come largest first: the rest are smaller still.
            if child.bytes < min_bytes {
                break;
            }
            let rel = if path.is_empty() {
                child.name.to_string()
            } else {
                format!("{path}/{}", child.name)
            };
            folders.push((rel.clone(), child.bytes));
            stack.push((cid, rel));
        }
    }
    folders.sort_by_key(|(_, b)| std::cmp::Reverse(*b));
    folders.truncate(MAX_FOLDERS);
    Snapshot {
        taken,
        bytes: total,
        from_file_table: tree.from_file_table,
        min_bytes,
        folders,
    }
}

/// The folders that grew and shrank most from `old` to `new`, each as deep as the change goes.
pub fn compare(root: &Path, old: &Snapshot, new: &Snapshot) -> Comparison {
    let floor = old.min_bytes.max(new.min_bytes);
    let mut sizes: HashMap<&str, (u64, u64)> = HashMap::new();
    for (path, bytes) in &old.folders {
        sizes.entry(path).or_default().0 = *bytes;
    }
    for (path, bytes) in &new.folders {
        sizes.entry(path).or_default().1 = *bytes;
    }
    // Changes smaller than this are noise: a folder left out counts as anything below `floor`.
    let worth = (floor * 4).max(new.bytes.max(old.bytes) / 1_000);
    let delta = |path: &str| {
        sizes
            .get(path)
            .map(|(b, a)| i128::from(*a) - i128::from(*b))
            .unwrap_or(0)
    };
    let mut kids: HashMap<&str, Vec<&str>> = HashMap::new();
    for path in sizes.keys() {
        let parent = path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        kids.entry(parent).or_default().push(path);
    }
    let pick = |sign: i128| -> Vec<Change> {
        let mut candidates: Vec<(&str, i128)> = sizes
            .keys()
            .map(|p| (*p, delta(p) * sign))
            .filter(|(_, d)| *d >= i128::from(worth))
            .filter(|(p, d)| {
                // Shown only when no single child explains most of the change.
                kids.get(p)
                    .into_iter()
                    .flatten()
                    .all(|k| delta(k) * sign * 10 < *d * EXPLAINS_TENTHS)
            })
            .collect();
        candidates.sort_by_key(|(p, d)| (std::cmp::Reverse(*d), *p));
        let mut chosen: Vec<&str> = Vec::new();
        for (path, _) in candidates {
            let related = chosen
                .iter()
                .any(|c| path.starts_with(&format!("{c}/")) || c.starts_with(&format!("{path}/")));
            if !related {
                chosen.push(path);
            }
            if chosen.len() == LISTED {
                break;
            }
        }
        chosen
            .into_iter()
            .map(|rel| {
                let (before, after) = sizes.get(rel).copied().unwrap_or_default();
                let names: Vec<String> = rel.split('/').map(str::to_owned).collect();
                let mut path = root.to_path_buf();
                path.extend(&names);
                Change {
                    path,
                    names,
                    before,
                    after,
                }
            })
            .collect()
    };
    Comparison::Changes {
        since: old.taken,
        before: old.bytes,
        after: new.bytes,
        min_bytes: floor,
        grew: pick(1),
        shrank: pick(-1),
    }
}

fn file_for(dir: &Path, root: &Path) -> PathBuf {
    let key = root.to_string_lossy();
    let key = if cfg!(windows) {
        key.to_lowercase()
    } else {
        key.into_owned()
    };
    let hash = blake3::hash(key.as_bytes()).to_hex();
    dir.join(format!("{}.json", hash.get(..24).unwrap_or(hash.as_str())))
}

/// Compares `tree` with the last summary of the same folder kept in `dir`, then keeps its own.
/// A stopped scan is neither compared nor kept. A summary that cannot be read or written only
/// loses the comparison: the scan itself stands.
pub fn record(tree: &ScanTree, dir: &Path, now: DateTime<Utc>) -> Option<Comparison> {
    if tree.cancelled {
        return None;
    }
    let file = file_for(dir, &tree.root);
    let mut stored = fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        .filter(|s| s.root == tree.root)
        .unwrap_or(Stored {
            root: tree.root.clone(),
            snapshots: Vec::new(),
        });
    let new = snapshot(tree, now);
    // The last summary measured the same way.
    let comparison = match stored.snapshots.last() {
        None => Comparison::Nothing { reason: FIRST_SCAN },
        Some(old) if old.from_file_table != new.from_file_table => Comparison::Nothing {
            reason: MEASURED_OTHERWISE,
        },
        Some(old) => compare(&tree.root, old, &new),
    };
    stored.snapshots.push(new);
    let extra = stored.snapshots.len().saturating_sub(KEEP);
    stored.snapshots.drain(..extra);
    if let Ok(json) = serde_json::to_vec(&stored)
        && fs::create_dir_all(dir).is_ok()
    {
        // Written beside, then moved over: a crash never leaves half a file.
        let partial = file.with_extension("json.partial");
        if fs::write(&partial, json).is_ok() {
            let _ = fs::rename(&partial, &file);
        }
    }
    Some(comparison)
}
