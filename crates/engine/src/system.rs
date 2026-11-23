//! Overview of the computer: drives, memory and the large files Windows keeps for itself.

use rewindows_platform::elevation;
use serde::Serialize;
use std::path::PathBuf;
use sysinfo::{DiskKind, Disks, System};

#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub os: String,
    pub host: String,
    pub cpu: String,
    pub cpu_threads: usize,
    pub memory_total: u64,
    pub memory_used: u64,
    pub uptime_secs: u64,
    pub elevated: bool,
    pub drives: Vec<Drive>,
    pub system_files: Vec<SystemFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveKind {
    Ssd,
    Hdd,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Drive {
    pub mount: PathBuf,
    pub label: String,
    pub file_system: String,
    pub kind: DriveKind,
    pub removable: bool,
    pub total: u64,
    pub available: u64,
}

impl Drive {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemFileKind {
    /// `hiberfil.sys`: memory image for hibernation and fast startup.
    Hibernation,
    /// `pagefile.sys`: virtual memory.
    PageFile,
    /// `swapfile.sys`: virtual memory for Store apps.
    SwapFile,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemFile {
    pub path: PathBuf,
    pub kind: SystemFileKind,
    pub bytes: u64,
}

pub fn overview() -> Overview {
    // Each part may fail on its own; show what could be read instead of failing the overview.
    let mut sys = System::new().ok();
    if let Some(sys) = sys.as_mut() {
        sys.refresh_memory();
        sys.refresh_cpu_all();
    }
    let cpus = sys.as_ref().map(|s| s.cpus()).unwrap_or_default();
    let cpu = cpus
        .first()
        .map(|c| c.brand().trim().to_owned())
        .unwrap_or_default();
    let os = System::long_os_version()
        .or_else(|_| System::name())
        .unwrap_or_else(|_| "Unknown".to_owned());
    let disks = Disks::new_with_refreshed_list().ok();
    let drives = disks
        .as_ref()
        .map(|d| &d[..])
        .unwrap_or_default()
        .iter()
        .map(|d| Drive {
            mount: d.mount_point().to_path_buf(),
            label: d.name().to_string_lossy().into_owned(),
            file_system: d.file_system().to_string_lossy().into_owned(),
            kind: match d.kind() {
                DiskKind::SSD => DriveKind::Ssd,
                DiskKind::HDD => DriveKind::Hdd,
                _ => DriveKind::Unknown,
            },
            removable: d.is_removable(),
            total: d.total_space(),
            available: d.available_space(),
        })
        .collect();
    Overview {
        os,
        host: System::host_name().unwrap_or_default(),
        cpu,
        cpu_threads: cpus.len(),
        memory_total: sys.as_ref().map(|s| s.total_memory()).unwrap_or(0),
        memory_used: sys.as_ref().map(|s| s.used_memory()).unwrap_or(0),
        uptime_secs: System::uptime().unwrap_or(0),
        elevated: elevation::is_elevated(),
        drives,
        system_files: system_files(),
    }
}

/// Reads the sizes from the root folder listing: these files are always open, so opening
/// them directly would fail.
fn system_files() -> Vec<SystemFile> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    let root = PathBuf::from(format!("{drive}\\"));
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let kind = match name.as_str() {
                "hiberfil.sys" => SystemFileKind::Hibernation,
                "pagefile.sys" => SystemFileKind::PageFile,
                "swapfile.sys" => SystemFileKind::SwapFile,
                _ => return None,
            };
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            Some(SystemFile {
                path: entry.path(),
                kind,
                bytes,
            })
        })
        .collect()
}

/// Whether the app runs with administrator rights.
pub fn is_elevated() -> bool {
    elevation::is_elevated()
}
