use chrono::{Duration, Utc};
use rewind_os_engine::Task;
use rewind_os_engine::history::{self, Comparison};
use rewind_os_engine::scan::{self, ScanTree};
use std::fs::{self, File};
use std::path::Path;

const MIB: u64 = 1 << 20;

fn sized(path: &Path, bytes: u64) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    File::create(path)?.set_len(bytes)
}

fn scan(root: &Path) -> Result<ScanTree, rewind_os_engine::EngineError> {
    scan::scan(root, &Task::new())
}

type Outcome<T> = Result<T, Box<dyn std::error::Error>>;

/// Scans `root` before and after `change`, keeping summaries in `kept`.
fn before_and_after(root: &Path, kept: &Path, change: impl FnOnce()) -> Outcome<Comparison> {
    let earlier = Utc::now() - Duration::days(2);
    let first = history::record(&scan(root)?, kept, earlier).ok_or("not kept")?;
    assert!(matches!(first, Comparison::Nothing { reason } if reason == history::FIRST_SCAN));
    change();
    Ok(history::record(&scan(root)?, kept, Utc::now()).ok_or("not kept")?)
}

/// The grown and shrunk folders, below the root; `None` when nothing was compared.
fn grew_and_shrank(c: Comparison) -> Option<(Vec<String>, Vec<String>)> {
    let Comparison::Changes { grew, shrank, .. } = c else {
        return None;
    };
    let names = |v: Vec<history::Change>| v.into_iter().map(|c| c.names.join("/")).collect();
    Some((names(grew), names(shrank)))
}

#[test]
fn growth_is_named_as_deep_as_it_goes() {
    let root = tempfile::tempdir().unwrap();
    let kept = tempfile::tempdir().unwrap();
    sized(&root.path().join("a/b/cache/old.bin"), 2 * MIB).unwrap();
    sized(&root.path().join("other/keep.bin"), 3 * MIB).unwrap();
    let changes = before_and_after(root.path(), kept.path(), || {
        sized(&root.path().join("a/b/cache/new.bin"), 20 * MIB).unwrap();
    });
    let (grew, shrank) = grew_and_shrank(changes.unwrap()).unwrap();
    assert_eq!(grew, vec!["a/b/cache"], "not every folder above it");
    assert!(shrank.is_empty());
}

#[test]
fn growth_spread_over_several_folders_names_their_parent() {
    let root = tempfile::tempdir().unwrap();
    let kept = tempfile::tempdir().unwrap();
    sized(&root.path().join("base.bin"), 3 * MIB).unwrap();
    let changes = before_and_after(root.path(), kept.path(), || {
        sized(&root.path().join("a/x/one.bin"), 10 * MIB).unwrap();
        sized(&root.path().join("a/y/two.bin"), 10 * MIB).unwrap();
    });
    assert_eq!(grew_and_shrank(changes.unwrap()).unwrap().0, vec!["a"]);
}

#[test]
fn freed_space_is_listed_as_shrunk() {
    let root = tempfile::tempdir().unwrap();
    let kept = tempfile::tempdir().unwrap();
    sized(&root.path().join("videos/big.bin"), 30 * MIB).unwrap();
    sized(&root.path().join("docs/a.bin"), 2 * MIB).unwrap();
    let changes = before_and_after(root.path(), kept.path(), || {
        fs::remove_file(root.path().join("videos/big.bin")).unwrap();
    });
    let (grew, shrank) = grew_and_shrank(changes.unwrap()).unwrap();
    assert!(grew.is_empty());
    assert_eq!(shrank, vec!["videos"]);
}

#[test]
fn scans_measured_differently_or_stopped_are_not_compared() {
    let root = tempfile::tempdir().unwrap();
    let kept = tempfile::tempdir().unwrap();
    sized(&root.path().join("a/file.bin"), 2 * MIB).unwrap();
    let mut stopped = scan(root.path()).unwrap();
    stopped.cancelled = true;
    assert!(history::record(&stopped, kept.path(), Utc::now()).is_none());
    // The stopped scan was not kept: the next one is still the first.
    let first = history::record(&scan(root.path()).unwrap(), kept.path(), Utc::now()).unwrap();
    assert!(matches!(first, Comparison::Nothing { reason } if reason == history::FIRST_SCAN));
    let mut other_way = scan(root.path()).unwrap();
    other_way.from_file_table = true;
    let compared = history::record(&other_way, kept.path(), Utc::now()).unwrap();
    assert!(
        matches!(compared, Comparison::Nothing { reason } if reason == history::MEASURED_OTHERWISE)
    );
}

#[test]
fn a_changed_folder_is_found_in_the_new_tree() {
    let root = tempfile::tempdir().unwrap();
    let kept = tempfile::tempdir().unwrap();
    sized(&root.path().join("x/base.bin"), 2 * MIB).unwrap();
    before_and_after(root.path(), kept.path(), || {
        sized(&root.path().join("x/deep/new.bin"), 20 * MIB).unwrap();
    })
    .unwrap();
    let tree = scan(root.path()).unwrap();
    let id = tree.find(&["x".into(), "deep".into()]).unwrap();
    assert_eq!(tree.path(id), root.path().join("x/deep"));
}
