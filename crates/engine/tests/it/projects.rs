use rewind_os_engine::Task;
use rewind_os_engine::cleanup::Risk;
use rewind_os_engine::projects::{self, Kind, Places};
use std::fs;
use std::path::{Path, PathBuf};

fn write(path: &Path, bytes: usize) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, vec![7u8; bytes])
}

/// A folder whose name is not hidden: `tempfile` names its folders `.tmp…`.
fn workspace() -> std::io::Result<tempfile::TempDir> {
    tempfile::Builder::new().prefix("rewind-projects").tempdir()
}

/// A Node.js project with its lockfile and packages.
fn node_project(dir: &Path) -> std::io::Result<PathBuf> {
    write(&dir.join("package.json"), 20)?;
    write(&dir.join("package-lock.json"), 20)?;
    write(&dir.join("node_modules/left-pad/index.js"), 5_000)?;
    write(&dir.join("node_modules/left-pad/package.json"), 20)?;
    Ok(dir.join("node_modules"))
}

fn rust_project(dir: &Path) -> std::io::Result<PathBuf> {
    write(&dir.join("Cargo.toml"), 20)?;
    write(&dir.join("target/CACHEDIR.TAG"), 40)?;
    write(&dir.join("target/debug/app"), 9_000)?;
    Ok(dir.join("target"))
}

fn found(
    root: &Path,
    places: &Places,
) -> Result<Vec<(PathBuf, Kind)>, rewind_os_engine::EngineError> {
    let report = projects::find(root, places, &Task::new())?;
    Ok(report
        .folders
        .into_iter()
        .map(|f| (f.path, f.kind))
        .collect())
}

#[test]
fn build_folders_are_found_only_beside_the_file_that_proves_them() {
    let ws = workspace().unwrap();
    let root = ws.path();
    let node = node_project(&root.join("web")).unwrap();
    let target = rust_project(&root.join("tool")).unwrap();
    // Lookalikes: no lockfile, no cache tag, a plain folder named `build`.
    write(&root.join("loose/package.json"), 20).unwrap();
    write(&root.join("loose/node_modules/x/index.js"), 50).unwrap();
    write(&root.join("notes/target/plan.txt"), 50).unwrap();
    write(&root.join("photos/build/album.jpg"), 50).unwrap();

    let found = found(root, &Places::default()).unwrap();
    assert!(found.contains(&(node, Kind::NodeModules)));
    assert!(found.contains(&(target, Kind::RustTarget)));
    assert_eq!(found.len(), 2, "lookalikes are never offered: {found:?}");
}

#[test]
fn hidden_folders_and_app_data_are_never_searched() {
    let ws = workspace().unwrap();
    let root = ws.path();
    // An editor extension runs from its node_modules.
    node_project(&root.join(".vscode/extensions/linter")).unwrap();
    // An installed app's data.
    let app_data = root.join("AppData");
    node_project(&app_data.join("Local/SomeApp/resources/app")).unwrap();
    let places = Places {
        off_limits: vec![app_data],
    };
    assert!(found(root, &places).unwrap().is_empty());
}

#[test]
fn a_build_folder_inside_another_is_not_listed_twice() {
    let ws = workspace().unwrap();
    let root = ws.path();
    let outer = node_project(&root.join("app")).unwrap();
    node_project(&outer.join("some-package")).unwrap();
    assert_eq!(
        found(root, &Places::default()).unwrap(),
        vec![(outer, Kind::NodeModules)]
    );
}

#[test]
fn a_python_environment_is_marked_as_needing_care() {
    let ws = workspace().unwrap();
    let root = ws.path();
    write(&root.join("svc/requirements.txt"), 20).unwrap();
    write(&root.join("svc/.venv/pyvenv.cfg"), 20).unwrap();
    write(&root.join("svc/.venv/lib/site.py"), 2_000).unwrap();
    let report = projects::find(root, &Places::default(), &Task::new()).unwrap();
    let env = report.folders.first().unwrap();
    assert_eq!(env.kind, Kind::PythonEnv);
    assert!(
        env.risk > Risk::Safe,
        "packages installed by hand would be lost"
    );
}

#[test]
fn cleaning_deletes_only_recognised_build_folders() {
    let ws = workspace().unwrap();
    let root = ws.path();
    let node = node_project(&root.join("web")).unwrap();
    let lookalike = root.join("notes/target");
    write(&lookalike.join("plan.txt"), 50).unwrap();
    let hidden = node_project(&root.join(".config/tool")).unwrap();

    let results = projects::clean(
        &[node.clone(), lookalike.clone(), hidden.clone()],
        &Places::default(),
        &Task::new(),
    );
    assert!(!node.exists());
    assert!(root.join("web/package.json").exists(), "the project stays");
    assert!(lookalike.join("plan.txt").exists());
    assert!(hidden.exists());
    let refused: Vec<bool> = results.iter().map(|r| r.refused.is_some()).collect();
    assert_eq!(refused, vec![false, true, true]);
    assert!(results.first().is_some_and(|r| r.bytes >= 5_000));
}

#[cfg(unix)]
#[test]
fn cleaning_never_reaches_through_a_link() {
    let ws = workspace().unwrap();
    let outside = workspace().unwrap();
    write(&outside.path().join("precious.txt"), 10).unwrap();
    let node = node_project(&ws.path().join("web")).unwrap();
    // pnpm links packages from a shared store.
    std::os::unix::fs::symlink(outside.path(), node.join("linked")).unwrap();
    projects::clean(&[node], &Places::default(), &Task::new());
    assert!(outside.path().join("precious.txt").exists());
}
