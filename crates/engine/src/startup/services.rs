//! Windows services that programs install (updaters, helpers, sync agents). Only services of
//! programs are listed: never drivers, never anything inside the Windows folder or hosted by
//! `svchost`, never Windows Defender. "Turning one off" sets it to start when needed (manual),
//! never to disabled, so anything that asks for it still starts it. The services the app changed
//! are remembered, so they can be set back to start automatically.

use super::{Source, StartupEntry};
use crate::{EngineError, Result};
use rewind_os_platform::paths::Known;
use rewind_os_platform::registry::{self, Hive, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

const SERVICES: &str = r"SYSTEM\CurrentControlSet\Services";
const AUTO: u64 = 2;
const MANUAL: u64 = 3;
/// `SERVICE_WIN32_OWN_PROCESS` and `SERVICE_WIN32_SHARE_PROCESS`; drivers have other types.
const WIN32: u64 = 0x10 | 0x20;

/// Services whose name, display name or program says they keep the computer secure,
/// connected, audible or usable. Shown, never turned off.
const ESSENTIAL_WORDS: &[&str] = &[
    // security
    "defender",
    "antivirus",
    "antimalware",
    "avast",
    "avg",
    "bitdefender",
    "eset",
    "kaspersky",
    "malwarebytes",
    "mcafee",
    "norton",
    "sophos",
    "symantec",
    "trend micro",
    "crowdstrike",
    "sentinel",
    "cortex",
    "carbon black",
    "webroot",
    "f-secure",
    "panda",
    // VPN and corporate access
    "vpn",
    "tailscale",
    "wireguard",
    "openvpn",
    "globalprotect",
    "pangps",
    "forticlient",
    "anyconnect",
    "zscaler",
    "netskope",
    "cloudflare warp",
    "omnissa",
    "vmware horizon",
    // sound, input, sign-in hardware
    "audio",
    "realtek",
    "bluetooth",
    "touchpad",
    "synaptics",
    "elan",
    "fingerprint",
    "goodix",
    "biometric",
    "wacom",
    "pen ",
    // backups
    "backup",
];

#[derive(Debug, Clone)]
pub struct RawService {
    pub name: String,
    pub display_name: Option<String>,
    pub image_path: Option<String>,
    pub start: Option<u64>,
    pub kind: Option<u64>,
}

/// Whether the service belongs to a program, not to Windows, a driver or Defender.
pub fn is_program_service(s: &RawService, windows_dir: &str) -> bool {
    let Some(kind) = s.kind else { return false };
    if kind & WIN32 == 0 || kind & !(WIN32 | 0x100) != 0 {
        return false;
    }
    let Some(image) = &s.image_path else {
        return false;
    };
    let image = image.trim().trim_matches('"').to_lowercase();
    let windows_dir = windows_dir.to_lowercase();
    !(image.is_empty()
        || image.starts_with("%systemroot%")
        || image.starts_with(r"\systemroot")
        || image.starts_with(r"system32\")
        || (!windows_dir.is_empty() && image.starts_with(&windows_dir))
        || image.contains("svchost.exe")
        || image.contains(r"\microsoft\windows defender")
        || image.contains(r"\windows defender advanced threat protection"))
}

pub fn is_essential_service(s: &RawService) -> bool {
    let text = format!(
        "{} {} {}",
        s.name,
        s.display_name.as_deref().unwrap_or(""),
        s.image_path.as_deref().unwrap_or("")
    )
    .to_lowercase();
    ESSENTIAL_WORDS.iter().any(|w| text.contains(w))
}

/// The entries to show: program services that start automatically, and those the app set to
/// start when needed (`changed`), which can be set back.
pub fn entries(
    raw: &[RawService],
    windows_dir: &str,
    changed: &BTreeSet<String>,
) -> Vec<StartupEntry> {
    raw.iter()
        .filter(|s| is_program_service(s, windows_dir))
        .filter(|s| s.start == Some(AUTO) || (s.start == Some(MANUAL) && changed.contains(&s.name)))
        .map(|s| {
            let enabled = s.start == Some(AUTO);
            StartupEntry {
                id: format!("service|{}", s.name),
                name: clean_display_name(s),
                command: s.image_path.clone().unwrap_or_default(),
                source: Source::Service,
                enabled,
                needs_admin: true,
                essential: is_essential_service(s),
                when: Some(if enabled {
                    "Service: starts with the system"
                } else {
                    "Service: starts when needed"
                }),
            }
        })
        .collect()
}

/// Display names can be resource references (`@C:\x.dll,-101`); fall back to the key name.
fn clean_display_name(s: &RawService) -> String {
    s.display_name
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty() && !d.starts_with('@'))
        .unwrap_or(&s.name)
        .to_owned()
}

/// Where the app remembers the services it set to start when needed. Services belong to the
/// computer, so the record does too.
fn record_path() -> Option<PathBuf> {
    Known::ProgramData
        .path()
        .map(|p| p.join("rewind-os").join("services-set-to-manual.txt"))
}

fn changed() -> BTreeSet<String> {
    record_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn save_changed(names: &BTreeSet<String>) -> Result<()> {
    let path = record_path().ok_or_else(|| EngineError::NotFound("ProgramData".to_owned()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| EngineError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    let text: String = names.iter().map(|n| format!("{n}\n")).collect();
    std::fs::write(&path, text).map_err(|source| EngineError::Io { path, source })
}

fn read(name: &str) -> RawService {
    let path = format!(r"{SERVICES}\{name}");
    let values = registry::values(Hive::LocalMachine, &path);
    let get = |key: &str| {
        values
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    };
    let text = |key: &str| get(key).and_then(Value::as_text).map(str::to_owned);
    let number = |key: &str| match get(key) {
        Some(Value::Number(n)) => Some(*n),
        _ => None,
    };
    RawService {
        name: name.to_owned(),
        display_name: text("DisplayName"),
        image_path: text("ImagePath"),
        start: number("Start"),
        kind: number("Type"),
    }
}

fn windows_dir() -> String {
    Known::Windows
        .path()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub(super) fn list() -> Vec<StartupEntry> {
    let raw: Vec<RawService> = registry::subkeys(Hive::LocalMachine, SERVICES)
        .iter()
        .map(|n| read(n))
        .collect();
    entries(&raw, &windows_dir(), &changed())
}

/// `enabled` false sets the service to start when needed; true sets it back to start with the
/// system, and only for a service the app changed.
pub(super) fn set_enabled(name: &str, enabled: bool) -> Result<()> {
    if !rewind_os_platform::elevation::is_elevated() {
        return Err(EngineError::NeedsAdmin);
    }
    let service = read(name);
    if !is_program_service(&service, &windows_dir()) {
        return Err(EngineError::Refused(super::SYSTEM_SERVICE));
    }
    if !enabled && is_essential_service(&service) {
        return Err(EngineError::Refused(super::ESSENTIAL));
    }
    let mut record = changed();
    if enabled && !record.contains(name) {
        return Err(EngineError::Refused(super::NOT_CHANGED_BY_APP));
    }
    let path = format!(r"{SERVICES}\{name}");
    if enabled {
        registry::set_dword(Hive::LocalMachine, &path, "Start", AUTO as u32)?;
        record.remove(name);
        return save_changed(&record);
    }
    // Remember first: a change the app cannot find again could not be undone here.
    record.insert(name.to_owned());
    save_changed(&record)?;
    if let Err(err) = registry::set_dword(Hive::LocalMachine, &path, "Start", MANUAL as u32) {
        record.remove(name);
        let _ = save_changed(&record);
        return Err(err.into());
    }
    Ok(())
}
