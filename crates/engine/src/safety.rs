//! The guard every removal goes through, so no click, command or bad environment variable can
//! leave the computer unable to start or wipe a user's personal folders.
//!
//! Three kinds of places:
//!
//! - **Anchors**: drive roots, the users folder, every profile, the home folder, the program
//!   and system folders, the personal folders. Never removed, and nothing that *contains* one is
//!   removed either (removing `C:\Users` would take the home folder with it).
//! - **System zones**: the operating system's own folders (`C:\Windows`, `/usr`, `/System`).
//!   Nothing inside them is removed when a person picks it by hand.
//! - **Core zones**: the parts of the system no cleanup may ever touch (`System32`, `WinSxS`,
//!   `/etc`, `/boot`) and the personal folders (Documents, Pictures…). Cleanup categories may
//!   remove curated paths inside a system zone (`C:\Windows\Temp`) but never inside a core zone.
//!
//! Paths are compared after resolving links and `..`, case-insensitively where the file system
//! is, so `C:\Windows\..\WINDOWS\System32` and a junction into it are both refused.

use rewind_os_platform::Os;
use rewind_os_platform::paths::Known;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// English reasons; translate with `rewind_os_i18n::tr`.
pub const SYSTEM: &str =
    "This is part of the operating system. Removing it could stop the computer from starting.";
pub const CONTAINS: &str =
    "This folder holds your user folders or programs. Remove what is inside it instead.";
pub const PERSONAL: &str = "Cleanup never touches your personal folders.";
pub const KEYS: &str = "This holds your keys and saved passwords. Removing it could lock you out of your accounts and encrypted files.";

/// Folder names that hold a program's or a project's own files wherever they are. A copy of a
/// file in one of them is part of that program or project, not a duplicate to clean.
const PROGRAM_PARTS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "site-packages",
    "dist-packages",
    ".venv",
    "venv",
    "steamapps",
    ".cargo",
    ".rustup",
    ".gradle",
    ".m2",
    ".nuget",
    ".npm",
];

/// Folder name endings of macOS bundles, whose insides belong together.
const BUNDLES: &[&str] = &[
    ".app",
    ".framework",
    ".bundle",
    ".plugin",
    ".photoslibrary",
    ".musiclibrary",
];

/// Names that are part of Windows at the root of any drive, not only the system drive, so a
/// second Windows on another drive is protected too.
const WINDOWS_ROOT_NAMES: &[&str] = &[
    "windows",
    "boot",
    "efi",
    "recovery",
    "system volume information",
    "$recycle.bin",
    "bootmgr",
    "bootnxt",
    "pagefile.sys",
    "hiberfil.sys",
    "swapfile.sys",
];

#[derive(Debug, Clone, Default)]
pub struct Guard {
    anchors: Vec<PathBuf>,
    /// Folders whose direct children are user profiles (`C:\Users`, `/home`).
    profile_parents: Vec<PathBuf>,
    system: Vec<PathBuf>,
    core: Vec<PathBuf>,
    personal: Vec<PathBuf>,
    /// Single files that must never go (`C:\pagefile.sys`, `C:\bootmgr`).
    files: Vec<PathBuf>,
    /// Places inside every user profile, relative to it, with the reason they are kept. `true`
    /// when cleanup may not touch them either.
    profile_parts: Vec<(PathBuf, &'static str, bool)>,
    /// Folders whose files belong to installed programs or their settings.
    programs: Vec<PathBuf>,
    windows: bool,
    case_insensitive: bool,
}

impl Guard {
    /// The guard for this computer, built once.
    pub fn current() -> &'static Guard {
        static GUARD: OnceLock<Guard> = OnceLock::new();
        GUARD.get_or_init(|| Guard::for_system(Os::current(), &known_folders()))
    }

    /// The guard for `os`, given where its known folders are.
    pub fn for_system(os: Os, known: &[(Known, PathBuf)]) -> Guard {
        let get = |k: Known| {
            known
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, p)| p.clone())
        };
        let home = get(Known::Home);
        let mut g = Guard {
            case_insensitive: os != Os::Linux,
            windows: os == Os::Windows,
            ..Guard::default()
        };
        g.programs.extend(
            [Known::Cache, Known::Config, Known::Data]
                .into_iter()
                .filter_map(get),
        );
        let personal_names = [
            "Documents",
            "Desktop",
            "Pictures",
            "Music",
            "Videos",
            "Movies",
            "Documentos",
            "Escritorio",
            "Imágenes",
            "Música",
            "Vídeos",
        ];
        if let Some(home) = &home {
            g.personal
                .extend(personal_names.iter().map(|n| home.join(n)));
            g.personal.extend(xdg_user_dirs(home));
            if let Some(parent) = home.parent() {
                g.profile_parents.push(parent.to_path_buf());
            }
        }
        g.personal
            .extend(std::env::var_os("OneDrive").map(PathBuf::from));
        g.anchors.extend(g.personal.clone());
        g.anchors.extend(known.iter().map(|(_, p)| p.clone()));
        g.anchors.extend(g.profile_parents.clone());
        match os {
            Os::Windows => {
                let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
                let root = PathBuf::from(format!("{drive}\\"));
                let windows = get(Known::Windows).unwrap_or_else(|| root.join("Windows"));
                g.anchors.push(root.join("Users"));
                g.profile_parents.push(root.join("Users"));
                g.system.push(windows.clone());
                for name in [
                    "Boot",
                    "Recovery",
                    "System Volume Information",
                    "$Recycle.Bin",
                    "EFI",
                ] {
                    g.system.push(root.join(name));
                }
                for name in [
                    "System32",
                    "SysWOW64",
                    "WinSxS",
                    "servicing",
                    "Boot",
                    "Fonts",
                    "INF",
                    "assembly",
                    "Installer",
                    "Microsoft.NET",
                    "SystemApps",
                    "SystemResources",
                    "Resources",
                    "WaaS",
                ] {
                    g.core.push(windows.join(name));
                }
                for name in [
                    "bootmgr",
                    "BOOTNXT",
                    "pagefile.sys",
                    "hiberfil.sys",
                    "swapfile.sys",
                ] {
                    g.files.push(root.join(name));
                }
                for base in [get(Known::ProgramFiles), get(Known::ProgramFilesX86)]
                    .into_iter()
                    .flatten()
                {
                    g.core.push(base.join("WindowsApps"));
                    g.core.push(base.join("Common Files"));
                    g.core.push(base.join("Windows Defender"));
                }
                for base in [get(Known::ProgramFiles), get(Known::ProgramFilesX86)]
                    .into_iter()
                    .flatten()
                {
                    // Parts of Windows that live in Program Files.
                    for name in [
                        "Windows NT",
                        "WindowsPowerShell",
                        "Internet Explorer",
                        "Windows Mail",
                        "Windows Media Player",
                        "Windows Multimedia Platform",
                        "Windows Photo Viewer",
                        "Windows Portable Devices",
                        "Windows Security",
                        "Windows Sidebar",
                        "Windows Defender Advanced Threat Protection",
                        "ModifiableWindowsApps",
                        "Microsoft Update Health Tools",
                        "Reference Assemblies",
                        "dotnet",
                        "MSBuild",
                    ] {
                        g.system.push(base.join(name));
                    }
                    g.programs.push(base);
                }
                if let Some(data) = get(Known::ProgramData) {
                    g.core.push(data.join("Microsoft\\Windows Defender"));
                    g.core.push(data.join("Microsoft\\Crypto"));
                    // Windows' own data, and the installers programs need to repair or
                    // uninstall themselves.
                    g.system.push(data.join("Microsoft"));
                    g.system.push(data.join("Package Cache"));
                    g.programs.push(data);
                }
                g.programs.extend(
                    [Known::LocalAppData, Known::RoamingAppData]
                        .into_iter()
                        .filter_map(get),
                );
                for part in [
                    r"AppData\Roaming\Microsoft\Protect",
                    r"AppData\Roaming\Microsoft\Crypto",
                    r"AppData\Roaming\Microsoft\SystemCertificates",
                    r"AppData\Roaming\Microsoft\Credentials",
                    r"AppData\Roaming\Microsoft\Vault",
                    r"AppData\Local\Microsoft\Credentials",
                    r"AppData\Local\Microsoft\Vault",
                ] {
                    g.profile_parts.push((PathBuf::from(part), KEYS, true));
                }
                for part in [
                    r"AppData\Local\Packages",
                    r"AppData\Local\Microsoft\Windows",
                    r"AppData\Local\Microsoft\WindowsApps",
                    r"AppData\Roaming\Microsoft\Windows",
                ] {
                    g.profile_parts.push((PathBuf::from(part), SYSTEM, false));
                }
            }
            Os::Linux | Os::MacOs => {
                g.anchors.push(PathBuf::from("/"));
                g.anchors.push(PathBuf::from("/home"));
                g.profile_parents.push(PathBuf::from("/home"));
                let unix_core = [
                    "/bin",
                    "/boot",
                    "/dev",
                    "/etc",
                    "/lib",
                    "/lib32",
                    "/lib64",
                    "/libx32",
                    "/proc",
                    "/run",
                    "/sbin",
                    "/sys",
                    "/usr",
                    "/snap",
                    "/var/lib/dpkg",
                    "/var/lib/rpm",
                    "/var/lib/pacman",
                    "/var/lib/apt",
                    "/var/lib/flatpak",
                    "/System",
                    "/Library",
                    "/private/etc",
                    "/private/var/db",
                    "/cores",
                    "/efi",
                    "/nix",
                    "/gnu",
                ];
                g.core.extend(unix_core.iter().map(PathBuf::from));
                g.system.extend(g.core.clone());
                g.system.extend(
                    [
                        "/var",
                        "/private/var",
                        "/private/etc",
                        "/opt/homebrew/Cellar",
                    ]
                    .iter()
                    .map(PathBuf::from),
                );
                g.anchors.extend(
                    ["/root", "/Users", "/Applications", "/Volumes"]
                        .iter()
                        .map(PathBuf::from),
                );
                g.profile_parents.push(PathBuf::from("/Users"));
                if let Some(home) = &home {
                    g.anchors.push(home.join("Library"));
                    g.programs.push(home.join("Library"));
                    g.programs.push(home.join("Applications"));
                    g.programs.push(home.join(".var"));
                    g.programs.push(home.join(".local/lib"));
                }
                g.programs
                    .extend(["/opt", "/Applications"].iter().map(PathBuf::from));
                for part in [
                    ".ssh",
                    ".gnupg",
                    ".pki",
                    ".password-store",
                    ".local/share/keyrings",
                    "Library/Keychains",
                ] {
                    g.profile_parts.push((PathBuf::from(part), KEYS, true));
                }
            }
        }
        g.system.extend(g.core.clone());
        g
    }

    fn norm(&self, path: &Path) -> PathBuf {
        normalize(path, self.case_insensitive)
    }

    fn inside_any(&self, path: &Path, zones: &[PathBuf]) -> bool {
        zones.iter().any(|z| path.starts_with(self.norm(z)))
    }

    /// Why `path` may not be removed when a person picks it (disk analyzer, leftovers).
    pub fn removal_refusal(&self, path: &Path) -> Option<&'static str> {
        let p = self.norm(path);
        self.shared_refusal(&p)
            .or_else(|| self.profile_refusal(&p, false))
            .or_else(|| self.inside_any(&p, &self.system).then_some(SYSTEM))
    }

    /// Why a cleanup category may not delete `path`, a target its templates matched.
    pub fn cleanup_refusal(&self, path: &Path) -> Option<&'static str> {
        let p = self.norm(path);
        self.shared_refusal(&p)
            .or_else(|| self.profile_refusal(&p, true))
            .or_else(|| self.inside_any(&p, &self.core).then_some(SYSTEM))
            .or_else(|| self.inside_any(&p, &self.personal).then_some(PERSONAL))
    }

    /// True when `path` holds an installed program's files or settings, or a project's own
    /// tools (`.git`, `node_modules`, an `.app` bundle). Identical files there are each needed
    /// where they are, so they are never offered as duplicates.
    pub fn is_program_folder(&self, path: &Path) -> bool {
        let p = self.norm(path);
        if self.inside_any(&p, &self.programs) {
            return true;
        }
        p.components().any(|c| {
            let name = c.as_os_str().to_string_lossy().to_lowercase();
            PROGRAM_PARTS.contains(&name.as_str())
                || BUNDLES
                    .iter()
                    .any(|b| name.len() > b.len() && name.ends_with(b))
        })
    }

    /// The keys, the registry hive and Windows' own folders inside any user's profile.
    fn profile_refusal(&self, p: &Path, cleanup: bool) -> Option<&'static str> {
        let rel = self.profile_parents.iter().find_map(|u| {
            let rest = p.strip_prefix(self.norm(u)).ok()?;
            let mut parts = rest.components();
            parts.next()?;
            Some(parts.as_path().to_path_buf())
        })?;
        if self.windows
            && let Some(name) = p.file_name()
        {
            // The user's registry: NTUSER.DAT and its logs, UsrClass.dat.
            let name = name.to_string_lossy().to_lowercase();
            if name.starts_with("ntuser.") || name.starts_with("usrclass.dat") {
                return Some(SYSTEM);
            }
        }
        self.profile_parts
            .iter()
            .filter(|(_, _, core)| *core || !cleanup)
            .find(|(part, _, _)| rel.starts_with(normalize_text(part, self.case_insensitive)))
            .map(|(_, why, _)| *why)
    }

    fn shared_refusal(&self, p: &Path) -> Option<&'static str> {
        if p.parent().is_none() || self.files.iter().any(|f| self.norm(f) == p) {
            return Some(SYSTEM);
        }
        // Windows' boot and system folders at the root of any drive.
        if self.windows
            && p.parent().is_some_and(|d| d.parent().is_none())
            && let Some(name) = p.file_name()
            && WINDOWS_ROOT_NAMES.contains(&name.to_string_lossy().to_lowercase().as_str())
        {
            return Some(SYSTEM);
        }
        // An anchor itself, or anything containing one.
        if self.anchors.iter().any(|a| self.norm(a).starts_with(p)) {
            return Some(CONTAINS);
        }
        // A whole user profile, even one that is not the current user's.
        if self
            .profile_parents
            .iter()
            .any(|u| p.parent() == Some(self.norm(u).as_path()))
        {
            return Some(CONTAINS);
        }
        None
    }
}

/// English reasons a running program is not closed; translate with `rewind_os_i18n::tr`.
pub const SYSTEM_PROCESS: &str =
    "The system needs this to keep running. Closing it could freeze or restart the computer.";
pub const OTHER_ACCOUNT: &str = "It runs for the system or another account.";
pub const THIS_APP: &str = "This is Rewind OS itself.";
pub const HOLDS_APP: &str = "Rewind OS runs inside it; closing it would close Rewind OS too.";

/// Programs the system or the desktop needs, by file name in lowercase. Closing one could
/// freeze the computer, sign the user out or leave no taskbar.
const WINDOWS_PROCESSES: &[&str] = &[
    "system",
    "system idle process",
    "idle",
    "registry",
    "secure system",
    "memory compression",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "svchost.exe",
    "dwm.exe",
    "explorer.exe",
    "fontdrvhost.exe",
    "sihost.exe",
    "ctfmon.exe",
    "taskhostw.exe",
    "runtimebroker.exe",
    "dllhost.exe",
    "conhost.exe",
    "audiodg.exe",
    "spoolsv.exe",
    "wudfhost.exe",
    "msmpeng.exe",
    "nissrv.exe",
    "mpdefendercoreservice.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "smartscreen.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "shellhost.exe",
    "searchhost.exe",
    "textinputhost.exe",
    "applicationframehost.exe",
    "lockapp.exe",
    "userinit.exe",
    "logonui.exe",
];

const UNIX_PROCESSES: &[&str] = &[
    "init",
    "systemd",
    "kthreadd",
    "launchd",
    "kernel_task",
    "windowserver",
    "loginwindow",
    "dock",
    "finder",
    "systemuiserver",
    "xorg",
    "xwayland",
    "x",
    "gnome-shell",
    "gnome-session-binary",
    "gnome-session",
    "plasmashell",
    "kwin_x11",
    "kwin_wayland",
    "ksmserver",
    "mutter",
    "cinnamon",
    "cinnamon-session",
    "xfce4-session",
    "xfwm4",
    "xfce4-panel",
    "mate-session",
    "marco",
    "budgie-wm",
    "sway",
    "hyprland",
    "weston",
    "labwc",
    "wayfire",
    "dbus-daemon",
    "dbus-broker",
    "dbus-broker-launch",
    "pipewire",
    "pipewire-pulse",
    "wireplumber",
    "pulseaudio",
    "polkitd",
    "gdm",
    "gdm3",
    "sddm",
    "lightdm",
    "login",
    "agetty",
    "sshd",
    "networkmanager",
    "xdg-desktop-portal",
];

/// Prefixes of the same on Linux: `systemd-logind`, `xdg-desktop-portal-gnome`, `at-spi2-…`.
const UNIX_PROCESS_PREFIXES: &[&str] = &[
    "systemd-",
    "xdg-desktop-portal-",
    "at-spi",
    "gnome-session-",
];

/// Why the program with file name `name` must not be closed on `os`, or `None` when it may.
pub fn process_refusal_on(os: Os, name: &str) -> Option<&'static str> {
    let name = name.trim().to_lowercase();
    let ours = name.trim_end_matches(".exe");
    if ours == "rewind-os" || ours == "rewind-os-cli" {
        return Some(THIS_APP);
    }
    let system = match os {
        Os::Windows => WINDOWS_PROCESSES.contains(&name.as_str()),
        Os::Linux | Os::MacOs => {
            UNIX_PROCESSES.contains(&name.as_str())
                || UNIX_PROCESS_PREFIXES.iter().any(|p| name.starts_with(p))
        }
    };
    system.then_some(SYSTEM_PROCESS)
}

/// Why the program with file name `name` must not be closed on this computer.
pub fn process_refusal(name: &str) -> Option<&'static str> {
    process_refusal_on(Os::current(), name)
}

/// Why `path` may not be removed by hand on this computer, or `None` when it may.
pub fn removal_refusal(path: &Path) -> Option<&'static str> {
    Guard::current().removal_refusal(path)
}

fn known_folders() -> Vec<(Known, PathBuf)> {
    Known::ALL
        .iter()
        .filter_map(|k| k.path().map(|p| (*k, p)))
        .collect()
}

/// The folders named in `~/.config/user-dirs.dirs` (`XDG_DOCUMENTS_DIR="$HOME/Documentos"`).
fn xdg_user_dirs(home: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(home.join(".config/user-dirs.dirs")) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| k.trim().starts_with("XDG_") && !k.contains("DOWNLOAD"))
        .map(|(_, v)| {
            v.trim()
                .trim_matches('"')
                .replace("$HOME", &home.to_string_lossy())
        })
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.as_path() != home)
        .collect()
}

/// `.` and `..` worked out and lowercased where the system ignores case, without touching the
/// file system: for paths relative to a profile.
fn normalize_text(path: &Path, case_insensitive: bool) -> PathBuf {
    let out = lexical(path);
    if case_insensitive {
        PathBuf::from(out.to_string_lossy().to_lowercase())
    } else {
        out
    }
}

/// Resolves links in the longest part of `path` that exists and appends the rest, so a path
/// that does not exist yet still compares like its real parent (`/etc/x` is `/private/etc/x` on
/// macOS, where `/etc` is a link).
fn resolve_existing_prefix(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut current = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(current) {
            return rest.iter().rev().fold(real, |acc, part| acc.join(part));
        }
        match (current.parent(), current.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                current = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Resolves links where the path exists, then `.` and `..`, drops Windows' `\\?\` prefix and
/// trailing separators, and lowercases on case-insensitive systems.
fn normalize(path: &Path, case_insensitive: bool) -> PathBuf {
    let resolved = resolve_existing_prefix(&lexical(path));
    let text = resolved.to_string_lossy();
    let out = lexical(Path::new(text.strip_prefix(r"\\?\").unwrap_or(&text)));
    let out = without_data_volume(out);
    if case_insensitive {
        PathBuf::from(out.to_string_lossy().to_lowercase())
    } else {
        out
    }
}

/// On macOS the data volume, mounted at `/System/Volumes/Data`, mirrors the root: resolving
/// `/home` or `/Users` can lead there. It is user data, not part of `/System`, so compare it as
/// the path it mirrors.
fn without_data_volume(path: PathBuf) -> PathBuf {
    const DATA: &str = "/System/Volumes/Data";
    match path.strip_prefix(DATA) {
        Ok(rest) => Path::new("/").join(rest),
        Err(_) => path,
    }
}

/// `path` with `.` and `..` worked out from the text alone.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Whether `name` is the name Windows gives a driver package it added (`oem12.inf`): the only
/// driver packages Rewind OS may remove, and the only text it passes to `pnputil`.
pub fn is_oem_driver_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower
        .strip_prefix("oem")
        .and_then(|rest| rest.strip_suffix(".inf"))
        .is_some_and(|n| !n.is_empty() && n.len() <= 6 && n.bytes().all(|b| b.is_ascii_digit()))
}
