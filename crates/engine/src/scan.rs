//! Disk analyzer: measures every folder under a root in parallel.
//!
//! To stay small in memory the tree keeps folders only, plus the largest files overall and a
//! size per extension. The files of one folder are listed on demand with [`list_files`].

use crate::{EngineError, Result, Task, mft};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Serialize, Serializer};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

/// How many of the largest files a scan keeps.
pub const TOP_FILES: usize = 500;
/// How deep below a drive's root a scan still reads the master file table.
const FILE_TABLE_DEPTH: usize = 1;
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
    /// Measured from the volume's master file table (NTFS, with administrator rights) instead
    /// of folder by folder.
    pub from_file_table: bool,
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

/// One file's figures.
struct Sizes {
    bytes: u64,
    disk: u64,
    modified: Option<SystemTime>,
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

impl<'a> Context<'a> {
    fn new(task: &'a Task) -> Self {
        Context {
            task,
            top: TopFiles {
                heap: Mutex::new(BinaryHeap::new()),
                floor: AtomicU64::new(0),
            },
            extensions: Mutex::new(HashMap::new()),
            unreadable: AtomicU64::new(0),
            linked: Mutex::new(HashSet::new()),
            linked_bytes: AtomicU64::new(0),
        }
    }

    /// Adds the file `name` to `raw`, its folder.
    fn count_file(
        &self,
        raw: &mut RawDir,
        local_ext: &mut HashMap<String, (u64, u64)>,
        name: &std::ffi::OsStr,
        file: Sizes,
        path: impl FnOnce() -> PathBuf,
    ) {
        let Sizes {
            bytes,
            disk,
            modified,
        } = file;
        raw.disk_bytes += disk;
        raw.own_bytes += bytes;
        raw.own_files += 1;
        raw.modified = raw.modified.max(modified);
        let ext = Path::new(name)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .filter(|e| e.len() <= 12)
            .unwrap_or_default();
        let slot = local_ext.entry(ext).or_default();
        slot.0 += 1;
        slot.1 += bytes;
        self.top.offer(bytes, path, modified, disk);
    }

    /// Reports the files of one folder, once its listing is done.
    fn folder_done(&self, raw: &RawDir, local_ext: HashMap<String, (u64, u64)>) {
        self.task.add(raw.own_files, raw.own_bytes);
        if !local_ext.is_empty() {
            let mut global = self.extensions.lock();
            for (ext, (files, bytes)) in local_ext {
                let slot = global.entry(ext).or_default();
                slot.0 += files;
                slot.1 += bytes;
            }
        }
    }

    fn finish(self, raw: RawDir, root: &Path, started: Instant, from_file_table: bool) -> ScanTree {
        self.task.set_stage("Sorting results");
        let nodes = flatten(raw);
        let mut top_files: Vec<FileEntry> = self
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

        let mut extensions: Vec<ExtensionStat> = self
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

        ScanTree {
            root: root.to_path_buf(),
            nodes,
            top_files,
            extensions,
            unreadable_dirs: self.unreadable.into_inner(),
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            cancelled: self.task.is_cancelled(),
            hard_linked_bytes: self.linked_bytes.into_inner(),
            from_file_table,
        }
    }
}

impl RawDir {
    fn new(name: Box<str>) -> Self {
        RawDir {
            name,
            bytes: 0,
            disk_bytes: 0,
            files: 0,
            own_bytes: 0,
            own_files: 0,
            modified: None,
            incomplete: false,
            children: Vec::new(),
        }
    }

    /// Adds the children's totals to this folder's own.
    fn total(&mut self) {
        self.bytes = self.own_bytes;
        self.files = self.own_files;
        for child in &self.children {
            self.bytes += child.bytes;
            self.disk_bytes += child.disk_bytes;
            self.files += child.files;
            self.incomplete |= child.incomplete;
            self.modified = self.modified.max(child.modified);
        }
    }
}

/// Runs `job` on a pool whose threads have room for deep trees (one call per level).
fn with_deep_stacks<T: Send>(job: impl FnOnce() -> T + Send) -> T {
    let pool = rayon::ThreadPoolBuilder::new()
        .stack_size(16 * 1024 * 1024)
        .thread_name(|i| format!("scan-{i}"))
        .build()
        .ok();
    match &pool {
        Some(pool) => pool.install(job),
        None => job(),
    }
}

/// Scans `root`. Stops early, keeping what it measured, when `task` is cancelled.
///
/// On Windows, with administrator rights, a local NTFS drive (or a folder right below its root)
/// is measured from the volume's master file table instead, which is much faster; anything that keeps that from
/// working (another file system, a `subst` drive, a damaged table) falls back to walking the
/// folders.
pub fn scan(root: &Path, task: &Task) -> Result<ScanTree> {
    let meta = fs::metadata(root).map_err(|source| EngineError::Io {
        path: root.into(),
        source,
    })?;
    if !meta.is_dir() {
        return Err(EngineError::NotFound(root.display().to_string()));
    }
    if rewind_os_platform::elevation::is_elevated()
        && let Some(mut volume) = rewind_os_platform::volume::open_raw(root)
        // The whole table is read every time: worth it for a drive or a large folder near its
        // root, not for a small folder deep down.
        && volume.inside.len() <= FILE_TABLE_DEPTH
        && let Ok(tree) = from_volume(&mut volume.file, root, &volume.inside, task, true)
    {
        return Ok(tree);
    }
    task.set_stage("Scanning");
    let started = Instant::now();
    let ctx = Context::new(task);
    let raw = with_deep_stacks(|| scan_dir(root, root.display().to_string().into(), &ctx));
    Ok(ctx.finish(raw, root, started, false))
}

/// Measures `root` from the master file table of the NTFS volume that `volume` covers.
/// `inside` names the folders from the volume's root down to `root`; empty for the root.
pub fn scan_file_table<R: Read + Seek>(
    volume: &mut R,
    root: &Path,
    inside: &[String],
    task: &Task,
) -> Result<ScanTree> {
    from_volume(volume, root, inside, task, false)
}

/// Files and folders by parent folder: (record, which of its names).
type Children = HashMap<u64, Vec<(u64, usize)>>;

fn from_volume<R: Read + Seek>(
    volume: &mut R,
    root: &Path,
    inside: &[String],
    task: &Task,
    check_listing: bool,
) -> Result<ScanTree> {
    let started = Instant::now();
    task.set_stage("Reading the master file table");
    let table = mft::read(volume, task).map_err(|source| EngineError::Io {
        path: root.into(),
        source,
    })?;
    from_table(&table, root, inside, task, check_listing, started)
}

fn from_table(
    table: &mft::FileTable,
    root: &Path,
    inside: &[String],
    task: &Task,
    check_listing: bool,
    started: Instant,
) -> Result<ScanTree> {
    task.set_stage("Scanning");
    let entries = &table.entries;
    if !entries.contains_key(&mft::ROOT) {
        return Err(EngineError::NotFound(root.display().to_string()));
    }

    let mut children: Children = HashMap::new();
    // Folders that hold something but whose own record could not be read.
    let mut lost: HashSet<u64> = HashSet::new();
    for (&number, entry) in entries {
        if number < mft::FIRST_USER_RECORD {
            continue;
        }
        for (i, (parent, _)) in entry.names.iter().enumerate() {
            let in_metadata = *parent < mft::FIRST_USER_RECORD && *parent != mft::ROOT;
            if *parent == number || in_metadata {
                continue;
            }
            if entries.get(parent).is_some_and(|p| p.dir) {
                children.entry(*parent).or_default().push((number, i));
            } else {
                lost.insert(*parent);
            }
        }
    }
    let name_of = |(number, i): &(u64, usize)| {
        entries
            .get(number)
            .and_then(|e| e.names.get(*i))
            .map(|(_, name)| &**name)
    };
    let is_dir = |number: &u64| entries.get(number).is_some_and(|e| e.dir);

    let mut start = mft::ROOT;
    for name in inside {
        let wanted = name.to_lowercase();
        start = children
            .get(&start)
            .and_then(|kids| {
                kids.iter().find(|kid| {
                    is_dir(&kid.0) && name_of(kid).is_some_and(|n| n.to_lowercase() == wanted)
                })
            })
            .map(|kid| kid.0)
            .ok_or_else(|| EngineError::NotFound(root.display().to_string()))?;
    }
    if check_listing && !listing_matches(root, children.get(&start), name_of) {
        return Err(EngineError::NotFound(root.display().to_string()));
    }

    let ctx = Context::new(task);
    ctx.unreadable.store(lost.len() as u64, Ordering::Relaxed);
    let tables = Tables {
        entries,
        children: &children,
    };
    let mut raw = with_deep_stacks(|| {
        let mut seen = HashSet::from([start]);
        build(
            &tables,
            start,
            root.display().to_string().into(),
            root,
            &ctx,
            &mut seen,
        )
    });
    raw.incomplete |= !lost.is_empty() || table.damaged > 0;
    Ok(ctx.finish(raw, root, started, true))
}

/// Whether the files and folders directly in `root` are the ones the table puts there. A
/// `subst` drive opens the volume of the folder it stands for; this keeps its table from being
/// shown under the wrong names.
fn listing_matches<'t>(
    root: &Path,
    kids: Option<&Vec<(u64, usize)>>,
    name_of: impl Fn(&(u64, usize)) -> Option<&'t str>,
) -> bool {
    let Ok(listing) = fs::read_dir(root) else {
        return false;
    };
    let mut on_disk: Vec<String> = listing
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() || t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let mut in_table: Vec<String> = kids
        .into_iter()
        .flatten()
        .filter_map(|kid| name_of(kid).map(str::to_owned))
        .collect();
    on_disk.sort();
    in_table.sort();
    on_disk == in_table
}

struct Tables<'a> {
    entries: &'a HashMap<u64, mft::Entry>,
    children: &'a Children,
}

fn build(
    tables: &Tables<'_>,
    number: u64,
    name: Box<str>,
    path: &Path,
    ctx: &Context<'_>,
    seen: &mut HashSet<u64>,
) -> RawDir {
    let mut raw = RawDir::new(name);
    if ctx.task.is_cancelled() {
        raw.incomplete = true;
        return raw;
    }
    let mut local_ext: HashMap<String, (u64, u64)> = HashMap::new();
    let mut subdirs: Vec<(u64, &str)> = Vec::new();
    for &(kid, i) in tables.children.get(&number).into_iter().flatten() {
        let Some(entry) = tables.entries.get(&kid) else {
            continue;
        };
        let Some((_, kid_name)) = entry.names.get(i) else {
            continue;
        };
        if entry.dir {
            // A folder reached twice would be a damaged table; count it once.
            if seen.insert(kid) {
                subdirs.push((kid, kid_name));
            }
            continue;
        }
        // A second name of a file already counted adds nothing.
        if entry.names.len() > 1 && !ctx.linked.lock().insert((u64::MAX, kid)) {
            ctx.linked_bytes.fetch_add(entry.bytes, Ordering::Relaxed);
            continue;
        }
        ctx.count_file(
            &mut raw,
            &mut local_ext,
            std::ffi::OsStr::new(&**kid_name),
            Sizes {
                bytes: entry.bytes,
                disk: entry.disk_bytes,
                modified: entry.modified,
            },
            || path.join(&**kid_name),
        );
    }
    ctx.folder_done(&raw, local_ext);
    raw.children = subdirs
        .into_iter()
        .map(|(kid, kid_name)| {
            build(
                tables,
                kid,
                kid_name.into(),
                &path.join(kid_name),
                ctx,
                seen,
            )
        })
        .collect();
    raw.total();
    raw
}

fn scan_dir(path: &Path, name: Box<str>, ctx: &Context<'_>) -> RawDir {
    let mut raw = RawDir::new(name);
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
            let path = entry.path();
            let disk = rewind_os_platform::files::size_on_disk(&path, &meta);
            ctx.count_file(
                &mut raw,
                &mut local_ext,
                &entry.file_name(),
                Sizes {
                    bytes: meta.len(),
                    disk,
                    modified: meta.modified().ok(),
                },
                || path,
            );
        }
    }
    ctx.folder_done(&raw, local_ext);
    raw.children = subdirs
        .into_par_iter()
        .map(|(child, name)| scan_dir(&child, name, ctx))
        .collect();
    raw.total();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mft::{Entry, FileTable, ROOT};

    fn folder(parent: u64, name: &str) -> Entry {
        Entry {
            names: vec![(parent, name.into())],
            dir: true,
            ..Entry::default()
        }
    }

    fn file(names: &[(u64, &str)], bytes: u64) -> Entry {
        Entry {
            names: names.iter().map(|(p, n)| (*p, (*n).into())).collect(),
            dir: false,
            bytes,
            disk_bytes: bytes,
            modified: None,
        }
    }

    fn tree(entries: Vec<(u64, Entry)>) -> ScanTree {
        let mut all: HashMap<u64, Entry> = entries.into_iter().collect();
        all.insert(ROOT, folder(ROOT, "."));
        let table = FileTable {
            entries: all,
            damaged: 0,
        };
        from_table(
            &table,
            Path::new("X:\\"),
            &[],
            &Task::new(),
            false,
            Instant::now(),
        )
        .unwrap()
    }

    #[test]
    fn a_hard_linked_file_counts_once() {
        let tree = tree(vec![
            (30, folder(ROOT, "a")),
            (31, folder(ROOT, "b")),
            (40, file(&[(30, "x.bin"), (31, "y.bin")], 1_000)),
            (41, file(&[(31, "z.bin")], 10)),
        ]);
        let root = tree.root_node().unwrap();
        assert_eq!(root.bytes, 1_010);
        assert_eq!(tree.hard_linked_bytes, 1_000);
    }

    #[test]
    fn a_damaged_table_never_loops_and_says_what_it_lost() {
        let tree = tree(vec![
            // Two folders that name each other as parent, cut off from the root.
            (30, folder(31, "loop")),
            (31, folder(30, "back")),
            (32, folder(ROOT, "kept")),
            (40, file(&[(32, "a.txt")], 5)),
            // A file whose folder's record is gone.
            (41, file(&[(99, "orphan.txt")], 7)),
        ]);
        let root = tree.root_node().unwrap();
        assert_eq!(root.bytes, 5);
        assert!(root.incomplete);
        assert!(tree.unreadable_dirs > 0);
    }

    #[test]
    fn ntfs_metadata_is_never_shown() {
        let tree = tree(vec![
            (0, file(&[(ROOT, "$MFT")], 1 << 30)),
            (11, folder(ROOT, "$Extend")),
            (30, file(&[(11, "$UsnJrnl")], 1 << 30)),
            (40, file(&[(ROOT, "pagefile.sys")], 100)),
        ]);
        assert_eq!(tree.root_node().unwrap().bytes, 100);
        assert!(
            tree.top_files
                .iter()
                .all(|f| !f.path.to_string_lossy().contains('$'))
        );
    }
}
