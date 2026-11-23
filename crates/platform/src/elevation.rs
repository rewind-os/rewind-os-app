//! Whether the process runs with administrator rights, and relaunching it with them.

use crate::{PlatformError, Result, process};
use std::sync::OnceLock;

/// True when the current process is elevated. Asked once and cached.
pub fn is_elevated() -> bool {
    static ELEVATED: OnceLock<bool> = OnceLock::new();
    *ELEVATED.get_or_init(detect)
}

#[cfg(windows)]
fn detect() -> bool {
    /// Mandatory integrity level SIDs that mean the token is elevated (high or system).
    const ELEVATED_LEVELS: [&str; 2] = ["S-1-16-12288", "S-1-16-16384"];
    // `whoami /groups` lists the token's integrity level; this needs no unsafe Win32 calls.
    match process::run("whoami", &["/groups", "/fo", "csv", "/nh"]) {
        Ok(out) => ELEVATED_LEVELS.iter().any(|sid| out.stdout.contains(sid)),
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn detect() -> bool {
    process::run("id", &["-u"])
        .map(|out| out.stdout.trim() == "0")
        .unwrap_or(false)
}

/// Starts a new elevated copy of this program (Windows shows the consent prompt). The caller
/// should exit afterwards. Fails if the user declines the prompt.
pub fn relaunch_elevated(args: &[String]) -> Result<()> {
    if !cfg!(windows) {
        return Err(PlatformError::Unsupported("relaunch as administrator"));
    }
    let exe = std::env::current_exe().map_err(|source| PlatformError::Io {
        path: "current executable".into(),
        source,
    })?;
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let mut script = format!(
        "Start-Process -FilePath {} -Verb RunAs",
        quote(&exe.to_string_lossy())
    );
    if !args.is_empty() {
        let list: Vec<String> = args.iter().map(|a| quote(a)).collect();
        script.push_str(&format!(" -ArgumentList {}", list.join(",")));
    }
    process::run_checked(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )
    .map(|_| ())
}
