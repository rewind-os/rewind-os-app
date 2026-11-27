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
