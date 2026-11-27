//! Windows: old driver packages. Every driver update leaves the previous version in the driver
//! store (`System32\DriverStore\FileRepository`); for a graphics card each can take a gigabyte.
//!
//! Packages are read from Windows' own record (`HKLM\SYSTEM\DriverDatabase\DriverPackages`),
//! which does not depend on the system's language. They are grouped by driver: the same
//! original INF, provider and device class. In each group the newest version always stays, and
//! so does every package a device uses. Only the older ones nothing uses are offered, and they
//! are removed with `pnputil /delete-driver`, never with `/force`, so Windows itself refuses a
//! package a device still needs. Removing one cannot be undone.

use crate::safety;
use crate::task::Task;
use crate::walk::{self, Filter};
use crate::{EngineError, Result};
use chrono::{DateTime, NaiveDate};
use rewind_os_platform::paths::Known;
use rewind_os_platform::registry::{self, Hive, Value};
use rewind_os_platform::{Os, elevation, process};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

const PACKAGES: &str = r"SYSTEM\DriverDatabase\DriverPackages";

/// English reason; translate with `rewind_os_i18n::tr`.
pub const NOT_OLD: &str = "This is not an old driver package: it is the newest of its driver, a device uses it, or it is gone.";

/// English reason; translate with `rewind_os_i18n::tr`.
pub const IN_USE_UNKNOWN: &str =
    "Windows did not say which drivers its devices use, so no driver package is offered.";

/// One package of the driver store, as Windows records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The name Windows gave it (`oem32.inf`).
    pub published: String,
    /// The driver's own INF name (`amdafd.inf`).
    pub original: String,
    pub provider: String,
    /// The device class it is for, as Windows stores it.
    pub class: [u8; 16],
    /// Version as four numbers, highest first (24.10.0.1).
    pub version: [u16; 4],
    pub date: Option<NaiveDate>,
    /// Its folder in `DriverStore\FileRepository`.
    pub folder: String,
}

/// An old package that can be removed.
#[derive(Debug, Clone, Serialize)]
pub struct OldPackage {
    pub published: String,
    pub original: String,
    pub provider: String,
    pub version: String,
    pub date: Option<NaiveDate>,
    /// The version that stays.
    pub newest: String,
    /// Space its folder takes.
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Old packages nothing uses, largest first.
    pub old: Vec<OldPackage>,
    /// Driver packages programs and Windows Update added.
    pub packages: usize,
    /// Older packages kept because a device uses them.
    pub old_in_use: usize,
}

impl Report {
    pub fn reclaimable(&self) -> u64 {
        self.old.iter().map(|p| p.bytes).sum()
    }
}

/// What removing did.
#[derive(Debug, Clone, Serialize)]
pub struct Removed {
    pub removed: Vec<String>,
    pub bytes: u64,
    /// Packages not removed, with why (Windows' own words, or an English reason).
    pub failed: Vec<(String, String)>,
}

/// Reads the version and date from the `Version` value: the driver date as a FILETIME at byte
/// 24 and the version as four 16-bit numbers, lowest first, at byte 32.
pub fn parse_version(bytes: &[u8]) -> Option<([u16; 4], Option<NaiveDate>)> {
    let date = u64::from_le_bytes(bytes.get(24..32)?.try_into().ok()?);
    let raw = bytes.get(32..40)?;
    let part = |i: usize| -> Option<u16> {
        Some(u16::from_le_bytes(
            raw.get(i * 2..i * 2 + 2)?.try_into().ok()?,
        ))
    };
    let version = [part(3)?, part(2)?, part(1)?, part(0)?];
    // FILETIME counts 100 ns since 1601; Unix time starts 11 644 473 600 s later.
    let seconds = i64::try_from(date / 10_000_000).ok()? - 11_644_473_600;
    let date = (date > 0)
        .then(|| DateTime::from_timestamp(seconds, 0).map(|d| d.date_naive()))
        .flatten();
    Some((version, date))
}

pub fn version_text(v: [u16; 4]) -> String {
    format!("{}.{}.{}.{}", v[0], v[1], v[2], v[3])
}

/// The packages to offer: in each driver's group, every one older than the newest that no
/// device uses. `in_use` holds published names, lowercase.
pub fn old_packages<'a>(
    packages: &'a [Package],
    in_use: &HashSet<String>,
) -> Vec<(&'a Package, &'a Package)> {
    let mut groups: HashMap<(String, String, [u8; 16]), Vec<&Package>> = HashMap::new();
    for p in packages {
        let key = (
            p.original.to_lowercase(),
            p.provider.to_lowercase(),
            p.class,
        );
        groups.entry(key).or_default().push(p);
    }
    let mut old = Vec::new();
    for group in groups.values() {
        let Some(newest) = group.iter().max_by_key(|p| (p.version, p.date)) else {
            continue;
        };
        for p in group {
            let is_newest = std::ptr::eq(*p, *newest);
            let used = in_use.contains(&p.published.to_lowercase());
            if !is_newest && !used && (p.version, p.date) < (newest.version, newest.date) {
                old.push((*p, *newest));
            }
        }
    }
    old
}

fn read_packages() -> Vec<Package> {
    registry::subkeys(Hive::LocalMachine, PACKAGES)
        .into_iter()
        .filter_map(|folder| {
            let values = registry::values(Hive::LocalMachine, &format!(r"{PACKAGES}\{folder}"));
            let text = |name: &str| {
                values
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name))
                    .and_then(|(_, v)| v.as_text())
                    .map(str::to_owned)
            };
            // Only packages added to Windows have a published `oem` name; the ones that come
            // with Windows never are offered.
            let published = text("")?;
            if !safety::is_oem_driver_name(&published) {
                return None;
            }
            let version = values.iter().find_map(|(n, v)| match v {
                Value::Binary(b) if n.eq_ignore_ascii_case("Version") => Some(b.clone()),
                _ => None,
            })?;
            let class: [u8; 16] = version.get(8..24)?.try_into().ok()?;
            let (version_numbers, date) = parse_version(&version)?;
            Some(Package {
                published,
                original: text("InfName")?,
                provider: text("Provider").unwrap_or_default(),
                class,
                version: version_numbers,
                date,
                folder,
            })
        })
        .collect()
}

/// The published names of the packages devices use, lowercase.
fn in_use() -> Result<HashSet<String>> {
    let out = process::run_checked(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance Win32_PnPSignedDriver | ForEach-Object { $_.InfName }",
        ],
    )
    .map_err(|_| EngineError::Refused(IN_USE_UNKNOWN))?;
    let names: HashSet<String> = out
        .stdout
        .lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    if names.is_empty() {
        return Err(EngineError::Refused(IN_USE_UNKNOWN));
    }
    Ok(names)
}

fn require_windows(tool: &'static str) -> Result<()> {
    if Os::current() != Os::Windows {
        return Err(EngineError::Unsupported {
            tool,
            os: Os::current().name(),
        });
    }
    Ok(())
}

/// The old driver packages nothing uses, with the space each takes.
pub fn list(task: &Task) -> Result<Report> {
    require_windows("drivers.list")?;
    task.set_stage("Reading the driver store");
    let packages = read_packages();
    let in_use = in_use()?;
    let store = Known::Windows
        .path()
        .map(|w| w.join(r"System32\DriverStore\FileRepository"));
    let mut old: Vec<OldPackage> = old_packages(&packages, &in_use)
        .into_iter()
        .map(|(p, newest)| {
            let bytes = store
                .as_ref()
                .map(|s| walk::measure(&s.join(&p.folder), &Filter::default(), task).bytes)
                .unwrap_or(0);
            OldPackage {
                published: p.published.clone(),
                original: p.original.clone(),
                provider: p.provider.clone(),
                version: version_text(p.version),
                date: p.date,
                newest: version_text(newest.version),
                bytes,
            }
        })
        .collect();
    old.sort_by_key(|p| std::cmp::Reverse(p.bytes));
    // Older packages that stay because a device uses them.
    let old_in_use = old_packages(&packages, &HashSet::new())
        .len()
        .saturating_sub(old.len());
    Ok(Report {
        old,
        packages: packages.len(),
        old_in_use,
    })
}

/// Removes the named old packages (every old one when `names` is empty). Each is checked again
/// first: only a package `list` would offer now is removed.
pub fn remove(names: &[String], task: &Task) -> Result<Removed> {
    require_windows("drivers.clean")?;
    if !elevation::is_elevated() {
        return Err(EngineError::NeedsAdmin);
    }
    let report = list(task)?;
    let offered: HashMap<String, u64> = report
        .old
        .iter()
        .map(|p| (p.published.to_lowercase(), p.bytes))
        .collect();
    let wanted: Vec<String> = if names.is_empty() {
        report.old.iter().map(|p| p.published.clone()).collect()
    } else {
        names.to_vec()
    };
    task.set_stage("Removing old driver packages");
    let mut done = Removed {
        removed: Vec::new(),
        bytes: 0,
        failed: Vec::new(),
    };
    for name in wanted {
        if task.is_cancelled() {
            break;
        }
        let Some(&bytes) = offered
            .get(&name.to_lowercase())
            .filter(|_| safety::is_oem_driver_name(&name))
        else {
            done.failed.push((name, NOT_OLD.to_owned()));
            continue;
        };
        // `offered` only holds `oem<n>.inf` names read from Windows' record.
        match process::run("pnputil", &["/delete-driver", &name]) {
            Ok(out) if out.success() => {
                task.add(1, bytes);
                done.bytes += bytes;
                done.removed.push(name);
            }
            Ok(out) => {
                let why = format!("{} {}", out.stdout.trim(), out.stderr.trim());
                done.failed.push((name, why.trim().to_owned()));
            }
            Err(err) => done.failed.push((name, err.to_string())),
        }
    }
    Ok(done)
}

/// A device Windows marks with a problem (the yellow mark of Device Manager).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceProblem {
    pub name: String,
    /// Its kind, as Windows names it (`Display`, `USB`), when known.
    pub class: Option<String>,
    /// Device Manager's problem code.
    pub code: u32,
}

impl DeviceProblem {
    /// What the code means, in English; translate with `rewind_os_i18n::tr`.
    pub fn meaning(&self) -> &'static str {
        match self.code {
            1 => "It is not set up correctly.",
            3 => "Its driver may be damaged, or the computer is short of memory.",
            10 => "It cannot start.",
            12 => "It cannot find enough free resources.",
            14 => "It works after the computer restarts.",
            18 | 19 => "Its driver needs to be installed again.",
            21 => "Windows is removing it.",
            28 => "Its driver is not installed.",
            31 => "Windows cannot load its driver.",
            39 => "Its driver is damaged or missing.",
            43 => "Windows stopped it because it reported problems.",
            48 => "Its driver is blocked because it has known problems.",
            52 => "Windows cannot verify its driver's signature.",
            _ => "Windows reports a problem with it.",
        }
    }
}

/// Codes that are not problems to fix: turned off on purpose (22) or not plugged in (24, 45).
const NOT_PROBLEMS: &[u32] = &[0, 22, 24, 45];

/// Reads the devices from `Win32_PnPEntity` as JSON (one object, or a list of them).
pub fn parse_problems(json: &str) -> Option<Vec<DeviceProblem>> {
    let value: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Null => Vec::new(),
        one => vec![one],
    };
    let mut problems: Vec<DeviceProblem> = items
        .iter()
        .filter_map(|item| {
            let code = u32::try_from(item.get("ConfigManagerErrorCode")?.as_u64()?).ok()?;
            let name = item
                .get("Name")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .unwrap_or("Unknown device")
                .to_owned();
            let class = item
                .get("PNPClass")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            (!NOT_PROBLEMS.contains(&code)).then_some(DeviceProblem { name, class, code })
        })
        .collect();
    problems.sort_by(|a, b| a.name.cmp(&b.name));
    problems.dedup();
    Some(problems)
}

/// The devices with a problem, as Device Manager shows them. Reading changes nothing.
pub fn problems() -> Result<Vec<DeviceProblem>> {
    require_windows("drivers.problems")?;
    let script = "Get-CimInstance Win32_PnPEntity -Filter 'ConfigManagerErrorCode <> 0' | Select-Object Name,PNPClass,ConfigManagerErrorCode | ConvertTo-Json -Compress";
    let out = process::run_checked(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )?;
    // No device with a problem: PowerShell prints nothing.
    if out.stdout.trim().is_empty() {
        return Ok(Vec::new());
    }
    parse_problems(&out.stdout).ok_or_else(|| {
        rewind_os_platform::PlatformError::Command {
            program: "powershell".to_owned(),
            code: None,
            detail: out.stdout.trim().to_owned(),
        }
        .into()
    })
}

/// Opens Windows' Device Manager.
pub fn open_device_manager() -> Result<()> {
    require_windows("drivers.device_manager")?;
    Ok(process::start(
        std::path::Path::new("mmc.exe"),
        &["devmgmt.msc"],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_value_reads_as_windows_shows_it() {
        // AMD's amdafd.inf as `pnputil /enum-drivers` showed it: 06/07/2024 24.10.0.1.
        let raw = [
            0x00, 0xFF, 0x09, 0x00, 0, 0, 0, 0, 0x7D, 0xE9, 0x36, 0x4D, 0x25, 0xE3, 0xCE, 0x11,
            0xBF, 0xC1, 0x08, 0x00, 0x2B, 0xE1, 0x03, 0x18, 0x00, 0x40, 0xCE, 0xA3, 0x6D, 0xB8,
            0xDA, 0x01, 0x01, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x18, 0x00, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let (version, date) = parse_version(&raw).unwrap();
        assert_eq!(version_text(version), "24.10.0.1");
        assert_eq!(date, NaiveDate::from_ymd_opt(2024, 6, 7));
        assert!(parse_version(&raw[..30]).is_none());
    }

    fn package(published: &str, original: &str, version: [u16; 4]) -> Package {
        Package {
            published: published.to_owned(),
            original: original.to_owned(),
            provider: "Vendor".to_owned(),
            class: [1; 16],
            version,
            date: None,
            folder: format!("{original}_{published}"),
        }
    }

    #[test]
    fn the_newest_version_and_every_package_in_use_stay() {
        let packages = [
            package("oem1.inf", "gpu.inf", [31, 0, 0, 1]),
            package("oem2.inf", "gpu.inf", [30, 0, 0, 5]),
            package("oem3.inf", "gpu.inf", [29, 0, 0, 0]),
            package("oem4.inf", "GPU.INF", [28, 0, 0, 0]),
            // Another driver with one version: nothing to offer.
            package("oem5.inf", "audio.inf", [1, 0, 0, 0]),
        ];
        let in_use = HashSet::from(["oem3.inf".to_owned()]);
        let old: Vec<&str> = old_packages(&packages, &in_use)
            .iter()
            .map(|(p, newest)| {
                assert_eq!(newest.published, "oem1.inf");
                p.published.as_str()
            })
            .collect();
        assert_eq!(old.len(), 2);
        assert!(old.contains(&"oem2.inf") && old.contains(&"oem4.inf"));
        assert!(!old.contains(&"oem1.inf") && !old.contains(&"oem3.inf"));
    }

    #[test]
    fn packages_of_other_providers_or_classes_are_other_drivers() {
        let mut other = package("oem2.inf", "gpu.inf", [1, 0, 0, 0]);
        other.provider = "Someone else".to_owned();
        let packages = [package("oem1.inf", "gpu.inf", [2, 0, 0, 0]), other];
        assert!(old_packages(&packages, &HashSet::new()).is_empty());
    }

    #[test]
    fn devices_turned_off_or_unplugged_are_not_problems() {
        let json = r#"[
            {"Name":"Wireless adapter","PNPClass":"Net","ConfigManagerErrorCode":28},
            {"Name":"Old webcam","PNPClass":"Camera","ConfigManagerErrorCode":45},
            {"Name":"Spare port","PNPClass":"Ports","ConfigManagerErrorCode":22},
            {"Name":"Graphics","PNPClass":"Display","ConfigManagerErrorCode":43}
        ]"#;
        let problems = parse_problems(json).unwrap();
        let names: Vec<&str> = problems.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Graphics", "Wireless adapter"]);
        // One device comes as an object, none as nothing at all.
        let one = r#"{"Name":"Card reader","PNPClass":null,"ConfigManagerErrorCode":10}"#;
        assert_eq!(parse_problems(one).map(|p| p.len()), Some(1));
        assert_eq!(parse_problems("").map(|p| p.len()), None);
        assert_eq!(parse_problems("null").map(|p| p.len()), Some(0));
    }
}
