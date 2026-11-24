//! Installed apps on macOS: the `.app` bundles in `/Applications` (and one folder below, such
//! as `Utilities`) and `~/Applications`. Uninstalling moves the bundle to the Trash, through the
//! safety guard; Apple's own apps are listed but never removed.

use crate::programs::{InstalledProgram, Scope};
use crate::{EngineError, Result, Task, walk};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use rewind_os_platform::paths::Known;
use rewind_os_platform::process;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// English reason; translate with `rewind_os_i18n::tr`.
pub const APPLE_APP: &str = "This app is part of macOS and is not removed.";

/// Reads what the app list needs from an `Info.plist` converted to JSON
/// (`plutil -convert json -o - Info.plist`).
pub fn from_info(bundle: &Path, info_json: &str, scope: Scope) -> Option<InstalledProgram> {
    let info: Value = serde_json::from_str(info_json).ok()?;
    let text = |key: &str| {
        info.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let file_name = bundle
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())?;
    let name = text("CFBundleDisplayName")
        .or_else(|| text("CFBundleName"))
        .unwrap_or(file_name);
    let id = text("CFBundleIdentifier");
    Some(InstalledProgram {
        key: format!("app|{}", bundle.display()),
        name,
        publisher: id.as_deref().and_then(publisher_of),
        version: text("CFBundleShortVersionString").or_else(|| text("CFBundleVersion")),
        install_location: Some(bundle.to_path_buf()),
        display_icon: None,
        uninstall_command: id,
        estimated_bytes: None,
        installed_on: None,
        scope,
        manager: Some("App"),
    })
}

/// `com.google.Chrome` -> `google`; Apple's apps say Apple.
fn publisher_of(bundle_id: &str) -> Option<String> {
    let mut parts = bundle_id.split('.');
    let first = parts.next()?;
    let second = parts.next()?;
    if matches!(first, "com" | "org" | "net" | "io" | "app" | "dev" | "co") {
        Some(if second == "apple" {
            "Apple".to_owned()
        } else {
            second.to_owned()
        })
    } else {
        None
    }
}

/// Whether a bundle id belongs to Apple.
pub fn is_apple(bundle_id: Option<&str>) -> bool {
    bundle_id.is_some_and(|id| id.starts_with("com.apple."))
}

fn bundles(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "app") {
            out.push(path);
        } else if depth > 0 {
            bundles(&path, depth - 1, out);
        }
    }
}

fn read(bundle: &Path, scope: Scope) -> Option<InstalledProgram> {
    let plist = bundle.join("Contents/Info.plist");
    let plist = plist.to_string_lossy();
    let done = process::run("plutil", &["-convert", "json", "-o", "-", &plist]).ok()?;
    if !done.success() {
        return None;
    }
    let mut program = from_info(bundle, &done.stdout, scope)?;
    program.estimated_bytes =
        Some(walk::measure(bundle, &walk::Filter::default(), &Task::new()).bytes);
    program.installed_on = std::fs::metadata(bundle)
        .and_then(|m| m.modified())
        .ok()
        .map(|t| DateTime::<Utc>::from(t).date_naive());
    Some(program)
}

pub fn installed() -> Vec<InstalledProgram> {
    let mut found: Vec<(PathBuf, Scope)> = Vec::new();
    let mut system = Vec::new();
    bundles(Path::new("/Applications"), 1, &mut system);
    found.extend(system.into_iter().map(|p| (p, Scope::Machine)));
    if let Some(home) = Known::Home.path() {
        let mut mine = Vec::new();
        bundles(&home.join("Applications"), 1, &mut mine);
        found.extend(mine.into_iter().map(|p| (p, Scope::User)));
    }
    let mut programs: Vec<InstalledProgram> = found
        .par_iter()
        .filter_map(|(path, scope)| read(path, *scope))
        .collect();
    programs.sort_by_key(|p| p.name.to_lowercase());
    programs
}

/// Moves the app with this key (`app|/Applications/X.app`) to the Trash.
pub fn uninstall(key: &str) -> Result<()> {
    let program = installed()
        .into_iter()
        .find(|p| p.key == key)
        .ok_or_else(|| EngineError::NotFound(key.to_owned()))?;
    if is_apple(program.uninstall_command.as_deref()) {
        return Err(EngineError::Refused(APPLE_APP));
    }
    let path = program
        .install_location
        .ok_or_else(|| EngineError::NotFound(key.to_owned()))?;
    if let Some(reason) = crate::safety::removal_refusal(&path) {
        return Err(EngineError::Refused(reason));
    }
    let outcome = rewind_os_platform::shell::move_to_trash(std::slice::from_ref(&path));
    match outcome.into_iter().next().and_then(|o| o.error) {
        None => Ok(()),
        Some(error) => Err(EngineError::Io {
            path,
            source: std::io::Error::other(error),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_info_plist_gives_name_version_and_maker() {
        let json = r#"{"CFBundleName":"Google Chrome","CFBundleShortVersionString":"120.0.6099.129","CFBundleIdentifier":"com.google.Chrome"}"#;
        let p = from_info(
            Path::new("/Applications/Google Chrome.app"),
            json,
            Scope::Machine,
        )
        .unwrap();
        assert_eq!(p.name, "Google Chrome");
        assert_eq!(p.version.as_deref(), Some("120.0.6099.129"));
        assert_eq!(p.publisher.as_deref(), Some("google"));
        assert!(p.key.ends_with("Google Chrome.app"));
    }

    #[test]
    fn the_bundle_name_is_used_when_the_plist_has_none() {
        let p = from_info(Path::new("/Applications/Tool.app"), "{}", Scope::User).unwrap();
        assert_eq!(p.name, "Tool");
        assert_eq!(p.scope, Scope::User);
    }

    #[test]
    fn apple_apps_are_recognised() {
        assert!(is_apple(Some("com.apple.Safari")));
        assert!(!is_apple(Some("com.google.Chrome")));
        assert!(!is_apple(None));
    }
}
