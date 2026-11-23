//! Well-known folders, resolved the way each operating system defines them.

use std::path::PathBuf;

/// Folders the engine needs. Some only exist on one system; [`Known::path`] returns `None`
/// for them elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Known {
    /// The user's home folder.
    Home,
    /// The user's temporary folder.
    Temp,
    /// Per-user caches: `%LOCALAPPDATA%`, `~/Library/Caches`, `$XDG_CACHE_HOME`.
    Cache,
    /// Per-user settings: `%APPDATA%`, `~/Library/Application Support`, `$XDG_CONFIG_HOME`.
    Config,
    /// Per-user data: `%LOCALAPPDATA%`, `~/Library/Application Support`, `$XDG_DATA_HOME`.
    Data,
    /// The Downloads folder.
    Downloads,
    /// Windows: `%LOCALAPPDATA%`.
    LocalAppData,
    /// Windows: `%APPDATA%`.
    RoamingAppData,
    /// Windows: `%PROGRAMDATA%`.
    ProgramData,
    /// Windows: `%PROGRAMFILES%`.
    ProgramFiles,
    /// Windows: `%PROGRAMFILES(X86)%`.
    ProgramFilesX86,
    /// Windows: `%WINDIR%`.
    Windows,
}

impl Known {
    pub const ALL: [Known; 12] = [
        Known::Home,
        Known::Temp,
        Known::Cache,
        Known::Config,
        Known::Data,
        Known::Downloads,
        Known::LocalAppData,
        Known::RoamingAppData,
        Known::ProgramData,
        Known::ProgramFiles,
        Known::ProgramFilesX86,
        Known::Windows,
    ];

    /// The name used in path templates: `{cache}/thumbnails/*`.
    pub fn token(self) -> &'static str {
        match self {
            Known::Home => "home",
            Known::Temp => "temp",
            Known::Cache => "cache",
            Known::Config => "config",
            Known::Data => "data",
            Known::Downloads => "downloads",
            Known::LocalAppData => "local_app_data",
            Known::RoamingAppData => "roaming_app_data",
            Known::ProgramData => "program_data",
            Known::ProgramFiles => "program_files",
            Known::ProgramFilesX86 => "program_files_x86",
            Known::Windows => "windows",
        }
    }

    /// The folder on this computer, or `None` when it does not exist on this system or the
    /// environment does not say where it is.
    pub fn path(self) -> Option<PathBuf> {
        if cfg!(windows) {
            self.windows()
        } else {
            self.unix()
        }
    }

    fn windows(self) -> Option<PathBuf> {
        match self {
            Known::Home => var("USERPROFILE"),
            Known::Temp => var("TEMP"),
            Known::Cache | Known::Data | Known::LocalAppData => var("LOCALAPPDATA"),
            Known::Config | Known::RoamingAppData => var("APPDATA"),
            Known::Downloads => var("USERPROFILE").map(|h| h.join("Downloads")),
            Known::ProgramData => var("PROGRAMDATA"),
            Known::ProgramFiles => var("PROGRAMFILES"),
            Known::ProgramFilesX86 => var("PROGRAMFILES(X86)"),
            Known::Windows => var("WINDIR"),
        }
    }

    fn unix(self) -> Option<PathBuf> {
        let home = var("HOME");
        let mac = cfg!(target_os = "macos");
        match self {
            Known::Home => home,
            Known::Temp => Some(std::env::temp_dir()),
            Known::Cache if mac => home.map(|h| h.join("Library/Caches")),
            Known::Cache => var("XDG_CACHE_HOME").or_else(|| home.map(|h| h.join(".cache"))),
            Known::Config | Known::Data if mac => {
                home.map(|h| h.join("Library/Application Support"))
            }
            Known::Config => var("XDG_CONFIG_HOME").or_else(|| home.map(|h| h.join(".config"))),
            Known::Data => var("XDG_DATA_HOME").or_else(|| home.map(|h| h.join(".local/share"))),
            Known::Downloads => home.map(|h| h.join("Downloads")),
            _ => None,
        }
    }
}

/// An environment variable as a path, if set and not empty. XDG variables must be absolute;
/// a relative value is ignored, as the specification says.
fn var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| !name.starts_with("XDG_") || p.is_absolute())
}
