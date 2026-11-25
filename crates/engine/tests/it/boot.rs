use rewind_os_engine::boot::{
    Kind, parse_duration, parse_linux_blame, parse_linux_desktop, parse_linux_time, parse_windows,
};

#[test]
fn systemd_durations_add_up_their_parts() {
    assert_eq!(parse_duration("845ms"), Some(845));
    assert_eq!(parse_duration("2.345s"), Some(2345));
    assert_eq!(parse_duration("1min 2.5s"), Some(62_500));
    assert_eq!(parse_duration("1h 2min"), parse_duration("62min"));
    assert_eq!(parse_duration("soon"), None);
    assert_eq!(parse_duration(""), None);
}

#[test]
fn the_whole_start_is_the_total_after_the_equals_sign() {
    let out = "Startup finished in 3.1s (firmware) + 1.2s (loader) + 2.345s (kernel) + 5.123s (userspace) = 11.768s\n\
               graphical.target reached after 4.9s in userspace.\n";
    assert_eq!(parse_linux_time(out), Some(11_768));
    assert!(parse_linux_desktop(out).is_some_and(|ms| ms < 11_768));
}

#[test]
fn a_start_with_one_part_only_is_that_part() {
    // As WSL reports it: no kernel part, no total.
    let out = "Startup finished in 679ms (userspace) \ngraphical.target reached after 667ms in userspace.\n";
    assert_eq!(parse_linux_time(out), Some(679));
    assert_eq!(parse_linux_desktop(out), Some(667));
}

#[test]
fn output_without_a_finished_start_gives_nothing() {
    assert_eq!(parse_linux_time("Bootup is not yet finished.\n"), None);
}

#[test]
fn blame_lists_each_unit_with_its_time() {
    let out = "1min 2.3s plymouth-quit-wait.service\n   1.128s apt-daily-upgrade.service\n 570ms apt-daily.service\nnot a line\n";
    let units = parse_linux_blame(out);
    assert_eq!(units.len(), 3);
    assert!(
        units
            .iter()
            .any(|(u, ms)| u == "plymouth-quit-wait.service" && *ms > 60_000)
    );
    assert!(
        units
            .iter()
            .any(|(u, ms)| u == "apt-daily.service" && *ms == 570)
    );
}

const EVENTS: &str = r#"[
 {"id":100,"at":"2026-11-20T08:00:00Z","data":{"BootTime":"41000","MainPathBootTime":"23000"}},
 {"id":100,"at":"2026-11-24T08:00:00Z","data":{"BootTime":"35000","MainPathBootTime":"20000"}},
 {"id":100,"at":"2026-11-22T08:00:00Z","data":{"BootTime":"0"}},
 {"id":101,"at":"2026-11-20T08:00:01Z","data":{"Name":"Updater.exe","FriendlyName":"Vendor Updater","DegradationTime":"1500"}},
 {"id":101,"at":"2026-11-24T08:00:01Z","data":{"Name":"updater.EXE","FriendlyName":"Vendor Updater","DegradationTime":"2500"}},
 {"id":103,"at":"2026-11-24T08:00:02Z","data":{"Name":"SlowSvc","FriendlyName":"","DegradationTime":"9000"}},
 {"id":102,"at":"2026-11-24T08:00:03Z","data":{"Name":"quick.sys","DegradationTime":"0"}},
 {"id":101,"at":"2026-11-24T08:00:04Z","data":{"FriendlyName":"No file"}}
]"#;

#[test]
fn windows_starts_are_listed_newest_first_and_empty_ones_dropped() {
    let report = parse_windows(EVENTS);
    assert_eq!(report.boots.len(), 2);
    assert!(report.boots[0].at > report.boots[1].at);
    assert!(
        report
            .boots
            .iter()
            .all(|b| b.desktop_ms.is_some_and(|d| d < b.total_ms))
    );
}

#[test]
fn what_slowed_windows_down_is_added_up_per_file_largest_first() {
    let report = parse_windows(EVENTS);
    let names: Vec<&str> = report.slow.iter().map(|s| s.name.as_str()).collect();
    // The service delayed one start by 9 s; the updater two starts by 4 s in all.
    assert_eq!(names, ["SlowSvc", "Vendor Updater"]);
    let updater = &report.slow[1];
    assert_eq!(
        (updater.kind, updater.times, updater.delay_ms),
        (Kind::Program, 2, 4000)
    );
    assert_eq!(report.slow[0].kind, Kind::Service);
}

#[test]
fn a_single_windows_event_is_read_too() {
    let one = r#"{"id":100,"at":"2026-11-24T08:00:00Z","data":{"BootTime":"30000"}}"#;
    assert_eq!(parse_windows(one).boots.len(), 1);
    assert!(parse_windows("").boots.is_empty());
}
