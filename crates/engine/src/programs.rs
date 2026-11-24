//! Installed programs, read from the uninstall entries Windows keeps in the registry.

use crate::{EngineError, Result};
use chrono::NaiveDate;
use rewind_os_platform::registry::{self, Hive, Value};
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Installed for every user.
    Machine,
    /// Installed for the current user only.
    User,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledProgram {
    /// Stable id: `<hive>\<view>\<subkey>`.
    pub key: String,
    pub name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub install_location: Option<PathBuf>,
    pub display_icon: Option<String>,
    #[serde(skip)]
    pub uninstall_command: Option<String>,
    /// Size the installer declared; often missing or outdated.
    pub estimated_bytes: Option<u64>,
    pub installed_on: Option<NaiveDate>,
    pub scope: Scope,
    /// The package manager that installed it, on Linux (`APT`, `Flatpak`…).
    pub manager: Option<&'static str>,
}

fn sources() -> [(Hive, &'static str, &'static str); 3] {
    [
        (Hive::LocalMachine, UNINSTALL, "hklm64"),
        (Hive::LocalMachine, UNINSTALL_32, "hklm32"),
        (Hive::CurrentUser, UNINSTALL, "hkcu"),
    ]
}

/// Every installed program: Windows' own list, or the package managers on Linux. Sorted by
/// name.
pub fn installed() -> Vec<InstalledProgram> {
    match rewind_os_platform::Os::current() {
        rewind_os_platform::Os::Windows => windows_installed(),
        rewind_os_platform::Os::Linux => crate::packages::installed(),
        rewind_os_platform::Os::MacOs => Vec::new(),
    }
}

fn windows_installed() -> Vec<InstalledProgram> {
    let mut seen = HashSet::new();
    let mut programs: Vec<InstalledProgram> = sources()
        .into_iter()
        .flat_map(|(hive, base, tag)| {
            registry::subkeys(hive, base)
                .into_iter()
                .filter_map(move |sub| read(hive, base, tag, &sub))
        })
        .filter(|p| seen.insert((p.name.to_lowercase(), p.version.clone())))
        .collect();
    programs.sort_by_key(|p| p.name.to_lowercase());
    programs
}

fn read(hive: Hive, base: &str, tag: &str, sub: &str) -> Option<InstalledProgram> {
    let path = format!(r"{base}\{sub}");
    let values = registry::values(hive, &path);
    let text = |name: &str| {
        values
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .and_then(|(_, v)| v.as_text())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let number = |name: &str| {
        values
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .and_then(|(_, v)| match v {
                Value::Number(n) => Some(*n),
                _ => None,
            })
    };
    let name = text("DisplayName")?;
    // Hidden components and updates of another entry are not separate programs.
    if number("SystemComponent") == Some(1) || text("ParentKeyName").is_some() {
        return None;
    }
    Some(InstalledProgram {
        key: format!(r"{tag}\{sub}"),
        name,
        publisher: text("Publisher"),
        version: text("DisplayVersion"),
        install_location: text("InstallLocation").map(|s| PathBuf::from(s.trim_matches('"'))),
        display_icon: text("DisplayIcon"),
        uninstall_command: text("UninstallString"),
        estimated_bytes: number("EstimatedSize").map(|kb| kb * 1024),
        installed_on: text("InstallDate")
            .and_then(|d| NaiveDate::parse_from_str(&d, "%Y%m%d").ok()),
        scope: if hive == Hive::CurrentUser {
            Scope::User
        } else {
            Scope::Machine
        },
        manager: None,
    })
}

/// Starts the uninstaller the program registered. It shows its own window; this returns as
/// soon as it started.
pub fn start_uninstaller(key: &str) -> Result<()> {
    let program = installed()
        .into_iter()
        .find(|p| p.key == key)
        .ok_or_else(|| EngineError::NotFound(key.to_owned()))?;
    let command = program
        .uninstall_command
        .ok_or_else(|| EngineError::NotFound(format!("{key}: no uninstall command")))?;
    launch(&command)
}

#[cfg(windows)]
fn launch(command: &str) -> Result<()> {
    use std::os::windows::process::CommandExt;
    // The registered string is a full command line with its own quoting; pass it untouched.
    rewind_os_platform::process::command("cmd.exe")
        .arg("/C")
        .raw_arg(command)
        .spawn()
        .map(|_| ())
        .map_err(|source| EngineError::Io {
            path: "cmd.exe".into(),
            source,
        })
}

#[cfg(not(windows))]
fn launch(_command: &str) -> Result<()> {
    Err(rewind_os_platform::PlatformError::Unsupported("program uninstallers").into())
}
