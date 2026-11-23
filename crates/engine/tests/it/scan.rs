use rewind_os_engine::Task;
use rewind_os_engine::scan::{self, ScanTree};
use std::fs;
use std::path::Path;

fn write(path: &Path, bytes: usize) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, vec![7u8; bytes])
}

fn sample() -> std::io::Result<(tempfile::TempDir, u64)> {
    let dir = tempfile::tempdir()?;
    let files = [
        ("a/big.bin", 50_000),
        ("a/deep/x/y/z.log", 3_000),
        ("b/one.txt", 1_200),
        ("b/two.txt", 800),
        ("c/empty/.keep", 0),
        ("root.dat", 9_000),
    ];
    let mut total = 0;
    for (rel, size) in files {
        write(&dir.path().join(rel), size)?;
        total += size as u64;
    }
    Ok((dir, total))
}

#[test]
fn sizes_add_up_and_parents_hold_their_children() {
    let (dir, total) = sample().unwrap();
    let tree = scan::scan(dir.path(), &Task::new()).unwrap();
    let root = tree.root_node().unwrap();
    assert_eq!(root.bytes, total);
    for id in 0..tree.dir_count() as u32 {
        let node = tree.node(id).unwrap();
        let children: Vec<_> = tree.children(id).collect();
        let sum: u64 = children.iter().map(|(_, c)| c.bytes).sum();
        assert_eq!(
            node.bytes,
            sum + node.own_bytes,
            "{}",
            tree.path(id).display()
        );
        assert!(
            children.windows(2).all(|w| w[0].1.bytes >= w[1].1.bytes),
            "children not largest first"
        );
    }
}

#[test]
fn node_paths_lead_back_to_real_folders() {
    let (dir, _) = sample().unwrap();
    let tree = scan::scan(dir.path(), &Task::new()).unwrap();
    for id in 0..tree.dir_count() as u32 {
        let path = tree.path(id);
        assert!(path.is_dir(), "{} is not a folder", path.display());
        assert!(path.starts_with(dir.path()));
        assert_eq!(tree.ancestry(id).first(), Some(&ScanTree::ROOT));
    }
}

#[test]
fn largest_files_come_first_and_include_the_biggest() {
    let (dir, _) = sample().unwrap();
    let tree = scan::scan(dir.path(), &Task::new()).unwrap();
    assert!(tree.top_files.windows(2).all(|w| w[0].bytes >= w[1].bytes));
    assert!(tree.top_files.first().unwrap().path.ends_with("big.bin"));
}

#[test]
fn a_cancelled_scan_says_so_and_never_overcounts() {
    let (dir, total) = sample().unwrap();
    let task = Task::new();
    task.cancel();
    let tree = scan::scan(dir.path(), &task).unwrap();
    assert!(tree.cancelled);
    assert!(tree.root_node().unwrap().bytes <= total);
}

#[cfg(unix)]
#[test]
fn links_to_other_folders_are_not_followed() {
    let (dir, total) = sample().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("huge.bin"), 200_000).unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
    let tree = scan::scan(dir.path(), &Task::new()).unwrap();
    assert_eq!(tree.root_node().unwrap().bytes, total);
}

#[test]
fn listing_a_folder_shows_only_its_own_files() {
    let (dir, _) = sample().unwrap();
    let files = scan::list_files(&dir.path().join("b")).unwrap();
    assert_eq!(files.len(), 2);
    assert!(files[0].bytes >= files[1].bytes);
}
