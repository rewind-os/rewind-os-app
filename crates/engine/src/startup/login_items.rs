//! macOS: login items, the apps "Open at Login" in System Settings › General › Login Items.
//! They are read and changed through System Events, which macOS lets the app control only after
//! the person allows it (Privacy & Security › Automation); without that the page says why they
//! are not shown instead of listing none.
//!
//! macOS has no "off" for a login item: it is either on the list or not. Turning one off
//! remembers it first (`~/Library/Application Support/rewind-os/login-items-off.json`) and then
//! takes it off the list, so it can be put back here as it was. The app itself is never touched.

use super::{Source, StartupEntry};
use crate::{EngineError, Result};
use rewind_os_platform::paths::Known;
use rewind_os_platform::process;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// English reason; translate with `rewind_os_i18n::tr`.
pub const NOT_ALLOWED: &str = "Login items are not shown: macOS does not let Rewind OS control System Events. Allow it in System Settings › Privacy & Security › Automation.";

/// English reason; translate with `rewind_os_i18n::tr`.
pub const UNREADABLE: &str = "Login items are not shown: System Events did not answer.";

/// English reason; translate with `rewind_os_i18n::tr`.
pub const APP_GONE: &str =
    "The app of this login item is no longer there, so it cannot be put back.";

/// A login item as System Events gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    /// The app or file it opens.
    pub path: String,
    /// Opens hidden.
    #[serde(default)]
    pub hidden: bool,
}

/// Every login item, as one JSON list, with three bulk reads instead of one per item.
const LIST: &str = r#"function run() {
  var items = Application("System Events").loginItems;
  var names = items.name(), paths = items.path(), hidden = items.hidden();
  return JSON.stringify(names.map(function (n, i) {
    return { name: n, path: paths[i] || "", hidden: hidden[i] === true };
  }));
}"#;

/// Takes the login item named by the first argument off the list.
const REMOVE: &str = r#"on run argv
  tell application "System Events" to delete login item (item 1 of argv)
end run"#;

/// Puts back a login item: the path, then "true" if it opens hidden.
const ADD: &str = r#"on run argv
  tell application "System Events" to make login item at end with properties {path:(item 1 of argv), hidden:((item 2 of argv) is "true")}
end run"#;

/// Reads the JSON list from [`LIST`]. Items without a name are left out.
pub fn parse(json: &str) -> Option<Vec<Item>> {
    let items: Vec<Item> = serde_json::from_str(json.trim()).ok()?;
    Some(
        items
            .into_iter()
            .filter(|i| !i.name.trim().is_empty())
            .collect(),
    )
}

/// Whether osascript failed because macOS did not allow controlling System Events
/// (`errAEEventNotPermitted`, -1743).
pub fn is_not_allowed(stderr: &str) -> bool {
    stderr.contains("-1743") || stderr.contains("Not authorized to send Apple events")
}

/// The entries: the items on the list (on) and the ones the app took off (off). An item that
/// is back on the list counts once, as on.
pub fn entries(on_list: &[Item], taken_off: &[Item]) -> Vec<StartupEntry> {
    let entry = |item: &Item, enabled: bool| StartupEntry {
        id: format!("{}|{}", Source::LoginItem.tag(), item.name),
        name: item.name.clone(),
        command: item.path.clone(),
        source: Source::LoginItem,
        enabled,
        needs_admin: false,
        essential: super::is_essential(&item.name),
        when: None,
    };
    let mut out: Vec<StartupEntry> = on_list.iter().map(|i| entry(i, true)).collect();
    for item in taken_off {
        if !on_list.iter().any(|i| i.name == item.name) {
            out.push(entry(item, false));
        }
    }
    out
}

fn osascript(args: &[&str]) -> std::result::Result<String, &'static str> {
    let done = process::run("osascript", args).map_err(|_| UNREADABLE)?;
    if done.success() {
        Ok(done.stdout)
    } else if is_not_allowed(&done.stderr) {
        Err(NOT_ALLOWED)
    } else {
        Err(UNREADABLE)
    }
}

fn read_list() -> std::result::Result<Vec<Item>, &'static str> {
    let json = osascript(&["-l", "JavaScript", "-e", LIST])?;
    parse(&json).ok_or(UNREADABLE)
}

fn record_path() -> Option<PathBuf> {
    Known::Data
        .path()
        .map(|p| p.join("rewind-os").join("login-items-off.json"))
}

fn taken_off() -> Vec<Item> {
    record_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_taken_off(items: &[Item]) -> Result<()> {
    let path =
        record_path().ok_or_else(|| EngineError::NotFound("Application Support".to_owned()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| EngineError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    let text = serde_json::to_string_pretty(items).map_err(|e| EngineError::Io {
        path: path.clone(),
        source: std::io::Error::other(e),
    })?;
    std::fs::write(&path, text).map_err(|source| EngineError::Io { path, source })
}

/// The login items, or why they could not be read.
pub fn list() -> std::result::Result<Vec<StartupEntry>, &'static str> {
    Ok(entries(&read_list()?, &taken_off()))
}

/// Takes the login item with this name off the list (remembering it first), or puts back one
/// the app took off.
pub fn set_enabled(name: &str, enabled: bool) -> Result<()> {
    let on_list = read_list().map_err(EngineError::Refused)?;
    let mut record = taken_off();
    if enabled {
        if on_list.iter().any(|i| i.name == name) {
            record.retain(|i| i.name != name);
            return save_taken_off(&record);
        }
        let item = record
            .iter()
            .find(|i| i.name == name)
            .cloned()
            .ok_or_else(|| EngineError::NotFound(name.to_owned()))?;
        if !std::path::Path::new(&item.path).exists() {
            return Err(EngineError::Refused(APP_GONE));
        }
        let hidden = if item.hidden { "true" } else { "false" };
        osascript(&["-e", ADD, &item.path, hidden]).map_err(EngineError::Refused)?;
        record.retain(|i| i.name != name);
        return save_taken_off(&record);
    }
    // Only items on the list, so a name can never reach anything else.
    let item = on_list
        .iter()
        .find(|i| i.name == name)
        .cloned()
        .ok_or_else(|| EngineError::NotFound(name.to_owned()))?;
    if super::is_essential(&item.name) {
        return Err(EngineError::Refused(super::ESSENTIAL));
    }
    // Remember first: an item the app cannot find again could not be put back here.
    record.retain(|i| i.name != name);
    record.push(item);
    save_taken_off(&record)?;
    if let Err(reason) = osascript(&["-e", REMOVE, name]) {
        record.retain(|i| i.name != name);
        let _ = save_taken_off(&record);
        return Err(EngineError::Refused(reason));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str) -> Item {
        Item {
            name: name.to_owned(),
            path: format!("/Applications/{name}.app"),
            hidden: false,
        }
    }

    #[test]
    fn the_list_is_read_and_nameless_items_are_left_out() {
        let items = parse(
            r#"[{"name":"Dropbox","path":"/Applications/Dropbox.app","hidden":true},{"name":" ","path":"/x"}]"#,
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert!(items.iter().all(|i| i.hidden && i.name == "Dropbox"));
        assert!(parse("execution error").is_none());
    }

    #[test]
    fn an_item_taken_off_shows_as_off_until_it_is_back() {
        let off = [item("Spotify")];
        let listed = entries(&[item("Dropbox")], &off);
        assert!(listed.iter().any(|e| e.name == "Spotify" && !e.enabled));
        assert!(listed.iter().any(|e| e.name == "Dropbox" && e.enabled));
        // Put back from System Settings: shown once, as on.
        let back = entries(&[item("Spotify")], &off);
        assert_eq!(back.len(), 1);
        assert!(back.iter().all(|e| e.enabled));
    }

    #[test]
    fn a_refused_permission_is_told_apart() {
        assert!(is_not_allowed(
            "execution error: Not authorized to send Apple events to System Events. (-1743)"
        ));
        assert!(!is_not_allowed(
            "execution error: Can't get login item \"x\". (-1728)"
        ));
    }
}
