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
