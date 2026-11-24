//! Linux: XDG autostart entries (`.desktop` files). A file in the user's folder overrides the
//! system file with the same name, so turning off a system entry writes a user copy with
//! `Hidden=true`; nothing outside the user's folder is ever changed.

use super::{Source, StartupEntry};
use crate::{EngineError, Result};
use rewind_os_platform::paths::Known;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Where autostart entries live.
#[derive(Debug, Clone)]
pub struct Dirs {
    /// `$XDG_CONFIG_HOME/autostart`, where changes are written.
    pub user: PathBuf,
    /// `$XDG_CONFIG_DIRS/*/autostart`, most important first. Read only.
    pub system: Vec<PathBuf>,
}

impl Dirs {
    pub fn current() -> Option<Self> {
        let user = Known::Config.path()?.join("autostart");
        let system = std::env::var("XDG_CONFIG_DIRS")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/etc/xdg".to_owned())
            .split(':')
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|p| p.join("autostart"))
            .collect();
        Some(Self { user, system })
    }

    fn system_file(&self, name: &str) -> Option<PathBuf> {
        self.system
            .iter()
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    }

    pub fn list(&self) -> Vec<StartupEntry> {
        let mut names = BTreeSet::new();
        for dir in std::iter::once(&self.user).chain(&self.system) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if valid_name(&name) {
                        names.insert(name);
                    }
                }
            }
        }
        let mut list: Vec<StartupEntry> = names
            .into_iter()
            .filter_map(|file| {
                let user = self.user.join(&file);
                let system = self.system_file(&file);
                let effective = if user.is_file() {
                    user
                } else {
                    system.clone()?
                };
                let text = std::fs::read_to_string(&effective).ok()?;
                let entry = parse(&text);
                let stem = file.trim_end_matches(".desktop").to_owned();
                Some(StartupEntry {
                    id: format!("{}|{file}", Source::UserAutostart.tag()),
                    name: entry.get("Name").unwrap_or(stem),
                    command: entry.get("Exec").unwrap_or_default(),
                    source: if system.is_some() {
                        Source::SystemAutostart
                    } else {
                        Source::UserAutostart
                    },
                    enabled: entry.enabled(),
                    needs_admin: false,
                    essential: super::is_essential(&file),
                    when: None,
                })
            })
            .collect();
        list.sort_by_key(|e| e.name.to_lowercase());
        list
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let file = id
            .strip_prefix("xdg|")
            .filter(|f| valid_name(f))
            .ok_or_else(|| EngineError::NotFound(id.to_owned()))?;
        if !enabled && super::is_essential(file) {
            return Err(EngineError::Refused(super::ESSENTIAL));
        }
        let user = self.user.join(file);
        let source = if user.is_file() {
            user.clone()
        } else {
            self.system_file(file)
                .ok_or_else(|| EngineError::NotFound(id.to_owned()))?
        };
        let text = read(&source)?;
        if parse(&text).enabled() == enabled {
            return Ok(());
        }
        let mut updated = set_key(&text, "Hidden", if enabled { "false" } else { "true" });
        if enabled {
            updated = set_key(&updated, "X-GNOME-Autostart-enabled", "true");
        }
        std::fs::create_dir_all(&self.user).map_err(|source| EngineError::Io {
            path: self.user.clone(),
            source,
        })?;
        std::fs::write(&user, updated).map_err(|source| EngineError::Io { path: user, source })
    }
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|source| EngineError::Io {
        path: path.into(),
        source,
    })
}

/// A plain `.desktop` file name: no folders, so an id can never point outside the folder.
fn valid_name(name: &str) -> bool {
    name.ends_with(".desktop")
        && name.len() > ".desktop".len()
        && !name.contains(['/', '\\'])
        && !name.starts_with('.')
}

struct DesktopEntry {
    keys: Vec<(String, String)>,
}

impl DesktopEntry {
    fn get(&self, key: &str) -> Option<String> {
        self.keys
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    fn flag(&self, key: &str) -> Option<bool> {
        self.get(key).map(|v| v.trim().eq_ignore_ascii_case("true"))
    }

    fn enabled(&self) -> bool {
        self.flag("Hidden") != Some(true) && self.flag("X-GNOME-Autostart-enabled") != Some(false)
    }
}

/// The unlocalised keys of the `[Desktop Entry]` group.
fn parse(text: &str) -> DesktopEntry {
    let mut in_entry = false;
    let mut keys = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            if !k.contains('[') && !keys.iter().any(|(seen, _)| seen == k) {
                keys.push((k.to_owned(), v.trim().to_owned()));
            }
        }
    }
    DesktopEntry { keys }
}

/// `text` with `key=value` in its `[Desktop Entry]` group, replacing the key if it is there.
fn set_key(text: &str, key: &str, value: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_entry = false;
    let mut header = None;
    let mut done = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_entry = trimmed == "[Desktop Entry]";
            if in_entry {
                header = Some(out.len());
            }
        } else if in_entry
            && !done
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key)
        {
            out.push(format!("{key}={value}"));
            done = true;
            continue;
        }
        out.push(line.to_owned());
    }
    if !done {
        match header {
            Some(at) => out.insert(at + 1, format!("{key}={value}")),
            None => {
                out.insert(0, format!("{key}={value}"));
                out.insert(0, "[Desktop Entry]".to_owned());
            }
        }
    }
    out.join("\n") + "\n"
}
