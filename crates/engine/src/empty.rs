//! Empty folders: folders with nothing in them, or only other empty folders. Found from a disk
//! scan (folders without files below them), then checked on the disk, since the scan does not
//! count links or the second names of hard-linked files. Only folders a person may remove are
//! offered: not the system's, not a program's or a project's, not hidden ones (`.name`), and
//! not the known folders (Documents, Downloads…), which the guard refuses.
//!
//! Removal (`files.trash_empty`) checks each folder again, right before it goes to the recycle
//! bin, so a folder that got something in the meantime is left alone.

use crate::scan::{NodeId, ScanTree};
use crate::{Task, TrashResult, safety};
use std::fs;
use std::path::{Path, PathBuf};

/// Folders are offered up to this many; the scan's tree may hold far more.
pub const MOST: usize = 1000;

/// Deeper than this is treated as not empty, rather than walked further.
const DEEPEST: usize = 64;

/// English reason; translate with `rewind_os_i18n::tr`.
pub const NOT_EMPTY: &str = "Something was added to this folder since the scan; it was left alone.";

/// True when `path` is a real folder (not a link or junction) holding nothing but other empty
/// folders. Anything that cannot be read counts as not empty.
pub fn is_empty_folder(path: &Path) -> bool {
    empty_below(path, 0)
}

fn empty_below(path: &Path, depth: usize) -> bool {
    if depth > DEEPEST {
        return false;
    }
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    // Links and junctions are not folders here: their file type is neither.
    if !meta.file_type().is_dir() {
        return false;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let Ok(kind) = entry.file_type() else {
            return false;
        };
        // A file, a link or anything else: not empty.
        if !kind.is_dir() || !empty_below(&entry.path(), depth + 1) {
            return false;
        }
    }
    true
}

fn hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// True when no folder below `id` is hidden or a program's or project's own.
fn plain_below(tree: &ScanTree, id: NodeId, guard: &safety::Guard) -> bool {
    tree.children(id).all(|(child, node)| {
        !hidden(&node.name)
            && !guard.is_program_folder(&tree.path(child))
            && plain_below(tree, child, guard)
    })
}

/// The highest empty folders below the scan's root (an empty folder's empty children are not
/// listed again), that a person may remove, checked on the disk. Sorted by path.
pub fn find(tree: &ScanTree) -> Vec<PathBuf> {
    let guard = safety::Guard::current();
    let mut found = Vec::new();
    let mut stack: Vec<NodeId> = vec![ScanTree::ROOT];
    while let Some(id) = stack.pop() {
        for (child, node) in tree.children(id) {
            if hidden(&node.name) || node.incomplete {
                continue;
            }
            if node.files > 0 {
                stack.push(child);
                continue;
            }
            let path = tree.path(child);
            // A folder holding a hidden, program or project folder is not offered whole; the
            // ones beside it still may be.
            if !plain_below(tree, child, guard) {
                stack.push(child);
                continue;
            }
            if guard.removal_refusal(&path).is_none()
                && !guard.is_program_folder(&path)
                && is_empty_folder(&path)
            {
                found.push(path);
                if found.len() >= MOST {
                    found.sort();
                    return found;
                }
            }
        }
    }
    found.sort();
    found
}

/// Moves `paths` to the recycle bin, each only if it is still an empty folder.
pub fn trash(paths: &[PathBuf], task: &Task) -> Vec<TrashResult> {
    let (empty, changed): (Vec<PathBuf>, Vec<PathBuf>) =
        paths.iter().cloned().partition(|p| is_empty_folder(p));
    let mut results = crate::trash(&empty, task);
    results.extend(changed.into_iter().map(|path| TrashResult {
        path,
        bytes: 0,
        error: Some(NOT_EMPTY.to_owned()),
    }));
    results
}
