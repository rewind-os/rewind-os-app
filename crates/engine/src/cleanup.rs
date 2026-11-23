//! Junk cleanup by category. Each category is a list of path templates plus rules (minimum age,
//! extensions, whether it needs administrator rights). A template starts with a known folder,
//! `{cache}/thumbnails/*`, or is absolute, `/var/crash/*`, and points at the entries to remove:
//! `{temp}/*` empties the temporary folder but keeps the folder itself.
//!
//! Every system has its own list; [`categories`] returns the one for this computer. Shared
//! ideas (browser caches, package caches, the recycle bin) keep the same id everywhere.

use crate::walk::{self, Filter, Tally};
use crate::{EngineError, Result, Task};
use rayon::prelude::*;
use rewind_os_platform::paths::Known;
use rewind_os_platform::{Os, elevation, shell};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    System,
    Browsers,
    Apps,
    Developer,
    Personal,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::System,
        Group::Browsers,
        Group::Apps,
        Group::Developer,
        Group::Personal,
    ];

    /// English label; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Group::System => "System",
            Group::Browsers => "Browsers",
            Group::Apps => "Applications",
            Group::Developer => "Developer tools",
            Group::Personal => "Your files",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Regenerated automatically; nothing is lost.
    Safe,
    /// Regenerated, but something gets slower for a while or a history is lost.
    Moderate,
    /// Your own files; they go to the recycle bin.
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposal {
    Delete,
    RecycleBin,
}

#[derive(Debug, Clone, Copy)]
enum Source {
    Patterns(&'static [&'static str]),
    RecycleBin,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Category {
    pub id: &'static str,
    /// English name and description; translate with `rewind_os_i18n::tr`.
    pub name: &'static str,
    pub description: &'static str,
    pub group: Group,
    pub risk: Risk,
    pub needs_admin: bool,
    pub selected_by_default: bool,
    /// Files modified more recently than this are kept.
    pub min_age_days: u32,
    pub disposal: Disposal,
    #[serde(skip)]
    source: Source,
    #[serde(skip)]
    extensions: &'static [&'static str],
    /// Entries whose name starts with one of these are never touched.
    #[serde(skip)]
    skip_prefixes: &'static [&'static str],
}

impl Category {
    const fn new(
        id: &'static str,
        name: &'static str,
        description: &'static str,
        patterns: &'static [&'static str],
    ) -> Self {
        Category {
            id,
            name,
            description,
            group: Group::System,
            risk: Risk::Safe,
            needs_admin: false,
            selected_by_default: false,
            min_age_days: 0,
            disposal: Disposal::Delete,
            source: Source::Patterns(patterns),
            extensions: &[],
            skip_prefixes: &[],
        }
    }

    const fn skipping(mut self, prefixes: &'static [&'static str]) -> Self {
        self.skip_prefixes = prefixes;
        self
    }

    const fn group(mut self, group: Group) -> Self {
        self.group = group;
        self
    }

    const fn risk(mut self, risk: Risk) -> Self {
        self.risk = risk;
        self
    }

    const fn admin(mut self) -> Self {
        self.needs_admin = true;
        self
    }

    const fn preselected(mut self) -> Self {
        self.selected_by_default = true;
        self
    }

    const fn older_than_days(mut self, days: u32) -> Self {
        self.min_age_days = days;
        self
    }

    /// Top-level files with these extensions, moved to the recycle bin (old installers).
    const fn installers(mut self, extensions: &'static [&'static str]) -> Self {
        self.extensions = extensions;
        self.disposal = Disposal::RecycleBin;
        self.risk = Risk::Review;
        self.group = Group::Personal;
        self.min_age_days = 30;
        self
    }
}

const RECYCLE_BIN: Category = Category {
    id: "personal.recycle_bin",
    name: "Recycle bin",
    description: "Files you already deleted. Emptying it is permanent.",
    group: Group::Personal,
    risk: Risk::Review,
    needs_admin: false,
    selected_by_default: false,
    min_age_days: 0,
    disposal: Disposal::Delete,
    source: Source::RecycleBin,
    extensions: &[],
    skip_prefixes: &[],
};

const BROWSER_NOTE: &str =
    "Downloaded copies of web pages and images. Close the browser first; files in use are skipped.";
const PACKAGES_NOTE: &str = "Downloaded packages kept by package managers such as npm, pip, Cargo and Gradle. They are downloaded again when a project needs them.";
const ELECTRON_NOTE: &str =
    "Web caches of apps such as Discord, Slack, Teams and Spotify. Close them first.";

pub const WINDOWS: &[Category] = &[
    Category::new(
        "windows.user_temp",
        "Your temporary files",
        "Files programs left in your temporary folder. Files from the last day are kept, in case an installer is still using them.",
        &["{temp}/*"],
    )
    .preselected()
    .older_than_days(1),
    Category::new(
        "windows.system_temp",
        "Windows temporary files",
        "The system-wide temporary folder.",
        &["{windows}/Temp/*"],
    )
    .admin()
    .preselected()
    .older_than_days(1),
    Category::new(
        "windows.update_cache",
        "Windows Update downloads",
        "Update packages Windows already installed. Windows downloads them again if it needs them.",
        &["{windows}/SoftwareDistribution/Download/*"],
    )
    .admin()
    .preselected(),
    Category::new(
        "windows.delivery_optimization",
        "Delivery Optimization cache",
        "Update pieces kept to share with other computers.",
        &["{windows}/ServiceProfiles/NetworkService/AppData/Local/Microsoft/Windows/DeliveryOptimization/Cache/*"],
    )
    .admin()
    .preselected(),
    Category::new(
        "windows.user_crash_dumps",
        "Program crash reports",
        "Memory dumps and error reports written when a program crashed.",
        &[
            "{local_app_data}/CrashDumps/*",
            "{local_app_data}/Microsoft/Windows/WER/ReportArchive/*",
            "{local_app_data}/Microsoft/Windows/WER/ReportQueue/*",
        ],
    )
    .preselected(),
    Category::new(
        "windows.system_crash_dumps",
        "System crash reports",
        "Memory dumps from system crashes (blue screens) and Windows error reports.",
        &[
            "{windows}/Minidump/*",
            "{windows}/MEMORY.DMP",
            "{program_data}/Microsoft/Windows/WER/ReportArchive/*",
            "{program_data}/Microsoft/Windows/WER/ReportQueue/*",
        ],
    )
    .admin()
    .preselected(),
    Category::new(
        "windows.logs",
        "Old Windows logs",
        "Log files older than a week. Useful only for diagnosing past problems.",
        &["{windows}/Logs/*", "{windows}/Panther/*.log"],
    )
    .risk(Risk::Moderate)
    .admin()
    .older_than_days(7),
    Category::new(
        "windows.thumbnails",
        "Thumbnail cache",
        "Image previews for File Explorer. Rebuilt as you browse folders; files in use are skipped.",
        &[
            "{local_app_data}/Microsoft/Windows/Explorer/thumbcache_*.db",
            "{local_app_data}/Microsoft/Windows/Explorer/iconcache_*.db",
        ],
    )
    .risk(Risk::Moderate),
    Category::new(
        "apps.shader_cache",
        "Graphics shader caches",
        "Compiled shaders from DirectX and graphics drivers. Games may stutter briefly while they rebuild them.",
        &[
            "{local_app_data}/D3DSCache/*",
            "{local_app_data}/NVIDIA/DXCache/*",
            "{local_app_data}/NVIDIA/GLCache/*",
            "{local_app_data}/AMD/DxCache/*",
            "{local_app_data}/AMD/DxcCache/*",
            "{local_app_data}/AMD/VkCache/*",
            "{local_app_data}/Intel/ShaderCache/*",
        ],
    )
    .group(Group::Apps)
    .risk(Risk::Moderate),
    Category::new(
        "browsers.chrome",
        "Google Chrome cache",
        BROWSER_NOTE,
        &[
            "{local_app_data}/Google/Chrome/User Data/*/Cache/*",
            "{local_app_data}/Google/Chrome/User Data/*/Code Cache/*",
            "{local_app_data}/Google/Chrome/User Data/*/GPUCache/*",
        ],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "browsers.edge",
        "Microsoft Edge cache",
        BROWSER_NOTE,
        &[
            "{local_app_data}/Microsoft/Edge/User Data/*/Cache/*",
            "{local_app_data}/Microsoft/Edge/User Data/*/Code Cache/*",
            "{local_app_data}/Microsoft/Edge/User Data/*/GPUCache/*",
        ],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "browsers.brave",
        "Brave cache",
        BROWSER_NOTE,
        &[
            "{local_app_data}/BraveSoftware/Brave-Browser/User Data/*/Cache/*",
            "{local_app_data}/BraveSoftware/Brave-Browser/User Data/*/Code Cache/*",
            "{local_app_data}/BraveSoftware/Brave-Browser/User Data/*/GPUCache/*",
        ],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "browsers.firefox",
        "Firefox cache",
        BROWSER_NOTE,
        &["{local_app_data}/Mozilla/Firefox/Profiles/*/cache2/*"],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "apps.electron_caches",
        "Desktop app caches",
        ELECTRON_NOTE,
        &[
            "{roaming_app_data}/discord/Cache/*",
            "{roaming_app_data}/discord/Code Cache/*",
            "{roaming_app_data}/Slack/Cache/*",
            "{roaming_app_data}/Slack/Code Cache/*",
            "{roaming_app_data}/Microsoft/Teams/Cache/*",
            "{local_app_data}/Spotify/Data/*",
            "{roaming_app_data}/Code/Cache/*",
            "{roaming_app_data}/Code/CachedData/*",
        ],
    )
    .group(Group::Apps),
    Category::new(
        "developer.package_caches",
        "Package manager caches",
        PACKAGES_NOTE,
        &[
            "{local_app_data}/npm-cache/*",
            "{local_app_data}/Yarn/Cache/*",
            "{local_app_data}/pnpm/store/*",
            "{local_app_data}/pip/Cache/*",
            "{local_app_data}/NuGet/v3-cache/*",
            "{home}/.nuget/packages/*",
            "{home}/.cargo/registry/cache/*",
            "{home}/.gradle/caches/*",
        ],
    )
    .group(Group::Developer)
    .risk(Risk::Moderate),
    Category::new(
        "personal.old_installers",
        "Old installers in Downloads",
        "Setup programs (.exe, .msi, .msix) in your Downloads folder that are more than 30 days old. They go to the recycle bin.",
        &["{downloads}/*"],
    )
    .installers(&["exe", "msi", "msix", "msixbundle", "appx"]),
    RECYCLE_BIN,
];

pub const LINUX: &[Category] = &[
    Category::new(
        "linux.thumbnails",
        "Thumbnail cache",
        "Image previews made by file managers. Rebuilt as you browse folders.",
        &["{cache}/thumbnails/*"],
    )
    .preselected(),
    Category::new(
        "linux.crash_reports",
        "Crash reports",
        "Core dumps and reports left by programs that crashed.",
        &["/var/crash/*", "/var/lib/systemd/coredump/*"],
    )
    .admin()
    .preselected(),
    Category::new(
        "linux.package_downloads",
        "Package manager downloads",
        "Packages your system already installed. They are downloaded again if needed.",
        &[
            "/var/cache/apt/archives/*.deb",
            "/var/cache/dnf/*",
            "/var/cache/pacman/pkg/*",
            "/var/cache/zypp/packages/*",
        ],
    )
    .admin()
    .preselected(),
    Category::new(
        "linux.old_logs",
        "Old rotated logs",
        "Compressed and rotated log files older than a week.",
        &["/var/log/*.gz", "/var/log/*.old", "/var/log/*.[0-9]", "/var/log/*/*.gz"],
    )
    .risk(Risk::Moderate)
    .admin()
    .older_than_days(7),
    Category::new(
        "browsers.chrome",
        "Google Chrome cache",
        BROWSER_NOTE,
        &[
            "{cache}/google-chrome/*/Cache/*",
            "{config}/google-chrome/*/Code Cache/*",
            "{config}/google-chrome/*/GPUCache/*",
        ],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "browsers.chromium",
        "Chromium cache",
        BROWSER_NOTE,
        &[
            "{cache}/chromium/*/Cache/*",
            "{config}/chromium/*/Code Cache/*",
            "{config}/chromium/*/GPUCache/*",
        ],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "browsers.edge",
        "Microsoft Edge cache",
        BROWSER_NOTE,
        &["{cache}/microsoft-edge/*/Cache/*", "{config}/microsoft-edge/*/Code Cache/*"],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "browsers.brave",
        "Brave cache",
        BROWSER_NOTE,
        &[
            "{cache}/BraveSoftware/Brave-Browser/*/Cache/*",
            "{config}/BraveSoftware/Brave-Browser/*/Code Cache/*",
        ],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "browsers.firefox",
        "Firefox cache",
        BROWSER_NOTE,
        &[
            "{cache}/mozilla/firefox/*/cache2/*",
            "{home}/snap/firefox/common/.cache/mozilla/firefox/*/cache2/*",
        ],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "apps.electron_caches",
        "Desktop app caches",
        ELECTRON_NOTE,
        &[
            "{config}/discord/Cache/*",
            "{config}/discord/Code Cache/*",
            "{config}/Slack/Cache/*",
            "{config}/Slack/Code Cache/*",
            "{config}/Code/Cache/*",
            "{config}/Code/CachedData/*",
            "{cache}/spotify/Data/*",
        ],
    )
    .group(Group::Apps),
    Category::new(
        "developer.package_caches",
        "Package manager caches",
        PACKAGES_NOTE,
        &[
            "{home}/.npm/_cacache/*",
            "{cache}/yarn/*",
            "{data}/pnpm/store/*",
            "{cache}/pip/*",
            "{cache}/go-build/*",
            "{home}/.nuget/packages/*",
            "{home}/.cargo/registry/cache/*",
            "{home}/.gradle/caches/*",
        ],
    )
    .group(Group::Developer)
    .risk(Risk::Moderate),
    Category::new(
        "personal.old_installers",
        "Old installers in Downloads",
        "Installer packages (.deb, .rpm, .AppImage) in your Downloads folder that are more than 30 days old. They go to the recycle bin.",
        &["{downloads}/*"],
    )
    .installers(&["deb", "rpm", "appimage", "run", "flatpakref"]),
    RECYCLE_BIN,
];

pub const MACOS: &[Category] = &[
    Category::new(
        "macos.user_caches",
        "App caches",
        "Caches apps keep in your Library, browsers included. Apps recreate them, but may be slower the first time.",
        &["{cache}/*"],
    )
    .skipping(&["com.apple."])
    .risk(Risk::Moderate),
    Category::new(
        "macos.logs",
        "Old logs and crash reports",
        "Log files and crash reports older than a week.",
        &["{home}/Library/Logs/*"],
    )
    .preselected()
    .older_than_days(7),
    Category::new(
        "developer.xcode",
        "Xcode build data",
        "Build products, indexes and device support files. Xcode recreates them when needed.",
        &[
            "{home}/Library/Developer/Xcode/DerivedData/*",
            "{home}/Library/Developer/Xcode/iOS DeviceSupport/*",
            "{home}/Library/Developer/CoreSimulator/Caches/*",
        ],
    )
    .group(Group::Developer)
    .risk(Risk::Moderate),
    Category::new(
        "developer.package_caches",
        "Package manager caches",
        PACKAGES_NOTE,
        &[
            "{home}/.npm/_cacache/*",
            "{home}/.nuget/packages/*",
            "{home}/.cargo/registry/cache/*",
            "{home}/.gradle/caches/*",
        ],
    )
    .group(Group::Developer)
    .risk(Risk::Moderate),
    Category::new(
        "personal.old_installers",
        "Old installers in Downloads",
        "Disk images and installer packages (.dmg, .pkg) in your Downloads folder that are more than 30 days old. They go to the recycle bin.",
        &["{downloads}/*"],
    )
    .installers(&["dmg", "pkg", "mpkg"]),
    RECYCLE_BIN,
];

/// The categories for this computer's system.
pub fn categories() -> &'static [Category] {
    categories_for(Os::current())
}

pub fn categories_for(os: Os) -> &'static [Category] {
    match os {
        Os::Windows => WINDOWS,
        Os::Linux => LINUX,
        Os::MacOs => MACOS,
    }
}

pub fn category(id: &str) -> Option<&'static Category> {
    categories().iter().find(|c| c.id == id)
}

/// Where the known folders are. [`Places::current`] reads this computer; tests build their own.
#[derive(Debug, Clone, Default)]
pub struct Places {
    folders: HashMap<&'static str, PathBuf>,
}

impl Places {
    pub fn current() -> Self {
        Self::from_pairs(
            Known::ALL
                .iter()
                .filter_map(|k| k.path().map(|p| (k.token(), p))),
        )
    }

    pub fn from_pairs(pairs: impl IntoIterator<Item = (&'static str, PathBuf)>) -> Self {
        Self {
            folders: pairs.into_iter().collect(),
        }
    }

    /// Turns a template into a glob pattern, or `None` when its folder is unknown here, so a
    /// template never silently becomes a relative path.
    fn expand(&self, template: &str) -> Option<String> {
        let Some(rest) = template.strip_prefix('{') else {
            return template.starts_with('/').then(|| template.to_owned());
        };
        let (token, tail) = rest.split_once('}')?;
        let base = self.folders.get(token)?;
        Some(format!(
            "{}{tail}",
            glob::Pattern::escape(&base.to_string_lossy())
        ))
    }

    /// Known folders themselves are never targets, whatever a template says.
    fn is_protected(&self, path: &Path) -> bool {
        if path.parent().is_none() || path.parent().and_then(Path::parent).is_none() {
            return true;
        }
        self.folders.values().any(|k| same_path(k, path))
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    norm(a) == norm(b)
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryReport {
    pub category: Category,
    pub files: u64,
    pub bytes: u64,
    /// At least one of the category's locations exists on this computer.
    pub present: bool,
    /// The category needs administrator rights and the app does not have them, so it was
    /// not measured.
    pub blocked: bool,
    /// The size could not be measured (the reason is in `note`).
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryResult {
    pub id: &'static str,
    pub files: u64,
    pub bytes: u64,
    /// Files that could not be removed (usually because a program has them open).
    pub failed: u64,
    /// Set when the category was not cleaned at all; English text.
    pub skipped: Option<&'static str>,
    pub error: Option<String>,
}

impl Category {
    fn filter(&self) -> Filter {
        let modified_before = (self.min_age_days > 0).then(|| {
            SystemTime::now()
                .checked_sub(Duration::from_secs(u64::from(self.min_age_days) * 86_400))
                .unwrap_or(SystemTime::UNIX_EPOCH)
        });
        // System locations keep their folders: some services expect them to exist.
        Filter {
            modified_before,
            extensions: self.extensions,
            keep_folders: self.needs_admin,
        }
    }

    /// The existing entries the templates match on this computer.
    fn targets(&self, places: &Places) -> Vec<PathBuf> {
        let Source::Patterns(patterns) = self.source else {
            return Vec::new();
        };
        let options = glob::MatchOptions {
            case_sensitive: !cfg!(windows),
            require_literal_separator: true,
            require_literal_leading_dot: false,
        };
        patterns
            .iter()
            .filter_map(|p| places.expand(p))
            .filter_map(|p| glob::glob_with(&p, options).ok())
            .flat_map(|matches| matches.filter_map(|m| m.ok()))
            .filter(|p| !places.is_protected(p))
            .filter(|p| crate::safety::Guard::current().cleanup_refusal(p).is_none())
            .filter(|p| {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                !self
                    .skip_prefixes
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
            })
            .collect()
    }
}

/// Measures every category of this computer.
pub fn analyze(task: &Task) -> Vec<CategoryReport> {
    analyze_in(
        categories(),
        &Places::current(),
        elevation::is_elevated(),
        task,
    )
}

/// Measures `list` against `places`.
pub fn analyze_in(
    list: &'static [Category],
    places: &Places,
    elevated: bool,
    task: &Task,
) -> Vec<CategoryReport> {
    task.set_stage("Measuring junk files");
    list.par_iter()
        .map(|category| measure(category, places, elevated, task))
        .collect()
}

fn sum(tallies: impl ParallelIterator<Item = Tally>) -> Tally {
    tallies.reduce(Tally::default, |mut a, b| {
        a.files += b.files;
        a.bytes += b.bytes;
        a.failed += b.failed;
        a
    })
}

fn measure(
    category: &'static Category,
    places: &Places,
    elevated: bool,
    task: &Task,
) -> CategoryReport {
    let mut report = CategoryReport {
        category: *category,
        files: 0,
        bytes: 0,
        present: false,
        blocked: false,
        note: None,
    };
    match category.source {
        Source::RecycleBin => match shell::recycle_bin_summary() {
            Ok(summary) => {
                report.present = true;
                report.files = summary.items;
                report.bytes = summary.bytes;
            }
            Err(err) => report.note = Some(err.to_string()),
        },
        Source::Patterns(_) => {
            let targets = category.targets(places);
            report.present = !targets.is_empty();
            if category.needs_admin && !elevated {
                // Without rights the folders are unreadable; a partial number would mislead.
                report.blocked = true;
                return report;
            }
            let filter = category.filter();
            let tally = sum(targets.par_iter().map(|t| walk::measure(t, &filter, task)));
            report.files = tally.files;
            report.bytes = tally.bytes;
        }
    }
    report
}

/// Cleans the categories with the given ids. Categories that need administrator rights are
/// skipped, and reported as skipped, when the app is not elevated.
pub fn run(ids: &[String], task: &Task) -> Result<Vec<CategoryResult>> {
    run_in(
        categories(),
        &Places::current(),
        elevation::is_elevated(),
        ids,
        task,
    )
}

/// Cleans the categories of `list` with the given ids, against `places`.
pub fn run_in(
    list: &'static [Category],
    places: &Places,
    elevated: bool,
    ids: &[String],
    task: &Task,
) -> Result<Vec<CategoryResult>> {
    let chosen: Vec<&'static Category> = ids
        .iter()
        .map(|id| {
            list.iter()
                .find(|c| c.id == id)
                .ok_or_else(|| EngineError::UnknownCategory(id.clone()))
        })
        .collect::<Result<_>>()?;
    let mut results = Vec::with_capacity(chosen.len());
    for category in chosen {
        if task.is_cancelled() {
            results.push(skipped(category, "Cancelled"));
            continue;
        }
        task.set_stage(category.name);
        results.push(clean(category, places, elevated, task));
    }
    Ok(results)
}

fn skipped(category: &'static Category, reason: &'static str) -> CategoryResult {
    CategoryResult {
        id: category.id,
        files: 0,
        bytes: 0,
        failed: 0,
        skipped: Some(reason),
        error: None,
    }
}

fn clean(
    category: &'static Category,
    places: &Places,
    elevated: bool,
    task: &Task,
) -> CategoryResult {
    if category.needs_admin && !elevated {
        return skipped(category, "Needs administrator rights");
    }
    let mut result = CategoryResult {
        id: category.id,
        files: 0,
        bytes: 0,
        failed: 0,
        skipped: None,
        error: None,
    };
    match category.source {
        Source::RecycleBin => {
            let before = shell::recycle_bin_summary().map(|s| s.bytes).unwrap_or(0);
            match shell::empty_recycle_bin() {
                Ok(items) => {
                    result.files = items;
                    result.bytes = before;
                    task.add(items, before);
                }
                Err(err) => result.error = Some(err.to_string()),
            }
        }
        Source::Patterns(_) => {
            let filter = category.filter();
            let targets = category.targets(places);
            match category.disposal {
                Disposal::Delete => {
                    let tally = sum(targets.par_iter().map(|t| walk::delete(t, &filter, task)));
                    result.files = tally.files;
                    result.bytes = tally.bytes;
                    result.failed = tally.failed;
                }
                Disposal::RecycleBin => {
                    // Only top-level files that pass the filter; folders are left alone.
                    let files: Vec<(PathBuf, u64)> = targets
                        .into_iter()
                        .filter(|t| t.is_file())
                        .filter_map(|t| {
                            let m = walk::measure(&t, &filter, &Task::new());
                            (m.files == 1).then_some((t, m.bytes))
                        })
                        .collect();
                    for (path, bytes) in files {
                        let outcome = shell::move_to_trash(std::slice::from_ref(&path));
                        if outcome.iter().all(|o| o.error.is_none()) {
                            result.files += 1;
                            result.bytes += bytes;
                            task.add(1, bytes);
                        } else {
                            result.failed += 1;
                        }
                    }
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_folders_and_roots_are_refused() {
        let places = Places::from_pairs([("home", PathBuf::from("/home/someone"))]);
        assert!(places.is_protected(Path::new("/")));
        assert!(places.is_protected(Path::new("/home")));
        assert!(places.is_protected(Path::new("/home/someone/")));
        assert!(!places.is_protected(Path::new("/home/someone/.cache")));
    }

    #[test]
    fn a_template_with_an_unknown_folder_matches_nothing() {
        let places = Places::from_pairs([("home", PathBuf::from("/home/someone"))]);
        assert_eq!(places.expand("{cache}/thumbnails/*"), None);
        assert_eq!(places.expand("relative/*"), None);
        assert!(places.expand("{home}/.npm/*").is_some());
    }

    #[test]
    fn folder_names_with_glob_characters_are_taken_literally() {
        let places = Places::from_pairs([("home", PathBuf::from("/home/a[1]"))]);
        let pattern = places.expand("{home}/*").unwrap_or_default();
        let compiled = glob::Pattern::new(&pattern).unwrap();
        assert!(compiled.matches("/home/a[1]/file"));
        assert!(!compiled.matches("/home/a1/file"));
    }
}
