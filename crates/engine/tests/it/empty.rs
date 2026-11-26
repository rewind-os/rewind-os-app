use rewind_os_engine::{Engine, Request, Response, Task, empty};
use std::fs;

fn tempdir() -> std::io::Result<tempfile::TempDir> {
    // Not the system's temporary folder: on Windows and macOS it is a program or system folder.
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))
}

#[test]
fn only_the_highest_truly_empty_folders_are_offered() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    for d in [
        "alone",
        "nest/inner/deeper",
        "full/sub",
        ".hidden",
        "project/node_modules/pkg",
    ] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    fs::write(root.join("full/sub/note.txt"), b"keep").unwrap();
    #[cfg(unix)]
    {
        fs::create_dir(root.join("only_link")).unwrap();
        std::os::unix::fs::symlink(root.join("full"), root.join("only_link/to_full")).unwrap();
    }
    let Ok(Response::Scan(tree)) = Engine::new().run(
        Request::DiskScan {
            root: root.to_path_buf(),
        },
        &Task::new(),
    ) else {
        panic!("the scan answers with a tree");
    };
    let found: Vec<String> = tree
        .empty_folders
        .iter()
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(
        found,
        vec!["alone", "nest"],
        "an empty folder's empty children are not listed again"
    );
}

#[test]
fn a_folder_that_got_something_since_the_scan_is_left_alone() {
    let dir = tempdir().unwrap();
    let folder = dir.path().join("was_empty");
    fs::create_dir(&folder).unwrap();
    assert!(empty::is_empty_folder(&folder));
    fs::write(folder.join("new.txt"), b"x").unwrap();
    let results = empty::trash(std::slice::from_ref(&folder), &Task::new());
    assert!(results.iter().all(|r| r.error.is_some()));
    assert!(folder.join("new.txt").exists());
}
