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
    /// Linux: `~/.config`.
    UserConfig,
    /// Linux: `~/.local/share`.
    UserData,
    /// macOS: `~/Library/Application Support`.
    ApplicationSupport,
    /// macOS: `~/Library/Containers`.
    Containers,
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
            Location::UserConfig => "~/.config",
            Location::UserData => "~/.local/share",
            Location::ApplicationSupport => "~/Library/Application Support",
            Location::Containers => "~/Library/Containers",
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
            Location::UserConfig => Known::Config.path(),
            Location::UserData => Known::Data.path(),
            Location::ApplicationSupport => Known::Home
                .path()
                .map(|h| h.join("Library/Application Support")),
            Location::Containers => Known::Home.path().map(|h| h.join("Library/Containers")),
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
    // The Windows SDK and driver kit, shared by Visual Studio, its build tools and others; the
    // SDK's entry in Programs names neither the folder nor where it is.
    "windows kits",
    "hyper-v",
    "placeholdertilelogofolder",
    "windowsoobeapphost",
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
    "node-gyp",
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
        // "Termius 9.43.1" also as "termius", so its `termius-updater` folder is its own.
        let words: Vec<&str> = name
            .split_whitespace()
            .filter(|w| {
                !w.starts_with(|c: char| c.is_ascii_digit())
                    && !w
                        .strip_prefix(['v', 'V'])
                        .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
                    && !w.eq_ignore_ascii_case("version")
            })
            .collect();
        let bare = normalize(&words.join(" "));
        if bare.len() >= 4 && self.names.last() != Some(&bare) {
            self.names.push(bare);
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
            || lower == crate::history::APP_FOLDER
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

/// The Windows paths (`C:/Program Files (x86)/World of Warcraft`) written as protobuf strings
/// in a binary record, such as Battle.net's `product.db`: each comes after a byte that gives
/// its length.
pub fn paths_in(bytes: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    for (i, window) in bytes.windows(3).enumerate() {
        let [drive, b':', b'/' | b'\\'] = window else {
            continue;
        };
        if !drive.is_ascii_alphabetic() {
            continue;
        }
        let Some(&len) = i.checked_sub(1).and_then(|before| bytes.get(before)) else {
            continue;
        };
        let Some(text) = bytes
            .get(i..i + usize::from(len))
            .filter(|t| t.iter().all(|b| b.is_ascii_graphic() || *b == b' '))
            .and_then(|t| std::str::from_utf8(t).ok())
        else {
            continue;
        };
        found.push(text.to_owned());
    }
    found
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
    // Battle.net games are not in Programs; its own record lists where each one is.
    if let Some(data) = Known::ProgramData.path()
        && let Ok(db) = std::fs::read(data.join(r"Battle.net\Agent\product.db"))
    {
        for path in paths_in(&db) {
            signals.add_path(&path);
        }
    }
    // Folders on the PATH hold tools in use that Programs may not list (Chocolatey, Scoop,
    // command line tools unpacked by hand).
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            signals.add_path(&dir.to_string_lossy());
        }
    }
    signals
}

/// Folders in `~/.config` and `~/.local/share` that belong to the desktop or are shared by
/// many programs. Lowercase.
const LINUX_SHARED: &[&str] = &[
    "autostart",
    "dconf",
    "gtk-2.0",
    "gtk-3.0",
    "gtk-4.0",
    "fontconfig",
    "fonts",
    "pulse",
    "systemd",
    "environment.d",
    "menus",
    "ibus",
    "mime",
    "applications",
    "icons",
    "themes",
    "keyrings",
    "trash",
    "user-dirs.dirs",
    "user-dirs.locale",
    "xdg-desktop-portal",
    "flatpak",
    "snap",
    "pki",
    "gnupg",
    "recently-used.xbel",
    "sounds",
    "desktop-directories",
    "evolution",
    "gnome-shell",
    "gnome-session",
    "nautilus",
    "tracker",
    "tracker3",
    "kdeglobals",
    "kde",
    "kded5",
    "kded6",
    "plasma-workspace",
    "kwin",
    "akonadi",
    "baloo",
    "xfce4",
    "lxsession",
    "cinnamon",
    "mate",
    "qt5ct",
    "qt6ct",
    "kvantum",
    "session",
    "zeitgeist",
    "gvfs-metadata",
    "nano",
    "vim",
    "nvim",
    "containers",
    "docker",
    "git",
    "htop",
    "procps",
    "wireplumber",
    "pipewire",
    "speech-dispatcher",
    "ubuntu-advantage",
    "go",
    "pip",
    "pnpm",
    "npm",
    "yarn",
    "cargo",
    "rustup",
    "virtualenv",
    "jupyter",
];

/// Folders in `~/Library` that belong to macOS itself or are shared. Lowercase.
const MAC_SHARED: &[&str] = &[
    "addressbook",
    "callhistorydb",
    "callhistorytransactions",
    "clouddocs",
    "knowledge",
    "fileprovider",
    "icloud",
    "mobile documents",
    "crashreporter",
    "syncservices",
    "dock",
    "icdd",
    "networkserviceproxy",
    "diskimages",
    "animoji",
    "appstore",
    "safari",
    "mobilesync",
    "accounts",
    "accessibility",
    "contextstoreagent",
    "cloudkit",
    "familycircle",
    "homekit",
    "identityservices",
    "ubiquity",
    "quick look",
    "quicklook",
    "siri",
    "spotlight",
    "assistant",
    "assistantservices",
    "notes",
    "reminders",
    "calendars",
    "photos",
    "facetime",
    "messages",
    "news",
    "stocks",
    "weather",
    "maps",
    "music",
    "tv",
    "podcasts",
    "books",
    "keychains",
    "dmd",
    "photoanalysisd",
    "tipsd",
    "transparencyd",
    "trustd",
    "useractivity",
    "videoconference",
    "audio units",
    "audio",
    "colorsync",
    "fonts",
    "input methods",
    "keyboard layouts",
    "preferencepanes",
    "screen savers",
    "services",
    "sounds",
    "spelling",
    "voices",
    "web plug-ins",
    "internet plug-ins",
    "sharedfilelistd",
    "sharedfilelist",
    "launchagents",
    "logs",
    "caches",
    "group containers",
    "metadata",
    "keyboard services",
    "backgroundtaskmanagement",
    "applicationsupport",
    "familysettings",
    "gamekit",
];

/// Whether a folder in `~/Library` (lowercase name) belongs to macOS or is shared.
pub fn mac_shared(lower: &str) -> bool {
    lower.starts_with("com.apple.")
        || lower.starts_with("group.com.apple.")
        || MAC_SHARED.contains(&lower)
}

/// Evidence on macOS: every app's name and bundle id (Apple's included), Homebrew packages,
/// the programs on the `PATH`, and the running programs.
fn gather_mac_signals() -> Signals {
    let mut signals = Signals::default();
    for name in crate::macapps::known_names() {
        signals.add_name(&name);
    }
    let bin_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for dir in bin_dirs {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            signals.add_name(&entry.file_name().to_string_lossy());
        }
    }
    if let Ok(mut sys) = System::new() {
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
        for process in sys.processes().values() {
            signals.add_name(&process.name().to_string_lossy());
            if let Some(exe) = process.exe() {
                signals.add_path(&exe.to_string_lossy());
            }
        }
    }
    signals
}

/// Evidence on Linux: every installed package and app, the programs on the `PATH`, the
/// programs desktop entries start, and the running programs.
fn gather_linux_signals() -> Signals {
    let mut signals = Signals::default();
    for name in crate::packages::all_names() {
        signals.add_name(&name);
    }
    // WSL adds the Windows folders (`/mnt/c/...`) to the PATH: slow to list, and Windows
    // programs say nothing about Linux folders.
    let mut bin_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    bin_dirs.retain(|d| !d.starts_with("/mnt"));
    if let Some(home) = Known::Home.path() {
        bin_dirs.push(home.join(".local/bin"));
    }
    for dir in bin_dirs {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            signals.add_name(&entry.file_name().to_string_lossy());
        }
    }
    let mut desktop_dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        PathBuf::from("/var/lib/snapd/desktop/applications"),
    ];
    if let Some(data) = Known::Data.path() {
        desktop_dirs.push(data.join("applications"));
        desktop_dirs.push(data.join("flatpak/exports/share/applications"));
    }
    for dir in desktop_dirs {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if let Some(stem) = path.file_stem() {
                signals.add_name(&stem.to_string_lossy());
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                for exec in text.lines().filter_map(|l| l.strip_prefix("Exec=")) {
                    if let Some(program) = exec.split_whitespace().next() {
                        signals.add_path(program);
                        if let Some(base) = Path::new(program).file_name() {
                            signals.add_name(&base.to_string_lossy());
                        }
                    }
                }
            }
        }
    }
    if let Ok(mut sys) = System::new() {
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
        for process in sys.processes().values() {
            signals.add_name(&process.name().to_string_lossy());
        }
    }
    signals
}

/// Lists candidate leftover folders, most likely first, then largest.
pub fn find(task: &Task) -> Vec<Leftover> {
    task.set_stage("Reading installed programs and services");
    let os = rewind_os_platform::Os::current();
    let linux = os == rewind_os_platform::Os::Linux;
    let mac = os == rewind_os_platform::Os::MacOs;
    let signals = if linux {
        gather_linux_signals()
    } else if mac {
        gather_mac_signals()
    } else {
        gather_signals()
    };
    task.set_stage("Checking folders");
    let mut candidates: Vec<(PathBuf, Location)> = Vec::new();
    let locations: &[Location] = if linux {
        &[Location::UserConfig, Location::UserData]
    } else if mac {
        &[Location::ApplicationSupport, Location::Containers]
    } else {
        &[
            Location::ProgramFiles,
            Location::ProgramFilesX86,
            Location::ProgramData,
            Location::RoamingAppData,
            Location::LocalAppData,
            Location::UserPrograms,
        ]
    };
    for &location in locations {
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
            let lower = name.to_lowercase();
            let shared_here =
                (linux && LINUX_SHARED.contains(&lower.as_str())) || (mac && mac_shared(&lower));
            if !shared_here && !signals.accounts_for(&name) {
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

/// A registry key under `Software` that nothing installed refers to. Reported only: Rewind OS
/// never removes registry keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegistryLeftover {
    /// Full path, as Registry Editor shows it (`HKEY_CURRENT_USER\Software\Vendor`).
    pub key: String,
    pub name: String,
}

/// Keys every Windows has, or that hold settings shared by many programs.
const REGISTRY_SHARED: &[&str] = &[
    "classes",
    "clients",
    "policies",
    "registeredapplications",
    "wow6432node",
    "software",
    "setup",
    "oem",
    "partner",
    "odbc",
    "khronos",
    "defaultuserenvironment",
    "systemfileassociations",
    "applications",
    "appdatalow",
    "deviceinfo",
    "gsettings",
    "syncengines",
    "javasoft",
    "changetracker",
    "inextuuid",
    "edge",
    "chromium",
    "qtproject",
    "valve",
    "python",
    "node.js",
    "gitforwindows",
];

/// Whether a key name is machine-made (a GUID, a number): nothing to show a person.
fn machine_made(name: &str) -> bool {
    let core = name.trim_matches(|c| c == '{' || c == '}');
    !core.is_empty() && core.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// The keys among `keys` (`(root, name)`, root like `HKEY_CURRENT_USER\Software`) that
/// `signals` does not account for.
pub fn registry_candidates(keys: &[(&str, String)], signals: &Signals) -> Vec<RegistryLeftover> {
    let mut out: Vec<RegistryLeftover> = keys
        .iter()
        .filter(|(_, name)| {
            let lower = name.to_lowercase();
            !REGISTRY_SHARED.contains(&lower.as_str())
                && !machine_made(name)
                && !signals.accounts_for(name)
        })
        .map(|(root, name)| RegistryLeftover {
            key: format!(r"{root}\{name}"),
            name: name.clone(),
        })
        .collect();
    out.sort_by_key(|l| l.name.to_lowercase());
    out
}

/// Vendor keys under `Software` (current user and computer) that no installed program,
/// service, running program or startup entry refers to.
pub fn find_registry(task: &Task) -> Vec<RegistryLeftover> {
    task.set_stage("Reading installed programs and services");
    let signals = gather_signals();
    task.set_stage("Checking registry keys");
    let mut keys = Vec::new();
    for (hive, path, root) in [
        (Hive::CurrentUser, "Software", r"HKEY_CURRENT_USER\Software"),
        (
            Hive::LocalMachine,
            "SOFTWARE",
            r"HKEY_LOCAL_MACHINE\SOFTWARE",
        ),
        (
            Hive::LocalMachine,
            r"SOFTWARE\WOW6432Node",
            r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node",
        ),
    ] {
        keys.extend(registry::subkeys(hive, path).into_iter().map(|k| (root, k)));
    }
    registry_candidates(&keys, &signals)
}
