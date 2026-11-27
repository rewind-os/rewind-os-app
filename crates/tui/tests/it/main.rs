//! The terminal interface's keys, without a terminal: what they change and which requests they
//! name.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rewind_os_engine::cleanup::{self, CategoryReport};
use rewind_os_engine::{Request, Response, Task};
use rewind_os_tui::{App, Effect, Jobs, Purpose, View};
use std::path::PathBuf;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn press(app: &mut App, keys: &[KeyCode]) -> Vec<Effect> {
    let jobs = Jobs::default();
    keys.iter().flat_map(|&k| app.key(key(k), &jobs)).collect()
}

fn runs(effects: &[Effect], purpose: Purpose) -> Option<&Request> {
    effects.iter().find_map(|e| match e {
        Effect::Run(p, request) if *p == purpose => Some(request),
        _ => None,
    })
}

/// Two categories with junk, the second needing rights the app does not have.
fn analysis() -> Vec<CategoryReport> {
    cleanup::categories()
        .iter()
        .take(2)
        .enumerate()
        .map(|(i, c)| CategoryReport {
            category: *c,
            files: 10,
            bytes: 1_000_000 * (2 - i as u64),
            present: true,
            blocked: i == 1,
            note: None,
        })
        .collect()
}

fn cleanup_view() -> App {
    let mut app = App::new(PathBuf::from("."));
    let opened = press(&mut app, &[KeyCode::Char('3')]);
    assert!(runs(&opened, Purpose::Analyze).is_some());
    app.cleanup.selected.clear();
    let mut reports = analysis();
    reports
        .iter_mut()
        .for_each(|r| r.category.selected_by_default = false);
    app.finished(Purpose::Analyze, Ok(Response::CleanupAnalysis(reports)));
    app
}

#[test]
fn cleaning_always_asks_first_and_cleans_only_what_was_picked() {
    let mut app = cleanup_view();
    // Nothing picked: nothing to confirm.
    assert!(press(&mut app, &[KeyCode::Enter]).is_empty());
    assert!(!app.cleanup.confirming);
    // Picked, then Enter: the question, no request yet; "n" closes it with none.
    assert!(press(&mut app, &[KeyCode::Char(' '), KeyCode::Enter]).is_empty());
    assert!(app.cleanup.confirming);
    assert!(press(&mut app, &[KeyCode::Char('n')]).is_empty());
    assert!(!app.cleanup.confirming);
    // Asked again and answered "y": the picked category, and only it.
    let effects = press(&mut app, &[KeyCode::Enter, KeyCode::Char('y')]);
    let picked = app
        .cleanup
        .reports
        .as_ref()
        .and_then(|r| r.first())
        .map(|r| r.category.id);
    match runs(&effects, Purpose::Clean) {
        Some(Request::CleanupRun { categories }) => {
            assert_eq!(categories.len(), 1);
            assert_eq!(categories.first().map(String::as_str), picked);
        }
        other => panic!("expected a cleanup, got {other:?}"),
    }
}

#[test]
fn a_category_without_the_rights_to_clean_it_cannot_be_picked() {
    let mut app = cleanup_view();
    press(&mut app, &[KeyCode::Down, KeyCode::Char(' ')]);
    assert!(app.cleanup.selected.is_empty());
    press(
        &mut app,
        &[KeyCode::Char('a'), KeyCode::Enter, KeyCode::Char('y')],
    );
    let blocked = app
        .cleanup
        .reports
        .as_ref()
        .and_then(|r| r.iter().find(|r| r.blocked));
    assert!(blocked.is_some_and(|b| !app.cleanup.selected.contains(b.category.id)));
}

#[test]
fn folders_open_largest_first_and_back_leaves_the_cursor_on_the_folder_left() -> std::io::Result<()>
{
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    for (folder, size) in [("small", 10), ("large", 5_000), ("middle", 500)] {
        std::fs::create_dir_all(dir.path().join(folder).join("inner"))?;
        std::fs::write(
            dir.path().join(folder).join("inner").join("f"),
            vec![0; size],
        )?;
    }
    let tree =
        rewind_os_engine::scan::scan(dir.path(), &Task::new()).map_err(std::io::Error::other)?;
    let mut app = App::new(dir.path().to_path_buf());
    press(&mut app, &[KeyCode::Char('2')]);
    app.finished(Purpose::Scan, Ok(Response::Scan(std::sync::Arc::new(tree))));
    let name = |app: &App, id| {
        app.scan
            .tree
            .as_ref()
            .and_then(|t| t.node(id))
            .map(|n| n.name.to_string())
    };
    let first = app.scan.rows().first().copied();
    assert_eq!(
        first.and_then(|id| name(&app, id)).as_deref(),
        Some("large")
    );
    // Into the second folder, then back: the cursor is on it again.
    press(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(name(&app, app.scan.current).as_deref(), Some("middle"));
    press(&mut app, &[KeyCode::Backspace]);
    assert_eq!(app.scan.cursor, 1);
    Ok(())
}

#[test]
fn typing_a_folder_never_runs_the_keys_it_contains() {
    let mut app = App::new(PathBuf::from("/"));
    press(&mut app, &[KeyCode::Char('2'), KeyCode::Char('e')]);
    let effects = press(
        &mut app,
        &[KeyCode::Char('q'), KeyCode::Char('3'), KeyCode::Enter],
    );
    assert!(!app.quit);
    assert_eq!(app.view, View::Scan);
    match runs(&effects, Purpose::Scan) {
        Some(Request::DiskScan { root }) => assert!(root.ends_with("q3")),
        other => panic!("expected a scan, got {other:?}"),
    }
}

#[test]
fn a_view_opens_by_the_start_of_its_name_and_an_unknown_name_changes_nothing() {
    let mut app = App::new(PathBuf::from("."));
    let typed: Vec<KeyCode> = ":dis\n"
        .chars()
        .map(|c| {
            if c == '\n' {
                KeyCode::Enter
            } else {
                KeyCode::Char(c)
            }
        })
        .collect();
    press(&mut app, &typed);
    assert_eq!(app.view, View::Scan);
    let typed: Vec<KeyCode> = ":nope\n"
        .chars()
        .map(|c| {
            if c == '\n' {
                KeyCode::Enter
            } else {
                KeyCode::Char(c)
            }
        })
        .collect();
    assert!(press(&mut app, &typed).is_empty());
    assert_eq!(app.view, View::Scan);
}

fn entry(name: &str, enabled: bool, essential: bool) -> rewind_os_engine::startup::StartupEntry {
    rewind_os_engine::startup::StartupEntry {
        id: format!("xdg|{name}"),
        name: name.to_owned(),
        command: format!("/usr/bin/{name}"),
        source: rewind_os_engine::startup::Source::UserAutostart,
        enabled,
        needs_admin: false,
        essential,
        when: None,
    }
}

fn startup_view(entries: Vec<rewind_os_engine::startup::StartupEntry>) -> App {
    let mut app = App::new(PathBuf::from("."));
    let opened = press(&mut app, &[KeyCode::Char('4')]);
    assert!(runs(&opened, Purpose::Startup).is_some());
    let report = rewind_os_engine::startup::Report {
        entries,
        not_shown: Vec::new(),
    };
    app.finished(Purpose::Startup, Ok(Response::Startup(report)));
    app
}

#[test]
fn space_turns_an_entry_the_other_way_and_never_turns_off_an_essential_one() {
    let mut app = startup_view(vec![
        entry("keyring", true, true),
        entry("updater", false, false),
    ]);
    assert!(press(&mut app, &[KeyCode::Char(' ')]).is_empty());
    assert!(app.startup.note.as_ref().is_some_and(|(_, error)| *error));
    let effects = press(&mut app, &[KeyCode::Down, KeyCode::Char(' ')]);
    match runs(&effects, Purpose::StartupSet) {
        Some(Request::StartupSet { id, enabled }) => {
            assert!(id.ends_with("updater"));
            assert!(*enabled);
        }
        other => panic!("expected a change, got {other:?}"),
    }
}

#[test]
fn the_filter_takes_the_keys_it_is_typed_with() {
    let mut app = startup_view(vec![
        entry("alpha", true, false),
        entry("quick", true, false),
    ]);
    let effects = press(
        &mut app,
        &[KeyCode::Char('/'), KeyCode::Char('q'), KeyCode::Char(' ')],
    );
    assert!(effects.is_empty());
    assert!(!app.quit);
    press(&mut app, &[KeyCode::Backspace, KeyCode::Enter]);
    let shown: Vec<&str> = app
        .startup
        .shown()
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(shown, ["quick"]);
}

#[test]
fn uninstalling_asks_first_and_names_the_program_under_the_cursor() {
    let program = |name: &str| rewind_os_engine::programs::InstalledProgram {
        key: format!("apt|{name}"),
        name: name.to_owned(),
        publisher: None,
        version: None,
        install_location: None,
        display_icon: None,
        uninstall_command: None,
        estimated_bytes: None,
        installed_on: None,
        scope: rewind_os_engine::programs::Scope::Machine,
        manager: Some("APT"),
    };
    let mut app = App::new(PathBuf::from("."));
    press(&mut app, &[KeyCode::Char('5')]);
    app.finished(
        Purpose::Programs,
        Ok(Response::Programs(vec![program("curl"), program("vim")])),
    );
    let available = rewind_os_engine::tool("programs.uninstall").is_some_and(|t| t.available());
    assert!(press(&mut app, &[KeyCode::Down, KeyCode::Char('u')]).is_empty());
    if !available {
        assert!(app.programs.confirming.is_none());
        return;
    }
    assert!(press(&mut app, &[KeyCode::Char('n')]).is_empty());
    let effects = press(&mut app, &[KeyCode::Char('u'), KeyCode::Char('y')]);
    match runs(&effects, Purpose::Uninstall) {
        Some(Request::ProgramUninstall { key }) => assert_eq!(key, "apt|vim"),
        other => panic!("expected an uninstall, got {other:?}"),
    }
}

fn duplicates_view() -> App {
    use rewind_os_engine::duplicates::{Copy, DuplicateReport, DuplicateSet};
    let set = |name: &str, n: usize| DuplicateSet {
        bytes: 1_000,
        copies: (0..n)
            .map(|i| Copy {
                path: PathBuf::from(format!("/d/{name}{i}")),
                modified: None,
            })
            .collect(),
    };
    let mut app = App::new(PathBuf::from("/d"));
    press(&mut app, &[KeyCode::Char('6')]);
    let report = DuplicateReport {
        root: PathBuf::from("/d"),
        min_bytes: 1,
        sets: vec![set("a", 2), set("b", 3)],
        files_seen: 5,
        unreadable: 0,
        cloud_skipped: 0,
        program_folders_skipped: 0,
        cancelled: false,
    };
    app.finished(Purpose::Duplicates, Ok(Response::Duplicates(report)));
    app
}

#[test]
fn the_last_copy_of_a_file_can_never_be_picked() {
    let mut app = duplicates_view();
    // Both copies of "a": the second is refused.
    press(
        &mut app,
        &[KeyCode::Char(' '), KeyCode::Down, KeyCode::Char(' ')],
    );
    assert_eq!(app.duplicates.selected.len(), 1);
    // All but the oldest, then cleaning: every set keeps its oldest copy.
    let effects = press(
        &mut app,
        &[KeyCode::Char('a'), KeyCode::Enter, KeyCode::Char('y')],
    );
    match runs(&effects, Purpose::DuplicatesTrash) {
        Some(Request::FilesTrash { paths }) => {
            assert_eq!(paths.len(), 3);
            assert!(!paths.iter().any(|p| p.ends_with("a0") || p.ends_with("b0")));
        }
        other => panic!("expected the copies to move, got {other:?}"),
    }
}

#[test]
fn a_file_left_with_one_copy_leaves_the_list() {
    let mut app = duplicates_view();
    let moved = vec![rewind_os_engine::TrashResult {
        path: PathBuf::from("/d/a1"),
        bytes: 1_000,
        error: None,
    }];
    app.finished(Purpose::DuplicatesTrash, Ok(Response::Trashed(moved)));
    let left = app.duplicates.remaining();
    assert_eq!(left.len(), 1);
    assert!(left.iter().all(|s| s.copies.len() == 3));
}

#[test]
fn leftovers_and_build_folders_remove_only_what_was_picked_after_asking() {
    use rewind_os_engine::leftovers::{Confidence, Leftover, Location};
    let leftover = |name: &str, confidence| Leftover {
        path: PathBuf::from(format!("/home/me/.config/{name}")),
        location: Location::UserConfig,
        bytes: 10,
        files: 1,
        last_change: None,
        confidence,
    };
    let mut app = App::new(PathBuf::from("/home/me"));
    let opened = press(&mut app, &[KeyCode::Char('7')]);
    assert!(runs(&opened, Purpose::Leftovers).is_some());
    app.finished(
        Purpose::Leftovers,
        Ok(Response::Leftovers(vec![
            leftover("old", Confidence::High),
            leftover("recent", Confidence::Low),
        ])),
    );
    // "Likely unused" only, then the picked one after the question.
    press(&mut app, &[KeyCode::Char('c'), KeyCode::Char('c')]);
    assert_eq!(app.leftovers.shown().len(), 1);
    assert!(press(&mut app, &[KeyCode::Char(' '), KeyCode::Enter]).is_empty());
    let effects = press(&mut app, &[KeyCode::Char('y')]);
    match runs(&effects, Purpose::LeftoversTrash) {
        Some(Request::FilesTrash { paths }) => {
            assert_eq!(paths, &[PathBuf::from("/home/me/.config/old")]);
        }
        other => panic!("expected the folder to move, got {other:?}"),
    }

    use rewind_os_engine::projects::{BuildFolder, Kind, Report};
    let folder = |name: &str| BuildFolder {
        path: PathBuf::from(format!("/home/me/{name}/node_modules")),
        project: PathBuf::from(format!("/home/me/{name}")),
        kind: Kind::NodeModules,
        risk: Kind::NodeModules.risk(),
        bytes: 100,
        files: 3,
        last_used: None,
    };
    press(&mut app, &[KeyCode::Char('8')]);
    app.finished(
        Purpose::BuildFolders,
        Ok(Response::BuildFolders(Report {
            root: PathBuf::from("/home/me"),
            folders: vec![folder("site"), folder("app")],
            unreadable: 0,
            cancelled: false,
        })),
    );
    let effects = press(
        &mut app,
        &[
            KeyCode::Down,
            KeyCode::Char(' '),
            KeyCode::Enter,
            KeyCode::Char('n'),
        ],
    );
    assert!(effects.is_empty());
    let effects = press(&mut app, &[KeyCode::Enter, KeyCode::Char('y')]);
    match runs(&effects, Purpose::BuildClean) {
        Some(Request::DevClean { paths }) => {
            assert_eq!(paths, &[PathBuf::from("/home/me/app/node_modules")]);
        }
        other => panic!("expected a deletion, got {other:?}"),
    }
}

#[test]
fn closing_a_program_asks_first_and_never_offers_one_that_is_refused() {
    use rewind_os_engine::processes::{Program, Report};
    let program = |name: &str, refusal| Program {
        name: name.to_owned(),
        exe: None,
        pids: vec![10, 11],
        cpu: 1.0,
        memory: 1_000,
        refusal,
        startup: None,
    };
    let mut app = App::new(PathBuf::from("."));
    let opened = press(&mut app, &[KeyCode::Char('9')]);
    assert!(runs(&opened, Purpose::Processes).is_some());
    app.finished(
        Purpose::Processes,
        Ok(Response::Processes(Report {
            programs: vec![
                program("init", Some(rewind_os_engine::safety::THIS_APP)),
                program("editor", None),
            ],
            cpu: 2.0,
            memory_used: 1,
            memory_total: 2,
            processes: 3,
        })),
    );
    assert!(press(&mut app, &[KeyCode::Char('x')]).is_empty());
    assert!(app.processes.confirming.is_none());
    assert!(
        press(
            &mut app,
            &[KeyCode::Down, KeyCode::Char('F'), KeyCode::Char('n')]
        )
        .is_empty()
    );
    let effects = press(&mut app, &[KeyCode::Char('x'), KeyCode::Char('y')]);
    match runs(&effects, Purpose::Close) {
        Some(Request::ProcessesClose { name, pids, force }) => {
            assert_eq!(name, "editor");
            assert_eq!(pids, &[10, 11]);
            assert!(!force);
        }
        other => panic!("expected a close, got {other:?}"),
    }
}

#[test]
fn a_maintenance_action_that_changes_the_system_asks_first_and_needs_its_rights() {
    let mut app = App::new(PathBuf::from("."));
    press(&mut app, &[KeyCode::Char('0')]);
    assert_eq!(app.view, View::Maintenance);
    let actions = app.maintenance.actions();
    let Some(index) = actions.iter().position(|a| a.question().is_some()) else {
        return; // No maintenance tools on this system.
    };
    let to_it: Vec<KeyCode> = std::iter::repeat_n(KeyCode::Down, index).collect();
    press(&mut app, &to_it);
    // Without the rights it needs: nothing runs and nothing is asked.
    app.maintenance.elevated = false;
    let needs_admin = rewind_os_engine::tool(actions[index].request().id())
        .is_some_and(|t| t.admin == rewind_os_engine::AdminNeed::Yes);
    if needs_admin {
        assert!(press(&mut app, &[KeyCode::Enter]).is_empty());
        assert!(app.maintenance.confirming.is_none());
    }
    app.maintenance.elevated = true;
    assert!(press(&mut app, &[KeyCode::Enter]).is_empty());
    assert!(app.maintenance.confirming.is_some());
    assert!(press(&mut app, &[KeyCode::Char('n')]).is_empty());
    let effects = press(&mut app, &[KeyCode::Enter, KeyCode::Char('y')]);
    assert!(runs(&effects, Purpose::Maintenance).is_some());
}
