use rewind_os_engine::Os;
use rewind_os_engine::processes::startup_entry;
use rewind_os_engine::safety::{SYSTEM_PROCESS, THIS_APP, process_refusal_on};
use rewind_os_engine::startup::{Source, StartupEntry};

#[test]
fn the_programs_the_system_runs_on_are_never_closed() {
    for name in [
        "csrss.exe",
        "WinLogon.exe",
        "svchost.exe",
        "explorer.exe",
        "dwm.exe",
    ] {
        assert_eq!(
            process_refusal_on(Os::Windows, name),
            Some(SYSTEM_PROCESS),
            "{name}"
        );
    }
    for name in [
        "systemd",
        "systemd-logind",
        "gnome-shell",
        "Xwayland",
        "pipewire",
    ] {
        assert_eq!(
            process_refusal_on(Os::Linux, name),
            Some(SYSTEM_PROCESS),
            "{name}"
        );
    }
    assert_eq!(
        process_refusal_on(Os::Windows, "rewind-os.exe"),
        Some(THIS_APP)
    );
    // Ordinary programs may be closed.
    assert_eq!(process_refusal_on(Os::Windows, "chrome.exe"), None);
    assert_eq!(process_refusal_on(Os::Linux, "firefox"), None);
}

fn entry(command: &str, enabled: bool) -> StartupEntry {
    StartupEntry {
        id: format!("run|{command}"),
        name: "x".into(),
        command: command.into(),
        source: Source::Service,
        enabled,
        needs_admin: false,
        essential: false,
        when: None,
    }
}

#[test]
fn a_running_program_is_matched_to_the_startup_entry_that_starts_it() {
    let entries = [
        entry("/usr/bin/slacker --hidden", true),
        entry(
            r#""C:\Program Files\Microsoft OneDrive\OneDrive.exe" /background"#,
            true,
        ),
        entry("/usr/bin/discord", false),
    ];
    assert!(startup_entry("OneDrive.exe", &entries).is_some());
    // The whole file name, not a part of it.
    assert!(startup_entry("slack", &entries).is_none());
    // An entry that is already off has nothing to turn off.
    assert!(startup_entry("discord", &entries).is_none());
}

#[cfg(unix)]
#[test]
fn closing_ends_only_the_named_program() {
    use rewind_os_engine::Task;
    use rewind_os_engine::processes::close;
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("sleep");
    let pid = child.id();
    // A pid that now belongs to another program is left alone.
    let other = close("not-sleep", &[pid], false, &Task::new()).expect("close");
    assert_eq!(other.closed, 0);
    assert!(child.try_wait().expect("wait").is_none());
    let done = close("sleep", &[pid], false, &Task::new()).expect("close");
    assert_eq!((done.closed, done.running), (1, 0));
    assert!(child.wait().is_ok());
}
