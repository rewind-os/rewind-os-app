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

#[test]
fn essential_entries_cannot_be_turned_off() {
    let (_root, dirs) = setup().unwrap();
    fs::write(
        dirs.system[0].join("polkit-gnome-authentication-agent-1.desktop"),
        "[Desktop Entry]\nName=PolicyKit Agent\nExec=polkit-agent\n",
    )
    .unwrap();
    let entry = find(&dirs.list(), "PolicyKit Agent").unwrap().clone();
    assert!(entry.essential);
    assert!(dirs.set_enabled(&entry.id, false).is_err());
    assert!(find(&dirs.list(), "PolicyKit Agent").unwrap().enabled);
    assert!(
        !dirs
            .user
            .join("polkit-gnome-authentication-agent-1.desktop")
            .exists()
    );
}

mod scheduled_tasks {
    use rewind_os_engine::startup::tasks::parse;

    const SAMPLE: &str = r#"[
      {"path":"\\GoogleUpdateTaskMachineUA","name":"GoogleUpdateTaskMachineUA","state":"Ready","user":"SYSTEM","triggers":["MSFT_TaskDailyTrigger"],"command":"C:\\Program Files (x86)\\Google\\Update\\GoogleUpdate.exe /ua"},
      {"path":"\\Vendor\\Tray helper","name":"Tray helper","state":"Disabled","user":"PC\\ana","triggers":["MSFT_TaskLogonTrigger"],"command":"C:\\Users\\ana\\AppData\\Local\\Vendor\\tray.exe"},
      {"path":"\\Microsoft\\Windows\\Defrag\\ScheduledDefrag","name":"ScheduledDefrag","state":"Ready","user":"SYSTEM","triggers":[],"command":"defrag.exe"}
    ]"#;

    #[test]
    fn windows_own_tasks_are_never_listed() {
        let entries = parse(SAMPLE, "ana");
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|e| !e.id.to_lowercase().contains("microsoft"))
        );
    }

    #[test]
    fn state_and_rights_come_from_the_task() {
        let entries = parse(SAMPLE, "ana");
        let google = entries
            .iter()
            .find(|e| e.name.starts_with("Google"))
            .unwrap();
        let tray = entries.iter().find(|e| e.name == "Tray helper").unwrap();
        assert!(
            google.enabled && google.needs_admin,
            "a SYSTEM task needs rights"
        );
        assert!(
            !tray.enabled && !tray.needs_admin,
            "your own task is yours to change"
        );
        assert_ne!(google.when, tray.when);
    }

    #[test]
    fn unexpected_output_lists_nothing() {
        assert!(parse("Get-ScheduledTask : Access denied", "ana").is_empty());
        assert!(parse("", "ana").is_empty());
    }
}

mod scheduled_task_names {
    use rewind_os_engine::startup::tasks::{display_name, parse};

    #[test]
    fn machine_made_suffixes_are_dropped_from_names() {
        assert_eq!(
            display_name("NVIDIA App SelfUpdate_{B2FE1952-0186-46C3-BAEC-A80AA35AC5B8}"),
            "NVIDIA App SelfUpdate"
        );
        assert_eq!(
            display_name("OneDrive Reporting Task-S-1-5-21-1097274811-1001"),
            "OneDrive Reporting Task"
        );
        assert_eq!(
            display_name(
                "ActionCenterRefreshForExpirationTask-{97aa830e-eae8-490e-9369-c607022919c9}"
            ),
            "ActionCenterRefreshForExpirationTask"
        );
        assert_eq!(display_name("GHelper"), "GHelper");
    }

    #[test]
    fn a_trigger_without_a_class_does_not_hide_every_task() {
        let json = r#"[{"path":"\\Vendor","name":"Vendor","state":"Running","user":"me","triggers":[null],"command":"v.exe"}]"#;
        assert_eq!(parse(json, "me").len(), 1);
    }
}

mod services {
    use rewind_os_engine::startup::services::{RawService, entries, is_program_service};
    use std::collections::BTreeSet;

    const WINDOWS: &str = r"C:\WINDOWS";

    fn service(name: &str, kind: u64, start: u64, image: &str) -> RawService {
        RawService {
            name: name.to_owned(),
            display_name: Some(name.to_owned()),
            image_path: Some(image.to_owned()),
            start: Some(start),
            kind: Some(kind),
        }
    }

    #[test]
    fn only_services_of_programs_are_listed() {
        let program = service(
            "Updater",
            0x10,
            2,
            r#""C:\Program Files\Vendor\updater.exe" /svc"#,
        );
        assert!(is_program_service(&program, WINDOWS));
        for not_a_program in [
            service(
                "Dhcp",
                0x20,
                2,
                r"%SystemRoot%\system32\svchost.exe -k LocalService",
            ),
            service("Spooler", 0x10, 2, r"C:\WINDOWS\System32\spoolsv.exe"),
            service(
                "nvlddmkm",
                0x1,
                3,
                r"\SystemRoot\System32\DriverStore\nvlddmkm.sys",
            ),
            service(
                "WinDefend",
                0x10,
                2,
                r#""C:\ProgramData\Microsoft\Windows Defender\Platform\4.18\MsMpEng.exe""#,
            ),
        ] {
            assert!(
                !is_program_service(&not_a_program, WINDOWS),
                "{} must never be listed",
                not_a_program.name
            );
        }
    }

    #[test]
    fn automatic_services_show_and_manual_ones_only_when_the_app_changed_them() {
        let raw = vec![
            service("Updater", 0x10, 2, r"C:\Program Files\Vendor\updater.exe"),
            service("Helper", 0x10, 3, r"C:\Program Files\Vendor\helper.exe"),
            service("Other", 0x10, 3, r"C:\Program Files\Other\other.exe"),
        ];
        let changed: BTreeSet<String> = ["Helper".to_owned()].into();
        let list = entries(&raw, WINDOWS, &changed);
        let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Updater", "Helper"]);
        assert!(list[0].enabled && !list[1].enabled);
        assert!(list.iter().all(|e| e.needs_admin));
    }

    #[test]
    fn security_vpn_and_sound_services_are_essential() {
        let raw = vec![
            service(
                "Tailscale",
                0x10,
                2,
                r"C:\Program Files\Tailscale\tailscaled.exe",
            ),
            service(
                "RtkAudioUniversalService",
                0x10,
                2,
                r"C:\Program Files\Realtek\Audio\RtkAudUService64.exe",
            ),
            service(
                "GoogleUpdater",
                0x10,
                2,
                r"C:\Program Files (x86)\Google\updater.exe",
            ),
        ];
        let list = entries(&raw, WINDOWS, &BTreeSet::new());
        let essential: Vec<bool> = list.iter().map(|e| e.essential).collect();
        assert_eq!(essential, vec![true, true, false]);
    }
}
