use rewind_os_engine::disks::{
    DETAILS_NEED_ADMIN, Health, Media, Problem, apply_udisks, parse_lsblk, parse_systemd_unix,
    parse_udisks, parse_windows,
};
use rewind_os_engine::recommend::{Action, Facts, Level, rules};

/// Two drives as the Windows script prints them with administrator rights: a healthy NVMe SSD
/// holding C: and a hard drive Windows calls unhealthy.
const WINDOWS_ADMIN: &str = r#"{"disks":[
 {"name":"Samsung SSD 980 1TB","number":"0","media":"SSD","bus":"NVMe","size":1000204886016,"health":"Healthy",
  "counters":true,"temperature":38,"wear":3,"hours":5120,"read_errors":0,"write_errors":0,"letters":["C"]},
 {"name":"WDC WD20EZRZ","number":"1","media":"3","bus":"11","size":2000398934016,"health":"Unhealthy",
  "counters":true,"temperature":41,"wear":0,"hours":30210,"read_errors":12,"write_errors":null,"letters":["D","E"]}
],"task":"Ready","last":"2026-11-20T03:12:00.0000000Z"}"#;

#[test]
fn windows_drives_keep_what_each_reports_about_itself() {
    let report = parse_windows(WINDOWS_ADMIN);
    assert_eq!(report.disks.len(), 2);
    assert!(report.missing.is_none());
    let ssd = &report.disks[0];
    let hdd = &report.disks[1];
    assert_eq!(ssd.media, Media::Ssd);
    assert_eq!(ssd.health, Health::Good);
    assert!(ssd.problems.is_empty());
    assert_eq!(ssd.volumes, vec!["C:"]);
    // Numbers stand for the same names.
    assert_eq!(hdd.media, Media::Hdd);
    assert_eq!(hdd.bus, "SATA");
    assert_eq!(hdd.health, Health::Failing);
    assert!(hdd.problems.contains(&Problem::ReportsFailing));
    assert!(hdd.problems.contains(&Problem::Errors));
    // Hard drives have no rated writes: their 0 is not "nothing used".
    assert_eq!(hdd.used_life, None);
    assert_eq!(report.worst(), Some(Health::Failing));
    assert_eq!(report.schedule.enabled, Some(true));
    assert!(report.schedule.last.is_some());
}

#[test]
fn without_administrator_rights_windows_says_the_figures_are_missing() {
    let json = r#"{"disks":[{"name":"Disk","number":"0","media":"SSD","health":"Healthy","counters":false,
        "temperature":null,"wear":null,"hours":null,"read_errors":null,"write_errors":null,"letters":["C"]}],
        "task":"Disabled","last":"1999-11-30T00:00:00.0000000Z"}"#;
    let report = parse_windows(json);
    assert_eq!(report.missing, Some(DETAILS_NEED_ADMIN));
    let disk = &report.disks[0];
    assert_eq!(disk.health, Health::Good);
    assert_eq!((disk.temperature_c, disk.errors), (None, None));
    assert_eq!(report.schedule.enabled, Some(false));
    // A task that never ran is not "last run in 1999".
    assert!(report.schedule.last.is_none());
}

#[test]
fn figures_raise_a_drive_that_reports_itself_healthy() {
    let worn = WINDOWS_ADMIN.replace("\"wear\":3", "\"wear\":96");
    let worn = &parse_windows(&worn).disks[0];
    assert_eq!(worn.health, Health::Warning);
    assert_eq!(worn.problems, vec![Problem::WornOut]);
    // 60 °C is warm for an SSD, but too hot for a hard drive.
    let warm = WINDOWS_ADMIN
        .replace("\"temperature\":38", "\"temperature\":60")
        .replace("\"temperature\":41", "\"temperature\":60");
    let report = parse_windows(&warm);
    assert!(!report.disks[0].problems.contains(&Problem::Hot));
    assert!(report.disks[1].problems.contains(&Problem::Hot));
}

#[test]
fn unreadable_windows_output_lists_no_drives() {
    assert!(parse_windows("").disks.is_empty());
    assert!(
        parse_windows("Get-PhysicalDisk : Access denied")
            .disks
            .is_empty()
    );
}

const LSBLK: &str = r#"{"blockdevices":[
  {"name":"loop0","model":null,"size":4096,"rota":false,"tran":null,"type":"loop","mountpoints":["/snap/core/1"]},
  {"name":"zram0","model":null,"size":8589934592,"rota":false,"tran":null,"type":"disk","mountpoints":["[SWAP]"]},
  {"name":"sr0","model":"DVD","size":1073741312,"rota":true,"tran":"sata","type":"rom","mountpoints":[null]},
  {"name":"sdb","model":"Card Reader","size":0,"rota":true,"tran":"usb","type":"disk","mountpoints":[null]},
  {"name":"nvme0n1","model":"WD Blue SN570","size":500107862016,"rota":true,"tran":"nvme","type":"disk","mountpoints":[null],
   "children":[
     {"name":"nvme0n1p1","size":536870912,"type":"part","mountpoints":["/boot/efi"]},
     {"name":"nvme0n1p2","size":499570991104,"type":"part","mountpoints":["/","/home"]}]},
  {"name":"sda","model":"ST2000DM008","size":"2000398934016","rota":"1","tran":"sata","type":"disk","mountpoint":"/data"}
]}"#;

#[test]
fn only_physical_drives_are_listed_with_the_mount_points_on_them() {
    let disks = parse_lsblk(LSBLK);
    let devices: Vec<&str> = disks.iter().map(|d| d.device.as_str()).collect();
    assert_eq!(devices, vec!["/dev/nvme0n1", "/dev/sda"]);
    let nvme = &disks[0];
    // NVMe drives are solid state whatever the rotation flag says.
    assert_eq!((nvme.media, nvme.bus.as_str()), (Media::Ssd, "NVMe"));
    assert_eq!(nvme.volumes, vec!["/boot/efi", "/", "/home"]);
    // Older lsblk prints numbers as strings and one mount point.
    let hdd = &disks[1];
    assert_eq!((hdd.media, hdd.size), (Media::Hdd, 2_000_398_934_016));
    assert_eq!(hdd.volumes, vec!["/data"]);
    assert_eq!(hdd.health, Health::Unknown);
}

const UDISKS: &str = "\
/org/freedesktop/UDisks2/block_devices/nvme0n1:
  org.freedesktop.UDisks2.Block:
    Device:                     /dev/nvme0n1
    Drive:                      '/org/freedesktop/UDisks2/drives/WD_Blue_SN570'
    Symlinks:                   /dev/disk/by-id/nvme-WD_Blue_SN570
                                /dev/disk/by-path/pci-0000:01:00.0-nvme-1
/org/freedesktop/UDisks2/block_devices/sda:
  org.freedesktop.UDisks2.Block:
    Device:                     /dev/sda
    Drive:                      '/org/freedesktop/UDisks2/drives/ST2000DM008'
/org/freedesktop/UDisks2/drives/WD_Blue_SN570:
  org.freedesktop.UDisks2.Drive:
    Model:                      WD Blue SN570
  org.freedesktop.UDisks2.NVMe.Controller:
    SmartCriticalWarning:       
    SmartPowerOnHours:          1820
    SmartTemperature:           312.15
/org/freedesktop/UDisks2/drives/ST2000DM008:
  org.freedesktop.UDisks2.Drive:
    Model:                      ST2000DM008
  org.freedesktop.UDisks2.Drive.Ata:
    SmartEnabled:               true
    SmartFailing:               true
    SmartNumBadSectors:         24
    SmartPowerOnSeconds:        108000000
    SmartSupported:             true
    SmartTemperature:           313.15
";

#[test]
fn udisks_tells_each_drive_what_its_smart_data_says() {
    let mut disks = parse_lsblk(LSBLK);
    apply_udisks(&mut disks, &parse_udisks(UDISKS));
    let nvme = &disks[0];
    assert_eq!(nvme.reported, Health::Good);
    assert_eq!(nvme.temperature_c, Some(39));
    assert_eq!(nvme.power_on_hours, Some(1820));
    let hdd = &disks[1];
    assert_eq!(hdd.reported, Health::Failing);
    assert_eq!(hdd.power_on_hours, Some(30_000));
    assert_eq!(hdd.bad_sectors, Some(24));
    // Health is worked out again by the report; the figures here are what udisks said.
    let warning = UDISKS.replace(
        "SmartCriticalWarning:       ",
        "SmartCriticalWarning:       spare",
    );
    let mut disks = parse_lsblk(LSBLK);
    apply_udisks(&mut disks, &parse_udisks(&warning));
    assert_eq!(disks[0].reported, Health::Warning);
}

#[test]
fn a_timer_that_never_ran_has_no_last_run() {
    assert!(parse_systemd_unix("LastTriggerUSec=\n").is_none());
    assert!(parse_systemd_unix("LastTriggerUSec=@0\n").is_none());
    assert!(parse_systemd_unix("LastTriggerUSec=@1764115200\n").is_some());
}

#[test]
fn a_failing_drive_comes_before_everything_else_on_home() {
    let facts = Facts {
        disks: vec![
            ("Old disk".to_owned(), Health::Failing),
            ("Fine disk".to_owned(), Health::Good),
        ],
        startup_enabled: Some(20),
        ..Facts::default()
    };
    let list = rules(&facts);
    let first = list.first().expect("recommendations");
    assert_eq!(first.level, Level::Important);
    assert_eq!(first.action, Action::DiskHealth);
    assert_eq!(
        list.iter()
            .filter(|r| r.action == Action::DiskHealth)
            .count(),
        1
    );
}
