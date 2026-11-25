use chrono::{Duration, TimeZone, Utc};
use rewind_os_engine::update::{RELEASES_PAGE, due, is_newer, parse_latest};

#[test]
fn versions_compare_by_number_not_by_text() {
    assert!(is_newer("0.10.0", "0.9.0"));
    assert!(is_newer("1.0.0", "0.99.99"));
    assert!(!is_newer("0.9.0", "0.10.0"));
}

#[test]
fn the_same_version_is_not_newer() {
    assert!(!is_newer("0.12.0", "0.12.0"));
}

#[test]
fn a_release_comes_after_its_own_pre_releases_only() {
    assert!(is_newer("0.13.0", "0.13.0-rc.1"));
    assert!(!is_newer("0.13.0-rc.1", "0.13.0"));
    assert!(!is_newer("0.12.0", "0.13.0-rc.1"));
}

#[test]
fn text_that_is_not_a_version_is_never_newer() {
    for odd in ["", "latest", "1.2", "1.2.3.4", "1.2.x", "9.9.9-"] {
        assert!(!is_newer(odd, "0.1.0"), "{odd:?}");
    }
}

fn answer(tag: &str, url: &str, draft: bool, prerelease: bool) -> String {
    serde_json::json!({
        "tag_name": tag,
        "html_url": url,
        "draft": draft,
        "prerelease": prerelease,
        "published_at": "2026-11-25T18:00:00Z",
        "body": "notes",
    })
    .to_string()
}

#[test]
fn the_latest_release_is_read_without_the_v() {
    let url = format!("{RELEASES_PAGE}tag/v0.13.0");
    let release = parse_latest(&answer("v0.13.0", &url, false, false));
    assert_eq!(
        release.map(|r| (r.version, r.url)),
        Some(("0.13.0".to_owned(), url))
    );
}

#[test]
fn pages_outside_the_projects_releases_are_not_trusted() {
    for url in [
        "https://example.com/rewind-os/rewind-os-app/releases/tag/v0.13.0",
        "https://github.com/someone-else/rewind-os-app/releases/tag/v0.13.0",
        "http://github.com/rewind-os/rewind-os-app/releases/tag/v0.13.0",
    ] {
        assert!(
            parse_latest(&answer("v0.13.0", url, false, false)).is_none(),
            "{url}"
        );
    }
}

#[test]
fn drafts_pre_releases_and_odd_tags_are_not_offered() {
    let url = format!("{RELEASES_PAGE}tag/v0.13.0");
    assert!(parse_latest(&answer("v0.13.0", &url, true, false)).is_none());
    assert!(parse_latest(&answer("v0.13.0", &url, false, true)).is_none());
    assert!(parse_latest(&answer("nightly", &url, false, false)).is_none());
    assert!(parse_latest("not json").is_none());
}

#[test]
fn the_daily_check_asks_again_after_a_day_only_when_on() {
    let now = Utc
        .with_ymd_and_hms(2026, 11, 25, 12, 0, 0)
        .single()
        .expect("a date");
    assert!(due(true, None, now));
    assert!(!due(true, Some(now - Duration::hours(2)), now));
    assert!(due(true, Some(now - Duration::hours(25)), now));
    assert!(!due(false, None, now));
    assert!(!due(false, Some(now - Duration::days(30)), now));
}

#[test]
fn an_answer_from_the_future_does_not_stop_the_daily_check() {
    let now = Utc
        .with_ymd_and_hms(2026, 11, 25, 12, 0, 0)
        .single()
        .expect("a date");
    assert!(due(true, Some(now + Duration::days(3)), now));
}

use rewind_os_engine::Os;
use rewind_os_engine::update::{
    BAD_SIGNATURE, FROM_FLATPAK, FROM_PACKAGE_MANAGER, NOT_LISTED, OTHER_VERSION, Way,
    package_name, replace_programs, sha256_of, signed_checksum, way_of,
};
use std::path::{Path, PathBuf};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/update");
const PACKAGE: &str = "rewind-os-9.9.9-windows-x64-portable.zip";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join(name)).unwrap_or_default()
}

#[test]
fn a_signed_checksum_for_the_expected_version_is_trusted() {
    let sums = fixture("SHA256SUMS.txt");
    let found = signed_checksum(
        &sums,
        &fixture("SHA256SUMS.txt.minisig"),
        &fixture("test.pub"),
        "9.9.9",
        PACKAGE,
    );
    // The listed checksum is that of the bytes the fixture's README names.
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a temporary folder");
    let file = dir.path().join(PACKAGE);
    std::fs::write(&file, b"pkg-bytes\n").expect("the package is written");
    assert_eq!(found.ok(), sha256_of(&file).ok());
}

#[test]
fn changed_checksums_are_refused() {
    let sums = fixture("SHA256SUMS.txt").replace("b296", "0296");
    let found = signed_checksum(
        &sums,
        &fixture("SHA256SUMS.txt.minisig"),
        &fixture("test.pub"),
        "9.9.9",
        PACKAGE,
    );
    assert_eq!(found, Err(BAD_SIGNATURE));
}

#[test]
fn a_signature_by_another_key_is_refused() {
    let sums = fixture("SHA256SUMS.txt");
    let found = signed_checksum(
        &sums,
        &fixture("other-key.minisig"),
        &fixture("test.pub"),
        "9.9.9",
        PACKAGE,
    );
    assert_eq!(found, Err(BAD_SIGNATURE));
}

#[test]
fn the_signed_checksums_of_another_version_are_refused() {
    let sums = fixture("SHA256SUMS.txt");
    let signature = fixture("other-version.minisig");
    let key = fixture("test.pub");
    assert_eq!(
        signed_checksum(&sums, &signature, &key, "9.9.9", PACKAGE),
        Err(OTHER_VERSION)
    );
    assert_eq!(
        signed_checksum(
            &sums,
            &fixture("SHA256SUMS.txt.minisig"),
            &key,
            "9.9.8",
            PACKAGE
        ),
        Err(OTHER_VERSION)
    );
}

#[test]
fn only_the_exact_package_name_matches_a_checksum_line() {
    let sums = fixture("SHA256SUMS.txt");
    let signature = fixture("SHA256SUMS.txt.minisig");
    let key = fixture("test.pub");
    for name in [
        "rewind-os-9.9.9-windows-x64-portable",
        "portable.zip",
        "rewind-os-9.9.9-windows-arm64-portable.zip",
    ] {
        assert_eq!(
            signed_checksum(&sums, &signature, &key, "9.9.9", name),
            Err(NOT_LISTED),
            "{name}"
        );
    }
}

#[test]
fn the_release_key_is_a_minisign_public_key() {
    let key = include_str!("../../../../packaging/minisign.pub");
    // The fixture's signature is by another key: refused as unsigned, not as an unreadable key.
    let found = signed_checksum(
        &fixture("SHA256SUMS.txt"),
        &fixture("SHA256SUMS.txt.minisig"),
        key,
        "9.9.9",
        PACKAGE,
    );
    assert_eq!(found, Err(BAD_SIGNATURE));
    assert!(key.lines().nth(1).is_some_and(|line| line.len() == 56));
}

#[test]
fn the_msi_folder_is_updated_by_the_installer_and_any_other_windows_folder_in_place() {
    let pf = vec![PathBuf::from(r"C:\Program Files")];
    let msi = PathBuf::from(r"C:\Program Files\Rewind OS\rewind-os.exe");
    let portable = PathBuf::from(r"C:\Users\ana\Downloads\rewind-os\rewind-os.exe");
    let elsewhere = PathBuf::from(r"C:\Program Files\Other\rewind-os.exe");
    if cfg!(windows) {
        assert_eq!(way_of(&msi, Os::Windows, &pf, false).0, Way::Installer);
        assert_eq!(way_of(&elsewhere, Os::Windows, &pf, false).0, Way::Portable);
    }
    assert_eq!(way_of(&portable, Os::Windows, &pf, false).0, Way::Portable);
}

#[test]
fn copies_from_the_package_manager_or_flatpak_are_left_to_them() {
    let pf: Vec<PathBuf> = Vec::new();
    assert_eq!(
        way_of(Path::new("/usr/bin/rewind-os"), Os::Linux, &pf, false),
        (Way::Manual, Some(FROM_PACKAGE_MANAGER))
    );
    assert_eq!(
        way_of(Path::new("/app/bin/rewind-os"), Os::Linux, &pf, true),
        (Way::Manual, Some(FROM_FLATPAK))
    );
    assert_eq!(
        way_of(
            Path::new("/home/ana/rewind-os-0.13.0-linux-x64/bin/rewind-os"),
            Os::Linux,
            &pf,
            false
        )
        .0,
        Way::Portable
    );
    assert_eq!(
        way_of(Path::new("/home/ana/rewind-os"), Os::Linux, &pf, false).0,
        Way::Manual
    );
    assert_eq!(
        way_of(
            Path::new("/Applications/Rewind OS.app/x"),
            Os::MacOs,
            &pf,
            false
        )
        .0,
        Way::Manual
    );
}

#[test]
fn each_way_takes_its_own_package_and_others_none() {
    let msi = package_name("1.2.3", Os::Windows, Way::Installer, "x86_64");
    let zip = package_name("1.2.3", Os::Windows, Way::Portable, "aarch64");
    let tar = package_name("1.2.3", Os::Linux, Way::Portable, "x86_64");
    assert!(msi.is_some_and(|n| n.ends_with("-windows-x64.msi")));
    assert!(zip.is_some_and(|n| n.ends_with("-windows-arm64-portable.zip")));
    assert!(tar.is_some_and(|n| n.ends_with("-linux-x64.tar.gz")));
    assert!(package_name("1.2.3", Os::Linux, Way::Manual, "x86_64").is_none());
    assert!(package_name("1.2.3", Os::MacOs, Way::Portable, "aarch64").is_none());
    assert!(package_name("1.2.3", Os::Windows, Way::Portable, "riscv64").is_none());
}

fn names() -> Vec<String> {
    vec!["app".to_owned(), "app-cli".to_owned()]
}

fn copy_with(dir: &Path, content: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for n in names() {
        std::fs::write(dir.join(n), content)?;
    }
    Ok(())
}

#[test]
fn replaced_programs_are_new_and_the_old_ones_kept_aside() -> std::io::Result<()> {
    let root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let (dir, new) = (root.path().join("copy"), root.path().join("new"));
    copy_with(&dir, "old")?;
    copy_with(&new, "new")?;
    assert!(replace_programs(&dir, &new, &names()).is_ok());
    for n in names() {
        assert_eq!(std::fs::read_to_string(dir.join(&n))?, "new");
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("{n}.old")))?,
            "old"
        );
    }
    Ok(())
}

#[test]
fn an_incomplete_package_changes_nothing() -> std::io::Result<()> {
    let root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let (dir, new) = (root.path().join("copy"), root.path().join("new"));
    copy_with(&dir, "old")?;
    copy_with(&new, "new")?;
    std::fs::remove_file(new.join("app-cli"))?;
    assert!(replace_programs(&dir, &new, &names()).is_err());
    for n in names() {
        assert_eq!(std::fs::read_to_string(dir.join(&n))?, "old");
        assert!(!dir.join(format!("{n}.old")).exists());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_failed_copy_puts_every_program_back() -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let (dir, new) = (root.path().join("copy"), root.path().join("new"));
    copy_with(&dir, "old")?;
    copy_with(&new, "new")?;
    // The second program cannot be read, so its copy fails after the first one worked.
    let unreadable = new.join("app-cli");
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000))?;
    if std::fs::read(&unreadable).is_ok() {
        // Running as root, which reads anything: the failure cannot be made here.
        return Ok(());
    }
    assert!(replace_programs(&dir, &new, &names()).is_err());
    for n in names() {
        assert_eq!(std::fs::read_to_string(dir.join(&n))?, "old");
        assert!(!dir.join(format!("{n}.old")).exists());
    }
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}
