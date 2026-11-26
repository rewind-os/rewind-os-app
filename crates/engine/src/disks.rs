//! Whether the drives report problems, and keeping them optimized.
//!
//! - Windows: `Get-PhysicalDisk` gives each drive and the health it reports itself; its
//!   reliability counters (temperature, wear, power-on hours, uncorrected errors) need
//!   administrator rights. Windows optimizes drives on its own with the `ScheduledDefrag` task;
//!   `defrag /C /O` runs the same optimization now (TRIM on SSDs, defragmentation on hard
//!   drives).
//! - Linux: `lsblk` lists the drives and udisks (`udisksctl dump`) gives what their SMART data
//!   says, without a password. `fstrim.timer` trims on its own; `fstrim --all` trims now.
//!
//! Reading changes nothing. Optimizing moves no file and deletes nothing, but it can take long
//! on a hard drive, so it is only run when asked.

use crate::{EngineError, Os, Result};
use chrono::{DateTime, Utc};
use rewind_os_platform::{elevation, process};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// What a drive is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Media {
    Ssd,
    Hdd,
    Unknown,
}

impl Media {
    /// English; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Media::Ssd => "SSD",
            Media::Hdd => "Hard drive",
            Media::Unknown => "Drive",
        }
    }
}

/// How a drive is doing, from good to bad. `Unknown` when nothing could be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Good,
    Unknown,
    Warning,
    Failing,
}

impl Health {
    /// English; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Health::Good => "Healthy",
            Health::Unknown => "Unknown",
            Health::Warning => "Needs attention",
            Health::Failing => "Failing",
        }
    }
}

/// Why a drive needs attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Problem {
    /// The drive itself says it is failing.
    ReportsFailing,
    /// The drive itself reports a warning.
    ReportsWarning,
    /// Most of its rated writes are used up.
    WornOut,
    /// It could not read or write some data.
    Errors,
    /// Sectors it had to set aside.
    BadSectors,
    /// It runs hot.
    Hot,
}

impl Problem {
    /// English; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Problem::ReportsFailing => {
                "The drive reports that it is failing. Copy your files to another drive now."
            }
            Problem::ReportsWarning => "The drive reports a problem with itself.",
            Problem::WornOut => "Most of the writes it is rated for are used up.",
            Problem::Errors => "It could not read or write some data.",
            Problem::BadSectors => "It has set aside damaged sectors.",
            Problem::Hot => "It runs hot, which shortens its life.",
        }
    }
}

/// One physical drive.
#[derive(Debug, Clone, Serialize)]
pub struct Disk {
    /// The model, as the drive names itself.
    pub name: String,
    /// `0` (Windows disk number) or `/dev/nvme0n1`.
    pub device: String,
    pub media: Media,
    /// How it is connected: `NVMe`, `SATA`, `USB`… Empty when unknown.
    pub bus: String,
    pub size: u64,
    /// Drive letters (`C:`) or mount points (`/`, `/home`) on it.
    pub volumes: Vec<String>,
    /// What the drive reports about itself.
    pub reported: Health,
    pub temperature_c: Option<u32>,
    /// Share of its rated life used, in percent (SSDs).
    pub used_life: Option<u32>,
    pub power_on_hours: Option<u64>,
    /// Reads and writes it could not correct.
    pub errors: Option<u64>,
    pub bad_sectors: Option<u64>,
    /// The worst of what it reports and what its figures show.
    pub health: Health,
    pub problems: Vec<Problem>,
}

/// The system's own regular optimization.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Schedule {
    /// Whether the system optimizes drives on its own; `None` when it could not be read.
    pub enabled: Option<bool>,
    pub last: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub disks: Vec<Disk>,
    /// English reason the figures (temperature, wear, errors) could not be read; translate with
    /// `rewind_os_i18n::tr`.
    pub missing: Option<&'static str>,
    pub schedule: Schedule,
}

impl Report {
    /// The worst health among the drives.
    pub fn worst(&self) -> Option<Health> {
        self.disks.iter().map(|d| d.health).max()
    }
}

/// English reasons, translated with `rewind_os_i18n::tr`.
pub const DETAILS_NEED_ADMIN: &str =
    "Temperature, wear and errors can only be read with administrator rights.";
pub const NO_UDISKS: &str =
    "Install udisks2 so Rewind OS can read what the drives report about their health.";

/// From this many percent of rated life used, a drive needs attention.
pub const WORN_PERCENT: u32 = 90;
/// Temperatures that shorten a drive's life, in °C.
const HOT_HDD_C: u32 = 55;
const HOT_SSD_C: u32 = 70;

pub fn report() -> Result<Report> {
    match Os::current() {
        Os::Windows => windows(),
        Os::Linux => Ok(linux()),
        Os::MacOs => Err(unsupported("disk.health")),
    }
}

/// Runs the system's own optimization of every drive now and returns what it printed.
pub fn optimize() -> Result<String> {
    if !elevation::is_elevated() {
        return Err(EngineError::NeedsAdmin);
    }
    let out = match Os::current() {
        // /O: the right optimization for each kind of drive.
        Os::Windows => process::run_checked("defrag.exe", &["/C", "/O"])?,
        Os::Linux => process::run_checked("fstrim", &["--all", "--verbose"])?,
        Os::MacOs => return Err(unsupported("disk.optimize")),
    };
    Ok(format!("{}{}", out.stdout, out.stderr).trim().to_owned())
}

fn unsupported(tool: &'static str) -> EngineError {
    EngineError::Unsupported {
        tool,
        os: Os::current().name(),
    }
}

/// The health a drive's own report and its figures add up to, and why.
pub fn assess(disk: &Disk) -> (Health, Vec<Problem>) {
    let mut health = disk.reported;
    let mut problems = Vec::new();
    let mut flag = |problem: Problem, level: Health| {
        problems.push(problem);
        health = health.max(level);
    };
    match disk.reported {
        Health::Failing => flag(Problem::ReportsFailing, Health::Failing),
        Health::Warning => flag(Problem::ReportsWarning, Health::Warning),
        Health::Good | Health::Unknown => {}
    }
    if disk.used_life.is_some_and(|p| p >= WORN_PERCENT) {
        flag(Problem::WornOut, Health::Warning);
    }
    if disk.errors.is_some_and(|n| n > 0) {
        flag(Problem::Errors, Health::Warning);
    }
    if disk.bad_sectors.is_some_and(|n| n > 0) {
        flag(Problem::BadSectors, Health::Warning);
    }
    let hot = if disk.media == Media::Hdd {
        HOT_HDD_C
    } else {
        HOT_SSD_C
    };
    if disk.temperature_c.is_some_and(|c| c >= hot) {
        flag(Problem::Hot, Health::Warning);
    }
    (health, problems)
}

fn finish(mut disks: Vec<Disk>) -> Vec<Disk> {
    for disk in &mut disks {
        let (health, problems) = assess(disk);
        disk.health = health;
        disk.problems = problems;
    }
    disks
}

fn blank(name: String, device: String) -> Disk {
    Disk {
        name,
        device,
        media: Media::Unknown,
        bus: String::new(),
        size: 0,
        volumes: Vec::new(),
        reported: Health::Unknown,
        temperature_c: None,
        used_life: None,
        power_on_hours: None,
        errors: None,
        bad_sectors: None,
        health: Health::Unknown,
        problems: Vec::new(),
    }
}

// ---- Windows ----------------------------------------------------------------------------------

/// Every physical drive with its reliability counters (empty without administrator rights),
/// its drive letters, and the scheduled optimization task, as JSON.
const WINDOWS_SCRIPT: &str = r#"[Console]::OutputEncoding = [Text.Encoding]::UTF8
$disks = @(Get-PhysicalDisk -ErrorAction Stop | ForEach-Object {
  $r = $null
  try { $r = $_ | Get-StorageReliabilityCounter -ErrorAction Stop } catch {}
  $letters = @(Get-Partition -DiskNumber ([int]$_.DeviceId) -ErrorAction SilentlyContinue | Where-Object { $_.DriveLetter } | ForEach-Object { [string]$_.DriveLetter })
  [pscustomobject]@{
    name = [string]$_.FriendlyName; number = [string]$_.DeviceId; media = [string]$_.MediaType
    bus = [string]$_.BusType; size = [uint64]$_.Size; health = [string]$_.HealthStatus
    counters = [bool]$r; temperature = $r.Temperature; wear = $r.Wear; hours = $r.PowerOnHours
    read_errors = $r.ReadErrorsUncorrected; write_errors = $r.WriteErrorsUncorrected; letters = $letters
  }
})
$task = Get-ScheduledTask -TaskPath '\Microsoft\Windows\Defrag\' -TaskName 'ScheduledDefrag' -ErrorAction SilentlyContinue
$last = $null
if ($task) { $info = $task | Get-ScheduledTaskInfo -ErrorAction SilentlyContinue; if ($info -and $info.LastRunTime) { $last = $info.LastRunTime.ToUniversalTime().ToString('o') } }
$state = $null
if ($task) { $state = [string]$task.State }
ConvertTo-Json -InputObject ([pscustomobject]@{ disks = $disks; task = $state; last = $last }) -Depth 4 -Compress"#;

fn windows() -> Result<Report> {
    let out = process::run_checked(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", WINDOWS_SCRIPT],
    )?;
    Ok(parse_windows(&out.stdout))
}

#[derive(Debug, Deserialize)]
struct WinOutput {
    #[serde(default)]
    disks: Vec<WinDisk>,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    last: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WinDisk {
    #[serde(default)]
    name: String,
    #[serde(default)]
    number: String,
    #[serde(default)]
    media: String,
    #[serde(default)]
    bus: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    health: String,
    #[serde(default)]
    counters: bool,
    #[serde(default)]
    temperature: Option<u64>,
    #[serde(default)]
    wear: Option<u64>,
    #[serde(default)]
    hours: Option<u64>,
    #[serde(default)]
    read_errors: Option<u64>,
    #[serde(default)]
    write_errors: Option<u64>,
    #[serde(default)]
    letters: Vec<String>,
}

/// `MediaType` as its name or its number.
fn windows_media(text: &str) -> Media {
    match text.trim().to_ascii_uppercase().as_str() {
        "SSD" | "4" | "SCM" | "5" => Media::Ssd,
        "HDD" | "3" => Media::Hdd,
        _ => Media::Unknown,
    }
}

/// `HealthStatus` as its name or its number.
fn windows_health(text: &str) -> Health {
    match text.trim().to_ascii_lowercase().as_str() {
        "healthy" | "0" => Health::Good,
        "warning" | "1" => Health::Warning,
        "unhealthy" | "2" => Health::Failing,
        _ => Health::Unknown,
    }
}

/// `BusType` as its name or its number; empty when it says nothing useful.
fn windows_bus(text: &str) -> String {
    let name = match text.trim() {
        "17" => "NVMe",
        "11" => "SATA",
        "7" => "USB",
        "3" => "ATA",
        "10" => "SAS",
        "8" => "RAID",
        "12" => "SD",
        "13" => "MMC",
        "0" | "Unknown" | "" => "",
        // "15" (file-backed virtual) and the names Windows already prints.
        "15" => "Virtual",
        other => other,
    };
    name.to_owned()
}

/// Reads what the Windows script prints.
pub fn parse_windows(json: &str) -> Report {
    let Ok(out) = serde_json::from_str::<WinOutput>(json.trim()) else {
        return Report {
            disks: Vec::new(),
            missing: None,
            schedule: Schedule::default(),
        };
    };
    let any_counters = out.disks.iter().any(|d| d.counters);
    let disks = out
        .disks
        .into_iter()
        .map(|d| {
            let media = windows_media(&d.media);
            let errors = match (d.read_errors, d.write_errors) {
                (None, None) => None,
                (r, w) => Some(r.unwrap_or(0).saturating_add(w.unwrap_or(0))),
            };
            let name = if d.name.trim().is_empty() {
                format!("Disk {}", d.number)
            } else {
                d.name.trim().to_owned()
            };
            Disk {
                media,
                bus: windows_bus(&d.bus),
                size: d.size,
                volumes: d.letters.iter().map(|l| format!("{}:", l.trim())).collect(),
                reported: windows_health(&d.health),
                // Windows reports 0 when it does not know the temperature.
                temperature_c: d
                    .temperature
                    .filter(|c| *c > 0)
                    .and_then(|c| u32::try_from(c).ok()),
                // Hard drives have no rated writes; Windows reports 0 for them.
                used_life: d
                    .wear
                    .filter(|_| media == Media::Ssd)
                    .and_then(|w| u32::try_from(w).ok()),
                power_on_hours: d.hours.filter(|h| *h > 0),
                errors,
                ..blank(name, d.number)
            }
        })
        .collect::<Vec<_>>();
    let missing = (!any_counters && !disks.is_empty()).then_some(DETAILS_NEED_ADMIN);
    let enabled = out
        .task
        .map(|state| !state.eq_ignore_ascii_case("disabled"));
    // A task that never ran reports a date in 1999.
    let last = out
        .last
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc))
        .filter(|d| d.timestamp() > 946_684_800);
    Report {
        disks: finish(disks),
        missing,
        schedule: Schedule { enabled, last },
    }
}

// ---- Linux ------------------------------------------------------------------------------------

fn linux() -> Report {
    let mut disks = process::run_checked(
        "lsblk",
        &[
            "--json",
            "--bytes",
            "--output",
            "NAME,MODEL,SIZE,ROTA,TRAN,TYPE,MOUNTPOINTS",
        ],
    )
    .map(|out| parse_lsblk(&out.stdout))
    .unwrap_or_default();
    let dump = process::run_checked("udisksctl", &["dump"]).ok();
    let missing = match &dump {
        Some(out) => {
            apply_udisks(&mut disks, &parse_udisks(&out.stdout));
            None
        }
        None => Some(NO_UDISKS),
    };
    let enabled = process::run("systemctl", &["is-enabled", "fstrim.timer"])
        .ok()
        .and_then(|out| match out.stdout.trim() {
            "enabled" | "static" | "alias" => Some(true),
            "disabled" | "masked" => Some(false),
            _ => None,
        });
    let last = process::run(
        "systemctl",
        &[
            "show",
            "fstrim.timer",
            "--property=LastTriggerUSec",
            "--timestamp=unix",
        ],
    )
    .ok()
    .and_then(|out| parse_systemd_unix(&out.stdout));
    Report {
        missing: missing.filter(|_| !disks.is_empty()),
        disks: finish(disks),
        schedule: Schedule { enabled, last },
    }
}

/// `LastTriggerUSec=@1732550400` as a moment; `None` when the timer never ran.
pub fn parse_systemd_unix(output: &str) -> Option<DateTime<Utc>> {
    let value = output.trim().split_once('=')?.1.trim();
    let seconds: i64 = value.strip_prefix('@')?.parse().ok()?;
    (seconds > 0)
        .then(|| DateTime::from_timestamp(seconds, 0))
        .flatten()
}

/// A JSON flag as lsblk prints it: `true`, `1` or `"1"`.
fn flag(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_u64().map(|n| n != 0),
        Value::String(s) => match s.trim() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn number(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

/// The mount points of a device and of every partition on it.
fn mount_points(device: &Value, into: &mut Vec<String>) {
    let mut add = |point: &Value| {
        if let Some(p) = point.as_str()
            && p.starts_with('/')
            && !into.iter().any(|seen| seen == p)
        {
            into.push(p.to_owned());
        }
    };
    match device.get("mountpoints") {
        Some(Value::Array(points)) => points.iter().for_each(&mut add),
        // lsblk before util-linux 2.37 prints one mount point.
        _ => {
            if let Some(point) = device.get("mountpoint") {
                add(point);
            }
        }
    }
    if let Some(Value::Array(children)) = device.get("children") {
        for child in children {
            mount_points(child, into);
        }
    }
}

/// The physical drives in `lsblk --json --bytes` output: no partitions, loop devices, RAM
/// disks or empty card readers.
pub fn parse_lsblk(json: &str) -> Vec<Disk> {
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(Value::Array(devices)) = root.get("blockdevices") else {
        return Vec::new();
    };
    devices
        .iter()
        .filter_map(|d| {
            let name = text(d.get("name"));
            let virtual_device = ["loop", "ram", "zram", "nbd"]
                .iter()
                .any(|p| name.starts_with(p));
            let size = number(d.get("size")).unwrap_or(0);
            if text(d.get("type")) != "disk" || virtual_device || size == 0 {
                return None;
            }
            let transport = text(d.get("tran"));
            let nvme = name.starts_with("nvme") || transport == "nvme";
            let media = match flag(d.get("rota")) {
                _ if nvme => Media::Ssd,
                Some(false) => Media::Ssd,
                Some(true) => Media::Hdd,
                None => Media::Unknown,
            };
            let bus = match transport.as_str() {
                _ if nvme => "NVMe".to_owned(),
                "" => String::new(),
                "sata" | "ata" => "SATA".to_owned(),
                "usb" => "USB".to_owned(),
                "mmc" => "MMC".to_owned(),
                other => other.to_ascii_uppercase(),
            };
            let mut volumes = Vec::new();
            mount_points(d, &mut volumes);
            let model = text(d.get("model"));
            let device = format!("/dev/{name}");
            Some(Disk {
                media,
                bus,
                size,
                volumes,
                ..blank(
                    if model.is_empty() {
                        device.clone()
                    } else {
                        model
                    },
                    device,
                )
            })
        })
        .collect()
}

/// `udisksctl dump`: each object's properties, keyed by `Interface.Property` with the
/// interface's last name (`Block.Device`, `Ata.SmartFailing`, `Controller.SmartTemperature`).
pub fn parse_udisks(output: &str) -> HashMap<String, HashMap<String, String>> {
    let mut objects: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut object = String::new();
    let mut interface = String::new();
    for line in output.lines() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if indent == 0 {
            if let Some(path) = trimmed.strip_suffix(':') {
                object = path.to_owned();
                interface.clear();
            }
            continue;
        }
        if indent <= 2 {
            if let Some(name) = trimmed.strip_suffix(':') {
                interface = name.rsplit('.').next().unwrap_or(name).to_owned();
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        // Lines that carry on a list (more symlinks, mount points) have no property name.
        if object.is_empty()
            || interface.is_empty()
            || key.is_empty()
            || !key.chars().all(|c| c.is_ascii_alphanumeric())
        {
            continue;
        }
        objects
            .entry(object.clone())
            .or_default()
            .insert(format!("{interface}.{key}"), value.trim().to_owned());
    }
    objects
}

/// Kelvin, as udisks reports temperatures, in whole °C; `None` for 0 (not known).
fn celsius(kelvin: &str) -> Option<u32> {
    let k: f64 = kelvin.trim().parse().ok()?;
    let c = (k - 273.15).round();
    (k > 0.0 && (0.0..200.0).contains(&c)).then_some(c as u32)
}

/// Adds what udisks knows about each drive's health.
pub fn apply_udisks(disks: &mut [Disk], objects: &HashMap<String, HashMap<String, String>>) {
    for disk in disks {
        let Some(drive) = objects
            .values()
            .find(|o| o.get("Block.Device").is_some_and(|d| *d == disk.device))
            .and_then(|block| block.get("Block.Drive"))
            .map(|d| d.trim_matches('\'').to_owned())
            .and_then(|path| objects.get(&path))
        else {
            continue;
        };
        let get = |key: &str| drive.get(key).map(String::as_str);
        if get("Ata.SmartSupported") == Some("true") && get("Ata.SmartEnabled") == Some("true") {
            disk.reported = if get("Ata.SmartFailing") == Some("true") {
                Health::Failing
            } else {
                Health::Good
            };
            disk.temperature_c = get("Ata.SmartTemperature").and_then(celsius);
            disk.power_on_hours = get("Ata.SmartPowerOnSeconds")
                .and_then(|s| s.parse::<u64>().ok())
                .map(|s| s / 3600)
                .filter(|h| *h > 0);
            disk.bad_sectors = get("Ata.SmartNumBadSectors").and_then(|s| s.parse().ok());
        } else if let Some(warning) = get("Controller.SmartCriticalWarning") {
            // An empty list means no warning.
            let warning = warning.trim_matches(|c| c == '[' || c == ']').trim();
            disk.reported = if warning.is_empty() {
                Health::Good
            } else {
                Health::Warning
            };
            disk.temperature_c = get("Controller.SmartTemperature").and_then(celsius);
            disk.power_on_hours = get("Controller.SmartPowerOnHours")
                .and_then(|s| s.parse().ok())
                .filter(|h| *h > 0);
        }
    }
}
