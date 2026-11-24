//! One request at a time with administrator rights, on Linux and macOS. The app itself never
//! runs as root (a root window cannot open on Wayland, and it would write root-owned files in
//! the home folder); instead the command-line tool next to it runs the single request, after the
//! system asks for the password (`pkexec` on Linux, the authorization dialog on macOS).
//!
//! Only a short list of requests may run this way, and only ones that work on system folders,
//! so running as root never points them at root's own home.

use crate::{EngineError, Request, Result};
use rewind_os_platform::{Os, PlatformError, process};
use serde_json::Value;
use std::path::PathBuf;

/// The requests that may run with administrator rights.
pub const ALLOWED: &[&str] = &[
    "cleanup.run",
    "maintenance.journal_usage",
    "maintenance.journal_vacuum",
];

/// English reason; translate with `rewind_os_i18n::tr`.
pub const NOT_ALLOWED: &str = "This action cannot be run with administrator rights from here.";
pub const NO_HELPER: &str = "The command-line tool (rewind-os-cli) was not found next to the app.";

fn helper() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let cli = exe.parent()?.join("rewind-os-cli");
    cli.is_file().then_some(cli)
}

/// Whether this computer can ask for the password for one action: the command-line tool is
/// next to the app, and on Linux `pkexec` is installed.
pub fn available() -> bool {
    let prompt = match Os::current() {
        Os::Linux => std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|dir| dir.join("pkexec").is_file())),
        Os::MacOs => true,
        Os::Windows => false,
    };
    prompt && helper().is_some()
}

/// Text inside an AppleScript string literal.
pub fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The AppleScript that runs `cli --json run <request>` with administrator rights. Both the
/// program and the request are passed through `quoted form of`, so the shell sees each as one
/// word whatever it contains.
pub fn applescript(cli: &str, request_json: &str) -> String {
    format!(
        "do shell script (quoted form of {}) & \" --json run \" & (quoted form of {}) with administrator privileges",
        applescript_string(cli),
        applescript_string(request_json)
    )
}

/// Runs `request` with administrator rights and returns the engine's response as JSON (the
/// same shape `rewind-os-cli --json` prints).
pub fn run(request: &Request) -> Result<Value> {
    if !ALLOWED.contains(&request.id()) {
        return Err(EngineError::Refused(NOT_ALLOWED));
    }
    let json = serde_json::to_string(request).map_err(|e| EngineError::Io {
        path: PathBuf::from("request"),
        source: std::io::Error::other(e),
    })?;
    let cli = helper().ok_or(EngineError::Refused(NO_HELPER))?;
    let cli = cli.to_string_lossy().into_owned();
    let done = match Os::current() {
        Os::Linux => process::run("pkexec", &[&cli, "--json", "run", &json])?,
        Os::MacOs => process::run("osascript", &["-e", &applescript(&cli, &json)])?,
        Os::Windows => return Err(PlatformError::Unsupported("run one request elevated").into()),
    };
    if !done.success() {
        // pkexec: 126 = the password prompt was dismissed, 127 = not authorized. osascript
        // reports a cancelled dialog as error -128.
        if matches!(done.code, Some(126) | Some(127)) || done.stderr.contains("-128") {
            return Err(EngineError::NeedsAdmin);
        }
        return Err(PlatformError::Command {
            program: cli,
            code: done.code,
            detail: done.stderr.trim().to_owned(),
        }
        .into());
    }
    serde_json::from_str(done.stdout.trim()).map_err(|e| EngineError::Io {
        path: PathBuf::from(cli),
        source: std::io::Error::other(e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_listed_requests_may_run_elevated() {
        let refused = run(&Request::FilesTrash {
            paths: vec![PathBuf::from("/")],
        });
        assert!(matches!(refused, Err(EngineError::Refused(NOT_ALLOWED))));
    }

    #[test]
    fn applescript_keeps_quotes_and_backslashes_inside_the_string() {
        let script = applescript(
            "/Applications/Rewind OS.app/Contents/MacOS/rewind-os-cli",
            r#"{"tool":"cleanup.run","categories":["a\"b"]}"#,
        );
        assert!(script.contains(r#"\"tool\""#));
        assert!(script.contains(r#"a\\\"b"#));
        assert!(script.ends_with("with administrator privileges"));
        assert_eq!(script.matches("quoted form of").count(), 2);
    }
}
