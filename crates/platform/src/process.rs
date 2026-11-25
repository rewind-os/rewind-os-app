//! Runs external programs without flashing a console window.

use crate::{PlatformError, Result};
use std::process::Command;

/// Output of a finished program.
#[derive(Debug, Clone)]
pub struct Finished {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Finished {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Whether this copy runs inside a Flatpak sandbox.
pub fn in_flatpak() -> bool {
    cfg!(target_os = "linux") && std::path::Path::new("/.flatpak-info").exists()
}

/// Builds a command that never opens a console window on Windows. Inside a Flatpak sandbox the
/// program runs on the host (`flatpak-spawn --host`), where the package managers, `pkexec` and
/// the system's tools are.
pub fn command(program: &str) -> Command {
    let cmd = if in_flatpak() {
        let mut cmd = Command::new("flatpak-spawn");
        cmd.args(["--host", program]);
        cmd
    } else {
        Command::new(program)
    };
    #[cfg(windows)]
    let cmd = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = cmd;
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    };
    cmd
}

/// Starts `program` with `args` and does not wait for it; it keeps running after this program
/// exits.
pub fn start(program: &std::path::Path, args: &[&str]) -> Result<()> {
    let name = program.display().to_string();
    command(&name)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|source| PlatformError::Spawn {
            program: name,
            source,
        })
}

/// Runs `program` with `args` and waits for it. A non-zero exit code is not an error here;
/// callers decide what it means.
pub fn run(program: &str, args: &[&str]) -> Result<Finished> {
    let output = command(program)
        .args(args)
        .output()
        .map_err(|source| PlatformError::Spawn {
            program: program.to_owned(),
            source,
        })?;
    Ok(Finished {
        code: output.status.code(),
        stdout: decode(&output.stdout),
        stderr: decode(&output.stderr),
    })
}

/// Runs `program` and turns a non-zero exit code into [`PlatformError::Command`].
pub fn run_checked(program: &str, args: &[&str]) -> Result<Finished> {
    let finished = run(program, args)?;
    if finished.success() {
        Ok(finished)
    } else {
        let detail = if finished.stderr.trim().is_empty() {
            finished.stdout.trim().to_owned()
        } else {
            finished.stderr.trim().to_owned()
        };
        Err(PlatformError::Command {
            program: program.to_owned(),
            code: finished.code,
            detail,
        })
    }
}

/// Console programs on Windows print in the OEM code page; keep whatever is valid text.
fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
