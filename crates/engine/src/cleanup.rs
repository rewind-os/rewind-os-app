//! Junk cleanup by category. Each category is a list of path patterns plus rules (minimum age,
//! extensions, whether it needs administrator rights). Patterns point at the entries to
//! remove: `%TEMP%\*` empties the temporary folder but keeps the folder itself.

use crate::walk::{self, Filter, Tally};
use crate::{EngineError, Result, Task};
use rayon::prelude::*;
use rewindows_platform::{elevation, paths, shell};
use serde::Serialize;
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
    /// English label; translate with `rewindows_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Group::System => "Windows",
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
    /// English name and description; translate with `rewindows_i18n::tr`.
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
        }
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
}

pub const CATEGORIES: &[Category] = &[
    Category::new(
        "windows.user_temp",
        "Your temporary files",
        "Files programs left in your temporary folder. Files from the last day are kept, in case an installer is still using them.",
        &[r"%TEMP%\*"],
    )
    .group(Group::System)
    .preselected()
    .older_than_days(1),
    Category::new(
        "windows.system_temp",
        "Windows temporary files",
        "The system-wide temporary folder.",
        &[r"%WINDIR%\Temp\*"],
    )
    .group(Group::System)
    .admin()
    .preselected()
    .older_than_days(1),
    Category::new(
        "windows.update_cache",
        "Windows Update downloads",
        "Update packages Windows already installed. Windows downloads them again if it needs them.",
        &[r"%WINDIR%\SoftwareDistribution\Download\*"],
    )
    .group(Group::System)
    .admin()
    .preselected(),
    Category::new(
        "windows.delivery_optimization",
        "Delivery Optimization cache",
        "Update pieces kept to share with other computers.",
        &[r"%WINDIR%\ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache\*"],
    )
    .group(Group::System)
    .admin()
    .preselected(),
    Category::new(
        "windows.user_crash_dumps",
        "Program crash reports",
        "Memory dumps and error reports written when a program crashed.",
        &[r"%LOCALAPPDATA%\CrashDumps\*", r"%LOCALAPPDATA%\Microsoft\Windows\WER\ReportArchive\*", r"%LOCALAPPDATA%\Microsoft\Windows\WER\ReportQueue\*"],
    )
    .group(Group::System)
    .preselected(),
    Category::new(
        "windows.system_crash_dumps",
        "System crash reports",
        "Memory dumps from system crashes (blue screens) and Windows error reports.",
        &[r"%WINDIR%\Minidump\*", r"%WINDIR%\MEMORY.DMP", r"%PROGRAMDATA%\Microsoft\Windows\WER\ReportArchive\*", r"%PROGRAMDATA%\Microsoft\Windows\WER\ReportQueue\*"],
    )
    .group(Group::System)
    .admin()
    .preselected(),
    Category::new(
        "windows.logs",
        "Old Windows logs",
        "Log files older than a week. Useful only for diagnosing past problems.",
        &[r"%WINDIR%\Logs\*", r"%WINDIR%\Panther\*.log"],
    )
    .group(Group::System)
    .risk(Risk::Moderate)
    .admin()
    .older_than_days(7),
    Category::new(
        "windows.thumbnails",
        "Thumbnail cache",
        "Image previews for File Explorer. Rebuilt as you browse folders; files in use are skipped.",
        &[r"%LOCALAPPDATA%\Microsoft\Windows\Explorer\thumbcache_*.db", r"%LOCALAPPDATA%\Microsoft\Windows\Explorer\iconcache_*.db"],
    )
    .group(Group::System)
    .risk(Risk::Moderate),
    Category::new(
        "apps.shader_cache",
        "Graphics shader caches",
        "Compiled shaders from DirectX and graphics drivers. Games may stutter briefly while they rebuild them.",
        &[r"%LOCALAPPDATA%\D3DSCache\*", r"%LOCALAPPDATA%\NVIDIA\DXCache\*", r"%LOCALAPPDATA%\NVIDIA\GLCache\*", r"%LOCALAPPDATA%\AMD\DxCache\*", r"%LOCALAPPDATA%\AMD\DxcCache\*", r"%LOCALAPPDATA%\AMD\VkCache\*", r"%LOCALAPPDATA%\Intel\ShaderCache\*"],
    )
    .group(Group::Apps)
    .risk(Risk::Moderate),
    Category::new(
        "browsers.chrome",
        "Google Chrome cache",
        "Downloaded copies of web pages and images. Close the browser first; files in use are skipped.",
        &[r"%LOCALAPPDATA%\Google\Chrome\User Data\*\Cache\*", r"%LOCALAPPDATA%\Google\Chrome\User Data\*\Code Cache\*", r"%LOCALAPPDATA%\Google\Chrome\User Data\*\GPUCache\*"],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "browsers.edge",
        "Microsoft Edge cache",
        "Downloaded copies of web pages and images. Close the browser first; files in use are skipped.",
        &[r"%LOCALAPPDATA%\Microsoft\Edge\User Data\*\Cache\*", r"%LOCALAPPDATA%\Microsoft\Edge\User Data\*\Code Cache\*", r"%LOCALAPPDATA%\Microsoft\Edge\User Data\*\GPUCache\*"],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "browsers.brave",
        "Brave cache",
        "Downloaded copies of web pages and images. Close the browser first; files in use are skipped.",
        &[r"%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\*\Cache\*", r"%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\*\Code Cache\*", r"%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\*\GPUCache\*"],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "browsers.firefox",
        "Firefox cache",
        "Downloaded copies of web pages and images. Close the browser first; files in use are skipped.",
        &[r"%LOCALAPPDATA%\Mozilla\Firefox\Profiles\*\cache2\*"],
    )
    .group(Group::Browsers)
    .preselected(),
    Category::new(
        "apps.electron_caches",
        "Desktop app caches",
        "Web caches of apps such as Discord, Slack, Teams and Spotify. Close them first.",
        &[r"%APPDATA%\discord\Cache\*", r"%APPDATA%\discord\Code Cache\*", r"%APPDATA%\Slack\Cache\*", r"%APPDATA%\Slack\Code Cache\*", r"%APPDATA%\Microsoft\Teams\Cache\*", r"%LOCALAPPDATA%\Spotify\Data\*", r"%APPDATA%\Code\Cache\*", r"%APPDATA%\Code\CachedData\*"],
    )
    .group(Group::Apps),
    Category::new(
        "developer.package_caches",
        "Package manager caches",
        "Downloaded packages kept by npm, Yarn, pnpm, pip, NuGet, Cargo and Gradle. They are downloaded again when a project needs them.",
        &[r"%LOCALAPPDATA%\npm-cache\*", r"%LOCALAPPDATA%\Yarn\Cache\*", r"%LOCALAPPDATA%\pnpm\store\*", r"%LOCALAPPDATA%\pip\Cache\*", r"%LOCALAPPDATA%\NuGet\v3-cache\*", r"%USERPROFILE%\.nuget\packages\*", r"%USERPROFILE%\.cargo\registry\cache\*", r"%USERPROFILE%\.gradle\caches\*"],
    )
    .group(Group::Developer)
    .risk(Risk::Moderate),
    Category {
        id: "personal.old_installers",
        name: "Old installers in Downloads",
        description: "Setup programs (.exe, .msi, .msix) in your Downloads folder that are more than 30 days old. They go to the recycle bin.",
        group: Group::Personal,
        risk: Risk::Review,
        needs_admin: false,
        selected_by_default: false,
        min_age_days: 30,
        disposal: Disposal::RecycleBin,
        source: Source::Patterns(&[r"%USERPROFILE%\Downloads\*"]),
        extensions: &["exe", "msi", "msix", "msixbundle", "appx"],
    },
    Category {
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
    },
];

pub fn category(id: &str) -> Option<&'static Category> {
    CATEGORIES.iter().find(|c| c.id == id)
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
        Filter {
            modified_before,
            extensions: self.extensions,
        }
    }

    /// The existing entries the patterns match on this computer.
    fn targets(&self) -> Vec<PathBuf> {
        let Source::Patterns(patterns) = self.source else {
            return Vec::new();
        };
        let options = glob::MatchOptions {
            case_sensitive: false,
            require_literal_separator: true,
            require_literal_leading_dot: false,
        };
        patterns
            .iter()
            .filter_map(|p| paths::expand(p, glob::Pattern::escape))
            .filter_map(|p| glob::glob_with(&p, options).ok())
            .flat_map(|matches| matches.filter_map(|m| m.ok()))
            .filter(|p| !is_protected(p))
            .collect()
    }
}

/// Folders a pattern must never resolve to, whatever the environment says.
fn is_protected(path: &Path) -> bool {
    use rewindows_platform::paths::Known;
    if path.parent().is_none() || path.parent().and_then(Path::parent).is_none() {
        return true;
    }
    [
        Known::UserProfile,
        Known::RoamingAppData,
        Known::LocalAppData,
        Known::ProgramData,
        Known::ProgramFiles,
        Known::ProgramFilesX86,
        Known::Windows,
        Known::Temp,
    ]
    .iter()
    .filter_map(|k| k.path())
    .any(|k| same_path(&k, path))
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    norm(a) == norm(b)
}

/// Measures every category on this computer.
pub fn analyze(task: &Task) -> Vec<CategoryReport> {
    task.set_stage("Measuring junk files");
    let elevated = elevation::is_elevated();
    CATEGORIES
        .par_iter()
        .map(|category| measure(category, elevated, task))
        .collect()
}

fn measure(category: &'static Category, elevated: bool, task: &Task) -> CategoryReport {
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
            let targets = category.targets();
            report.present = !targets.is_empty();
            if category.needs_admin && !elevated {
                // Without rights the folders are unreadable; a partial number would mislead.
                report.blocked = true;
                return report;
            }
            let filter = category.filter();
            let tally = targets
                .par_iter()
                .map(|t| walk::measure(t, &filter, task))
                .reduce(Tally::default, |mut a, b| {
                    a.files += b.files;
                    a.bytes += b.bytes;
                    a.failed += b.failed;
                    a
                });
            report.files = tally.files;
            report.bytes = tally.bytes;
        }
    }
    report
}

/// Cleans the categories with the given ids. Categories that need administrator rights are
/// skipped, and reported as skipped, when the app is not elevated.
pub fn run(ids: &[String], task: &Task) -> Result<Vec<CategoryResult>> {
    let chosen: Vec<&'static Category> = ids
        .iter()
        .map(|id| category(id).ok_or_else(|| EngineError::UnknownCategory(id.clone())))
        .collect::<Result<_>>()?;
    let elevated = elevation::is_elevated();
    let mut results = Vec::with_capacity(chosen.len());
    for category in chosen {
        if task.is_cancelled() {
            results.push(skipped(category, "Cancelled"));
            continue;
        }
        task.set_stage(category.name);
        results.push(clean(category, elevated, task));
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

fn clean(category: &'static Category, elevated: bool, task: &Task) -> CategoryResult {
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
            let targets = category.targets();
            match category.disposal {
                Disposal::Delete => {
                    let tally = targets
                        .par_iter()
                        .map(|t| walk::delete(t, &filter, task))
                        .reduce(Tally::default, |mut a, b| {
                            a.files += b.files;
                            a.bytes += b.bytes;
                            a.failed += b.failed;
                            a
                        });
                    result.files = tally.files;
                    result.bytes = tally.bytes;
                    result.failed = tally.failed;
                }
                Disposal::RecycleBin => {
                    // Only top-level files that pass the filter; folders are left alone.
                    let files: Vec<(PathBuf, u64)> = targets
                        .into_iter()
                        .filter_map(|t| {
                            let m = walk::measure(&t, &filter, &Task::new());
                            (m.files == 1 && t.is_file()).then_some((t, m.bytes))
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
    fn protected_roots_are_refused() {
        assert!(is_protected(Path::new("/")));
        assert!(is_protected(Path::new("/home")));
    }
}
