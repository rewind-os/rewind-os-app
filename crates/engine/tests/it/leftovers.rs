use rewindows_engine::leftovers::{Confidence, Signals, confidence};
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
