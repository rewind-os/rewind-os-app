use rewind_os_engine::Task;
use rewind_os_engine::cleanup::{self, Places};
use std::fs;
use std::path::Path;

fn write(path: &Path, bytes: usize) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, vec![3u8; bytes])
}

/// A fake home with a Linux-style cache, so the whole path from template to deletion runs.
fn places(root: &Path) -> Places {
    Places::from_pairs([
        ("home", root.join("home")),
        ("cache", root.join("home/.cache")),
        ("config", root.join("home/.config")),
        ("data", root.join("home/.local/share")),
        ("downloads", root.join("home/Downloads")),
    ])
}

#[test]
fn a_category_is_measured_and_cleaned_inside_its_folder_only() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    write(
        &root.path().join("home/.cache/thumbnails/normal/a.png"),
        4000,
    )
    .unwrap();
    write(
        &root.path().join("home/.cache/thumbnails/large/b.png"),
        6000,
    )
    .unwrap();
    write(&root.path().join("home/.cache/other-app/keep.bin"), 9000).unwrap();

    let reports = cleanup::analyze_in(cleanup::LINUX, &places, false, &Task::new());
    let thumbs = reports
        .iter()
        .find(|r| r.category.id == "linux.thumbnails")
        .unwrap();
    assert!(thumbs.present);
    assert!(thumbs.bytes >= 10_000);

    let ids = vec!["linux.thumbnails".to_owned()];
    let done = cleanup::run_in(cleanup::LINUX, &places, false, &ids, &Task::new()).unwrap();
    assert_eq!(done[0].bytes, thumbs.bytes);
    assert!(
        root.path().join("home/.cache/thumbnails").is_dir(),
        "the folder itself stays"
    );
    assert!(
        !root
            .path()
            .join("home/.cache/thumbnails/normal/a.png")
            .exists()
    );
    assert!(root.path().join("home/.cache/other-app/keep.bin").exists());
}

#[test]
fn categories_whose_folders_are_missing_are_not_present() {
    let root = tempfile::tempdir().unwrap();
    let reports = cleanup::analyze_in(cleanup::LINUX, &places(root.path()), false, &Task::new());
    let chrome = reports
        .iter()
        .find(|r| r.category.id == "browsers.chrome")
        .unwrap();
    assert!(!chrome.present);
    assert_eq!(chrome.bytes, 0);
}

#[test]
fn categories_needing_rights_are_reported_and_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let ids = vec!["linux.package_downloads".to_owned()];
    let done = cleanup::run_in(
        cleanup::LINUX,
        &places(root.path()),
        false,
        &ids,
        &Task::new(),
    )
    .unwrap();
    assert!(done[0].skipped.is_some());
    assert_eq!(done[0].bytes, 0);
}

#[test]
fn every_system_has_its_own_list_with_unique_ids() {
    for list in [cleanup::WINDOWS, cleanup::LINUX, cleanup::MACOS] {
        let mut ids: Vec<_> = list.iter().map(|c| c.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate category id");
    }
}
