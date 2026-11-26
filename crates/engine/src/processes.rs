//! What is running now and how much of the processor and memory each program takes, so the
//! programs that slow the computer down can be closed (or kept from starting with the system).
//!
//! Processes are grouped by program (a browser's many processes count as one). Closing asks the
//! program to close as its window would; forcing it ends it at once. The system's own programs,
//! other accounts' programs and Rewind OS itself are never closed (`safety::process_refusal`).

use crate::{Result, Task, safety, startup};
use rewind_os_platform::process;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// How long the processor is watched to see what each program uses.
const SAMPLE: Duration = Duration::from_millis(1000);
/// How long a closed program is given to end.
const CLOSE_WAIT: Duration = Duration::from_secs(5);

/// One program and all its processes.
#[derive(Debug, Clone, Serialize)]
pub struct Program {
    /// The file name without `.exe` (`chrome`, `firefox`).
    pub name: String,
    pub exe: Option<PathBuf>,
    pub pids: Vec<u32>,
    /// Share of the whole processor, 0 to 100.
    pub cpu: f32,
    /// Memory in use, in bytes.
    pub memory: u64,
    /// English reason it is not closed; translate with `rewind_os_i18n::tr`.
    pub refusal: Option<&'static str>,
    /// The startup entry that starts it with the system, when the app can turn it off.
    pub startup: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Heaviest on the processor first.
    pub programs: Vec<Program>,
    /// Share of the whole processor in use, 0 to 100.
    pub cpu: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub processes: usize,
}

/// What closing a program did.
#[derive(Debug, Clone, Serialize)]
pub struct Closed {
    pub name: String,
    pub closed: usize,
    /// Processes still running after the wait: the program may be asking to save, or it only
    /// ends when forced.
    pub running: usize,
    /// English reason nothing was closed; translate with `rewind_os_i18n::tr`.
    pub refusal: Option<&'static str>,
}

/// This process and the ones it runs inside (the terminal or launcher that started it):
/// closing one of them would close Rewind OS too.
fn holders(sys: &System, me: Option<Pid>) -> Vec<Pid> {
    let mut chain = Vec::new();
    let mut next = me;
    while let Some(pid) = next {
        if chain.contains(&pid) || chain.len() > 64 {
            break;
        }
        chain.push(pid);
        next = sys.process(pid).and_then(|p| p.parent());
    }
    chain
}

/// Waits `time`, stopping early when the task is cancelled.
fn wait(time: Duration, task: &Task) -> Result<()> {
    let started = Instant::now();
    while started.elapsed() < time {
        if task.is_cancelled() {
            return Err(crate::EngineError::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(50).min(time));
    }
    Ok(())
}

fn system() -> Result<System> {
    System::new().map_err(|_| {
        rewind_os_platform::PlatformError::Unsupported("reading the running programs").into()
    })
}

fn refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_user(UpdateKind::OnlyIfNotSet)
}

/// The name a process is shown and matched by: its file name without `.exe`.
fn program_name(name: &str) -> String {
    let name = name.trim();
    name.strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name)
        .to_owned()
}

/// The file name of each word of a command line, lowercase (`"C:\Program Files\x.exe" /b`
/// gives `x.exe` among others).
fn command_files(command: &str) -> Vec<String> {
    command
        .split(|c: char| c == '"' || c.is_whitespace())
        .filter_map(|word| word.rsplit(['/', '\\']).next())
        .filter(|file| !file.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The startup entry that starts the program with file name `file`.
pub fn startup_entry<'a>(
    file: &str,
    entries: &'a [startup::StartupEntry],
) -> Option<&'a startup::StartupEntry> {
    let file = file.to_lowercase();
    entries
        .iter()
        .find(|e| e.enabled && !e.essential && command_files(&e.command).contains(&file))
}

/// Measures what every program uses for about a second.
pub fn list(task: &Task) -> Result<Report> {
    task.set_stage("Measuring what each program uses");
    let mut sys = system()?;
    sys.refresh_cpu_usage();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
    // On Windows a process seen for the first time is measured against everything since the
    // computer started, which makes every share tiny: take one short reading first, then the
    // one that counts.
    wait(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL, task)?;
    sys.refresh_cpu_usage();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
    wait(SAMPLE, task)?;
    sys.refresh_cpu_usage();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
    sys.refresh_memory();

    let me = sysinfo::get_current_pid().ok();
    let my_user = me
        .and_then(|pid| sys.process(pid))
        .and_then(|p| p.user_id())
        .cloned();
    let holders = holders(&sys, me);
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1) as f32;
    let entries = if crate::tool("startup.list").is_some_and(|t| t.available()) {
        startup::list()
    } else {
        Vec::new()
    };

    let mut groups: HashMap<(String, bool, bool), Program> = HashMap::new();
    for (pid, p) in sys.processes() {
        // Linux lists threads and the kernel's own workers as processes; skip both.
        if p.thread_kind().is_some() {
            continue;
        }
        let file = p
            .exe()
            .and_then(Path::file_name)
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.name().to_string_lossy().into_owned());
        if file.is_empty() {
            continue;
        }
        let owned = my_user.is_some() && p.user_id() == my_user.as_ref();
        let ours = holders.contains(pid);
        let key = (
            p.exe()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| file.to_lowercase()),
            owned,
            ours,
        );
        let program = groups.entry(key).or_insert_with(|| Program {
            name: program_name(&file),
            exe: p.exe().map(Path::to_path_buf),
            pids: Vec::new(),
            cpu: 0.0,
            memory: 0,
            refusal: safety::process_refusal(&file)
                .or(ours.then_some(if Some(*pid) == me {
                    safety::THIS_APP
                } else {
                    safety::HOLDS_APP
                }))
                .or((!owned).then_some(safety::OTHER_ACCOUNT)),
            startup: startup_entry(&file, &entries).map(|e| e.id.clone()),
        });
        program.pids.push(pid.as_u32());
        program.cpu += p.cpu_usage() / cores;
        program.memory += p.memory();
    }
    let processes = groups.values().map(|g| g.pids.len()).sum();
    let mut programs: Vec<Program> = groups.into_values().collect();
    programs.sort_by(|a, b| {
        b.cpu
            .total_cmp(&a.cpu)
            .then(b.memory.cmp(&a.memory))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(Report {
        programs,
        cpu: sys.global_cpu_usage().clamp(0.0, 100.0),
        memory_used: sys.used_memory(),
        memory_total: sys.total_memory(),
        processes,
    })
}

/// Closes the processes `pids` of the program `name` (as `list` named it). A process whose pid
/// now belongs to another program is left alone, and so is everything the safety guard
/// refuses.
pub fn close(name: &str, pids: &[u32], force: bool, task: &Task) -> Result<Closed> {
    task.set_stage("Closing");
    let mut sys = system()?;
    let wanted: Vec<Pid> = pids.iter().map(|p| Pid::from_u32(*p)).collect();
    let kind = ProcessRefreshKind::nothing()
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_user(UpdateKind::OnlyIfNotSet);
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    let me = sysinfo::get_current_pid().ok();
    let my_user = me
        .and_then(|pid| sys.process(pid))
        .and_then(|p| p.user_id())
        .cloned();
    let holders = holders(&sys, me);
    let file_of = |p: &sysinfo::Process| {
        p.exe()
            .and_then(Path::file_name)
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.name().to_string_lossy().into_owned())
    };
    let mut targets = Vec::new();
    let mut refusal = None;
    for pid in &wanted {
        let Some(p) = sys.process(*pid) else {
            continue;
        };
        let file = file_of(p);
        if program_name(&file) != name {
            continue;
        }
        let why = safety::process_refusal(&file)
            .or(holders.contains(pid).then_some(safety::HOLDS_APP))
            .or((my_user.is_none() || p.user_id() != my_user.as_ref())
                .then_some(safety::OTHER_ACCOUNT));
        match why {
            Some(why) => refusal = Some(why),
            None => targets.push(*pid),
        }
    }
    if targets.is_empty() {
        return Ok(Closed {
            name: name.to_owned(),
            closed: 0,
            running: 0,
            refusal,
        });
    }
    for pid in &targets {
        // Whether it really ended is checked below; a process that already ended is fine.
        let _ = process::close(pid.as_u32(), force);
    }
    let started = Instant::now();
    let mut running = targets.len();
    loop {
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&targets), true, kind);
        running = targets
            .iter()
            .filter(|pid| {
                // A process that ended but was not collected by its parent yet is gone.
                sys.process(**pid).is_some_and(|p| {
                    p.status() != sysinfo::ProcessStatus::Zombie
                        && program_name(&file_of(p)) == name
                })
            })
            .count()
            .min(running);
        if running == 0 || started.elapsed() >= CLOSE_WAIT || task.is_cancelled() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Ok(Closed {
        name: name.to_owned(),
        closed: targets.len() - running,
        running,
        refusal: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_files_finds_the_program_in_quotes_with_spaces() {
        let files =
            command_files(r#""C:\Program Files\Microsoft OneDrive\OneDrive.exe" /background"#);
        assert!(files.contains(&"onedrive.exe".to_owned()));
        assert!(command_files("/usr/bin/slack --startup").contains(&"slack".to_owned()));
    }
}
