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
            ..Guard::default()
        };
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
                if let Some(data) = get(Known::ProgramData) {
                    g.core.push(data.join("Microsoft\\Windows Defender"));
                    g.core.push(data.join("Microsoft\\Crypto"));
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
            .or_else(|| self.inside_any(&p, &self.system).then_some(SYSTEM))
    }

    /// Why a cleanup category may not delete `path`, a target its templates matched.
    pub fn cleanup_refusal(&self, path: &Path) -> Option<&'static str> {
        let p = self.norm(path);
        self.shared_refusal(&p)
            .or_else(|| self.inside_any(&p, &self.core).then_some(SYSTEM))
            .or_else(|| self.inside_any(&p, &self.personal).then_some(PERSONAL))
    }

    fn shared_refusal(&self, p: &Path) -> Option<&'static str> {
        if p.parent().is_none() || self.files.iter().any(|f| self.norm(f) == p) {
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
