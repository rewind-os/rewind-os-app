//! Scheduled cleaning: the system's own scheduler cleans, every week or every four weeks, the
//! categories that are selected by default, marked safe and need no administrator rights. It is
//! off until turned on, runs as the person who turned it on, and keeps a record of each run.
//!
//! - Windows: a task `\Rewind OS\Cleanup` (Task Scheduler) that starts the app with
//!   `--scheduled-cleanup`, so no console window opens; a run missed while the computer was off
//!   runs when it is next on.
//! - Linux: a systemd timer of the user (`rewind-os-cleanup.timer`, `Persistent=true`).
//! - macOS: a LaunchAgent with a calendar interval, which launchd runs after a missed time too.

use crate::cleanup::{self, Category, Risk};
use crate::history::APP_FOLDER;
use crate::task::Task;
use crate::{EngineError, Result};
use chrono::{DateTime, Utc};
use rewind_os_platform::paths::Known;
use rewind_os_platform::{Os, process};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The argument that makes the desktop app run a scheduled cleanup and exit, with no window.
pub const APP_FLAG: &str = "--scheduled-cleanup";
/// Runs kept in the record.
const KEPT: usize = 20;

const TASK_PATH: &str = r"\Rewind OS\";
const TASK_NAME: &str = "Cleanup";
const UNIT: &str = "rewind-os-cleanup";
const LABEL: &str = "io.github.rewind_os.rewind_os_app.cleanup";

/// English reasons; translate with `rewind_os_i18n::tr`.
pub const NO_PROGRAM: &str =
    "Rewind OS could not find its own program next to this one, so it cannot be scheduled.";
pub const NO_SYSTEMD: &str =
    "This system has no systemd for your user, so Rewind OS cannot schedule a cleanup.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frequency {
    Weekly,
    /// Every four weeks: what every scheduler can do in the same way.
    FourWeeks,
}

impl Frequency {
    pub fn label(self) -> &'static str {
        match self {
            Frequency::Weekly => "Every week",
            Frequency::FourWeeks => "Every four weeks",
        }
    }

    fn weeks(self) -> u32 {
        match self {
            Frequency::Weekly => 1,
            Frequency::FourWeeks => 4,
        }
    }
}

/// One scheduled cleanup, as it went.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub at: DateTime<Utc>,
    pub files: u64,
    pub bytes: u64,
    /// Files that could not be removed (a program had them open).
    pub failed: u64,
    /// Categories not cleaned, with why (English).
    pub skipped: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// How often it runs; `None` when it is off.
    pub frequency: Option<Frequency>,
    /// The categories a run cleans on this system (ids).
    pub categories: Vec<&'static str>,
    /// Earlier runs, newest first.
    pub runs: Vec<Run>,
}

/// The categories a scheduled run cleans: selected by default, safe, and no rights needed.
pub fn chosen(list: &'static [Category]) -> Vec<&'static Category> {
    list.iter()
        .filter(|c| c.selected_by_default && c.risk == Risk::Safe && !c.needs_admin)
        .collect()
}

/// The program the scheduler starts, and its arguments.
fn command() -> Result<(PathBuf, Vec<String>)> {
    let here = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .ok_or(EngineError::Refused(NO_PROGRAM))?;
    // In a Flatpak the program's path is the sandbox's; the host starts it through flatpak.
    if let Ok(id) = std::env::var("FLATPAK_ID") {
        return Ok((
            PathBuf::from("/usr/bin/flatpak"),
            vec![
                "run".into(),
                "--command=rewind-os-cli".into(),
                id,
                "schedule".into(),
                "run".into(),
                "--yes".into(),
            ],
        ));
    }
    let (file, args): (&str, Vec<String>) = match Os::current() {
        Os::Windows => ("rewind-os.exe", vec![APP_FLAG.into()]),
        _ => (
            "rewind-os-cli",
            vec!["schedule".into(), "run".into(), "--yes".into()],
        ),
    };
    let program = here.join(file);
    if !program.is_file() {
        return Err(EngineError::Refused(NO_PROGRAM));
    }
    Ok((program, args))
}

/// A word for PowerShell between single quotes.
fn ps_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The PowerShell that registers the Windows task: as the current user, only while signed in,
/// on battery too, and late when the computer was off at the time.
pub fn windows_script(program: &Path, args: &[String], frequency: Frequency) -> String {
    format!(
        "$a = New-ScheduledTaskAction -Execute {program} -Argument {args}; \
         $t = New-ScheduledTaskTrigger -Weekly -WeeksInterval {weeks} -DaysOfWeek Sunday -At 12:00; \
         $s = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 1); \
         Register-ScheduledTask -TaskPath {path} -TaskName {name} -Action $a -Trigger $t -Settings $s -Description {about} -Force | Out-Null",
        program = ps_quote(&program.to_string_lossy()),
        args = ps_quote(&args.join(" ")),
        weeks = frequency.weeks(),
        path = ps_quote(TASK_PATH),
        name = ps_quote(TASK_NAME),
        about = ps_quote("Rewind OS: scheduled cleanup of the safe categories"),
    )
}

/// The frequency of the task in `schtasks /Query /XML` (the same in every language); `None`
/// when it is turned off in Task Scheduler.
pub fn parse_windows_task(xml: &str) -> Option<Frequency> {
    let disabled = xml
        .split("<Settings>")
        .nth(1)
        .is_some_and(|s| s.contains("<Enabled>false</Enabled>"));
    if disabled {
        return None;
    }
    let weeks = xml
        .split("<WeeksInterval>")
        .nth(1)
        .and_then(|s| s.split('<').next())
        .and_then(|n| n.trim().parse::<u32>().ok())
        .unwrap_or(1);
    Some(if weeks >= 4 {
        Frequency::FourWeeks
    } else {
        Frequency::Weekly
    })
}

/// A word for a systemd command line.
fn unit_quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The systemd user service and timer.
pub fn systemd_units(program: &Path, args: &[String], frequency: Frequency) -> (String, String) {
    let mut line = vec![unit_quote(&program.to_string_lossy())];
    line.extend(args.iter().map(|a| unit_quote(a)));
    let service = format!(
        "[Unit]\nDescription=Rewind OS: scheduled cleanup of the safe categories\n\n\
         [Service]\nType=oneshot\nExecStart={}\nNice=10\nIOSchedulingClass=idle\n",
        line.join(" ")
    );
    let calendar = match frequency {
        Frequency::Weekly => "Sun 12:00",
        // systemd has no "every four weeks"; the 1st of the month is the nearest.
        Frequency::FourWeeks => "*-*-01 12:00",
    };
    let timer = format!(
        "[Unit]\nDescription=Rewind OS: scheduled cleanup ({})\n\n\
         [Timer]\nOnCalendar={calendar}\nPersistent=true\nRandomizedDelaySec=30min\n\n\
         [Install]\nWantedBy=timers.target\n",
        frequency.label()
    );
    (service, timer)
}

/// The frequency written in a timer file.
pub fn parse_timer(timer: &str) -> Option<Frequency> {
    let calendar = timer
        .lines()
        .find_map(|l| l.trim().strip_prefix("OnCalendar="))?;
    Some(if calendar.contains("-01") {
        Frequency::FourWeeks
    } else {
        Frequency::Weekly
    })
}

/// The LaunchAgent's property list.
pub fn launch_agent(program: &Path, args: &[String], frequency: Frequency) -> String {
    let escape = |t: &str| {
        t.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let mut arguments = format!(
        "\t\t<string>{}</string>\n",
        escape(&program.to_string_lossy())
    );
    for a in args {
        arguments.push_str(&format!("\t\t<string>{}</string>\n", escape(a)));
    }
    // launchd has no "every four weeks" either; the 1st of the month is the nearest.
    let when = match frequency {
        Frequency::Weekly => "\t\t<key>Weekday</key>\n\t\t<integer>0</integer>\n",
        Frequency::FourWeeks => "\t\t<key>Day</key>\n\t\t<integer>1</integer>\n",
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>Label</key>\n\t<string>{LABEL}</string>\n\
         \t<key>ProgramArguments</key>\n\t<array>\n{arguments}\t</array>\n\
         \t<key>StartCalendarInterval</key>\n\t<dict>\n{when}\
         \t\t<key>Hour</key>\n\t\t<integer>12</integer>\n\
         \t\t<key>Minute</key>\n\t\t<integer>0</integer>\n\t</dict>\n\
         \t<key>ProcessType</key>\n\t<string>Background</string>\n\
         </dict>\n</plist>\n"
    )
}

pub fn parse_launch_agent(plist: &str) -> Option<Frequency> {
    plist.contains(LABEL).then(|| {
        if plist.contains("<key>Day</key>") {
            Frequency::FourWeeks
        } else {
            Frequency::Weekly
        }
    })
}

fn io_error(path: &Path, source: std::io::Error) -> EngineError {
    EngineError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    std::fs::write(path, contents).map_err(|e| io_error(path, e))
}

fn create_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|e| io_error(path, e))
}

fn remove(path: &Path) -> Result<()> {
    std::fs::remove_file(path).map_err(|e| io_error(path, e))
}

fn user_units() -> Option<PathBuf> {
    Known::Config.path().map(|c| c.join("systemd/user"))
}

fn agent_file() -> Option<PathBuf> {
    Known::Home.path().map(|h| {
        h.join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist"))
    })
}

fn record_file() -> Option<PathBuf> {
    Known::Data
        .path()
        .map(|d| d.join(APP_FOLDER).join("scheduled-cleanups.json"))
}

fn runs() -> Vec<Run> {
    record_file()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn current() -> Option<Frequency> {
    match Os::current() {
        Os::Windows => {
            let name = format!("{TASK_PATH}{TASK_NAME}");
            let done = process::run("schtasks", &["/Query", "/TN", &name, "/XML"]).ok()?;
            done.success()
                .then(|| parse_windows_task(&done.stdout))
                .flatten()
        }
        Os::Linux => {
            let timer =
                std::fs::read_to_string(user_units()?.join(format!("{UNIT}.timer"))).ok()?;
            let enabled = process::run(
                "systemctl",
                &["--user", "is-enabled", &format!("{UNIT}.timer")],
            )
            .is_ok_and(|d| d.stdout.trim() == "enabled");
            enabled.then(|| parse_timer(&timer)).flatten()
        }
        Os::MacOs => parse_launch_agent(&std::fs::read_to_string(agent_file()?).ok()?),
    }
}

/// Whether it is on, how often, what it cleans and how its last runs went.
pub fn status() -> Result<Status> {
    Ok(Status {
        frequency: current(),
        categories: chosen(cleanup::categories()).iter().map(|c| c.id).collect(),
        runs: runs(),
    })
}

/// Turns the scheduled cleanup on (`Some`) at that frequency, or off (`None`).
pub fn set(frequency: Option<Frequency>) -> Result<()> {
    match (Os::current(), frequency) {
        (Os::Windows, Some(f)) => {
            let (program, args) = command()?;
            let script = windows_script(&program, &args, f);
            process::run_checked(
                "powershell",
                &["-NoProfile", "-NonInteractive", "-Command", &script],
            )?;
        }
        (Os::Windows, None) => {
            let name = format!("{TASK_PATH}{TASK_NAME}");
            let done = process::run("schtasks", &["/Delete", "/TN", &name, "/F"])?;
            // Already gone is off too.
            if !done.success() && current().is_some() {
                return Err(EngineError::Refused(NO_PROGRAM));
            }
        }
        (Os::Linux, Some(f)) => {
            let dir = user_units().ok_or(EngineError::Refused(NO_SYSTEMD))?;
            if process::run("systemctl", &["--user", "--version"]).map_or(true, |d| !d.success()) {
                return Err(EngineError::Refused(NO_SYSTEMD));
            }
            let (program, args) = command()?;
            let (service, timer) = systemd_units(&program, &args, f);
            create_dir(&dir)?;
            write(&dir.join(format!("{UNIT}.service")), service)?;
            write(&dir.join(format!("{UNIT}.timer")), timer)?;
            process::run_checked("systemctl", &["--user", "daemon-reload"])?;
            process::run_checked(
                "systemctl",
                &["--user", "enable", "--now", &format!("{UNIT}.timer")],
            )?;
        }
        (Os::Linux, None) => {
            let _ = process::run(
                "systemctl",
                &["--user", "disable", "--now", &format!("{UNIT}.timer")],
            );
            if let Some(dir) = user_units() {
                for kind in ["timer", "service"] {
                    let file = dir.join(format!("{UNIT}.{kind}"));
                    if file.is_file() {
                        remove(&file)?;
                    }
                }
            }
            let _ = process::run("systemctl", &["--user", "daemon-reload"]);
        }
        (Os::MacOs, f) => {
            let file = agent_file().ok_or(EngineError::Refused(NO_PROGRAM))?;
            let domain = crate::startup::launchd::domain()?;
            // Unloaded first, so turning it on again picks up the new time.
            let _ = process::run("launchctl", &["bootout", &format!("{domain}/{LABEL}")]);
            match f {
                Some(f) => {
                    let (program, args) = command()?;
                    if let Some(dir) = file.parent() {
                        create_dir(dir)?;
                    }
                    write(&file, launch_agent(&program, &args, f))?;
                    process::run_checked(
                        "launchctl",
                        &["bootstrap", &domain, &file.to_string_lossy()],
                    )?;
                }
                None => {
                    if file.is_file() {
                        remove(&file)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Cleans the chosen categories now, as the scheduler does, and adds the run to the record.
pub fn run(task: &Task) -> Result<Run> {
    let ids: Vec<String> = chosen(cleanup::categories())
        .iter()
        .map(|c| c.id.to_owned())
        .collect();
    let results = cleanup::run(&ids, task)?;
    let run = Run {
        at: Utc::now(),
        files: results.iter().map(|r| r.files).sum(),
        bytes: results.iter().map(|r| r.bytes).sum(),
        failed: results.iter().map(|r| r.failed).sum(),
        skipped: results
            .iter()
            .filter_map(|r| {
                r.skipped
                    .map(str::to_owned)
                    .or_else(|| r.error.clone())
                    .map(|why| (r.id.to_owned(), why))
            })
            .collect(),
    };
    if let Some(file) = record_file() {
        let mut all = runs();
        all.insert(0, run.clone());
        all.truncate(KEPT);
        if let Some(dir) = file.parent() {
            create_dir(dir)?;
        }
        write(&file, serde_json::to_vec_pretty(&all).unwrap_or_default())?;
    }
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_safe_preselected_categories_without_rights_are_scheduled() {
        for list in [cleanup::WINDOWS, cleanup::LINUX, cleanup::MACOS] {
            let chosen = chosen(list);
            assert!(!chosen.is_empty());
            for c in &chosen {
                assert!(c.selected_by_default && c.risk == Risk::Safe && !c.needs_admin);
            }
            // The recycle bin is never emptied on a schedule.
            assert!(chosen.iter().all(|c| c.id != "personal.recycle_bin"));
        }
    }

    #[test]
    fn each_scheduler_reads_back_what_it_was_given() {
        let program = Path::new("/opt/Rewind O'S/rewind-os-cli");
        let args = vec!["schedule".to_owned(), "run".to_owned(), "--yes".to_owned()];
        for f in [Frequency::Weekly, Frequency::FourWeeks] {
            let (service, timer) = systemd_units(program, &args, f);
            assert_eq!(parse_timer(&timer), Some(f));
            assert!(service.contains("ExecStart=\"/opt/Rewind O'S/rewind-os-cli\" \"schedule\""));
            assert_eq!(
                parse_launch_agent(&launch_agent(program, &args, f)),
                Some(f)
            );
            let script = windows_script(program, &args, f);
            // A quote in the path is doubled inside PowerShell's single quotes.
            assert!(script.contains("'/opt/Rewind O''S/rewind-os-cli'"));
            assert!(script.contains(&format!("-WeeksInterval {}", f.weeks())));
        }
        assert_eq!(parse_launch_agent("<plist>someone else's</plist>"), None);
    }

    #[test]
    fn a_task_turned_off_in_task_scheduler_counts_as_off() {
        let on = "<Task><Triggers><CalendarTrigger><ScheduleByWeek><WeeksInterval>4</WeeksInterval></ScheduleByWeek></CalendarTrigger></Triggers><Settings><Enabled>true</Enabled></Settings></Task>";
        assert_eq!(parse_windows_task(on), Some(Frequency::FourWeeks));
        let off = on.replace("<Settings><Enabled>true", "<Settings><Enabled>false");
        assert_eq!(parse_windows_task(&off), None);
    }
}
