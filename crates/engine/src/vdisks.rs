//! The virtual disks of WSL distributions and Docker Desktop (`.vhdx` files). They grow as Linux
//! writes to them and never shrink by themselves: deleting files inside frees space for Linux,
//! not for Windows. Compacting gives Windows that space back.
//!
//! Compacting, on request and with administrator rights:
//! 1. `fstrim` inside a WSL distribution, so the disk knows which blocks Linux freed;
//! 2. `wsl --shutdown`, which closes every Linux program (the disk must not be in use);
//! 3. `diskpart`: `attach vdisk readonly`, `compact vdisk`, `detach vdisk`.
//!
//! Nothing inside the disk is deleted. Docker Desktop keeps WSL running, so it must be closed
//! first. Only a disk [`list`] found is ever compacted.

use crate::{EngineError, Os, Result, Task, safety};
use rewind_os_platform::paths::Known;
use rewind_os_platform::registry::{self, Hive, Value};
use rewind_os_platform::{elevation, process};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LXSS: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";
/// Disks smaller than this are left out: nothing worth giving back.
const SMALLEST: u64 = 512 * 1024 * 1024;

/// English reasons; translate with `rewind_os_i18n::tr`.
pub const NOT_LISTED: &str = "This is not one of the virtual disks Rewind OS found.";
pub const QUIT_DOCKER: &str =
    "Quit Docker Desktop first: it keeps WSL running, and the disk must not be in use.";
pub const STILL_RUNNING: &str =
    "WSL did not stop, so the disk is still in use. Close the Linux programs and try again.";

/// Who a virtual disk belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    /// A WSL distribution (Debian, Ubuntu…).
    Wsl,
    /// Docker Desktop's images, containers and volumes.
    Docker,
}

#[derive(Debug, Clone, Serialize)]
pub struct VirtualDisk {
    /// The distribution's name, or "Docker Desktop".
    pub name: String,
    pub owner: Owner,
    pub path: PathBuf,
    /// What the file takes on Windows.
    pub bytes: u64,
    /// What Linux uses inside, when the distribution is running.
    pub used_inside: Option<u64>,
    pub running: bool,
}

impl VirtualDisk {
    /// Space compacting would give back, about, when what is used inside is known.
    pub fn reclaimable(&self) -> Option<u64> {
        self.used_inside.map(|used| self.bytes.saturating_sub(used))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Largest first.
    pub disks: Vec<VirtualDisk>,
    pub docker_running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Compacted {
    pub name: String,
    pub before: u64,
    pub after: u64,
    /// What fstrim and diskpart printed.
    pub output: String,
}

fn unsupported(tool: &'static str) -> EngineError {
    EngineError::Unsupported {
        tool,
        os: Os::current().name(),
    }
}

/// `wsl.exe` prints UTF-16 to a pipe: the names of `--list --quiet` with NULs between letters.
pub fn parse_names(output: &str) -> Vec<String> {
    output
        .replace('\0', "")
        .lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}').to_owned())
        .filter(|l| !l.is_empty())
        .collect()
}

/// `df -B1 --output=used /`: the number under the header.
pub fn parse_df_used(output: &str) -> Option<u64> {
    output.lines().nth(1)?.trim().parse().ok()
}

/// A registry path to a folder, without the `\\?\` prefix diskpart does not take.
pub fn plain_path(raw: &str) -> PathBuf {
    PathBuf::from(raw.strip_prefix(r"\\?\").unwrap_or(raw))
}

/// The diskpart script that compacts `path`; `None` for a path that could break out of its
/// quotes.
pub fn diskpart_script(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    if text.is_empty() || text.contains(['"', '\n', '\r']) {
        return None;
    }
    Some(format!(
        "select vdisk file=\"{text}\"\nattach vdisk readonly\ncompact vdisk\ndetach vdisk\n"
    ))
}

/// Whether two Windows paths name the same file.
fn same(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

fn wsl(args: &[&str]) -> Result<process::Finished> {
    Ok(process::run("wsl.exe", args)?)
}

fn running_distributions() -> Vec<String> {
    wsl(&["--list", "--running", "--quiet"])
        .ok()
        .filter(|done| done.success())
        .map(|done| parse_names(&done.stdout))
        .unwrap_or_default()
}

fn docker_running() -> bool {
    let Ok(mut sys) = sysinfo::System::new() else {
        return false;
    };
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes().values().any(|p| {
        let name = p.name().to_string_lossy().to_ascii_lowercase();
        name == "docker desktop.exe" || name == "com.docker.backend.exe"
    })
}

/// Every WSL distribution's disk and Docker Desktop's data disk, largest first.
pub fn list() -> Result<Report> {
    if Os::current() != Os::Windows {
        return Err(unsupported("vdisk.list"));
    }
    let running = running_distributions();
    let mut disks = Vec::new();
    for key in registry::subkeys(Hive::CurrentUser, LXSS) {
        let path = format!(r"{LXSS}\{key}");
        let text = |name: &str| {
            registry::value(Hive::CurrentUser, &path, name)
                .and_then(|v| v.as_text().map(str::to_owned))
        };
        // WSL 1 keeps files as files; only WSL 2 has a disk.
        if registry::value(Hive::CurrentUser, &path, "Version") != Some(Value::Number(2)) {
            continue;
        }
        let (Some(name), Some(base)) = (text("DistributionName"), text("BasePath")) else {
            continue;
        };
        let file =
            plain_path(&base).join(text("VhdFileName").unwrap_or_else(|| "ext4.vhdx".into()));
        let Ok(meta) = std::fs::metadata(&file) else {
            continue;
        };
        let is_running = running.iter().any(|r| r.eq_ignore_ascii_case(&name));
        let docker = name.starts_with("docker-desktop");
        // What Linux uses, read from a distribution that is already running (never started
        // just to measure it).
        let used_inside = (is_running && !docker)
            .then(|| wsl(&["-d", &name, "-e", "df", "-B1", "--output=used", "/"]).ok())
            .flatten()
            .filter(process::Finished::success)
            .and_then(|done| parse_df_used(&done.stdout));
        disks.push(VirtualDisk {
            name,
            owner: if docker { Owner::Docker } else { Owner::Wsl },
            path: file,
            bytes: meta.len(),
            used_inside,
            running: is_running,
        });
    }
    // Docker Desktop's images, containers and volumes, where it keeps them by default.
    if let Some(local) = Known::LocalAppData.path() {
        for file in [
            local.join(r"Docker\wsl\disk\docker_data.vhdx"),
            local.join(r"Docker\wsl\data\ext4.vhdx"),
        ] {
            if let Ok(meta) = std::fs::metadata(&file)
                && !disks.iter().any(|d| same(&d.path, &file))
            {
                disks.push(VirtualDisk {
                    name: "Docker Desktop".to_owned(),
                    owner: Owner::Docker,
                    path: file,
                    bytes: meta.len(),
                    used_inside: None,
                    running: false,
                });
            }
        }
    }
    disks.retain(|d| d.bytes >= SMALLEST);
    disks.sort_by_key(|d| std::cmp::Reverse(d.bytes));
    Ok(Report {
        disks,
        docker_running: docker_running(),
    })
}

/// Compacts the disk at `path`, which [`list`] must have found. Stops WSL.
pub fn compact(path: &Path, task: &Task) -> Result<Compacted> {
    if Os::current() != Os::Windows {
        return Err(unsupported("vdisk.compact"));
    }
    if !elevation::is_elevated() {
        return Err(EngineError::NeedsAdmin);
    }
    let report = list()?;
    let disk = report
        .disks
        .into_iter()
        .find(|d| same(&d.path, path))
        .ok_or(EngineError::Refused(NOT_LISTED))?;
    if report.docker_running {
        return Err(EngineError::Refused(QUIT_DOCKER));
    }
    let script = diskpart_script(&disk.path).ok_or(EngineError::Refused(NOT_LISTED))?;
    let mut output = String::new();

    if disk.owner == Owner::Wsl {
        task.set_stage("Telling the disk which space Linux freed");
        // Starts the distribution when it is not running; that is part of what was asked.
        if let Ok(done) = wsl(&["-d", &disk.name, "-u", "root", "-e", "fstrim", "-v", "/"]) {
            output.push_str(done.stdout.trim());
            output.push('\n');
        }
    }

    task.set_stage("Stopping WSL");
    wsl(&["--shutdown"])?;
    let started = Instant::now();
    while !running_distributions().is_empty() {
        if started.elapsed() > Duration::from_secs(20) {
            return Err(EngineError::Refused(STILL_RUNNING));
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    task.set_stage("Compacting the virtual disk");
    let file = std::env::temp_dir().join(format!("rewind-os-compact-{}.txt", std::process::id()));
    std::fs::write(&file, script).map_err(|source| EngineError::Io {
        path: file.clone(),
        source,
    })?;
    let program = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join(r"System32\diskpart.exe"))
        .unwrap_or_else(|| PathBuf::from("diskpart.exe"));
    let script_arg = file.to_string_lossy().into_owned();
    let done = process::run(&program.to_string_lossy(), &["/s", &script_arg]);
    // The script is ours, in the temporary folder; the guard still has the last word.
    if safety::removal_refusal(&file).is_none() {
        let _ = std::fs::remove_file(&file);
    }
    let done = done?;
    output.push_str(done.stdout.trim());
    if !done.success() {
        return Err(rewind_os_platform::PlatformError::Command {
            program: "diskpart".to_owned(),
            code: done.code,
            detail: output.trim().to_owned(),
        }
        .into());
    }
    let after = std::fs::metadata(&disk.path)
        .map(|m| m.len())
        .unwrap_or(disk.bytes);
    task.add(1, disk.bytes.saturating_sub(after));
    Ok(Compacted {
        name: disk.name,
        before: disk.bytes,
        after,
        output: output.trim().to_owned(),
    })
}
