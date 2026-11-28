//! Recommendations: what is worth doing on this computer right now, most important first.
//!
//! [`rules`] is a pure function of [`Facts`], so the advice can be tested without a computer;
//! [`recommend`] gathers the facts.

use crate::cleanup::CategoryReport;
use crate::{
    Task, cleanup, disks, duplicates, leftovers, power, restore, startup, system, tweaks, vdisks,
    walk,
};
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
    /// Open the power plan (on the running programs page).
    PowerPlan,
    /// Open the virtual disks of WSL and Docker Desktop.
    VirtualDisks,
    /// Open the restore points (on the maintenance page).
    RestorePoints,
    /// Open Windows' Device Manager.
    DeviceManager,
    /// Open the battery's health (on the health page).
    Battery,
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

#[derive(Debug, Clone, Copy, Default)]
pub struct VirtualDiskFacts {
    /// What all of them take on Windows.
    pub total: u64,
    /// About what compacting would give back, from the disks whose use inside is known.
    pub reclaimable: Option<u64>,
    /// What the disks whose use inside is not known take.
    pub unknown: u64,
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
    /// The power plan (or Windows' power mode when plugged in) saves energy at the cost of
    /// speed.
    pub power_saving: bool,
    /// The virtual disks of WSL and Docker Desktop.
    pub virtual_disks: Option<VirtualDiskFacts>,
    /// Restore points all but the newest of which could go, and what all of them take; known
    /// only with administrator rights.
    pub restore_points: Option<(usize, u64)>,
    /// Devices with a driver problem: their name and what is wrong (English).
    pub device_problems: Vec<(String, &'static str)>,
    /// Worn batteries: what each still holds, as a percent of what it was made to hold.
    pub worn_batteries: Vec<u8>,
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

    // A device that does not work is worth knowing first, after failing drives.
    match facts.device_problems.as_slice() {
        [] => {}
        [(name, what)] => out.push(Recommendation {
            id: "device_problem",
            level: Level::Suggested,
            title: "{name} is not working properly",
            detail: what,
            args: vec![("name", name.clone())],
            saving: None,
            action: Action::DeviceManager,
        }),
        many => out.push(Recommendation {
            id: "device_problem",
            level: Level::Suggested,
            title: "{count} devices are not working properly",
            detail: "Device Manager marks them in yellow. The maker's driver or Windows Update usually fixes them.",
            args: vec![("count", many.len().to_string())],
            saving: None,
            action: Action::DeviceManager,
        }),
    }

    // A worn battery is worth knowing, though nothing on the computer fixes it.
    if let Some(&percent) = facts.worn_batteries.iter().min() {
        out.push(Recommendation {
            id: "battery_worn",
            level: Level::Tip,
            title: "The battery holds {percent}% of the charge it was made to hold",
            detail: "It lasts about that share of its first time on a charge. Only a new battery brings that back; settings and calibration do not.",
            args: vec![("percent", percent.to_string())],
            saving: None,
            action: Action::Battery,
        });
    }

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

    if let Some(v) = facts.virtual_disks {
        const DETAIL: &str = "Their disk files grow as Linux writes and never shrink by themselves. Compacting returns what Linux no longer uses; nothing inside is deleted.";
        if let Some(free) = v.reclaimable.filter(|f| *f >= 5 * GB) {
            out.push(Recommendation {
                id: "virtual_disks",
                level: Level::Suggested,
                title: "Compacting the disks of WSL and Docker gives back about {size}",
                detail: DETAIL,
                args: vec![("size", size(free))],
                saving: Some(free),
                action: Action::VirtualDisks,
            });
        } else if v.unknown >= 30 * GB {
            // What is used inside is not known (Docker, or a distribution that is not
            // running): a tip, with no promise of space.
            out.push(Recommendation {
                id: "virtual_disks",
                level: Level::Tip,
                title: "The disks of WSL and Docker use {size}",
                detail: DETAIL,
                args: vec![("size", size(v.total))],
                saving: None,
                action: Action::VirtualDisks,
            });
        }
    }

    if let Some((old, bytes)) = facts.restore_points
        && old > 0
        && bytes >= 5 * GB
    {
        out.push(Recommendation {
            id: "restore_points",
            level: if system_drive_tight {
                Level::Suggested
            } else {
                Level::Tip
            },
            title: "Restore points use {size}",
            detail: "Deleting all but the newest frees most of it; the newest one on each drive is kept.",
            args: vec![("size", size(bytes))],
            saving: None,
            action: Action::RestorePoints,
        });
    }

    if facts.power_saving {
        out.push(Recommendation {
            id: "power_saving",
            level: Level::Tip,
            title: "The power plan favours saving energy",
            detail: "The computer runs slower to save energy. When it is plugged in, a balanced plan is usually better.",
            args: Vec::new(),
            saving: None,
            action: Action::PowerPlan,
        });
    }

    out.sort_by(|a, b| a.level.cmp(&b.level).then(b.saving.cmp(&a.saving)));
    out
}

/// Looks at this computer and returns what is worth doing. The checks do not depend on each
/// other, so each runs on its own thread and the whole takes about as long as the slowest one.
pub fn recommend(task: &Task) -> Vec<Recommendation> {
    task.set_stage("Looking for recommendations");
    let tool_available = |id: &str| crate::tool(id).is_some_and(|t| t.available());
    let downloads = Known::Downloads.path();
    let facts = std::thread::scope(|s| {
        let cleanup = s.spawn(|| cleanup::analyze(task));
        let startup_enabled = s.spawn(|| {
            tool_available("startup.list").then(|| {
                // Scheduled tasks mostly run later (updaters), and services are a different
                // kind of thing to turn off; count programs only.
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
            })
        });
        let tweaks_pending = s.spawn(|| {
            (Os::current() == Os::Windows).then(|| {
                tweaks::list()
                    .iter()
                    .filter(|s| s.tweak.recommended && s.state != tweaks::State::Applied)
                    .count()
            })
        });
        let downloads_size = s.spawn(|| {
            downloads.clone().map(|p| {
                let bytes = walk::measure(&p, &walk::Filter::default(), task).bytes;
                (p, bytes)
            })
        });
        let download_duplicates = s.spawn(|| {
            downloads.clone().and_then(|p| {
                duplicates::find(&p, duplicates::DEFAULT_MIN_BYTES, task)
                    .ok()
                    .map(|r| (p, r.reclaimable()))
            })
        });
        let leftovers = s.spawn(|| {
            tool_available("leftovers.find").then(|| {
                let likely: Vec<_> = leftovers::find(task)
                    .into_iter()
                    .filter(|l| l.confidence == leftovers::Confidence::High)
                    .collect();
                (likely.len(), likely.iter().map(|l| l.bytes).sum())
            })
        });
        let power_saving = s.spawn(|| {
            tool_available("power.list")
                && power::report().is_ok_and(|r| {
                    r.active().is_some_and(|p| p.kind == power::Kind::Saver)
                        || r.mode_plugged_in == Some(power::Mode::BestEfficiency)
                })
        });
        let disks = s.spawn(|| {
            tool_available("disk.health")
                .then(disks::report)
                .and_then(|r| r.ok())
                .map(|r| {
                    r.disks
                        .into_iter()
                        .filter(|d| d.health >= disks::Health::Warning)
                        .map(|d| (d.name, d.health))
                        .collect()
                })
                .unwrap_or_default()
        });
        let virtual_disks = s.spawn(|| {
            tool_available("vdisk.list")
                .then(vdisks::list)
                .and_then(|r| r.ok())
                .filter(|r| !r.disks.is_empty())
                .map(|r| {
                    let known: Vec<u64> = r
                        .disks
                        .iter()
                        .filter_map(vdisks::VirtualDisk::reclaimable)
                        .collect();
                    VirtualDiskFacts {
                        total: r.disks.iter().map(|d| d.bytes).sum(),
                        reclaimable: (!known.is_empty()).then(|| known.iter().sum()),
                        unknown: r
                            .disks
                            .iter()
                            .filter(|d| d.used_inside.is_none())
                            .map(|d| d.bytes)
                            .sum(),
                    }
                })
        });
        // Only with administrator rights; without them Windows hides the restore points.
        let restore_points = s.spawn(|| {
            (tool_available("restore.list") && rewind_os_platform::elevation::is_elevated())
                .then(restore::list)
                .and_then(|r| r.ok())
                .map(|r| (restore::all_but_newest(&r.copies).len(), r.used()))
        });
        let worn_batteries = s.spawn(|| {
            tool_available("battery.health")
                .then(crate::battery::report)
                .and_then(|r| r.ok())
                .map(|list| {
                    list.iter()
                        .filter(|b| b.worn())
                        .filter_map(crate::battery::Battery::percent)
                        .collect()
                })
                .unwrap_or_default()
        });
        let device_problems = s.spawn(|| {
            tool_available("drivers.problems")
                .then(crate::drivers::problems)
                .and_then(|r| r.ok())
                .map(|list| list.iter().map(|p| (p.name.clone(), p.meaning())).collect())
                .unwrap_or_default()
        });
        // A check that failed on its own thread gives no recommendation, as when it finds
        // nothing.
        // The overview is quick; this thread reads it while the others work.
        let overview = system::overview();
        Facts {
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
            cleanup: cleanup.join().unwrap_or_default(),
            startup_enabled: startup_enabled.join().unwrap_or_default(),
            tweaks_pending: tweaks_pending.join().unwrap_or_default(),
            downloads: downloads_size.join().unwrap_or_default(),
            download_duplicates: download_duplicates.join().unwrap_or_default(),
            leftovers: leftovers.join().unwrap_or_default(),
            hibernation_file: overview
                .system_files
                .iter()
                .find(|f| f.kind == system::SystemFileKind::Hibernation)
                .map(|f| f.bytes),
            power_saving: power_saving.join().unwrap_or_default(),
            disks: disks.join().unwrap_or_default(),
            virtual_disks: virtual_disks.join().unwrap_or_default(),
            restore_points: restore_points.join().unwrap_or_default(),
            device_problems: device_problems.join().unwrap_or_default(),
            worn_batteries: worn_batteries.join().unwrap_or_default(),
        }
    });
    rules(&facts)
}
