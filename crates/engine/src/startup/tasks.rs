//! Windows scheduled tasks that programs add to start themselves (updaters, helpers, tray
//! programs). Windows' own tasks (`\Microsoft\...`) are never listed, so they cannot be turned
//! off here. Turning a task off uses Windows' own switch (`schtasks /Change /DISABLE`): the task
//! stays, and turning it back on restores it exactly.

use super::{Source, StartupEntry};
use crate::{EngineError, Result};
use rewind_os_platform::process;
use serde::Deserialize;

/// Lists the tasks outside `\Microsoft\` as JSON, one object per task, in UTF-8.
const LIST_SCRIPT: &str = r#"[Console]::OutputEncoding = [Text.Encoding]::UTF8
$tasks = @(Get-ScheduledTask | Where-Object { $_.TaskPath -notlike '\Microsoft\*' } | ForEach-Object {
  [pscustomobject]@{
    path = $_.TaskPath + $_.TaskName
    name = $_.TaskName
    state = [string]$_.State
    user = [string]$_.Principal.UserId
    triggers = @($_.Triggers | ForEach-Object { $_.CimClass.CimClassName })
    command = (@($_.Actions | ForEach-Object { ([string]$_.Execute + ' ' + [string]$_.Arguments).Trim() }) -join ' & ')
  }
})
ConvertTo-Json -InputObject $tasks -Depth 3 -Compress"#;

#[derive(Debug, Deserialize)]
struct RawTask {
    path: String,
    name: String,
    state: String,
    #[serde(default)]
    user: String,
    /// Some triggers have no class name (`null`).
    #[serde(default)]
    triggers: Vec<Option<String>>,
    #[serde(default)]
    command: String,
}

/// When a task runs, from its triggers. English; translate with `rewind_os_i18n::tr`.
fn when(triggers: &[Option<String>]) -> &'static str {
    let has = |class: &str| {
        triggers
            .iter()
            .flatten()
            .any(|t| t.eq_ignore_ascii_case(class))
    };
    if has("MSFT_TaskLogonTrigger") || has("MSFT_TaskSessionStateChangeTrigger") {
        "Scheduled task: at sign-in"
    } else if has("MSFT_TaskBootTrigger") {
        "Scheduled task: at startup"
    } else if has("MSFT_TaskIdleTrigger") {
        "Scheduled task: when idle"
    } else if has("MSFT_TaskEventTrigger") {
        "Scheduled task: on an event"
    } else {
        "Scheduled task: on a schedule"
    }
}

/// The task name without the machine-made suffixes many programs add: `_{GUID}`, `-{GUID}` and the user's
/// security id (`-S-1-5-21-...`).
pub fn display_name(name: &str) -> String {
    let mut name = name.trim();
    if name.ends_with('}')
        && let Some(start) = name.rfind("_{").or_else(|| name.rfind("-{"))
    {
        name = name.get(..start).unwrap_or(name);
    }
    if let Some(start) = name.find("-S-1-5-") {
        name = name.get(..start).unwrap_or(name);
    }
    let name = name.trim();
    if name.is_empty() {
        "?".to_owned()
    } else {
        name.to_owned()
    }
}

/// Turns the script's output into entries. Tasks of `\Microsoft\` are dropped even if listed.
pub fn parse(json: &str, current_user: &str) -> Vec<StartupEntry> {
    let raw: Vec<RawTask> = serde_json::from_str(json.trim()).unwrap_or_default();
    raw.into_iter()
        .filter(|t| !t.path.to_ascii_lowercase().starts_with(r"\microsoft\"))
        .map(|t| {
            let user = t.user.rsplit('\\').next().unwrap_or("");
            StartupEntry {
                id: format!("task|{}", t.path),
                essential: super::is_essential(&t.name),
                name: display_name(&t.name),
                command: t.command,
                source: Source::ScheduledTask,
                enabled: !t.state.eq_ignore_ascii_case("disabled"),
                // Tasks that run as the person signed in are theirs to change.
                needs_admin: user.is_empty() || !user.eq_ignore_ascii_case(current_user),
                when: Some(when(&t.triggers)),
            }
        })
        .collect()
}

pub(super) fn list() -> Vec<StartupEntry> {
    let Ok(done) = process::run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", LIST_SCRIPT],
    ) else {
        return Vec::new();
    };
    if !done.success() {
        return Vec::new();
    }
    let user = std::env::var("USERNAME").unwrap_or_default();
    parse(&done.stdout, &user)
}

/// `path` is the task's full path (`\Vendor\Updater`).
pub(super) fn set_enabled(path: &str, enabled: bool) -> Result<()> {
    if path.to_ascii_lowercase().starts_with(r"\microsoft\") {
        return Err(EngineError::Refused(super::ESSENTIAL));
    }
    let name = path.rsplit('\\').next().unwrap_or(path);
    if !enabled && super::is_essential(name) {
        return Err(EngineError::Refused(super::ESSENTIAL));
    }
    let switch = if enabled { "/ENABLE" } else { "/DISABLE" };
    let done = process::run("schtasks", &["/Change", "/TN", path, switch])?;
    if done.success() {
        return Ok(());
    }
    // schtasks exits with 1 for "access denied" too; without rights, that is the likely cause.
    if !rewind_os_platform::elevation::is_elevated() {
        return Err(EngineError::NeedsAdmin);
    }
    Err(rewind_os_platform::PlatformError::Command {
        program: "schtasks".to_owned(),
        code: done.code,
        detail: if done.stderr.trim().is_empty() {
            done.stdout.trim().to_owned()
        } else {
            done.stderr.trim().to_owned()
        },
    }
    .into())
}
