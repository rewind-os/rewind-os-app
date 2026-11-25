//! Disk analyzer: measures every folder under a root in parallel.
//!
//! To stay small in memory the tree keeps folders only, plus the largest files overall and a
//! size per extension. The files of one folder are listed on demand with [`list_files`].

use crate::{EngineError, Result, Task};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Serialize, Serializer};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

/// How many of the largest files a scan keeps.
pub const TOP_FILES: usize = 500;
/// How many extensions a scan reports.
const TOP_EXTENSIONS: usize = 40;

pub type NodeId = u32;

#[derive(Debug, Clone, Serialize)]
pub struct DirNode {
    pub name: Box<str>,
    /// Total size of everything below, in bytes (logical size, not size on disk).
    pub bytes: u64,
    /// Space everything below takes on the disk: less than `bytes` for compressed, sparse and
    /// cloud-only files, a little more where files fill whole blocks.
    pub disk_bytes: u64,
    /// Files below, at any depth.
    pub files: u64,
    /// Size of the files directly inside this folder.
    pub own_bytes: u64,
    pub own_files: u64,
    pub modified: Option<DateTime<Utc>>,
    /// The folder or one below it could not be read.
    pub incomplete: bool,
    pub parent: Option<NodeId>,
    first_child: NodeId,
    child_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub path: PathBuf,
    pub bytes: u64,
    /// Space the file takes on the disk.
    pub disk_bytes: u64,
    pub modified: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtensionStat {
    /// Lowercase, without the dot; empty for files without an extension.
    pub extension: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ScanTree {
    pub root: PathBuf,
    nodes: Vec<DirNode>,
    pub top_files: Vec<FileEntry>,
    pub extensions: Vec<ExtensionStat>,
    /// Folders that could not be read (usually access denied).
    pub unreadable_dirs: u64,
    pub elapsed_ms: u64,
    /// The scan was stopped before the end; sizes are lower bounds.
    pub cancelled: bool,
    /// Size of the extra names of hard-linked files, counted once. The folder that holds the
    /// first name found gets the size.
    pub hard_linked_bytes: u64,
}

impl ScanTree {
    pub const ROOT: NodeId = 0;

    pub fn node(&self, id: NodeId) -> Option<&DirNode> {
        self.nodes.get(id as usize)
    }

    pub fn root_node(&self) -> Option<&DirNode> {
        self.node(Self::ROOT)
    }

    pub fn dir_count(&self) -> usize {
        self.nodes.len()
    }

    /// Children of `id`, largest first.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = (NodeId, &DirNode)> {
        let (start, len) = self
            .node(id)
            .map(|n| (n.first_child, n.child_count))
            .unwrap_or((0, 0));
        (start..start + len).filter_map(move |cid| self.node(cid).map(|n| (cid, n)))
    }

    /// Full path of `id`.
    pub fn path(&self, id: NodeId) -> PathBuf {
        let mut names = Vec::new();
        let mut current = Some(id);
        while let Some(cid) = current {
            let Some(node) = self.node(cid) else { break };
            if node.parent.is_none() {
                break;
            }
            names.push(&*node.name);
            current = node.parent;
        }
        let mut path = self.root.clone();
        for name in names.iter().rev() {
            path.push(name);
        }
        path
    }

    /// Ids from the root down to `id`.
    pub fn ancestry(&self, id: NodeId) -> Vec<NodeId> {
        let mut chain = vec![id];
        let mut current = self.node(id).and_then(|n| n.parent);
        while let Some(pid) = current {
            chain.push(pid);
            current = self.node(pid).and_then(|n| n.parent);
        }
        chain.reverse();
        chain
    }
}

/// Serializes a short summary instead of every folder, for the CLI and automation.
pub fn serialize_summary<S: Serializer>(
    tree: &std::sync::Arc<ScanTree>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Child {
        path: PathBuf,
        bytes: u64,
        disk_bytes: u64,
        files: u64,
        modified: Option<DateTime<Utc>>,
        incomplete: bool,
    }
    #[derive(Serialize)]
    struct Summary<'a> {
        root: &'a Path,
        bytes: u64,
        disk_bytes: u64,
        files: u64,
        dirs: usize,
        unreadable_dirs: u64,
        cancelled: bool,
        elapsed_ms: u64,
        largest_folders: Vec<Child>,
        largest_files: &'a [FileEntry],
        extensions: &'a [ExtensionStat],
    }
    let root = tree.root_node();
    let summary = Summary {
        root: &tree.root,
        bytes: root.map(|r| r.bytes).unwrap_or(0),
        disk_bytes: root.map(|r| r.disk_bytes).unwrap_or(0),
        files: root.map(|r| r.files).unwrap_or(0),
        dirs: tree.dir_count(),
        unreadable_dirs: tree.unreadable_dirs,
        cancelled: tree.cancelled,
        elapsed_ms: tree.elapsed_ms,
        largest_folders: tree
            .children(ScanTree::ROOT)
            .take(50)
            .map(|(id, n)| Child {
                path: tree.path(id),
                bytes: n.bytes,
                disk_bytes: n.disk_bytes,
                files: n.files,
                modified: n.modified,
                incomplete: n.incomplete,
            })
            .collect(),
        largest_files: &tree.top_files[..tree.top_files.len().min(100)],
        extensions: &tree.extensions,
    };
    summary.serialize(serializer)
}

struct RawDir {
    name: Box<str>,
    bytes: u64,
    disk_bytes: u64,
    files: u64,
    own_bytes: u64,
    own_files: u64,
    modified: Option<SystemTime>,
    incomplete: bool,
    children: Vec<RawDir>,
}

/// Keeps the `TOP_FILES` largest files. `floor` lets threads skip the lock for small files.
type Ranked = Reverse<(u64, PathBuf, Option<SystemTime>, u64)>;

struct TopFiles {
    heap: Mutex<BinaryHeap<Ranked>>,
    floor: AtomicU64,
}

impl TopFiles {
    fn offer(
        &self,
        bytes: u64,
        path: impl FnOnce() -> PathBuf,
        modified: Option<SystemTime>,
        disk_bytes: u64,
    ) {
        if bytes <= self.floor.load(Ordering::Relaxed) {
            return;
        }
        let mut heap = self.heap.lock();
        heap.push(Reverse((bytes, path(), modified, disk_bytes)));
        if heap.len() > TOP_FILES {
            heap.pop();
        }
        if heap.len() == TOP_FILES
            && let Some(Reverse((smallest, _, _, _))) = heap.peek()
        {
            self.floor.store(*smallest, Ordering::Relaxed);
        }
    }
}

struct Context<'a> {
    task: &'a Task,
    top: TopFiles,
    extensions: Mutex<HashMap<String, (u64, u64)>>,
    unreadable: AtomicU64,
    /// Hard-linked files already counted.
    linked: Mutex<HashSet<(u64, u64)>>,
    linked_bytes: AtomicU64,
}

/// Scans `root`. Stops early, keeping what it measured, when `task` is cancelled.
pub fn scan(root: &Path, task: &Task) -> Result<ScanTree> {
    let meta = fs::metadata(root).map_err(|source| EngineError::Io {
        path: root.into(),
        source,
    })?;
    if !meta.is_dir() {
        return Err(EngineError::NotFound(root.display().to_string()));
    }
    task.set_stage("Scanning");
    let started = Instant::now();
    let ctx = Context {
        task,
        top: TopFiles {
            heap: Mutex::new(BinaryHeap::new()),
            floor: AtomicU64::new(0),
        },
        extensions: Mutex::new(HashMap::new()),
        unreadable: AtomicU64::new(0),
        linked: Mutex::new(HashSet::new()),
        linked_bytes: AtomicU64::new(0),
    };
    // Deep trees recurse once per level; give the workers room.
    let pool = rayon::ThreadPoolBuilder::new()
        .stack_size(16 * 1024 * 1024)
        .thread_name(|i| format!("scan-{i}"))
        .build()
        .ok();
    let raw = match &pool {
        Some(pool) => pool.install(|| scan_dir(root, root.display().to_string().into(), &ctx)),
        None => scan_dir(root, root.display().to_string().into(), &ctx),
    };
    task.set_stage("Sorting results");
    let nodes = flatten(raw);

    let mut top_files: Vec<FileEntry> = ctx
        .top
        .heap
        .into_inner()
        .into_iter()
        .map(|Reverse((bytes, path, modified, disk_bytes))| FileEntry {
            path,
            bytes,
            disk_bytes,
            modified: modified.map(DateTime::<Utc>::from),
        })
        .collect();
    top_files.sort_by_key(|e| Reverse(e.bytes));

    let mut extensions: Vec<ExtensionStat> = ctx
        .extensions
        .into_inner()
        .into_iter()
        .map(|(extension, (files, bytes))| ExtensionStat {
            extension,
            files,
            bytes,
        })
        .collect();
    extensions.sort_by_key(|e| Reverse(e.bytes));
    extensions.truncate(TOP_EXTENSIONS);

    Ok(ScanTree {
        root: root.to_path_buf(),
        nodes,
        top_files,
        extensions,
        unreadable_dirs: ctx.unreadable.into_inner(),
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        cancelled: task.is_cancelled(),
        hard_linked_bytes: ctx.linked_bytes.into_inner(),
    })
}

fn scan_dir(path: &Path, name: Box<str>, ctx: &Context<'_>) -> RawDir {
    let mut raw = RawDir {
        name,
        bytes: 0,
        disk_bytes: 0,
        files: 0,
        own_bytes: 0,
        own_files: 0,
        modified: None,
        incomplete: false,
        children: Vec::new(),
    };
    if ctx.task.is_cancelled() {
        raw.incomplete = true;
        return raw;
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => {
            ctx.unreadable.fetch_add(1, Ordering::Relaxed);
            raw.incomplete = true;
            return raw;
        }
    };
    let check_links = rewind_os_platform::files::hard_links_expected(path);
    let mut subdirs: Vec<(PathBuf, Box<str>)> = Vec::new();
    let mut local_ext: HashMap<String, (u64, u64)> = HashMap::new();
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // `is_dir` and `is_file` are both false for symbolic links and junctions: skipped.
        if kind.is_dir() {
            subdirs.push((entry.path(), entry.file_name().to_string_lossy().into()));
        } else if kind.is_file() {
            // On Windows this comes from the directory listing itself, without opening the file.
            let Ok(meta) = entry.metadata() else { continue };
            // A second name of a file already counted adds nothing.
            if check_links
                && let Some(id) = rewind_os_platform::files::link_identity(&entry.path(), &meta)
                && !ctx.linked.lock().insert(id)
            {
                ctx.linked_bytes.fetch_add(meta.len(), Ordering::Relaxed);
                continue;
            }
            let bytes = meta.len();
            let path = entry.path();
            let disk = rewind_os_platform::files::size_on_disk(&path, &meta);
            let modified = meta.modified().ok();
            raw.disk_bytes += disk;
            raw.own_bytes += bytes;
            raw.own_files += 1;
            raw.modified = raw.modified.max(modified);
            let file_name = entry.file_name();
            let ext = Path::new(&file_name)
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .filter(|e| e.len() <= 12)
                .unwrap_or_default();
            let slot = local_ext.entry(ext).or_default();
            slot.0 += 1;
            slot.1 += bytes;
            ctx.top.offer(bytes, || path, modified, disk);
        }
    }
    ctx.task.add(raw.own_files, raw.own_bytes);
    if !local_ext.is_empty() {
        let mut global = ctx.extensions.lock();
        for (ext, (files, bytes)) in local_ext {
            let slot = global.entry(ext).or_default();
            slot.0 += files;
            slot.1 += bytes;
        }
    }
    raw.children = subdirs
        .into_par_iter()
        .map(|(child, name)| scan_dir(&child, name, ctx))
        .collect();
    raw.bytes = raw.own_bytes;
    raw.files = raw.own_files;
    for child in &raw.children {
        raw.bytes += child.bytes;
        raw.disk_bytes += child.disk_bytes;
        raw.files += child.files;
        raw.incomplete |= child.incomplete;
        raw.modified = raw.modified.max(child.modified);
    }
    raw
}

/// Lays the tree out breadth first so each folder's children sit next to each other.
fn flatten(root: RawDir) -> Vec<DirNode> {
    fn node(raw: &RawDir, parent: Option<NodeId>) -> DirNode {
        DirNode {
            name: raw.name.clone(),
            bytes: raw.bytes,
            disk_bytes: raw.disk_bytes,
            files: raw.files,
            own_bytes: raw.own_bytes,
            own_files: raw.own_files,
            modified: raw.modified.map(DateTime::<Utc>::from),
            incomplete: raw.incomplete,
            parent,
            first_child: 0,
            child_count: 0,
        }
    }
    let mut nodes = vec![node(&root, None)];
    let mut queue: VecDeque<(usize, RawDir)> = VecDeque::from([(0, root)]);
    while let Some((id, mut raw)) = queue.pop_front() {
        let mut children = std::mem::take(&mut raw.children);
        children.sort_by_key(|e| Reverse(e.bytes));
        let first = nodes.len();
        if let Some(parent) = nodes.get_mut(id) {
            parent.first_child = NodeId::try_from(first).unwrap_or(NodeId::MAX);
            parent.child_count = u32::try_from(children.len()).unwrap_or(u32::MAX);
        }
        let parent_id = NodeId::try_from(id).ok();
        for child in children {
            let cid = nodes.len();
            nodes.push(node(&child, parent_id));
            queue.push_back((cid, child));
        }
    }
    nodes
}

/// Files directly inside `dir`, largest first.
pub fn list_files(dir: &Path) -> Result<Vec<FileEntry>> {
    let entries = fs::read_dir(dir).map_err(|source| EngineError::Io {
        path: dir.into(),
        source,
    })?;
    let mut files: Vec<FileEntry> = entries
        .flatten()
        .filter(|e| e.file_type().map(|k| k.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let path = e.path();
            Some(FileEntry {
                disk_bytes: rewind_os_platform::files::size_on_disk(&path, &meta),
                path,
                bytes: meta.len(),
                modified: meta.modified().ok().map(DateTime::<Utc>::from),
            })
        })
        .collect();
    files.sort_by_key(|e| Reverse(e.bytes));
    Ok(files)
}
