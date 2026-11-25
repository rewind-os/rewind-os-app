//! Web requests, through the system's own `curl` (Windows 10 1803 and later, macOS and nearly
//! every Linux system ship it), so the app carries no TLS stack of its own.

use crate::{PlatformError, Result, process};

/// The system's curl. On Windows, the copy in System32, so a `curl.exe` next to the app or on
/// the PATH is never run instead.
fn curl() -> String {
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        return std::path::Path::new(&root)
            .join("System32")
            .join("curl.exe")
            .display()
            .to_string();
    }
    "curl".to_owned()
}

/// Fetches `url` and returns the body as text. Only `https` addresses are fetched, redirects
/// only to `https`; the request gives up after 20 seconds and refuses bodies over `max_bytes`.
/// A server error (4xx, 5xx) is a [`PlatformError::Command`].
pub fn get_text(url: &str, user_agent: &str, max_bytes: u64) -> Result<String> {
    if !url.starts_with("https://") {
        return Err(PlatformError::Unsupported("web addresses other than https"));
    }
    let max = max_bytes.to_string();
    let program = curl();
    let finished = process::run_checked(
        &program,
        &[
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "20",
            "--max-filesize",
            &max,
            "--user-agent",
            user_agent,
            url,
        ],
    )?;
    Ok(finished.stdout)
}
