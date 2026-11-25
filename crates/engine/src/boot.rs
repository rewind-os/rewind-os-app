//! How long the computer takes to start, and what slowed it down.
//!
//! - Windows records every start in its own performance log
//!   (`Microsoft-Windows-Diagnostics-Performance/Operational`): event 100 is one start with its
//!   durations, and events 101 (programs), 102 (drivers), 103 (services) and 109 (devices) name
//!   what made a start slower than usual and by how much. Reading that log needs administrator
//!   rights.
//! - Linux with systemd: `systemd-analyze` gives the last start and how long each unit took.
//!
//! Read only: nothing here changes the system.

use crate::{EngineError, Os, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Starts kept, newest first.
const BOOTS: usize = 20;
/// Items listed as slowing the start down.
const LISTED: usize = 15;

/// One start of the computer.
#[derive(Debug, Clone, Serialize)]
pub struct Boot {
    pub at: Option<DateTime<Utc>>,
    /// From power on (or the kernel) until the start was over, in milliseconds.
    pub total_ms: u64,
    /// Until the desktop could be used, when known.
    pub desktop_ms: Option<u64>,
}

/// What a slow item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Program,
    Driver,
    Service,
    Device,
    /// A systemd unit that is neither a service nor a device (a mount, a socket…).
    Unit,
}

impl Kind {
    /// English; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Program => "Program",
            Kind::Driver => "Driver",
            Kind::Service => "Service",
            Kind::Device => "Device",
            Kind::Unit => "System step",
        }
    }
}

/// Something that made starts slower.
#[derive(Debug, Clone, Serialize)]
pub struct Slow {
    pub name: String,
    /// The file or unit behind it (`OneDrive.exe`, `NetworkManager-wait-online.service`).
    pub file: String,
    pub kind: Kind,
    /// Windows: the delay it added, summed over the starts it slowed. Linux: how long it took
    /// in the last start.
    pub delay_ms: u64,
    /// The starts it slowed (1 on Linux).
    pub times: u32,
    pub last: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Newest first.
    pub boots: Vec<Boot>,
    /// Largest delay first.
    pub slow: Vec<Slow>,
    /// Windows counts what made a start slower than usual over many starts; Linux only knows
    /// how long each unit took in the last one.
    pub history: bool,
}

/// English reason shown when the system keeps no record of its starts.
pub const NO_SYSTEMD: &str =
    "This system does not start with systemd, so Rewind OS cannot measure its start.";

pub fn report() -> Result<Report> {
    match Os::current() {
        Os::Windows => windows(),
        Os::Linux => linux(),
        Os::MacOs => Err(EngineError::Unsupported {
            tool: "boot.report",
            os: Os::MacOs.name(),
        }),
    }
}

// ---- Windows ----------------------------------------------------------------------------------

/// The performance log's start events as JSON: id, time and the named data of each.
const WINDOWS_SCRIPT: &str = r#"[Console]::OutputEncoding = [Text.Encoding]::UTF8
$events = @(Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-Diagnostics-Performance/Operational'; Id = 100,101,102,103,109 } -MaxEvents 500 -ErrorAction Stop | ForEach-Object {
  $x = [xml]$_.ToXml()
  $d = @{}
  foreach ($n in $x.Event.EventData.Data) { $d[[string]$n.Name] = [string]$n.'#text' }
  [pscustomobject]@{ id = $_.Id; at = $_.TimeCreated.ToUniversalTime().ToString('o'); data = $d }
})
ConvertTo-Json -InputObject $events -Depth 3 -Compress"#;

fn windows() -> Result<Report> {
    if !rewind_os_platform::elevation::is_elevated() {
        return Err(EngineError::NeedsAdmin);
    }
    let done = rewind_os_platform::process::run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", WINDOWS_SCRIPT],
    )?;
    // An empty log makes Get-WinEvent fail with "No events were found".
    if !done.success() && done.stderr.contains("No events were found") {
        return Ok(Report {
            boots: Vec::new(),
            slow: Vec::new(),
            history: true,
        });
    }
    if !done.success() {
        return Err(EngineError::Platform(
            rewind_os_platform::PlatformError::Command {
                program: "powershell".to_owned(),
                code: done.code,
                detail: done.stderr.trim().to_owned(),
            },
        ));
    }
    Ok(parse_windows(&done.stdout))
}

#[derive(Debug, Deserialize)]
struct Event {
    id: u32,
    #[serde(default)]
    at: Option<DateTime<Utc>>,
    #[serde(default)]
    data: HashMap<String, String>,
}

fn millis(data: &HashMap<String, String>, key: &str) -> Option<u64> {
    data.get(key)?.trim().parse().ok()
}

/// Reads the events the Windows script prints (a JSON array; a lone object when there is one).
pub fn parse_windows(json: &str) -> Report {
    let events: Vec<Event> = serde_json::from_str::<Vec<Event>>(json)
        .or_else(|_| serde_json::from_str::<Event>(json).map(|e| vec![e]))
        .unwrap_or_default();
    let mut boots = Vec::new();
    let mut slow: HashMap<(Kind, String), Slow> = HashMap::new();
    for event in events {
        let kind = match event.id {
            100 => {
                if let Some(total_ms) = millis(&event.data, "BootTime").filter(|ms| *ms > 0) {
                    boots.push(Boot {
                        at: event.at,
                        total_ms,
                        desktop_ms: millis(&event.data, "MainPathBootTime"),
                    });
                }
                continue;
            }
            101 => Kind::Program,
            102 => Kind::Driver,
            103 => Kind::Service,
            109 => Kind::Device,
            _ => continue,
        };
        let text = |key: &str| {
            event
                .data
                .get(key)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        };
        let Some(file) = text("Name").or_else(|| text("FileName")) else {
            continue;
        };
        let name = text("FriendlyName").unwrap_or_else(|| file.clone());
        let delay = millis(&event.data, "DegradationTime").unwrap_or(0);
        let entry = slow
            .entry((kind, file.to_ascii_lowercase()))
            .or_insert_with(|| Slow {
                name,
                file,
                kind,
                delay_ms: 0,
                times: 0,
                last: None,
            });
        entry.delay_ms += delay;
        entry.times += 1;
        entry.last = entry.last.max(event.at);
    }
    boots.sort_by_key(|b| std::cmp::Reverse(b.at));
    boots.truncate(BOOTS);
    Report {
        boots,
        slow: largest(slow.into_values().collect()),
        history: true,
    }
}

fn largest(mut slow: Vec<Slow>) -> Vec<Slow> {
    slow.retain(|s| s.delay_ms > 0);
    slow.sort_by(|a, b| {
        b.delay_ms
            .cmp(&a.delay_ms)
            .then_with(|| a.name.cmp(&b.name))
    });
    slow.truncate(LISTED);
    slow
}

// ---- Linux ------------------------------------------------------------------------------------

fn linux() -> Result<Report> {
    let time = rewind_os_platform::process::run("systemd-analyze", &["time"])
        .map_err(|_| EngineError::Refused(NO_SYSTEMD))?;
    if !time.success() {
        return Err(EngineError::Refused(NO_SYSTEMD));
    }
    let total_ms = parse_linux_time(&time.stdout).ok_or(EngineError::Refused(NO_SYSTEMD))?;
    let blame = rewind_os_platform::process::run("systemd-analyze", &["blame", "--no-pager"])?;
    let since_boot = rewind_os_platform::process::run("uptime", &["-s"])
        .ok()
        .and_then(|done| {
            chrono::NaiveDateTime::parse_from_str(done.stdout.trim(), "%Y-%m-%d %H:%M:%S").ok()
        })
        .and_then(|local| local.and_local_timezone(chrono::Local).single())
        .map(|local| local.with_timezone(&Utc));
    let desktop_ms = parse_linux_desktop(&time.stdout);
    Ok(Report {
        boots: vec![Boot {
            at: since_boot,
            total_ms,
            desktop_ms,
        }],
        slow: largest(
            parse_linux_blame(&blame.stdout)
                .into_iter()
                .map(|(file, ms)| Slow {
                    name: file.strip_suffix(".service").unwrap_or(&file).to_owned(),
                    kind: unit_kind(&file),
                    file,
                    delay_ms: ms,
                    times: 1,
                    last: since_boot,
                })
                .collect(),
        ),
        history: false,
    })
}

/// What a systemd unit is, from its suffix.
pub fn unit_kind(unit: &str) -> Kind {
    if unit.ends_with(".service") {
        Kind::Service
    } else if unit.ends_with(".device") {
        Kind::Device
    } else {
        Kind::Unit
    }
}

/// A systemd duration (`1min 2.345s`, `845ms`, `1h 2min`) in milliseconds.
pub fn parse_duration(text: &str) -> Option<u64> {
    let mut total = 0.0_f64;
    let mut any = false;
    for part in text.split_whitespace() {
        let split = part
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .filter(|i| *i > 0)?;
        let (number, unit) = part.split_at(split);
        let value: f64 = number.parse().ok()?;
        let factor = match unit {
            "us" | "µs" => 0.001,
            "ms" => 1.0,
            "s" => 1000.0,
            "min" => 60_000.0,
            "h" => 3_600_000.0,
            _ => return None,
        };
        total += value * factor;
        any = true;
    }
    any.then(|| total.round() as u64)
}

/// The whole start from `systemd-analyze time`: the part after `=`, or the only part there is
/// (`Startup finished in 679ms (userspace)`).
pub fn parse_linux_time(output: &str) -> Option<u64> {
    let line = output
        .lines()
        .find(|l| l.trim_start().starts_with("Startup finished in"))?;
    let rest = line.trim_start().strip_prefix("Startup finished in")?;
    if let Some((_, total)) = rest.rsplit_once('=') {
        return parse_duration(total.trim());
    }
    // One part only: "679ms (userspace)".
    let part = rest.split('(').next()?;
    parse_duration(part.trim())
}

/// When the graphical session (or the default target) was reached, after the kernel.
pub fn parse_linux_desktop(output: &str) -> Option<u64> {
    let line = output
        .lines()
        .find(|l| l.contains(".target reached after"))?;
    let after = line.split("reached after").nth(1)?;
    parse_duration(
        after
            .trim()
            .strip_suffix('.')
            .unwrap_or(after.trim())
            .split(" in ")
            .next()?,
    )
}

/// `systemd-analyze blame`: one unit per line, its duration first.
pub fn parse_linux_blame(output: &str) -> Vec<(String, u64)> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (time, unit) = line.rsplit_once(' ')?;
            let ms = parse_duration(time.trim())?;
            (!unit.is_empty()).then(|| (unit.to_owned(), ms))
        })
        .collect()
}
