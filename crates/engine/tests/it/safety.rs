//! The guard must refuse anything whose loss could stop the computer from working or wipe a
//! user's folders, and still allow what cleanup and the analyzer are for.

use rewind_os_engine::safety::Guard;
use rewind_os_platform::Os;
use rewind_os_platform::paths::Known;
use std::path::{Path, PathBuf};

fn refused_by_hand(guard: &Guard, path: &str) -> bool {
    guard.removal_refusal(Path::new(path)).is_some()
}

fn refused_by_cleanup(guard: &Guard, path: &str) -> bool {
    guard.cleanup_refusal(Path::new(path)).is_some()
}

#[cfg(unix)]
fn unix_guard() -> Guard {
    let home = PathBuf::from("/home/ana");
    Guard::for_system(
        Os::Linux,
        &[
            (Known::Home, home.clone()),
            (Known::Cache, home.join(".cache")),
            (Known::Config, home.join(".config")),
            (Known::Data, home.join(".local/share")),
            (Known::Downloads, home.join("Downloads")),
            (Known::Temp, PathBuf::from("/tmp")),
        ],
    )
}

#[cfg(unix)]
#[test]
fn system_and_user_roots_are_refused_by_hand() {
    let g = unix_guard();
    for path in [
        "/",
        "/usr",
        "/usr/lib/libc.so.6",
        "/etc/fstab",
        "/boot/vmlinuz",
        "/var",
        "/var/lib/dpkg",
        "/home",
        "/home/ana",
        "/home/bob",
        "/home/ana/Documents",
        "/home/ana/.cache",
        "/usr/../usr/bin",
        "/home/ana/../../etc",
    ] {
        assert!(refused_by_hand(&g, path), "{path} was allowed");
    }
}

#[cfg(unix)]
#[test]
fn ordinary_files_and_folders_are_allowed_by_hand() {
    let g = unix_guard();
    for path in [
        "/home/ana/Downloads/big.iso",
        "/home/ana/Documents/old-report.pdf",
        "/home/ana/.cache/thumbnails",
        "/opt/some-app",
        "/tmp/build-output",
    ] {
        assert!(!refused_by_hand(&g, path), "{path} was refused");
    }
}

#[cfg(unix)]
#[test]
fn cleanup_may_enter_curated_system_places_but_never_core_ones_or_personal_folders() {
    let g = unix_guard();
    assert!(!refused_by_cleanup(&g, "/var/cache/apt/archives/x.deb"));
    assert!(!refused_by_cleanup(&g, "/var/lib/systemd/coredump/core.1"));
    assert!(!refused_by_cleanup(
        &g,
        "/home/ana/.cache/thumbnails/normal"
    ));
    for path in [
        "/etc/passwd",
        "/usr/share",
        "/var/lib/dpkg/status",
        "/home/ana/Pictures/cat.jpg",
        "/home/ana",
    ] {
        assert!(refused_by_cleanup(&g, path), "{path} was allowed");
    }
}

#[cfg(windows)]
fn windows_guard() -> Guard {
    let home = PathBuf::from(r"C:\Users\ana");
    Guard::for_system(
        Os::Windows,
        &[
            (Known::Home, home.clone()),
            (Known::LocalAppData, home.join(r"AppData\Local")),
            (Known::RoamingAppData, home.join(r"AppData\Roaming")),
            (Known::Temp, home.join(r"AppData\Local\Temp")),
            (Known::Windows, PathBuf::from(r"C:\Windows")),
            (Known::ProgramFiles, PathBuf::from(r"C:\Program Files")),
            (Known::ProgramData, PathBuf::from(r"C:\ProgramData")),
        ],
    )
}

#[cfg(windows)]
#[test]
fn windows_system_and_user_roots_are_refused_by_hand() {
    let g = windows_guard();
    for path in [
        r"C:\",
        r"C:\Windows",
        r"C:\windows\system32",
        r"C:\Windows\System32\drivers\x.sys",
        r"C:\Windows\WinSxS",
        r"C:\Windows\..\Windows\System32",
        r"C:\Users",
        r"C:\Users\bob",
        r"C:\Users\ana",
        r"C:\Users\ana\Documents",
        r"C:\Program Files",
        r"C:\ProgramData",
        r"C:\pagefile.sys",
        r"C:\bootmgr",
        r"C:\Boot",
        r"C:\Recovery",
    ] {
        assert!(refused_by_hand(&g, path), "{path} was allowed");
    }
}

#[cfg(windows)]
#[test]
fn windows_ordinary_places_are_allowed() {
    let g = windows_guard();
    for path in [
        r"C:\Users\ana\Downloads\setup.exe",
        r"C:\Program Files\Old Game",
        r"C:\ProgramData\Old Vendor",
        r"C:\Users\ana\AppData\Local\Temp\x",
    ] {
        assert!(!refused_by_hand(&g, path), "{path} was refused");
    }
    assert!(!refused_by_cleanup(&g, r"C:\Windows\Temp\old.tmp"));
    assert!(!refused_by_cleanup(
        &g,
        r"C:\Windows\SoftwareDistribution\Download\abc"
    ));
    assert!(refused_by_cleanup(&g, r"C:\Windows\System32\config"));
    assert!(refused_by_cleanup(&g, r"C:\Users\ana\Documents\x.docx"));
}

/// A protected folder reached through a link must stay protected even for paths inside it that
/// do not exist yet (on macOS `/etc` is a link to `/private/etc`).
#[cfg(unix)]
#[test]
fn links_to_protected_folders_are_seen_through_for_missing_paths() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real-home");
    std::fs::create_dir_all(real.join("Documents")).unwrap();
    let link = root.path().join("home-link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let g = Guard::for_system(Os::Linux, &[(Known::Home, link.clone())]);
    for path in [
        real.join("Documents/not-yet.txt"),
        link.join("Documents/not-yet.txt"),
    ] {
        assert!(
            g.cleanup_refusal(&path).is_some(),
            "{} was allowed",
            path.display()
        );
    }
    assert!(
        g.removal_refusal(&real).is_some(),
        "the real home behind the link was allowed"
    );
}

/// macOS mounts user data at /System/Volumes/Data and resolving /home or /Users can lead there;
/// that is user data, not the protected /System.
#[cfg(unix)]
#[test]
fn the_macos_data_volume_counts_as_the_root_it_mirrors() {
    let g = unix_guard();
    assert!(!refused_by_hand(
        &g,
        "/System/Volumes/Data/home/ana/Downloads/big.iso"
    ));
    assert!(refused_by_hand(&g, "/System/Library/CoreServices"));
    assert!(refused_by_hand(&g, "/System/Volumes/Data/home/ana"));
}
