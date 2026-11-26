use chrono::{TimeZone, Utc};
use rewind_os_engine::restore::{ShadowCopy, all_but_newest, is_shadow_id, parse};

fn copy(id: &str, drive: &str, day: u32) -> ShadowCopy {
    ShadowCopy {
        id: id.to_owned(),
        drive: drive.to_owned(),
        created: Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0).unwrap(),
        description: None,
    }
}

#[test]
fn the_newest_copy_of_each_drive_is_always_kept() {
    let copies = vec![
        copy("a", r"C:\", 1),
        copy("b", r"C:\", 9),
        copy("c", r"c:\", 5),
        copy("d", r"D:\", 3),
    ];
    let doomed: Vec<&str> = all_but_newest(&copies)
        .iter()
        .map(|c| c.id.as_str())
        .collect();
    assert!(!doomed.contains(&"b"), "the newest on C: is kept");
    assert!(!doomed.contains(&"d"), "the only one on D: is kept");
    assert!(doomed.contains(&"a") && doomed.contains(&"c"));
    assert!(all_but_newest(&copies[..1]).is_empty());
}

#[test]
fn only_a_guid_reaches_the_delete_script() {
    assert!(is_shadow_id("{4B1E2C3D-0A1B-4C2D-8E3F-123456789ABC}"));
    for bad in [
        "4B1E2C3D-0A1B-4C2D-8E3F-123456789ABC",
        "{4B1E2C3D-0A1B-4C2D-8E3F-123456789ABC}' OR ID LIKE '%",
        "{x}",
        "",
    ] {
        assert!(!is_shadow_id(bad), "{bad}");
    }
}

#[test]
fn copies_get_their_drive_letter_and_restore_point() {
    let json = r#"{"letters":{"\\\\?\\Volume{11111111-2222-3333-4444-555555555555}\\":"C:\\"},
      "copies":[
        {"id":"{AAAAAAAA-0000-0000-0000-000000000001}","volume":"\\\\?\\Volume{11111111-2222-3333-4444-555555555555}\\","created":"2026-10-09T12:00:30Z"},
        {"id":"{AAAAAAAA-0000-0000-0000-000000000002}","volume":"\\\\?\\Volume{11111111-2222-3333-4444-555555555555}\\","created":"2026-10-01T08:00:00Z"},
        {"id":"not a guid","volume":"x","created":"2026-10-02T08:00:00Z"}],
      "points":[{"description":"Windows Update","created":"2026-10-09T12:00:00Z"}],
      "storage":[{"volume":"\\\\?\\Volume{11111111-2222-3333-4444-555555555555}\\","used":3000000000,"max":18446744073709551615}]}"#;
    let report = parse(json).unwrap();
    assert_eq!(report.copies.len(), 2, "a copy without a GUID is left out");
    assert!(
        report.copies[0].created < report.copies[1].created,
        "oldest first"
    );
    assert!(report.copies.iter().all(|c| c.drive == r"C:\"));
    assert_eq!(
        report.copies[1].description.as_deref(),
        Some("Windows Update")
    );
    assert_eq!(report.copies[0].description, None);
    assert_eq!(
        report.storage[0].max, None,
        "Windows' 'no limit' is no limit"
    );
    assert!(report.used() > 0);
}
