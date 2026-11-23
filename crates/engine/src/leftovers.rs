//! Folders left behind by programs that are no longer installed.
//!
//! A folder in Program Files, ProgramData or AppData is "accounted for" when an installed
//! program, a service, a running process or a startup entry refers to it by name or path.
//! The rest are candidates. Matching is deliberately generous: a wrongly kept folder costs
//! some space, a wrongly flagged one could cost data. Candidates go to the recycle bin, never
//! straight to deletion.

use crate::walk::{self, Filter};
use crate::{Task, programs, startup};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use rewind_os_platform::paths::Known;
use rewind_os_platform::registry::{self, Hive};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Location {
    ProgramFiles,
    ProgramFilesX86,
    ProgramData,
    RoamingAppData,
    LocalAppData,
    UserPrograms,
}

impl Location {
    /// English label; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Location::ProgramFiles => "Program Files",
            Location::ProgramFilesX86 => "Program Files (x86)",
            Location::ProgramData => "ProgramData",
            Location::RoamingAppData => "AppData\\Roaming",
            Location::LocalAppData => "AppData\\Local",
            Location::UserPrograms => "AppData\\Local\\Programs",
        }
    }

    fn path(self) -> Option<PathBuf> {
        match self {
            Location::ProgramFiles => Known::ProgramFiles.path(),
            Location::ProgramFilesX86 => Known::ProgramFilesX86.path(),
            Location::ProgramData => Known::ProgramData.path(),
            Location::RoamingAppData => Known::RoamingAppData.path(),
            Location::LocalAppData => Known::LocalAppData.path(),
            Location::UserPrograms => Known::LocalAppData.path().map(|p| p.join("Programs")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Nothing refers to it, but something inside changed in the last 30 days.
    Low,
    /// Nothing refers to it and nothing inside changed in 30 days.
    Medium,
    /// Nothing refers to it and nothing inside changed in 180 days.
    High,
}

#[derive(Debug, Clone, Serialize)]
pub struct Leftover {
    pub path: PathBuf,
    pub location: Location,
    pub bytes: u64,
    pub files: u64,
    /// Newest change of any file inside.
    pub last_change: Option<DateTime<Utc>>,
    pub confidence: Confidence,
}

/// Folders that belong to Windows or are shared by many programs. Lowercase.
const SHARED: &[&str] = &[
    "microsoft",
    "windows",
    "common files",
    "internet explorer",
    "windowsapps",
    "modifiablewindowsapps",
    "microsoft.net",
    "reference assemblies",
    "msbuild",
    "windows defender",
    "windows defender advanced threat protection",
    "windows mail",
    "windows media player",
    "windows multimedia platform",
    "windows nt",
    "windows photo viewer",
    "windows portable devices",
    "windows security",
    "windows sidebar",
    "windowspowershell",
    "uninstall information",
    "packages",
    "temp",
    "package cache",
    "packagecache",
    "comms",
    "connecteddevicesplatform",
    "d3dscache",
    "crashdumps",
    "publishers",
    "programs",
    "history",
    "virtualstore",
    "microsoft_corporation",
    "ssh",
    "usoshared",
    "usoprivate",
    "softwaredistribution",
    "application data",
    "desktop",
    "documents",
    "start menu",
    "templates",
    "favorites",
    "regid.1991-06.com.microsoft",
    "microsoft help",
    "microsoft office",
    "dotnet",
    "iis",
    "iis express",
    "deliveryoptimization",
    "packagemanagement",
    "fontcache",
    "squirreltemp",
    "npm",
    "npm-cache",
    "pip",
    "pnpm",
    "yarn",
    "nuget",
    "pypa",
    "jedi",
    "mozilla",
    "google",
    "adobe",
    "nvidia",
    "nvidia corporation",
    "amd",
    "ati technologies",
    "intel",
    "realtek",
    "logitech",
    "oracle",
    "java",
    "apple",
    "apple computer",
    "steam",
    "epic games",
    "identities",
    "media center programs",
    "msn",
    "speech",
    "speech graphics",
    "ms-teams",
    "packaged",
    "peerdistrepub",
    "wsl",
    "lxss",
    "dbg",
    "ssl",
    "gnupg",
    "github",
    "git",
];

/// What the system says is in use.
#[derive(Debug, Default)]
pub struct Signals {
    /// Normalized program names and publishers.
    names: Vec<String>,
    /// Lowercase install locations, icon paths, uninstall commands, service binaries,
    /// running executables and startup commands.
    paths: Vec<String>,
}

impl Signals {
    pub fn add_name(&mut self, name: &str) {
        let n = normalize(name);
        if n.len() >= 3 {
            self.names.push(n);
        }
    }

    pub fn add_path(&mut self, path: &str) {
        let p = path
            .trim()
            .trim_matches('"')
            .to_lowercase()
            .replace('/', "\\");
        if !p.is_empty() {
            self.paths.push(p);
        }
    }

    /// True when anything refers to a folder with this name.
    pub fn accounts_for(&self, folder_name: &str) -> bool {
        let lower = folder_name.to_lowercase();
        if SHARED.contains(&lower.as_str())
            || lower.starts_with('{')
            || lower.starts_with("microsoft")
        {
            return true;
        }
        let needle_mid = format!("\\{lower}\\");
        let needle_end = format!("\\{lower}");
        if self
            .paths
            .iter()
            .any(|p| p.contains(&needle_mid) || p.ends_with(&needle_end))
        {
            return true;
        }
        let norm = normalize(folder_name);
        if norm.len() < 3 {
            // Too short to tell apart from noise; never flag it.
            return true;
        }
        self.names.iter().any(|name| {
            *name == norm
                || (norm.len() >= 4 && name.contains(&norm))
                || (name.len() >= 4 && norm.contains(name.as_str()))
        })
    }
}

/// Lowercase letters and digits only: "Mozilla Firefox (x64)" -> "mozillafirefoxx64".
pub fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn gather_signals() -> Signals {
    let mut signals = Signals::default();
    for program in programs::installed() {
        signals.add_name(&program.name);
        if let Some(p) = &program.publisher {
            signals.add_name(p);
        }
        if let Some(p) = &program.install_location {
            signals.add_path(&p.to_string_lossy());
        }
        for text in [&program.display_icon, &program.uninstall_command]
            .into_iter()
            .flatten()
        {
            signals.add_path(text);
        }
    }
    let services = r"SYSTEM\CurrentControlSet\Services";
    for service in registry::subkeys(Hive::LocalMachine, services) {
        let key = format!(r"{services}\{service}");
        if let Some(image) = registry::value(Hive::LocalMachine, &key, "ImagePath")
            && let Some(text) = image.as_text()
        {
            signals.add_path(text);
        }
    }
    if let Ok(mut sys) = System::new() {
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
        for process in sys.processes().values() {
            if let Some(exe) = process.exe() {
                signals.add_path(&exe.to_string_lossy());
            }
        }
    }
    for entry in startup::list() {
        signals.add_path(&entry.command);
    }
    signals
}

/// Lists candidate leftover folders, most likely first, then largest.
pub fn find(task: &Task) -> Vec<Leftover> {
    task.set_stage("Reading installed programs and services");
    let signals = gather_signals();
    task.set_stage("Checking folders");
    let mut candidates: Vec<(PathBuf, Location)> = Vec::new();
    for location in [
        Location::ProgramFiles,
        Location::ProgramFilesX86,
        Location::ProgramData,
        Location::RoamingAppData,
        Location::LocalAppData,
        Location::UserPrograms,
    ] {
        let Some(root) = location.path() else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            // Only real folders; links and junctions are skipped.
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !signals.accounts_for(&name) {
                candidates.push((entry.path(), location));
            }
        }
    }
    task.set_stage("Measuring candidate folders");
    let now = SystemTime::now();
    let mut found: Vec<Leftover> = candidates
        .into_par_iter()
        .map(|(path, location)| {
            task.set_detail(path.display().to_string());
            let tally = walk::measure(&path, &Filter::default(), task);
            let newest = tally.newest.or_else(|| modified(&path));
            Leftover {
                confidence: confidence(newest, now),
                last_change: newest.map(DateTime::<Utc>::from),
                path,
                location,
                bytes: tally.bytes,
                files: tally.files,
            }
        })
        .collect();
    found.sort_by(|a, b| b.confidence.cmp(&a.confidence).then(b.bytes.cmp(&a.bytes)));
    found
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Older untouched folders are more likely abandoned.
pub fn confidence(last_change: Option<SystemTime>, now: SystemTime) -> Confidence {
    let age = last_change
        .and_then(|t| now.duration_since(t).ok())
        .unwrap_or(Duration::ZERO);
    let days = age.as_secs() / 86_400;
    if days >= 180 {
        Confidence::High
    } else if days >= 30 {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}
