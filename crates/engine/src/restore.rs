//! Windows restore points and the other shadow copies of each drive (Volume Shadow Copy). They
//! can take several gigabytes; Windows' own Disk Cleanup offers to delete all but the newest.
//!
//! - `restore.list`: the shadow copies with their date and the restore point they belong to,
//!   and the space each drive sets aside for them. Needs administrator rights (WMI hides them
//!   otherwise).
//! - `restore.clean`: deletes every shadow copy except the newest on each drive, as Disk
//!   Cleanup does, so the newest restore point is always kept. Cannot be undone.
//!
//! Read with PowerShell and CIM (`Win32_ShadowCopy`, `Win32_ShadowStorage`), whose fields do
//! not change with the system's language, unlike `vssadmin`'s output.

use crate::{EngineError, Os, Result, Task};
use chrono::{DateTime, Utc};
use rewind_os_platform::{elevation, process};
use serde::{Deserialize, Serialize};

/// English reason; translate with `rewind_os_i18n::tr`.
pub const NOTHING_TO_DELETE: &str =
    "There is only the newest restore point on each drive; it is always kept.";

#[derive(Debug, Clone, Serialize)]
pub struct ShadowCopy {
    /// The shadow copy's GUID, `{…}`.
    pub id: String,
    /// The drive it belongs to (`C:\`), or the volume's name when it has no letter.
    pub drive: String,
    pub created: DateTime<Utc>,
    /// The restore point's description, when this copy is one ("Windows Update",
    /// "Installed …").
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Storage {
    pub drive: String,
    /// Space the copies take now.
    pub used: u64,
    /// The most Windows lets them take before deleting the oldest itself; `None` for no limit.
    pub max: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Oldest first.
    pub copies: Vec<ShadowCopy>,
    pub storage: Vec<Storage>,
}

impl Report {
    pub fn used(&self) -> u64 {
        self.storage.iter().map(|s| s.used).sum()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Cleaned {
    pub deleted: usize,
    /// Copies Windows did not delete, with what it said.
    pub failed: Vec<(String, String)>,
    /// What the copies took before and after.
    pub before: u64,
    pub after: u64,
}

/// Every copy except the newest on each drive: what `restore.clean` deletes. The newest copy
/// of a drive is never in the list.
pub fn all_but_newest(copies: &[ShadowCopy]) -> Vec<&ShadowCopy> {
    copies
        .iter()
        .filter(|c| {
            copies
                .iter()
                .any(|o| o.drive.eq_ignore_ascii_case(&c.drive) && o.created > c.created)
        })
        .collect()
}

/// A shadow copy id as Windows writes it: a GUID in braces. Nothing else reaches the script
/// that deletes one.
pub fn is_shadow_id(id: &str) -> bool {
    let Some(inner) = id.strip_prefix('{').and_then(|r| r.strip_suffix('}')) else {
        return false;
    };
    inner.len() == 36
        && inner.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

const LIST_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$letters = @{}
Get-CimInstance Win32_Volume | ForEach-Object { if ($_.DriveLetter) { $letters[[string]$_.DeviceID] = [string]$_.DriveLetter + '\' } }
$copies = @(Get-CimInstance Win32_ShadowCopy | ForEach-Object {
  [pscustomobject]@{ id = [string]$_.ID; volume = [string]$_.VolumeName; created = $_.InstallDate.ToUniversalTime().ToString('o') }
})
$points = @(Get-CimInstance -Namespace root/default -ClassName SystemRestore -ErrorAction SilentlyContinue | ForEach-Object {
  [pscustomobject]@{ description = [string]$_.Description; created = ([Management.ManagementDateTimeConverter]::ToDateTime($_.CreationTime)).ToUniversalTime().ToString('o') }
})
$storage = @(Get-CimInstance Win32_ShadowStorage | ForEach-Object {
  [pscustomobject]@{ volume = [string]$_.Volume.DeviceID; used = [uint64]$_.UsedSpace; max = [uint64]$_.MaxSpace }
})
ConvertTo-Json -InputObject ([pscustomobject]@{ letters = $letters; copies = $copies; points = $points; storage = $storage }) -Depth 4 -Compress"#;

#[derive(Debug, Deserialize)]
struct Raw {
    #[serde(default)]
    letters: std::collections::HashMap<String, String>,
    #[serde(default)]
    copies: Vec<RawCopy>,
    #[serde(default)]
    points: Vec<RawPoint>,
    #[serde(default)]
    storage: Vec<RawStorage>,
}

#[derive(Debug, Deserialize)]
struct RawCopy {
    id: String,
    volume: String,
    created: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct RawPoint {
    description: String,
    created: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct RawStorage {
    volume: String,
    used: u64,
    max: u64,
}

/// A restore point and its shadow copy are made within moments of each other.
const SAME_MOMENT_SECONDS: i64 = 10 * 60;

/// The report from the script's JSON.
pub fn parse(json: &str) -> Option<Report> {
    let raw: Raw = serde_json::from_str(json.trim()).ok()?;
    let drive = |volume: &str| {
        raw.letters
            .iter()
            .find(|(id, _)| id.eq_ignore_ascii_case(volume))
            .map(|(_, letter)| letter.clone())
            .unwrap_or_else(|| volume.to_owned())
    };
    let mut copies: Vec<ShadowCopy> = raw
        .copies
        .iter()
        .filter(|c| is_shadow_id(&c.id))
        .map(|c| ShadowCopy {
            id: c.id.clone(),
            drive: drive(&c.volume),
            created: c.created,
            description: raw
                .points
                .iter()
                .filter(|p| (c.created - p.created).num_seconds().abs() <= SAME_MOMENT_SECONDS)
                .min_by_key(|p| (c.created - p.created).num_seconds().abs())
                .map(|p| p.description.clone()),
        })
        .collect();
    copies.sort_by_key(|c| c.created);
    let storage = raw
        .storage
        .iter()
        .map(|s| Storage {
            drive: drive(&s.volume),
            used: s.used,
            // Windows writes "no limit" as the largest number it has.
            max: (s.max > 0 && s.max < u64::MAX / 2).then_some(s.max),
        })
        .collect();
    Some(Report { copies, storage })
}

fn require_windows_admin(tool: &'static str) -> Result<()> {
    if Os::current() != Os::Windows {
        return Err(EngineError::Unsupported {
            tool,
            os: Os::current().name(),
        });
    }
    if !elevation::is_elevated() {
        return Err(EngineError::NeedsAdmin);
    }
    Ok(())
}

fn powershell(script: &str) -> Result<String> {
    let out = process::run_checked(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )?;
    Ok(out.stdout)
}

pub fn list() -> Result<Report> {
    require_windows_admin("restore.list")?;
    let out = powershell(LIST_SCRIPT)?;
    parse(&out).ok_or_else(|| {
        rewind_os_platform::PlatformError::Command {
            program: "powershell".to_owned(),
            code: None,
            detail: out.trim().to_owned(),
        }
        .into()
    })
}

/// Deletes every shadow copy except the newest on each drive.
pub fn clean(task: &Task) -> Result<Cleaned> {
    require_windows_admin("restore.clean")?;
    let report = list()?;
    let doomed = all_but_newest(&report.copies);
    if doomed.is_empty() {
        return Err(EngineError::Refused(NOTHING_TO_DELETE));
    }
    task.set_stage("Deleting old restore points");
    let mut deleted = 0;
    let mut failed = Vec::new();
    for copy in doomed {
        if task.is_cancelled() {
            break;
        }
        // `is_shadow_id` held when the list was read; only a GUID reaches the script.
        let script = format!(
            "$ErrorActionPreference = 'Stop'; Get-CimInstance Win32_ShadowCopy -Filter \"ID='{}'\" | Remove-CimInstance",
            copy.id
        );
        match powershell(&script) {
            Ok(_) => {
                deleted += 1;
                task.add(1, 0);
            }
            Err(err) => failed.push((copy.id.clone(), err.to_string())),
        }
    }
    let after = list().map(|r| r.used()).unwrap_or(report.used());
    Ok(Cleaned {
        deleted,
        failed,
        before: report.used(),
        after,
    })
}
