//! Web requests, through the system's own `curl` (Windows 10 1803 and later, macOS and nearly
//! every Linux system ship it), so the app carries no TLS stack of its own.

use crate::{PlatformError, Result, process};

/// A program from System32 on Windows (so a copy next to the app or on the PATH is never run
/// instead), or from the PATH elsewhere.
pub(crate) fn system_program(name: &str) -> String {
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        return std::path::Path::new(&root)
            .join("System32")
            .join(format!("{name}.exe"))
            .display()
            .to_string();
    }
    name.to_owned()
}

/// The system's curl. On Windows, the copy in System32, so a `curl.exe` next to the app or on
/// the PATH is never run instead.
fn curl() -> String {
    system_program("curl")
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

/// Downloads `url` into the file `dest`, with the same limits as [`get_text`] but up to ten
/// minutes. A failed download leaves no file behind.
pub fn download(url: &str, dest: &std::path::Path, user_agent: &str, max_bytes: u64) -> Result<()> {
    if !url.starts_with("https://") {
        return Err(PlatformError::Unsupported("web addresses other than https"));
    }
    let max = max_bytes.to_string();
    let out = dest.display().to_string();
    let program = curl();
    let result = process::run_checked(
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
            "600",
            "--max-filesize",
            &max,
            "--user-agent",
            user_agent,
            "--output",
            &out,
            url,
        ],
    );
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result.map(|_| ())
}

/// Unpacks a `.zip` or `.tar.gz` archive into the folder `into` with the system's `tar`
/// (bsdtar on Windows 10 1803 and later, GNU tar or bsdtar elsewhere), which refuses entries that
/// would land outside it.
pub fn unpack(archive: &std::path::Path, into: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(into).map_err(|source| PlatformError::Io {
        path: into.to_path_buf(),
        source,
    })?;
    let archive = archive.display().to_string();
    let into = into.display().to_string();
    let program = system_program("tar");
    process::run_checked(&program, &["-x", "-f", &archive, "-C", &into]).map(|_| ())
}
