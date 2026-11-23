//! Programs that start with the system: the Run keys and Startup folders on Windows, XDG
//! autostart entries on Linux. Turning one off never deletes it; it can be turned back on here
//! or from the system's own settings.

mod windows;
pub mod xdg;

use crate::{EngineError, Result};
use rewind_os_platform::Os;
use serde::Serialize;

pub use windows::{approved_bytes, is_enabled};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    UserRun,
    MachineRun,
    MachineRun32,
    UserStartupFolder,
    CommonStartupFolder,
    /// Linux: `~/.config/autostart`.
    UserAutostart,
    /// Linux: `/etc/xdg/autostart` and the other `$XDG_CONFIG_DIRS`.
    SystemAutostart,
}

impl Source {
    /// The prefix of entry ids from this source.
    fn tag(self) -> &'static str {
        match self {
            Source::UserRun => "user_run",
            Source::MachineRun => "machine_run",
            Source::MachineRun32 => "machine_run32",
            Source::UserStartupFolder => "user_folder",
            Source::CommonStartupFolder => "common_folder",
            Source::UserAutostart | Source::SystemAutostart => "xdg",
        }
    }

    /// English label; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Source::UserRun => "Registry (you)",
            Source::MachineRun => "Registry (all users)",
            Source::MachineRun32 => "Registry (all users, 32-bit)",
            Source::UserStartupFolder => "Startup folder (you)",
            Source::CommonStartupFolder => "Startup folder (all users)",
            Source::UserAutostart => "Autostart (you)",
            Source::SystemAutostart => "Autostart (all users)",
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
    /// The system needs it (security, sign-in, accessibility, sound); it cannot be turned off.
    pub essential: bool,
}

/// English reason shown when an essential entry is not turned off.
pub const ESSENTIAL: &str =
    "The system needs this to work properly (security, sign-in, accessibility or sound).";

/// Entries that keep security, sign-in, accessibility or the desktop session working. Matched
/// on the start of the entry's value name (Windows) or file name (Linux), ignoring case.
const ESSENTIAL_PREFIXES: &[&str] = &[
    // Windows
    "securityhealth",
    "windowsdefender",
    "windows defender",
    "msascuil",
    // Linux desktop sessions
    "at-spi",
    "gnome-keyring",
    "polkit",
    "lxpolkit",
    "org.gnome.settingsdaemon",
    "gsd-",
    "pipewire",
    "pulseaudio",
    "wireplumber",
    "org.kde.plasmashell",
    "kaccess",
    "xembedsniproxy",
    "mate-settings-daemon",
    "cinnamon-settings-daemon",
    "xfsettingsd",
    "xfce4-notifyd",
];

/// Whether the entry with this value or file name is essential.
pub fn is_essential(name: &str) -> bool {
    let name = name.to_lowercase();
    ESSENTIAL_PREFIXES.iter().any(|p| name.starts_with(p))
}

pub fn list() -> Vec<StartupEntry> {
    match Os::current() {
        Os::Windows => windows::list(),
        Os::Linux => xdg::Dirs::current().map(|d| d.list()).unwrap_or_default(),
        Os::MacOs => Vec::new(),
    }
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<()> {
    match Os::current() {
        Os::Windows => windows::set_enabled(id, enabled),
        Os::Linux => xdg::Dirs::current()
            .ok_or_else(|| EngineError::NotFound("autostart folder".to_owned()))?
            .set_enabled(id, enabled),
        Os::MacOs => Err(EngineError::Unsupported {
            tool: "startup.set",
            os: Os::MacOs.name(),
        }),
    }
}
