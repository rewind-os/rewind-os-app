use rewind_os_engine::leftovers::{Confidence, Signals, confidence};
use std::time::{Duration, SystemTime};

fn signals() -> Signals {
    let mut s = Signals::default();
    s.add_name("Mozilla Firefox (x64 es-ES)");
    s.add_name("JetBrains s.r.o.");
    s.add_name("Visual Studio Code");
    s.add_path(r#""C:\Program Files\Tool Box\bin\toolbox.exe" --service"#);
    s
}

#[test]
fn folders_named_after_installed_programs_or_publishers_are_kept() {
    let s = signals();
    assert!(s.accounts_for("Mozilla Firefox"));
    assert!(s.accounts_for("JetBrains"));
    assert!(s.accounts_for("Code"));
}

#[test]
fn folders_referenced_by_a_path_are_kept() {
    assert!(signals().accounts_for("Tool Box"));
}

#[test]
fn unknown_folders_are_candidates() {
    let s = signals();
    assert!(!s.accounts_for("OldGameStudio"));
    assert!(!s.accounts_for("Uninstalled Editor 2019"));
}

#[test]
fn shared_and_ambiguous_folders_are_never_candidates() {
    let s = Signals::default();
    assert!(s.accounts_for("Common Files"));
    assert!(s.accounts_for("Microsoft Edge"));
    assert!(s.accounts_for("{3F2504E0-4F89-11D3-9A0C-0305E82C3301}"));
    assert!(s.accounts_for("ab"));
    // Windows' SDK, used by Visual Studio although Programs names no folder for it.
    assert!(s.accounts_for("Windows Kits"));
    assert!(s.accounts_for("Hyper-V"));
}

#[test]
fn a_folder_holding_a_tool_on_the_path_is_in_use() {
    let mut s = Signals::default();
    assert!(!s.accounts_for("chocolatey"));
    s.add_path(r"C:\ProgramData\chocolatey\bin");
    assert!(s.accounts_for("chocolatey"));
}

#[test]
fn older_folders_are_at_least_as_likely_abandoned() {
    let now = SystemTime::now();
    let ages = [0u64, 10, 40, 100, 200, 1000];
    let levels: Vec<Confidence> = ages
        .iter()
        .map(|d| confidence(now.checked_sub(Duration::from_secs(d * 86_400)), now))
        .collect();
    assert!(levels.windows(2).all(|w| w[0] <= w[1]));
    assert!(levels.first() < levels.last());
}

mod registry_keys {
    use rewind_os_engine::leftovers::{Signals, registry_candidates};

    const ROOT: &str = r"HKEY_CURRENT_USER\Software";

    fn names(keys: &[&str], signals: &Signals) -> Vec<String> {
        let keys: Vec<(&str, String)> = keys.iter().map(|k| (ROOT, (*k).to_owned())).collect();
        registry_candidates(&keys, signals)
            .into_iter()
            .map(|l| l.name)
            .collect()
    }

    #[test]
    fn keys_of_installed_programs_and_windows_keys_are_kept_out() {
        let mut signals = Signals::default();
        signals.add_name("7-Zip 23.01 (x64)");
        signals.add_name("Blackmagic Design");
        signals.add_path(r"C:\Program Files\Focusrite\Control\Focusrite Control.exe");
        let found = names(
            &[
                "7-Zip",
                "Blackmagic Design",
                "Focusrite",
                "Microsoft",
                "Classes",
                "Policies",
                "{4D57AFA0-265F-4DED-810B-8B81A02761BC}",
                "3d0ba22d-e02b-5c6d-93a1-4e2a9af9c1f2",
                "Battlestate Games",
            ],
            &signals,
        );
        assert_eq!(found, vec!["Battlestate Games"]);
    }

    #[test]
    fn the_report_names_the_full_key() {
        let keys = vec![(ROOT, "Old Vendor".to_owned())];
        let found = registry_candidates(&keys, &Signals::default());
        assert!(found[0].key.starts_with(ROOT) && found[0].key.ends_with("Old Vendor"));
    }
}

#[test]
fn a_folder_named_like_a_program_on_the_path_is_not_a_leftover() {
    let mut s = Signals::default();
    // What the Linux evidence adds: package names and the programs on the PATH.
    s.add_name("google-chrome-stable");
    s.add_name("k9s");
    s.add_name("code");
    assert!(s.accounts_for("google-chrome"));
    assert!(s.accounts_for("k9s"));
    assert!(s.accounts_for("Code"));
    assert!(!s.accounts_for("netlify"));
}

#[test]
fn macos_folders_of_apple_are_never_leftovers_and_bundle_ids_account_for_theirs() {
    use rewind_os_engine::leftovers::mac_shared;
    for apple in [
        "com.apple.Safari",
        "group.com.apple.notes",
        "callhistorydb",
        "mobile documents",
    ] {
        assert!(mac_shared(&apple.to_lowercase()), "{apple}");
    }
    assert!(!mac_shared("com.spotify.client"));
    let mut s = Signals::default();
    s.add_name("com.google.Chrome");
    s.add_name("Google Chrome");
    assert!(s.accounts_for("Google"));
    assert!(s.accounts_for("com.google.Chrome"));
    assert!(!s.accounts_for("com.spotify.client"));
}

#[test]
fn the_apps_own_folder_is_never_a_leftover() {
    // A portable copy is not installed, and its scan summaries live in this folder.
    let nothing_installed = Signals::default();
    assert!(nothing_installed.accounts_for(rewind_os_engine::history::APP_FOLDER));
}

#[test]
fn a_program_name_with_its_version_still_owns_its_folders() {
    let mut s = Signals::default();
    s.add_name("Termius 9.43.1");
    s.add_name("Ollama version 0.40.3");
    assert!(s.accounts_for("termius-updater"));
    assert!(s.accounts_for("Ollama"));
    assert!(!s.accounts_for("OldGameStudio"));
}

#[test]
fn game_folders_battle_net_records_are_found_in_its_binary_record() {
    use rewind_os_engine::leftovers::paths_in;
    let mut db = vec![0x0a, 0x12, b'w', b'o', b'w', 0x1a, 0x28];
    db.extend_from_slice(b"(C:/Program Files (x86)/World of Warcraft");
    db.extend_from_slice(&[0x22, 0x05, 0x00]);
    db.extend_from_slice(b"enUS no path here");
    let paths = paths_in(&db);
    assert_eq!(paths, ["C:/Program Files (x86)/World of Warcraft"]);
    let mut s = Signals::default();
    for p in &paths {
        s.add_path(p);
    }
    assert!(s.accounts_for("World of Warcraft"));
    assert!(!s.accounts_for("Diablo IV"));
}

#[test]
fn the_makers_and_names_of_audio_plugins_own_their_sound_libraries() {
    use rewind_os_engine::leftovers::plugin_items;
    let plugins = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(plugins.path().join("Toontrack").join("EZdrummer 3.vst3")).unwrap();
    std::fs::write(plugins.path().join("jamstix4.dll"), b"").unwrap();
    let mut s = Signals::default();
    for item in plugin_items(plugins.path()) {
        s.add_path(&item.to_string_lossy());
        if let Some(stem) = item.file_stem() {
            s.add_name(&stem.to_string_lossy());
        }
    }
    // `ProgramData\Toontrack` holds EZdrummer's drums; `ProgramData\Jamstix4` its plugin's.
    assert!(s.accounts_for("Toontrack"));
    assert!(s.accounts_for("Jamstix4"));
    assert!(s.accounts_for("EZdrummer 3"));
    assert!(!s.accounts_for("Old Synth Maker"));
}

#[test]
fn store_apps_own_the_folders_named_after_them() {
    use rewind_os_engine::leftovers::store_package_names;
    let mut s = Signals::default();
    for full in [
        "Claude_6.28899.6.0_x64__pzs8sxrjxfjjc",
        "5319275A.WhatsAppDesktop_2.2645.101.0_x64__cv1g1gvanyjgm",
    ] {
        for name in store_package_names(full) {
            s.add_name(&name);
        }
    }
    assert!(s.accounts_for("Claude-Data"));
    assert!(s.accounts_for("WhatsApp Desktop"));
    assert!(!s.accounts_for("OldGameStudio"));
}

#[test]
fn steam_lists_its_libraries_and_each_installed_game() {
    use rewind_os_engine::leftovers::{steam_game, steam_libraries};
    // As Steam writes them on Windows and on Linux.
    let vdf = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
	}
	"1"
	{
		"path"		"/home/ana/Games/SteamLibrary"
	}
}"#;
    let libraries = steam_libraries(vdf);
    assert_eq!(libraries.len(), 2);
    assert!(libraries[0].to_string_lossy().ends_with(r"(x86)\Steam"));
    let acf = r#""AppState"
{
	"appid"		"413150"
	"name"		"Stardew Valley"
	"installdir"		"Stardew Valley"
	"SizeOnDisk"		"682014543"
}"#;
    let mut s = Signals::default();
    for name in steam_game(acf) {
        s.add_name(&name);
    }
    // Its saves (`%APPDATA%\StardewValley`, `~/.config/StardewValley`) belong to it.
    assert!(s.accounts_for("StardewValley"));
    assert!(!s.accounts_for("TheIsle"));
}

#[test]
fn a_program_file_named_with_its_version_owns_its_folder() {
    let mut s = Signals::default();
    s.add_name("Obsidian-1.5.3");
    s.add_name("cursor_0.42.3_x86_64");
    assert!(s.accounts_for("obsidian"));
    assert!(s.accounts_for("Cursor"));
    assert!(!s.accounts_for("OldGameStudio"));
}

#[test]
fn only_real_folders_nothing_accounts_for_are_candidates() {
    use rewind_os_engine::leftovers::candidates_in;
    use rewind_os_platform::Os;
    let root = tempfile::tempdir().unwrap();
    for dir in [
        "Mozilla Firefox",
        "OldGameStudio",
        "gtk-3.0",
        "Common Files",
    ] {
        std::fs::create_dir(root.path().join(dir)).unwrap();
    }
    std::fs::write(root.path().join("notes.txt"), b"a file, never a candidate").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        root.path().join("OldGameStudio"),
        root.path().join("Linked Game"),
    )
    .unwrap();
    let names = |os| {
        let mut found: Vec<String> = candidates_in(root.path(), &signals(), os)
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        found.sort();
        found
    };
    // `gtk-3.0` is the desktop's own on Linux, an unknown folder elsewhere.
    assert_eq!(names(Os::Linux), ["OldGameStudio"]);
    assert_eq!(names(Os::Windows), ["OldGameStudio", "gtk-3.0"]);
}
