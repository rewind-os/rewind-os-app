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
