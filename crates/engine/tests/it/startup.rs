use rewind_os_engine::startup::xdg::Dirs;
use rewind_os_engine::startup::{Source, StartupEntry};
use std::fs;

fn setup() -> std::io::Result<(tempfile::TempDir, Dirs)> {
    let root = tempfile::tempdir()?;
    let dirs = Dirs {
        user: root.path().join("user"),
        system: vec![root.path().join("system")],
    };
    fs::create_dir_all(&dirs.system[0])?;
    fs::write(
        dirs.system[0].join("tray.desktop"),
        "[Desktop Entry]\nType=Application\nName=Tray Helper\nName[es]=Ayudante\nExec=tray --quiet\n",
    )?;
    Ok((root, dirs))
}

fn find<'a>(list: &'a [StartupEntry], name: &str) -> Option<&'a StartupEntry> {
    list.iter().find(|e| e.name == name)
}

#[test]
fn turning_off_a_system_entry_writes_only_a_user_override() {
    let (_root, dirs) = setup().unwrap();
    let before = fs::read_to_string(dirs.system[0].join("tray.desktop")).unwrap();
    let entry = find(&dirs.list(), "Tray Helper").unwrap().clone();
    assert!(entry.enabled);
    assert_eq!(entry.source, Source::SystemAutostart);

    dirs.set_enabled(&entry.id, false).unwrap();
    assert!(!find(&dirs.list(), "Tray Helper").unwrap().enabled);
    assert!(dirs.user.join("tray.desktop").is_file());
    assert_eq!(
        fs::read_to_string(dirs.system[0].join("tray.desktop")).unwrap(),
        before
    );

    dirs.set_enabled(&entry.id, true).unwrap();
    assert!(find(&dirs.list(), "Tray Helper").unwrap().enabled);
}

#[test]
fn a_user_entry_keeps_its_other_lines_when_toggled() {
    let (_root, dirs) = setup().unwrap();
    fs::create_dir_all(&dirs.user).unwrap();
    fs::write(
        dirs.user.join("notes.desktop"),
        "[Desktop Entry]\nName=Notes\nExec=notes\nX-GNOME-Autostart-enabled=false\n[Desktop Action New]\nHidden=true\n",
    )
    .unwrap();
    let entry = find(&dirs.list(), "Notes").unwrap().clone();
    assert!(!entry.enabled);
    assert_eq!(entry.source, Source::UserAutostart);

    dirs.set_enabled(&entry.id, true).unwrap();
    let text = fs::read_to_string(dirs.user.join("notes.desktop")).unwrap();
    assert!(find(&dirs.list(), "Notes").unwrap().enabled);
    assert!(text.contains("Exec=notes"));
    assert!(
        text.contains("[Desktop Action New]\nHidden=true"),
        "other groups must stay as they were"
    );
}

#[test]
fn ids_cannot_point_outside_the_autostart_folders() {
    let (root, dirs) = setup().unwrap();
    fs::write(
        root.path().join("evil.desktop"),
        "[Desktop Entry]\nName=Evil\n",
    )
    .unwrap();
    for id in [
        "xdg|../evil.desktop",
        "xdg|/etc/passwd",
        "xdg|tray",
        "user_run|tray.desktop",
    ] {
        assert!(dirs.set_enabled(id, false).is_err(), "{id} was accepted");
    }
    assert!(!dirs.user.join("../evil.desktop").exists() || !dirs.user.exists());
}
