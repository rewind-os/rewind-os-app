//! Windows Installer packages.

use crate::{PlatformError, Result};
use std::path::Path;

/// Starts the Windows Installer on `msi` with its progress shown (`/passive`; Windows asks for
/// permission when the package installs for every user), without waiting for it. When it
/// succeeds, `then` is started: the app the package installed. The caller should exit at once,
/// so the installer can replace its files.
pub fn install_then_start(msi: &Path, then: Option<&Path>) -> Result<()> {
    if !cfg!(windows) {
        return Err(PlatformError::Unsupported("Windows Installer packages"));
    }
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    // msiexec reads its own command line: the package path goes in double quotes.
    let package = quote(&format!("\"{}\"", msi.display()));
    let mut script = format!(
        "$p = Start-Process -FilePath 'msiexec.exe' -ArgumentList '/i',{package},'/passive','/norestart' -Wait -PassThru"
    );
    if let Some(app) = then {
        // 3010: installed, a restart of Windows is pending.
        script.push_str(&format!(
            "; if ($p.ExitCode -eq 0 -or $p.ExitCode -eq 3010) {{ Start-Process -FilePath {} }}",
            quote(&app.display().to_string())
        ));
    }
    crate::process::command("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .spawn()
        .map(|_| ())
        .map_err(|source| PlatformError::Spawn {
            program: "powershell".to_owned(),
            source,
        })
}
