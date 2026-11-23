//! Windows' own maintenance tools, run on the user's request. Their output is passed through
//! unchanged so nothing is hidden.

use crate::{EngineError, Result};
use rewind_os_platform::{elevation, process};

fn require_admin() -> Result<()> {
    if elevation::is_elevated() {
        Ok(())
    } else {
        Err(EngineError::NeedsAdmin)
    }
}

fn dism(args: &[&str]) -> Result<String> {
    require_admin()?;
    let mut full = vec!["/Online", "/Cleanup-Image"];
    full.extend_from_slice(args);
    let out = process::run_checked("dism.exe", &full)?;
    Ok(out.stdout)
}

/// Real size of the component store (WinSxS), which looks larger in a disk scan because of
/// hard links.
pub fn analyze_component_store() -> Result<String> {
    dism(&["/AnalyzeComponentStore"])
}

/// Removes superseded components. Takes several minutes.
pub fn component_cleanup() -> Result<String> {
    dism(&["/StartComponentCleanup"])
}

pub fn set_hibernation(enabled: bool) -> Result<()> {
    require_admin()?;
    process::run_checked(
        "powercfg.exe",
        &["/hibernate", if enabled { "on" } else { "off" }],
    )?;
    Ok(())
}

/// How much space the systemd journal takes (`journalctl --disk-usage`).
pub fn journal_usage() -> Result<String> {
    let out = process::run_checked("journalctl", &["--disk-usage"])?;
    Ok(out.stdout)
}

/// Deletes journal entries older than two weeks. Needs root.
pub fn journal_vacuum() -> Result<String> {
    require_admin()?;
    let out = process::run_checked("journalctl", &["--vacuum-time=2weeks"])?;
    // journalctl reports what it removed on stderr.
    Ok(format!("{}{}", out.stdout, out.stderr))
}
