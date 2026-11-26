//! Whether a newer version of Rewind OS is out.
//!
//! The app asks GitHub for the latest published release each time it starts, and keeps the
//! answer in its own data folder (`rewind-os/update.json`), so a start without a connection still
//! knows about a version it saw before. The request carries the app's name and version and
//! nothing about the computer. The check at start can be turned off; a check asked for by hand
//! still works then.
//!
//! Installing a newer version (`update.install`) trusts only what the project signed: the
//! release's `SHA256SUMS.txt` must carry a minisign signature by the key in
//! `packaging/minisign.pub`, built into the app, whose trusted comment names that version, and
//! the downloaded package must have the checksum listed there. The MSI is handed to the Windows
//! Installer; a portable copy has its programs replaced in place (the old ones are kept as
//! `.old` until the next start, and put back if the swap fails). Copies a package manager or
//! Flatpak installed are left to them.

use crate::history::APP_FOLDER;
use crate::{EngineError, Os, Task};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// The version of this build.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// The latest published release (drafts and pre-releases are left out by GitHub).
const LATEST_URL: &str = "https://api.github.com/repos/rewind-os/rewind-os-app/releases/latest";
/// Release pages the app opens; an answer pointing anywhere else is not trusted.
pub const RELEASES_PAGE: &str = "https://github.com/rewind-os/rewind-os-app/releases/";
/// Larger answers are refused (the real one is a few tens of kilobytes).
const MAX_ANSWER_BYTES: u64 = 4 * 1024 * 1024;
/// Where a release's files are downloaded from.
const DOWNLOADS: &str = "https://github.com/rewind-os/rewind-os-app/releases/download/";
/// Larger packages are refused (the real ones are about 10 MB).
const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;
/// The key that signs every release's checksums.
const RELEASE_KEY: &str = include_str!("../../../packaging/minisign.pub");

/// English reasons; translate with `rewind_os_i18n::tr`.
pub const FROM_PACKAGE_MANAGER: &str =
    "This copy was installed with the system's package manager; update it from there.";
pub const FROM_FLATPAK: &str = "This copy is a Flatpak; update it with Flatpak.";
pub const NOT_ON_MAC: &str =
    "Updating from the app is not available on macOS yet; download the new version from its page.";
pub const UNKNOWN_PLACE: &str =
    "Rewind OS cannot tell how this copy was installed; download the new version from its page.";
pub const NO_PACKAGE: &str =
    "There is no package of the new version for this system; download it from its page.";
pub const BAD_SIGNATURE: &str =
    "The new version's checksums are not signed by the Rewind OS project; nothing was installed.";
pub const OTHER_VERSION: &str =
    "The signed checksums belong to another version; nothing was installed.";
pub const NOT_LISTED: &str = "The package is not in the signed checksums; nothing was installed.";
pub const MISMATCH: &str =
    "The download does not match the signed checksums; nothing was installed.";
pub const INCOMPLETE_PACKAGE: &str =
    "The package does not contain the Rewind OS programs; nothing was replaced.";

/// A published release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// Without the leading `v`.
    pub version: String,
    /// Its page on GitHub, with the notes and the downloads.
    pub url: String,
    pub published: Option<DateTime<Utc>>,
}

/// What the app knows about new versions.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub current: &'static str,
    /// The newest published release, when the app has asked.
    pub latest: Option<Release>,
    /// `latest` is newer than this build.
    pub newer: bool,
    /// When the app last asked.
    pub checked: Option<DateTime<Utc>>,
    /// The app asks by itself each time it starts.
    pub auto: bool,
    /// How this copy can take a new version.
    pub way: Way,
    /// Why this copy cannot update itself (English), when `way` is [`Way::Manual`].
    pub manual: Option<&'static str>,
}

/// How a copy of Rewind OS takes a new version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Way {
    /// Installed with the MSI: the new MSI replaces it.
    Installer,
    /// Unpacked from the portable zip or the tar.gz: its programs are replaced in place.
    Portable,
    /// Installed by a package manager or Flatpak, or on a system without self-update.
    Manual,
}

/// What `update.install` did.
#[derive(Debug, Clone, Serialize)]
pub struct Installed {
    pub version: String,
    pub way: Way,
    /// The folder whose programs were replaced, for a portable copy.
    pub folder: Option<PathBuf>,
    /// The new version is starting (or starts when its installer finishes).
    pub restarting: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Stored {
    #[serde(default = "yes")]
    auto: bool,
    #[serde(default)]
    checked: Option<DateTime<Utc>>,
    #[serde(default)]
    latest: Option<Release>,
}

fn yes() -> bool {
    true
}

impl Default for Stored {
    fn default() -> Self {
        Self {
            auto: true,
            checked: None,
            latest: None,
        }
    }
}

fn file() -> Option<PathBuf> {
    rewind_os_platform::paths::Known::Data
        .path()
        .map(|data| data.join(APP_FOLDER).join("update.json"))
}

fn load() -> Stored {
    file()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(stored: &Stored) -> Result<(), EngineError> {
    let path = file().ok_or_else(|| EngineError::NotFound("the app's data folder".to_owned()))?;
    let io = |source| EngineError::Io {
        path: path.clone(),
        source,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let text = serde_json::to_vec_pretty(stored).map_err(|e| io(std::io::Error::other(e)))?;
    std::fs::write(&path, text).map_err(io)
}

impl Stored {
    fn status(&self) -> Status {
        let (way, manual) = placement();
        Status {
            way,
            manual,
            current: CURRENT,
            newer: self
                .latest
                .as_ref()
                .is_some_and(|r| is_newer(&r.version, CURRENT)),
            latest: self.latest.clone(),
            checked: self.checked,
            auto: self.auto,
        }
    }
}

/// The latest published release, from GitHub.
fn fetch() -> Result<Release, EngineError> {
    let answer = rewind_os_platform::web::get_text(
        LATEST_URL,
        &format!("rewind-os/{CURRENT}"),
        MAX_ANSWER_BYTES,
    )?;
    parse_latest(&answer).ok_or_else(|| EngineError::NotFound("a published release".to_owned()))
}

/// What the app knows about new versions. GitHub is asked when `now` is set (a check asked for
/// by hand, whose failure is reported) or when the check at start is on (whose failure falls
/// back quietly to the last answer kept). With neither, nothing leaves the computer.
pub fn check(now: bool) -> Result<Status, EngineError> {
    let mut stored = load();
    if !now && !stored.auto {
        return Ok(stored.status());
    }
    match fetch() {
        Ok(release) => {
            stored.latest = Some(release);
            stored.checked = Some(Utc::now());
            // An answer that cannot be kept is still shown; the next start asks again anyway.
            let _ = save(&stored);
        }
        Err(err) if now => return Err(err),
        // Offline or GitHub unreachable at start: what was known before still counts.
        Err(_) => {}
    }
    Ok(stored.status())
}

/// Turns the check at start on or off.
pub fn set_auto(enabled: bool) -> Result<(), EngineError> {
    let mut stored = load();
    stored.auto = enabled;
    save(&stored)
}

/// Opens the page of the newest known release, when it is newer than this build.
pub fn open_notes() -> Result<(), EngineError> {
    let release = load()
        .latest
        .filter(|r| is_newer(&r.version, CURRENT))
        .ok_or_else(|| EngineError::NotFound("a newer version".to_owned()))?;
    Ok(rewind_os_platform::shell::open_web(&release.url)?)
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<DateTime<Utc>>,
}

/// Reads GitHub's answer for the latest release. Drafts, pre-releases, tags that are not a
/// version and pages outside this project's releases are refused.
pub fn parse_latest(answer: &str) -> Option<Release> {
    let r: GitHubRelease = serde_json::from_str(answer).ok()?;
    let version = r.tag_name.strip_prefix('v').unwrap_or(&r.tag_name);
    if r.draft || r.prerelease || Version::parse(version).is_none() {
        return None;
    }
    if !r.html_url.starts_with(RELEASES_PAGE) {
        return None;
    }
    Some(Release {
        version: version.to_owned(),
        url: r.html_url,
        published: r.published_at,
    })
}

/// Whether version `candidate` comes after `current` (`0.10.0` after `0.9.0`, `0.13.0` after
/// `0.13.0-rc.1`). Text that is not a version is never newer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (Version::parse(candidate), Version::parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// `major.minor.patch`, with an optional pre-release suffix after `-`.
#[derive(Debug, PartialEq, Eq)]
struct Version {
    core: [u64; 3],
    pre: Option<String>,
}

impl Version {
    fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) if !pre.is_empty() => (core, Some(pre.to_owned())),
            Some(_) => return None,
            None => (text, None),
        };
        let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
        let core = [parts.next()??, parts.next()??, parts.next()??];
        if parts.next().is_some() {
            return None;
        }
        Some(Version { core, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core
            .cmp(&other.core)
            .then_with(|| match (&self.pre, &other.pre) {
                // A release comes after its own pre-releases.
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// How the running copy takes a new version, and why not when it cannot.
fn placement() -> (Way, Option<&'static str>) {
    let Ok(exe) = std::env::current_exe() else {
        return (Way::Manual, Some(UNKNOWN_PLACE));
    };
    let program_files: Vec<PathBuf> = ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    way_of(
        &exe,
        Os::current(),
        &program_files,
        rewind_os_platform::process::in_flatpak(),
    )
}

/// How a copy whose program is `exe` takes a new version: on Windows the MSI's folder
/// (`<Program Files>\Rewind OS`) or else a portable copy; on Linux a portable copy only when
/// unpacked from the tar.gz (`<folder>/bin/rewind-os`), since the system's folders belong to the
/// package manager.
pub fn way_of(
    exe: &Path,
    os: Os,
    program_files: &[PathBuf],
    flatpak: bool,
) -> (Way, Option<&'static str>) {
    let dir = exe.parent();
    match os {
        Os::Windows => {
            let in_program_files = dir.is_some_and(|d| {
                d.file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("Rewind OS"))
                    && d.parent().is_some_and(|p| {
                        program_files
                            .iter()
                            .any(|pf| pf.as_os_str().eq_ignore_ascii_case(p.as_os_str()))
                    })
            });
            if in_program_files {
                (Way::Installer, None)
            } else {
                (Way::Portable, None)
            }
        }
        Os::Linux if flatpak => (Way::Manual, Some(FROM_FLATPAK)),
        Os::Linux => {
            let system = ["/usr", "/opt", "/app", "/snap", "/nix"]
                .iter()
                .any(|root| exe.starts_with(root));
            if system {
                (Way::Manual, Some(FROM_PACKAGE_MANAGER))
            } else if dir.and_then(Path::file_name).is_some_and(|n| n == "bin") {
                (Way::Portable, None)
            } else {
                (Way::Manual, Some(UNKNOWN_PLACE))
            }
        }
        Os::MacOs => (Way::Manual, Some(NOT_ON_MAC)),
    }
}

/// The release file a copy takes: `arch` as Rust names it (`x86_64`, `aarch64`).
pub fn package_name(version: &str, os: Os, way: Way, arch: &str) -> Option<String> {
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return None,
    };
    match (os, way) {
        (Os::Windows, Way::Installer) => Some(format!("rewind-os-{version}-windows-{arch}.msi")),
        (Os::Windows, Way::Portable) => {
            Some(format!("rewind-os-{version}-windows-{arch}-portable.zip"))
        }
        (Os::Linux, Way::Portable) => Some(format!("rewind-os-{version}-linux-{arch}.tar.gz")),
        _ => None,
    }
}

/// The checksum of `package` in a release's `SHA256SUMS.txt`, once its minisign `signature` is
/// checked against `key` (a `minisign.pub` file) and names `version`. Errors are English reasons.
pub fn signed_checksum(
    sums: &str,
    signature: &str,
    key: &str,
    version: &str,
    package: &str,
) -> Result<String, &'static str> {
    let key = minisign_verify::PublicKey::decode(key.trim()).map_err(|_| BAD_SIGNATURE)?;
    let signature =
        minisign_verify::Signature::decode(signature.trim()).map_err(|_| BAD_SIGNATURE)?;
    key.verify(sums.as_bytes(), &signature, false)
        .map_err(|_| BAD_SIGNATURE)?;
    if signature.trusted_comment() != format!("rewind-os {version} SHA256SUMS.txt") {
        return Err(OTHER_VERSION);
    }
    sums.lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find(|(_, name)| name.trim_start().trim_start_matches('*') == package)
        .map(|(hash, _)| hash.to_ascii_lowercase())
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or(NOT_LISTED)
}

/// The SHA-256 of a file, in lowercase hex.
pub fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The programs a copy is made of.
fn programs() -> [String; 2] {
    let suffix = std::env::consts::EXE_SUFFIX;
    [
        format!("rewind-os{suffix}"),
        format!("rewind-os-cli{suffix}"),
    ]
}

/// The name a replaced program keeps until the next start.
fn old_name(name: &str) -> String {
    format!("{name}.old")
}

/// Puts the programs `names` from `new` into `dir`. The ones there are renamed `<name>.old` first
/// (a running program can be renamed, not overwritten); if any copy fails, every program is put
/// back as it was. Nothing changes when `new` lacks one of them.
pub fn replace_programs(dir: &Path, new: &Path, names: &[String]) -> Result<(), EngineError> {
    if names.iter().any(|n| !new.join(n).is_file()) {
        return Err(EngineError::Refused(INCOMPLETE_PACKAGE));
    }
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| EngineError::Io { path, source }
    };
    let mut moved: Vec<&String> = Vec::new();
    let mut outcome = Ok(());
    for name in names {
        let target = dir.join(name);
        let old = dir.join(old_name(name));
        if !target.exists() {
            continue;
        }
        if old.exists()
            && let Err(e) = std::fs::remove_file(&old).map_err(io(&old))
        {
            outcome = Err(e);
            break;
        }
        if let Err(e) = std::fs::rename(&target, &old).map_err(io(&target)) {
            outcome = Err(e);
            break;
        }
        moved.push(name);
    }
    if outcome.is_ok() {
        for name in names {
            let target = dir.join(name);
            if let Err(e) = std::fs::copy(new.join(name), &target).map_err(io(&target)) {
                outcome = Err(e);
                break;
            }
        }
    }
    if outcome.is_err() {
        for name in moved {
            let target = dir.join(name);
            let _ = std::fs::remove_file(&target);
            let _ = std::fs::rename(dir.join(old_name(name)), &target);
        }
    }
    outcome
}

/// Removes what an update left behind: the programs a portable copy replaced (`.old`, next to
/// the running one) and the downloads in the temporary folder. Both pass the safety guard.
pub fn tidy() {
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        for name in programs() {
            remove_file_safely(&dir.join(old_name(&name)));
        }
    }
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let ours = entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.starts_with(WORK_PREFIX));
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if ours && is_dir && crate::safety::removal_refusal(&entry.path()).is_none() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn remove_file_safely(path: &Path) {
    if path.is_file() && crate::safety::removal_refusal(path).is_none() {
        let _ = std::fs::remove_file(path);
    }
}

/// The temporary folders downloads go to start with this.
const WORK_PREFIX: &str = "rewind-os-update-";

/// Downloads, checks and installs the newest version. With `restart`, the new version starts
/// (for the MSI, when its installer finishes) and the caller should exit.
pub fn install(restart: bool, task: &Task) -> Result<Installed, EngineError> {
    task.set_stage("Checking for a new version");
    let status = check(true)?;
    let latest = status
        .latest
        .filter(|_| status.newer)
        .ok_or_else(|| EngineError::NotFound("a newer version".to_owned()))?;
    if let Some(why) = status.manual {
        return Err(EngineError::Refused(why));
    }
    let exe = std::env::current_exe().map_err(|source| EngineError::Io {
        path: PathBuf::from("current executable"),
        source,
    })?;
    let dir = exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| EngineError::NotFound("the app's folder".to_owned()))?;
    let version = latest.version;
    let package = package_name(&version, Os::current(), status.way, std::env::consts::ARCH)
        .ok_or(EngineError::Refused(NO_PACKAGE))?;
    let base = format!("{DOWNLOADS}v{version}/");
    let agent = format!("rewind-os/{CURRENT}");

    task.set_stage("Checking the signature");
    let sums = rewind_os_platform::web::get_text(
        &format!("{base}SHA256SUMS.txt"),
        &agent,
        MAX_ANSWER_BYTES,
    )?;
    let signature = rewind_os_platform::web::get_text(
        &format!("{base}SHA256SUMS.txt.minisig"),
        &agent,
        64 * 1024,
    )?;
    let expected = signed_checksum(&sums, &signature, RELEASE_KEY, &version, &package)
        .map_err(EngineError::Refused)?;
    if task.is_cancelled() {
        return Err(EngineError::Cancelled);
    }

    task.set_stage("Downloading the new version");
    let work = std::env::temp_dir().join(format!("{WORK_PREFIX}{version}-{}", std::process::id()));
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| EngineError::Io { path, source }
    };
    std::fs::create_dir_all(&work).map_err(io(&work))?;
    let file = work.join(&package);
    rewind_os_platform::web::download(
        &format!("{base}{package}"),
        &file,
        &agent,
        MAX_PACKAGE_BYTES,
    )?;
    task.set_stage("Checking the download");
    if sha256_of(&file).map_err(io(&file))? != expected {
        remove_file_safely(&file);
        return Err(EngineError::Refused(MISMATCH));
    }
    if task.is_cancelled() {
        return Err(EngineError::Cancelled);
    }

    task.set_stage("Installing the new version");
    match status.way {
        Way::Installer => {
            rewind_os_platform::installer::install_then_start(
                &file,
                restart.then_some(exe.as_path()),
            )?;
            Ok(Installed {
                version,
                way: Way::Installer,
                folder: None,
                restarting: restart,
            })
        }
        Way::Portable => {
            let unpacked = work.join("unpacked");
            rewind_os_platform::web::unpack(&file, &unpacked)?;
            let root = package
                .strip_suffix(".zip")
                .or_else(|| package.strip_suffix(".tar.gz"))
                .unwrap_or(&package);
            let new = match Os::current() {
                Os::Windows => unpacked.join(root),
                _ => unpacked.join(root).join("bin"),
            };
            replace_programs(&dir, &new, &programs())?;
            if restart {
                rewind_os_platform::process::start(&exe, &[])?;
            }
            Ok(Installed {
                version,
                way: Way::Portable,
                folder: Some(dir),
                restarting: restart,
            })
        }
        Way::Manual => Err(EngineError::Refused(UNKNOWN_PLACE)),
    }
}
