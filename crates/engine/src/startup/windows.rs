//! Programs that start with Windows. Enabling and disabling uses the same "startup approved"
//! records the Windows settings use, so the program's own entry is never deleted and the
//! change can be undone from Windows too.

use crate::{EngineError, Result};
use chrono::{DateTime, TimeZone, Utc};
use rewindows_platform::paths::Known;
use rewindows_platform::registry::{self, Hive, Value};
use rewindows_platform::{PlatformError, elevation};
use serde::Serialize;
use std::path::PathBuf;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_32: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    UserRun,
    MachineRun,
    MachineRun32,
    UserStartupFolder,
    CommonStartupFolder,
}

impl Source {
    const ALL: [Source; 5] = [
        Source::UserRun,
        Source::MachineRun,
        Source::MachineRun32,
        Source::UserStartupFolder,
        Source::CommonStartupFolder,
    ];

    fn tag(self) -> &'static str {
        match self {
            Source::UserRun => "user_run",
            Source::MachineRun => "machine_run",
            Source::MachineRun32 => "machine_run32",
            Source::UserStartupFolder => "user_folder",
            Source::CommonStartupFolder => "common_folder",
        }
    }

    fn hive(self) -> Hive {
        match self {
            Source::UserRun | Source::UserStartupFolder => Hive::CurrentUser,
            _ => Hive::LocalMachine,
        }
    }

    /// Where Windows records whether the entry is enabled.
    fn approved_key(self) -> String {
        let leaf = match self {
            Source::UserRun | Source::MachineRun => "Run",
            Source::MachineRun32 => "Run32",
            Source::UserStartupFolder | Source::CommonStartupFolder => "StartupFolder",
        };
        format!(r"{APPROVED}\{leaf}")
    }

    /// English label; translate with `rewindows_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Source::UserRun => "Registry (you)",
            Source::MachineRun => "Registry (all users)",
            Source::MachineRun32 => "Registry (all users, 32-bit)",
            Source::UserStartupFolder => "Startup folder (you)",
            Source::CommonStartupFolder => "Startup folder (all users)",
        }
    }

    fn folder(self) -> Option<PathBuf> {
        let suffix = r"Microsoft\Windows\Start Menu\Programs\Startup";
        match self {
            Source::UserStartupFolder => Known::RoamingAppData.path().map(|p| p.join(suffix)),
            Source::CommonStartupFolder => Known::ProgramData.path().map(|p| p.join(suffix)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StartupEntry {
    /// `<source>|<name>`, used by `startup.set`.
    pub id: String,
    pub name: String,
    pub command: String,
    pub source: Source,
    pub enabled: bool,
    /// Changing it needs administrator rights.
    pub needs_admin: bool,
}

pub fn list() -> Vec<StartupEntry> {
    let mut entries = Vec::new();
    for source in Source::ALL {
        let items: Vec<(String, String)> = match source.folder() {
            Some(folder) => std::fs::read_dir(&folder)
                .map(|rd| {
                    rd.flatten()
                        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                        .filter(|(n, _)| !n.eq_ignore_ascii_case("desktop.ini"))
                        .map(|(n, p)| (n, p.display().to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            None => {
                let key = if source == Source::MachineRun32 {
                    RUN_32
                } else {
                    RUN
                };
                registry::values(source.hive(), key)
                    .into_iter()
                    .filter_map(|(name, v)| v.as_text().map(|c| (name, c.to_owned())))
                    .filter(|(name, _)| !name.is_empty())
                    .collect()
            }
        };
        let approved = registry::values(source.hive(), &source.approved_key());
        for (name, command) in items {
            let enabled = approved
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(&name))
                .map(|(_, v)| match v {
                    Value::Binary(bytes) => is_enabled(bytes),
                    _ => true,
                })
                .unwrap_or(true);
            entries.push(StartupEntry {
                id: format!("{}|{name}", source.tag()),
                name,
                command,
                source,
                enabled,
                needs_admin: source.hive() == Hive::LocalMachine,
            });
        }
    }
    entries
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<()> {
    let (tag, name) = id
        .split_once('|')
        .ok_or_else(|| EngineError::NotFound(id.to_owned()))?;
    let source = Source::ALL
        .into_iter()
        .find(|s| s.tag() == tag)
        .ok_or_else(|| EngineError::NotFound(id.to_owned()))?;
    if source.hive() == Hive::LocalMachine && !elevation::is_elevated() {
        return Err(EngineError::NeedsAdmin);
    }
    let bytes = approved_bytes(enabled, Utc::now());
    registry::set_binary(source.hive(), &source.approved_key(), name, &bytes).map_err(|e| match e {
        PlatformError::NeedsAdmin => EngineError::NeedsAdmin,
        other => other.into(),
    })
}

/// Windows marks an entry disabled with an odd first byte (0x03) and enabled with an even one.
pub fn is_enabled(bytes: &[u8]) -> bool {
    bytes.first().is_none_or(|b| b & 1 == 0)
}

/// The 12-byte record: a state byte, three zero bytes and, when disabled, the moment it was
/// disabled as a Windows FILETIME.
pub fn approved_bytes(enabled: bool, now: DateTime<Utc>) -> [u8; 12] {
    let mut bytes = [0u8; 12];
    if enabled {
        bytes[0] = 0x02;
    } else {
        bytes[0] = 0x03;
        bytes[4..].copy_from_slice(&filetime(now).to_le_bytes());
    }
    bytes
}

/// 100-nanosecond intervals since 1601-01-01 UTC.
fn filetime(at: DateTime<Utc>) -> u64 {
    let Some(epoch) = Utc.with_ymd_and_hms(1601, 1, 1, 0, 0, 0).single() else {
        return 0;
    };
    let micros = at
        .signed_duration_since(epoch)
        .num_microseconds()
        .unwrap_or(0);
    u64::try_from(micros).unwrap_or(0).saturating_mul(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_state_reads_back() {
        let now = Utc::now();
        assert!(is_enabled(&approved_bytes(true, now)));
        assert!(!is_enabled(&approved_bytes(false, now)));
    }

    #[test]
    fn disabling_later_records_a_later_time() {
        let early = Utc::now();
        let later = early + chrono::Duration::days(1);
        let stamp = |d| {
            let b = approved_bytes(false, d);
            let mut t = [0u8; 8];
            t.copy_from_slice(&b[4..]);
            u64::from_le_bytes(t)
        };
        assert!(stamp(later) > stamp(early));
    }
}
