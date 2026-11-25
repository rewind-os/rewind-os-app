use rewind_os_engine::Task;
use rewind_os_engine::duplicates::{self, keeps_a_copy};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

type Outcome = Result<(), Box<dyn std::error::Error>>;

fn write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

/// Contents larger than the first block, so both comparison passes run.
/// A temporary folder outside the system's temp folder: on macOS that lives in `/private/var`,
/// where duplicates are never searched.
fn tempdir() -> std::io::Result<tempfile::TempDir> {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))
}

fn big(fill: u8, last: u8) -> Vec<u8> {
    let mut v = vec![fill; 100_000];
    if let Some(end) = v.last_mut() {
        *end = last;
    }
    v
}

fn paths_of(report: &duplicates::DuplicateReport) -> Vec<HashSet<PathBuf>> {
    report
        .sets
        .iter()
        .map(|s| s.copies.iter().map(|c| c.path.clone()).collect())
        .collect()
}

#[test]
fn identical_files_are_grouped_and_lookalikes_are_not() -> Outcome {
    let dir = tempdir()?;
    let root = dir.path();
    write(&root.join("a/photo.jpg"), &big(1, 9))?;
    write(&root.join("b/photo copy.jpg"), &big(1, 9))?;
    // Same size and same first block, different end: not a duplicate.
    write(&root.join("c/almost.jpg"), &big(1, 8))?;
    write(&root.join("small/x.txt"), b"same")?;
    write(&root.join("small/y.txt"), b"same")?;

    let report = duplicates::find(root, 1, &Task::new())?;
    let sets = paths_of(&report);
    let photo: HashSet<PathBuf> = [root.join("a/photo.jpg"), root.join("b/photo copy.jpg")].into();
    assert!(sets.contains(&photo));
    assert!(
        sets.iter().all(|s| !s.contains(&root.join("c/almost.jpg"))),
        "a file that differs only at the end is not a duplicate"
    );
    assert!(sets.contains(&[root.join("small/x.txt"), root.join("small/y.txt")].into()));
    assert!(report.reclaimable() >= 100_000);
    Ok(())
}

#[test]
fn files_below_the_minimum_size_are_not_compared() -> Outcome {
    let dir = tempdir()?;
    write(&dir.path().join("x.txt"), b"same")?;
    write(&dir.path().join("y.txt"), b"same")?;
    let report = duplicates::find(dir.path(), 1024, &Task::new())?;
    assert!(report.sets.is_empty());
    Ok(())
}

#[test]
fn hard_links_to_one_file_are_not_duplicates() -> Outcome {
    let dir = tempdir()?;
    write(&dir.path().join("original.bin"), &big(3, 3))?;
    fs::hard_link(dir.path().join("original.bin"), dir.path().join("link.bin"))?;
    let report = duplicates::find(dir.path(), 1, &Task::new())?;
    assert!(
        report.sets.is_empty(),
        "removing a hard link frees nothing, so it must not be offered"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn links_to_other_folders_are_not_followed() -> Outcome {
    let dir = tempdir()?;
    let outside = tempdir()?;
    write(&dir.path().join("mine.bin"), &big(5, 5))?;
    write(&outside.path().join("theirs.bin"), &big(5, 5))?;
    std::os::unix::fs::symlink(outside.path(), dir.path().join("link"))?;
    let report = duplicates::find(dir.path(), 1, &Task::new())?;
    assert!(report.sets.is_empty());
    Ok(())
}

#[test]
fn choosing_all_but_the_oldest_keeps_one_copy_of_each() -> Outcome {
    let dir = tempdir()?;
    for name in ["a.bin", "b.bin", "c.bin"] {
        write(&dir.path().join(name), &big(7, 7))?;
    }
    for name in ["d.bin", "e.bin"] {
        write(&dir.path().join(name), &big(8, 8))?;
    }
    let report = duplicates::find(dir.path(), 1, &Task::new())?;
    assert_eq!(report.sets.len(), 2);
    let chosen: HashSet<PathBuf> = report.all_but_oldest().into_iter().collect();
    assert!(keeps_a_copy(&report.sets, &chosen));
    let everything: HashSet<PathBuf> = paths_of(&report).into_iter().flatten().collect();
    assert!(!keeps_a_copy(&report.sets, &everything));
    Ok(())
}

#[cfg(unix)]
#[test]
fn system_folders_are_refused() {
    assert!(duplicates::find(Path::new("/usr"), 1, &Task::new()).is_err());
}

#[test]
fn copies_inside_programs_and_projects_are_not_offered() -> Outcome {
    let dir = tempdir()?;
    let root = dir.path();
    write(&root.join("photos/a.jpg"), &big(3, 1))?;
    write(&root.join("backup/a.jpg"), &big(3, 1))?;
    // Two clones of one repository and two projects sharing a package: removing one copy
    // would break the other.
    write(&root.join("one/.git/objects/pack/p.pack"), &big(4, 2))?;
    write(&root.join("two/.git/objects/pack/p.pack"), &big(4, 2))?;
    write(&root.join("site/node_modules/lib/x.js"), &big(5, 3))?;
    write(&root.join("app/node_modules/lib/x.js"), &big(5, 3))?;

    let report = duplicates::find(root, 1, &Task::new())?;
    let offered: Vec<PathBuf> = paths_of(&report).into_iter().flatten().collect();
    assert!(offered.contains(&root.join("backup/a.jpg")));
    assert!(
        offered.iter().all(|p| !p
            .components()
            .any(|c| c.as_os_str() == ".git" || c.as_os_str() == "node_modules")),
        "a program's or project's file was offered: {offered:?}"
    );
    assert!(
        report.program_folders_skipped >= 4,
        "the skipped folders are reported"
    );
    assert!(duplicates::find(&root.join("site/node_modules"), 1, &Task::new()).is_err());
    Ok(())
}
