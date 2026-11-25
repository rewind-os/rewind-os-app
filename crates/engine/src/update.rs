//! Whether a newer version of Rewind OS is out.
//!
//! The app asks GitHub for the latest published release, at most once a day unless asked to
//! check now, and keeps the answer in its own data folder (`rewind-os/update.json`). The request
//! carries the app's name and version and nothing about the computer. The daily check can be
//! turned off; a check asked for by hand still works then.

use crate::EngineError;
use crate::history::APP_FOLDER;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::PathBuf;

/// The version of this build.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// The latest published release (drafts and pre-releases are left out by GitHub).
const LATEST_URL: &str = "https://api.github.com/repos/rewind-os/rewind-os-app/releases/latest";
/// Release pages the app opens; an answer pointing anywhere else is not trusted.
pub const RELEASES_PAGE: &str = "https://github.com/rewind-os/rewind-os-app/releases/";
/// How long an answer is kept before the daily check asks again.
const RECHECK_HOURS: i64 = 24;
/// Larger answers are refused (the real one is a few tens of kilobytes).
const MAX_ANSWER_BYTES: u64 = 4 * 1024 * 1024;

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
    /// The app asks once a day by itself.
    pub auto: bool,
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
        Status {
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

/// Whether the daily check should ask again: it is on, and the last answer is a day old (or
/// from the future, when the clock was wrong).
pub fn due(auto: bool, checked: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    auto && checked.is_none_or(|at| at > now || now - at >= chrono::Duration::hours(RECHECK_HOURS))
}

/// What the app knows about new versions, asking GitHub first when `now` is set or the daily
/// check is due. Without either, nothing leaves the computer.
pub fn check(now: bool) -> Result<Status, EngineError> {
    let mut stored = load();
    let at = Utc::now();
    if now || due(stored.auto, stored.checked, at) {
        let answer = rewind_os_platform::web::get_text(
            LATEST_URL,
            &format!("rewind-os/{CURRENT}"),
            MAX_ANSWER_BYTES,
        )?;
        let release = parse_latest(&answer)
            .ok_or_else(|| EngineError::NotFound("a published release".to_owned()))?;
        stored.latest = Some(release);
        stored.checked = Some(at);
        // When the answer cannot be kept, the next start simply asks again.
        if save(&stored).is_err() {
            stored.checked = None;
        }
    }
    Ok(stored.status())
}

/// Turns the daily check on or off.
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
