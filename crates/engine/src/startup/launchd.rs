//! macOS: LaunchAgents, the programs launchd starts when someone signs in. They are read from
//! `~/Library/LaunchAgents` (you) and `/Library/LaunchAgents` (all users); Apple's own
//! (`com.apple.*`, and everything in `/System`) are never listed.
//!
//! Turning one off uses launchd's own on/off record (`launchctl disable gui/<uid>/<label>`),
//! like "startup approved" on Windows: the agent's file is never changed or deleted, it can be
//! turned back on here or with `launchctl enable`, and it takes effect at the next sign-in.
//! Both folders load into the signed-in person's own domain, so no administrator rights are
//! needed.

use super::{Source, StartupEntry};
use crate::{EngineError, Result};
use rewind_os_platform::paths::Known;
use rewind_os_platform::process;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// English reason; translate with `rewind_os_i18n::tr`.
pub const APPLE_AGENT: &str =
    "This is part of macOS. Rewind OS only changes agents that programs installed.";

/// What a LaunchAgent's property list says.
#[derive(Debug, PartialEq, Eq)]
pub struct Agent {
    pub label: String,
    /// The program and its arguments, as one line.
    pub command: String,
    /// The file itself says `Disabled = true`.
    pub disabled_in_file: bool,
}

/// Reads an agent from its property list converted to JSON (`plutil -convert json -o -`).
pub fn parse_agent(json: &str) -> Option<Agent> {
    let plist: Value = serde_json::from_str(json).ok()?;
    let label = plist
        .get("Label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_owned();
    let arguments: Vec<&str> = plist
        .get("ProgramArguments")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let command = match plist.get("Program").and_then(Value::as_str) {
        Some(program) if arguments.len() > 1 => {
            let rest: Vec<&str> = arguments.iter().skip(1).copied().collect();
            format!("{program} {}", rest.join(" "))
        }
        Some(program) => program.to_owned(),
        None => arguments.join(" "),
    };
    Some(Agent {
        label,
        command,
        disabled_in_file: plist.get("Disabled").and_then(Value::as_bool) == Some(true),
    })
}

/// launchd's on/off record from `launchctl print-disabled gui/<uid>`: label to "is disabled".
/// Newer systems write `=> disabled` / `=> enabled`, older ones `=> true` / `=> false`.
pub fn parse_disabled(text: &str) -> HashMap<String, bool> {
    text.lines()
        .filter_map(|line| {
            let (label, state) = line.split_once("=>")?;
            let label = label.trim().trim_matches('"');
            let disabled = match state.trim().trim_end_matches(';') {
                "disabled" | "true" => true,
                "enabled" | "false" => false,
                _ => return None,
            };
            (!label.is_empty()).then(|| (label.to_owned(), disabled))
        })
        .collect()
}

/// Whether an agent starts: launchd's record wins over the file's own `Disabled` key.
pub fn starts(agent: &Agent, record: &HashMap<String, bool>) -> bool {
    match record.get(&agent.label) {
        Some(disabled) => !disabled,
        None => !agent.disabled_in_file,
    }
}

/// Apple's agents are part of macOS.
pub fn is_apple(label: &str) -> bool {
    label.to_ascii_lowercase().starts_with("com.apple.")
}

fn folders() -> Vec<(PathBuf, Source)> {
    let mut out = Vec::new();
    if let Some(home) = Known::Home.path() {
        out.push((home.join("Library/LaunchAgents"), Source::UserLaunchAgent));
    }
    out.push((PathBuf::from("/Library/LaunchAgents"), Source::LaunchAgent));
    out
}

fn read_agent(path: &Path) -> Option<Agent> {
    let path = path.to_string_lossy();
    let done = process::run("plutil", &["-convert", "json", "-o", "-", &path]).ok()?;
    if !done.success() {
        return None;
    }
    parse_agent(&done.stdout)
}

fn domain() -> Result<String> {
    let uid = process::run_checked("id", &["-u"])?;
    Ok(format!("gui/{}", uid.stdout.trim()))
}

fn record(domain: &str) -> HashMap<String, bool> {
    process::run("launchctl", &["print-disabled", domain])
        .map(|done| parse_disabled(&done.stdout))
        .unwrap_or_default()
}

/// Every agent programs installed, in both folders.
pub fn list() -> Vec<StartupEntry> {
    let record = domain().map(|d| record(&d)).unwrap_or_default();
    let mut entries = Vec::new();
    for (folder, source) in folders() {
        let Ok(files) = std::fs::read_dir(&folder) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            let is_plist = path.extension().is_some_and(|e| e == "plist");
            // Links are not followed, as everywhere else.
            let is_file = file.file_type().is_ok_and(|t| t.is_file());
            if !is_plist || !is_file {
                continue;
            }
            let Some(agent) = read_agent(&path) else {
                continue;
            };
            if is_apple(&agent.label) {
                continue;
            }
            entries.push(StartupEntry {
                id: format!("{}|{}", source.tag(), agent.label),
                name: agent.label.clone(),
                enabled: starts(&agent, &record),
                essential: super::is_essential(&agent.label),
                command: agent.command,
                source,
                needs_admin: false,
                when: None,
            });
        }
    }
    entries.sort_by_key(|e| e.name.to_lowercase());
    entries
}

/// Turns the agent with this label on or off in launchd's record.
pub fn set_enabled(label: &str, enabled: bool) -> Result<()> {
    if is_apple(label) {
        return Err(EngineError::Refused(APPLE_AGENT));
    }
    // Only agents this page lists, so an id can never reach anything else.
    let entry = list()
        .into_iter()
        .find(|e| e.name == label)
        .ok_or_else(|| EngineError::NotFound(label.to_owned()))?;
    if !enabled && entry.essential {
        return Err(EngineError::Refused(super::ESSENTIAL));
    }
    if entry.enabled == enabled {
        return Ok(());
    }
    let target = format!("{}/{label}", domain()?);
    let verb = if enabled { "enable" } else { "disable" };
    process::run_checked("launchctl", &[verb, &target])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_gives_its_label_and_command() {
        let json = r#"{"Label":"com.vendor.updater","ProgramArguments":["/Applications/Vendor.app/Contents/MacOS/updater","--background"],"RunAtLoad":true}"#;
        let agent = parse_agent(json).unwrap();
        assert_eq!(agent.label, "com.vendor.updater");
        assert!(agent.command.ends_with("updater --background"));
        assert!(!agent.disabled_in_file);
        assert!(parse_agent(r#"{"ProgramArguments":["/x"]}"#).is_none());
    }

    #[test]
    fn launchd_record_wins_over_the_file() {
        let record = parse_disabled(
            "disabled services = {\n\t\"com.vendor.a\" => disabled\n\t\"com.vendor.b\" => enabled\n\t\"com.vendor.c\" => true\n}\n",
        );
        let agent = |label: &str, disabled_in_file| Agent {
            label: label.to_owned(),
            command: String::new(),
            disabled_in_file,
        };
        assert!(!starts(&agent("com.vendor.a", false), &record));
        assert!(starts(&agent("com.vendor.b", true), &record));
        assert!(!starts(&agent("com.vendor.c", false), &record));
        // Not in the record: the file decides.
        assert!(!starts(&agent("com.vendor.d", true), &record));
        assert!(starts(&agent("com.vendor.e", false), &record));
    }

    #[test]
    fn apple_agents_are_recognised_whatever_the_case() {
        assert!(is_apple("com.apple.Safari.History"));
        assert!(is_apple("COM.APPLE.x"));
        assert!(!is_apple("com.applecart.sync"));
    }
}
