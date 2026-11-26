//! Recommendations: what is worth doing on this computer right now, most important first.
//!
//! [`rules`] is a pure function of [`Facts`], so the advice can be tested without a computer;
//! [`recommend`] gathers the facts.

use crate::cleanup::CategoryReport;
use crate::{Task, cleanup, disks, duplicates, leftovers, startup, system, tweaks, walk};
use rewind_os_platform::Os;
use rewind_os_platform::paths::Known;
use serde::Serialize;
use std::path::PathBuf;

const GB: u64 = 1024 * 1024 * 1024;
const MB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Something is wrong or about to be (a full drive).
    Important,
    /// Clearly worth doing.
    Suggested,
    /// Worth knowing.
    Tip,
}

/// What the recommendation's button does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Open junk cleanup with these categories selected.
    Clean {
        categories: Vec<String>,
    },
    /// Scan this folder in the disk analyzer.
    Analyze {
        path: PathBuf,
    },
    /// Look for duplicate files in this folder.
    Duplicates {
        path: PathBuf,
    },
    /// Open program leftovers and search.
    Leftovers,
    ReviewStartup,
    Lighten,
    Hibernation,
    /// Open disk health.
    DiskHealth,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub id: &'static str,
    pub level: Level,
    /// English text with `{name}` placeholders filled from `args`; translate with
    /// `rewind_os_i18n::trf`.
    pub title: &'static str,
    pub detail: &'static str,
    pub args: Vec<(&'static str, String)>,
    /// Space it would free, when known.
    pub saving: Option<u64>,
    pub action: Action,
}

#[derive(Debug, Clone)]
pub struct DriveFact {
    pub mount: PathBuf,
    pub total: u64,
    pub available: u64,
    pub removable: bool,
}

/// What the rules look at.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub drives: Vec<DriveFact>,
    pub cleanup: Vec<CategoryReport>,
    /// Programs that start with the system and are on, when this system lists them.
    pub startup_enabled: Option<usize>,
    /// Recommended "lighter Windows" settings not applied yet, on Windows.
    pub tweaks_pending: Option<usize>,
    pub downloads: Option<(PathBuf, u64)>,
    /// Space that keeping one copy of each duplicate in Downloads would free.
    pub download_duplicates: Option<(PathBuf, u64)>,
    /// Folders of uninstalled programs nothing has changed in months: how many, and their size.
    pub leftovers: Option<(usize, u64)>,
    pub hibernation_file: Option<u64>,
    /// Drives that need attention or are failing: their name and health.
    pub disks: Vec<(String, disks::Health)>,
}

fn size(bytes: u64) -> String {
    crate::format::bytes(bytes)
}

fn category_bytes(facts: &Facts, id: &str) -> u64 {
    facts
        .cleanup
        .iter()
        .filter(|r| r.category.id == id && !r.blocked)
        .map(|r| r.bytes)
        .sum()
}

pub fn rules(facts: &Facts) -> Vec<Recommendation> {
    let mut out = Vec::new();

    for (name, health) in &facts.disks {
        let (id, title, detail) = match health {
            disks::Health::Failing => (
                "disk_failing",
                "{name} reports that it is failing",
                "Copy your important files to another drive or online storage now, and plan to replace it.",
            ),
            disks::Health::Warning => (
                "disk_warning",
                "{name} needs attention",
                "Back up your important files and look at what the drive reports.",
            ),
            disks::Health::Good | disks::Health::Unknown => continue,
        };
        out.push(Recommendation {
            id,
            level: Level::Important,
            title,
            detail,
            args: vec![("name", name.clone())],
            saving: None,
            action: Action::DiskHealth,
        });
    }

    let mut system_drive_tight = false;
    for d in facts
        .drives
        .iter()
        .filter(|d| !d.removable && d.total >= GB)
    {
        let free_share = d.available as f64 / d.total as f64;
        if free_share < 0.10 || (d.available < 5 * GB && free_share < 0.25) {
            system_drive_tight = true;
            out.push(Recommendation {
                id: "drive_full",
                level: Level::Important,
                title: "{drive} is almost full",
                detail: "With less than about 10% free, the system and apps slow down and updates can fail. See what takes the space.",
                args: vec![("drive", d.mount.display().to_string()), ("free", size(d.available))],
                saving: None,
                action: Action::Analyze { path: d.mount.clone() },
            });
        } else if free_share < 0.20 {
            system_drive_tight = true;
        }
    }

    let safe: Vec<&CategoryReport> = facts
        .cleanup
        .iter()
        .filter(|r| r.category.selected_by_default && !r.blocked && r.bytes > 0)
        .collect();
    let safe_total: u64 = safe.iter().map(|r| r.bytes).sum();
    if safe_total >= 300 * MB {
        out.push(Recommendation {
            id: "junk",
            level: Level::Suggested,
            title: "Clean {size} of junk files",
            detail: "Temporary files, caches and crash reports that are recreated when needed.",
            args: vec![("size", size(safe_total))],
            saving: Some(safe_total),
            action: Action::Clean {
                categories: safe.iter().map(|r| r.category.id.to_owned()).collect(),
            },
        });
    }

    let bin = category_bytes(facts, "personal.recycle_bin");
    if bin >= GB {
        out.push(Recommendation {
            id: "recycle_bin",
            level: Level::Suggested,
            title: "Empty the recycle bin ({size})",
            detail: "Files you deleted still take space until the recycle bin is emptied.",
            args: vec![("size", size(bin))],
            saving: Some(bin),
            action: Action::Clean {
                categories: vec!["personal.recycle_bin".to_owned()],
            },
        });
    }

    let installers = category_bytes(facts, "personal.old_installers");
    if installers >= 200 * MB {
        out.push(Recommendation {
            id: "old_installers",
            level: Level::Suggested,
            title: "Old installers in Downloads use {size}",
            detail: "Setup files you already used. They go to the recycle bin.",
            args: vec![("size", size(installers))],
            saving: Some(installers),
            action: Action::Clean {
                categories: vec!["personal.old_installers".to_owned()],
            },
        });
    }

    let dev = category_bytes(facts, "developer.package_caches");
    if dev >= 2 * GB {
        out.push(Recommendation {
            id: "developer_caches",
            level: Level::Tip,
            title: "Developer package caches use {size}",
            detail: "Packages are downloaded again when a project needs them.",
            args: vec![("size", size(dev))],
            saving: Some(dev),
            action: Action::Clean {
                categories: vec!["developer.package_caches".to_owned()],
            },
        });
    }

    if let Some((path, bytes)) = &facts.downloads
        && *bytes >= 5 * GB
    {
        out.push(Recommendation {
            id: "downloads",
            level: Level::Tip,
            title: "Your Downloads folder uses {size}",
            detail: "Look for large files you no longer need.",
            args: vec![("size", size(*bytes))],
            saving: None,
            action: Action::Analyze { path: path.clone() },
        });
    }

    if let Some((path, bytes)) = &facts.download_duplicates
        && *bytes >= 500 * MB
    {
        out.push(Recommendation {
            id: "duplicates",
            level: Level::Suggested,
            title: "Duplicate files in Downloads use {size}",
            detail: "The same file downloaded more than once. Keep one copy of each.",
            args: vec![("size", size(*bytes))],
            saving: Some(*bytes),
            action: Action::Duplicates { path: path.clone() },
        });
    }

    if let Some((n, bytes)) = facts.leftovers
        && n > 0
        && bytes >= 500 * MB
    {
        out.push(Recommendation {
            id: "leftovers",
            level: Level::Tip,
            title: "{n} folders of removed programs use {size}",
            detail: "Nothing installed refers to them and nothing in them changed for months. Check each before moving it to the recycle bin.",
            args: vec![("n", n.to_string()), ("size", size(bytes))],
            saving: Some(bytes),
            action: Action::Leftovers,
        });
    }

    if let Some(n) = facts.startup_enabled
        && n > 8
    {
        out.push(Recommendation {
            id: "startup",
            level: Level::Suggested,
            title: "{n} programs start with the system",
            detail: "Each one makes starting up slower. Turn off the ones you do not need right away.",
            args: vec![("n", n.to_string())],
            saving: None,
            action: Action::ReviewStartup,
        });
    }

    if let Some(n) = facts.tweaks_pending
        && n > 0
    {
        out.push(Recommendation {
            id: "lighten",
            level: Level::Suggested,
            title: "{n} settings can make Windows lighter",
            detail: "Ads, suggestions and data collection you can turn off. Every change can be undone.",
            args: vec![("n", n.to_string())],
            saving: None,
            action: Action::Lighten,
        });
    }

    if let Some(bytes) = facts.hibernation_file
        && bytes >= 4 * GB
        && system_drive_tight
    {
        out.push(Recommendation {
            id: "hibernation",
            level: Level::Tip,
            title: "The hibernation file uses {size}",
            detail: "If you never hibernate the computer, turning hibernation off frees this space.",
            args: vec![("size", size(bytes))],
            saving: Some(bytes),
            action: Action::Hibernation,
        });
    }

    out.sort_by(|a, b| a.level.cmp(&b.level).then(b.saving.cmp(&a.saving)));
    out
}

/// Looks at this computer and returns what is worth doing.
pub fn recommend(task: &Task) -> Vec<Recommendation> {
    task.set_stage("Looking for recommendations");
    let overview = system::overview();
    let tool_available = |id: &str| crate::tool(id).is_some_and(|t| t.available());
    let downloads = Known::Downloads.path();
    let facts = Facts {
        drives: overview
            .drives
            .iter()
            .map(|d| DriveFact {
                mount: d.mount.clone(),
                total: d.total,
                available: d.available,
                removable: d.removable,
            })
            .collect(),
        cleanup: cleanup::analyze(task),
        startup_enabled: tool_available("startup.list").then(|| {
            // Scheduled tasks mostly run later (updaters), and services are a different kind
            // of thing to turn off; count programs only.
            startup::list()
                .iter()
                .filter(|e| {
                    e.enabled
                        && !matches!(
                            e.source,
                            startup::Source::ScheduledTask | startup::Source::Service
                        )
                })
                .count()
        }),
        tweaks_pending: (Os::current() == Os::Windows).then(|| {
            tweaks::list()
                .iter()
                .filter(|s| s.tweak.recommended && s.state != tweaks::State::Applied)
                .count()
        }),
        downloads: downloads.clone().map(|p| {
            let bytes = walk::measure(&p, &walk::Filter::default(), task).bytes;
            (p, bytes)
        }),
        download_duplicates: downloads.and_then(|p| {
            duplicates::find(&p, duplicates::DEFAULT_MIN_BYTES, task)
                .ok()
                .map(|r| (p, r.reclaimable()))
        }),
        leftovers: tool_available("leftovers.find").then(|| {
            let likely: Vec<_> = leftovers::find(task)
                .into_iter()
                .filter(|l| l.confidence == leftovers::Confidence::High)
                .collect();
            (likely.len(), likely.iter().map(|l| l.bytes).sum())
        }),
        hibernation_file: overview
            .system_files
            .iter()
            .find(|f| f.kind == system::SystemFileKind::Hibernation)
            .map(|f| f.bytes),
        disks: tool_available("disk.health")
            .then(disks::report)
            .and_then(|r| r.ok())
            .map(|r| {
                r.disks
                    .into_iter()
                    .filter(|d| d.health >= disks::Health::Warning)
                    .map(|d| (d.name, d.health))
                    .collect()
            })
            .unwrap_or_default(),
    };
    rules(&facts)
}
