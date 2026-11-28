//! The Rewind OS engine. Every user-visible action is a [`Request`] with a stable id listed in
//! [`TOOLS`]; the desktop app and the CLI both go through [`Engine::run`], so anything the GUI
//! can do the CLI can do too.

pub mod apps;
pub mod battery;
pub mod boot;
pub mod cleanup;
pub mod disks;
pub mod drivers;
pub mod duplicates;
pub mod elevated;
pub mod empty;
pub mod format;
pub mod history;
pub mod icons;
pub mod kernels;
pub mod leftovers;
pub mod macapps;
pub mod maintenance;
pub mod mft;
pub mod packages;
pub mod power;
pub mod processes;
pub mod programs;
pub mod projects;
pub mod recommend;
pub mod restore;
pub mod safety;
pub mod scan;
pub mod startup;
pub mod system;
pub mod task;
pub mod tweaks;
pub mod update;
pub mod vdisks;
mod walk;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

pub use rewind_os_platform::{Os, PlatformError};
pub use task::{Progress, Task};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Platform(#[from] PlatformError),
    #[error("administrator rights are required")]
    NeedsAdmin,
    #[error("cancelled")]
    Cancelled,
    #[error("unknown cleanup category: {0}")]
    UnknownCategory(String),
    #[error("not found: {0}")]
    NotFound(String),
    /// The safety guard refused; the text (English) says why.
    #[error("{0}")]
    Refused(&'static str),
    #[error("{tool} is not available on {os} yet")]
    Unsupported {
        tool: &'static str,
        os: &'static str,
    },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl EngineError {
    /// The error as English text with `{name}` placeholders and their values, for the front
    /// ends to translate with `rewind_os_i18n::trf`. `None` for errors that carry a system's
    /// own output, which is shown as is.
    pub fn message(&self) -> Option<(&'static str, Vec<(&'static str, String)>)> {
        Some(match self {
            EngineError::NeedsAdmin | EngineError::Platform(PlatformError::NeedsAdmin) => {
                ("Administrator rights are required.", Vec::new())
            }
            EngineError::Cancelled => ("Cancelled.", Vec::new()),
            EngineError::Refused(why) => (why, Vec::new()),
            EngineError::Unsupported { tool, os } => (
                "{tool} is not available on {os} yet.",
                vec![("tool", (*tool).to_owned()), ("os", (*os).to_owned())],
            ),
            EngineError::NotFound(what) => ("Not found: {what}", vec![("what", what.clone())]),
            EngineError::UnknownCategory(id) => {
                ("Unknown cleanup category: {id}", vec![("id", id.clone())])
            }
            _ => return None,
        })
    }
}

/// The answer to `files.reveal` when the path is gone and its nearest folder was opened.
pub const REVEALED_NEAREST: &str =
    "It is no longer there; opened the nearest folder that still exists";

pub type Result<T> = std::result::Result<T, EngineError>;

/// How much of a tool needs administrator rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminNeed {
    No,
    /// Some parts are skipped, and reported as skipped, without elevation.
    Partly,
    Yes,
}

/// Description of one tool, for `rewind-os-cli commands` and for documentation.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ToolSpec {
    pub id: &'static str,
    /// English text; translate with `rewind_os_i18n::tr`.
    pub summary: &'static str,
    pub admin: AdminNeed,
    /// Deletes, moves or changes something on the computer.
    pub changes_system: bool,
    /// The systems the tool works on. Elsewhere it answers [`EngineError::Unsupported`].
    #[serde(serialize_with = "serialize_platforms")]
    pub platforms: &'static [Os],
}

const EVERYWHERE: &[Os] = &[Os::Windows, Os::Linux, Os::MacOs];
const WINDOWS: &[Os] = &[Os::Windows];
const LINUX: &[Os] = &[Os::Linux];
const LINUX_MAC: &[Os] = &[Os::Linux, Os::MacOs];
const WINDOWS_LINUX: &[Os] = &[Os::Windows, Os::Linux];

impl ToolSpec {
    const fn new(id: &'static str, summary: &'static str) -> Self {
        Self {
            id,
            summary,
            admin: AdminNeed::No,
            changes_system: false,
            platforms: EVERYWHERE,
        }
    }

    const fn admin(mut self, admin: AdminNeed) -> Self {
        self.admin = admin;
        self
    }

    const fn changes(mut self) -> Self {
        self.changes_system = true;
        self
    }

    const fn only(mut self, platforms: &'static [Os]) -> Self {
        self.platforms = platforms;
        self
    }

    /// Whether the tool works on the system this program runs on.
    pub fn available(&self) -> bool {
        self.platforms.contains(&Os::current())
    }
}

fn serialize_platforms<S: serde::Serializer>(
    platforms: &&'static [Os],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.collect_seq(platforms.iter().map(|os| os.name()))
}

pub const TOOLS: &[ToolSpec] = &[
    ToolSpec::new(
        "system.overview",
        "Show drives, memory and large system files",
    ),
    ToolSpec::new(
        "system.recommendations",
        "Suggest what is worth doing on this computer",
    ),
    ToolSpec::new(
        "system.relaunch_elevated",
        "Restart the app with administrator rights",
    )
    .only(WINDOWS),
    ToolSpec::new(
        "system.run_elevated",
        "Run one cleanup or maintenance request with administrator rights (asks for the password)",
    )
    .admin(AdminNeed::Yes)
    .changes()
    .only(LINUX_MAC),
    ToolSpec::new(
        "disk.scan",
        "Measure every folder under a path and find the largest files",
    )
    .admin(AdminNeed::Partly),
    ToolSpec::new(
        "disk.list_files",
        "List the files directly inside a folder, largest first",
    ),
    ToolSpec::new(
        "disk.duplicates",
        "Find files with identical contents under a folder",
    ),
    ToolSpec::new(
        "dev.find",
        "Find build folders of software projects (node_modules, target, virtual environments) under a folder",
    ),
    ToolSpec::new(
        "dev.clean",
        "Delete chosen build folders of software projects; each project's own tool makes them again",
    )
    .changes(),
    ToolSpec::new(
        "vdisk.list",
        "List the virtual disks of WSL distributions and Docker Desktop, with what they take on Windows",
    )
    .only(WINDOWS),
    ToolSpec::new(
        "vdisk.compact",
        "Give Windows back the space a WSL or Docker virtual disk no longer uses (stops WSL; nothing inside is deleted)",
    )
    .admin(AdminNeed::Yes)
    .changes()
    .only(WINDOWS),
    ToolSpec::new(
        "update.check",
        "Check whether a newer version of Rewind OS is out (the app asks GitHub each time it starts unless that is turned off; nothing about this computer is sent)",
    ),
    ToolSpec::new(
        "update.auto",
        "Turn the check for a newer version at start on or off",
    ),
    ToolSpec::new(
        "update.install",
        "Download the newer version, check that the Rewind OS project signed it, and install it (the MSI or a portable copy)",
    )
    .changes()
    .only(WINDOWS_LINUX),
    ToolSpec::new(
        "update.notes",
        "Open the page of the newer version, with its notes and downloads, in the web browser",
    ),
    ToolSpec::new(
        "cleanup.analyze",
        "Measure temporary files, caches and other junk by category",
    )
    .admin(AdminNeed::Partly),
    ToolSpec::new("cleanup.run", "Delete the chosen junk categories")
        .admin(AdminNeed::Partly)
        .changes(),
    ToolSpec::new("programs.list", "List installed programs"),
    ToolSpec::new(
        "programs.uninstall",
        "Uninstall a program: its own uninstaller on Windows, the package manager on Linux, the Trash on macOS",
    )
    .changes(),
    ToolSpec::new(
        "leftovers.find",
        "Find folders that no installed program, service or startup entry refers to",
    ),
    ToolSpec::new(
        "leftovers.registry",
        "Report registry keys of programs that are no longer installed (never removes them)",
    )
    .only(WINDOWS),
    ToolSpec::new("files.trash", "Move files or folders to the recycle bin")
        .admin(AdminNeed::Partly)
        .changes(),
    ToolSpec::new(
        "files.trash_empty",
        "Move empty folders a disk scan found to the recycle bin, each only if it is still empty",
    )
    .changes(),
    ToolSpec::new("files.reveal", "Show a file or folder in the file manager"),
    ToolSpec::new(
        "processes.list",
        "Measure for a second how much of the processor and memory each running program uses",
    )
    .only(WINDOWS_LINUX),
    ToolSpec::new(
        "processes.close",
        "Close a running program (unsaved work in it can be lost; never the system's own programs or other accounts')",
    )
    .changes()
    .only(WINDOWS_LINUX),
    ToolSpec::new(
        "power.list",
        "Show the power plans and which one is in use (and Windows' power mode)",
    )
    .only(WINDOWS_LINUX),
    ToolSpec::new(
        "power.set",
        "Switch to another power plan (switch back to undo)",
    )
    .changes()
    .only(WINDOWS_LINUX),
    ToolSpec::new(
        "power.settings",
        "Open Windows' power settings, where the power mode is chosen",
    )
    .only(WINDOWS),
    ToolSpec::new("startup.list", "List programs that start with the system"),
    ToolSpec::new(
        "boot.report",
        "How long the computer takes to start, and what slowed it down (on Windows it needs administrator rights)",
    )
    .only(WINDOWS_LINUX),
    ToolSpec::new(
        "disk.health",
        "Whether the drives report problems: health, temperature, wear and errors (on Windows the figures need administrator rights)",
    )
    .admin(AdminNeed::Partly)
    .only(WINDOWS_LINUX),
    ToolSpec::new(
        "disk.optimize",
        "Optimize every drive now the way the system does on its own: TRIM on SSDs, defragmentation on hard drives on Windows (can take long)",
    )
    .admin(AdminNeed::Yes)
    .changes()
    .only(WINDOWS_LINUX),
    ToolSpec::new("startup.set", "Enable or disable a startup program")
        .admin(AdminNeed::Partly)
        .changes(),
    ToolSpec::new(
        "maintenance.analyze_component_store",
        "Report how much space the Windows component store really uses",
    )
    .admin(AdminNeed::Yes)
    .only(WINDOWS),
    ToolSpec::new(
        "maintenance.component_cleanup",
        "Remove superseded Windows components",
    )
    .admin(AdminNeed::Yes)
    .changes()
    .only(WINDOWS),
    ToolSpec::new(
        "drivers.list",
        "List old driver packages that a newer version replaced and no device uses, and the space they take",
    )
    .only(WINDOWS),
    ToolSpec::new(
        "drivers.problems",
        "List devices with a driver problem, as Device Manager marks them",
    )
    .only(WINDOWS),
    ToolSpec::new("drivers.device_manager", "Open Device Manager").only(WINDOWS),
    ToolSpec::new(
        "drivers.clean",
        "Remove old driver packages; the newest of each driver and every one a device uses stay",
    )
    .admin(AdminNeed::Yes)
    .changes()
    .only(WINDOWS),
    ToolSpec::new(
        "battery.health",
        "How much charge each battery still holds against what it was made to hold, and its charge cycles",
    ),
    ToolSpec::new(
        "kernels.list",
        "List the installed kernels, which are old and the space they take",
    )
    .only(LINUX),
    ToolSpec::new(
        "kernels.clean",
        "Remove old kernels with the package manager; the running one and the newest stay",
    )
    .changes()
    .only(LINUX),
    ToolSpec::new(
        "restore.list",
        "List restore points and the other shadow copies of each drive, and the space they take",
    )
    .admin(AdminNeed::Yes)
    .only(WINDOWS),
    ToolSpec::new(
        "restore.clean",
        "Delete every restore point and shadow copy except the newest on each drive, as Disk Cleanup does",
    )
    .admin(AdminNeed::Yes)
    .changes()
    .only(WINDOWS),
    ToolSpec::new(
        "maintenance.hibernation",
        "Turn hibernation (and its hiberfil.sys file) on or off",
    )
    .admin(AdminNeed::Yes)
    .changes()
    .only(WINDOWS),
    ToolSpec::new(
        "tweaks.list",
        "List settings that make Windows lighter, and whether they are on",
    )
    .only(WINDOWS),
    ToolSpec::new(
        "tweaks.set",
        "Apply or undo a setting that makes Windows lighter",
    )
    .admin(AdminNeed::Partly)
    .changes()
    .only(WINDOWS),
    ToolSpec::new(
        "apps.list",
        "List preinstalled Store apps that can be removed",
    )
    .only(WINDOWS),
    ToolSpec::new(
        "apps.remove",
        "Remove preinstalled Store apps for you (they can be installed again from the Store)",
    )
    .changes()
    .only(WINDOWS),
    ToolSpec::new(
        "maintenance.journal_usage",
        "Report how much space the system journal uses",
    )
    .only(LINUX),
    ToolSpec::new(
        "maintenance.journal_vacuum",
        "Delete system journal entries older than two weeks",
    )
    .admin(AdminNeed::Yes)
    .changes()
    .only(LINUX),
];

/// The tool with this id.
pub fn tool(id: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|t| t.id == id)
}

/// One call into the engine. Serialized as `{"tool": "<id>", ...arguments}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool")]
pub enum Request {
    #[serde(rename = "system.overview")]
    SystemOverview,
    #[serde(rename = "system.recommendations")]
    Recommendations,
    #[serde(rename = "tweaks.list")]
    TweaksList,
    #[serde(rename = "apps.list")]
    AppsList,
    #[serde(rename = "apps.remove")]
    AppsRemove { ids: Vec<String> },
    #[serde(rename = "tweaks.set")]
    TweakSet { id: String, applied: bool },
    #[serde(rename = "system.relaunch_elevated")]
    RelaunchElevated { args: Vec<String> },
    /// Runs `request` with administrator rights (Linux and macOS); see [`elevated::ALLOWED`].
    #[serde(rename = "system.run_elevated")]
    RunElevated { request: Box<Request> },
    #[serde(rename = "disk.scan")]
    DiskScan { root: PathBuf },
    #[serde(rename = "disk.list_files")]
    ListFiles { dir: PathBuf },
    /// `min_bytes` defaults to [`duplicates::DEFAULT_MIN_BYTES`].
    #[serde(rename = "disk.duplicates")]
    Duplicates {
        root: PathBuf,
        #[serde(default)]
        min_bytes: Option<u64>,
    },
    /// `root` defaults to the home folder.
    #[serde(rename = "dev.find")]
    DevFind {
        #[serde(default)]
        root: Option<PathBuf>,
    },
    #[serde(rename = "dev.clean")]
    DevClean { paths: Vec<PathBuf> },
    #[serde(rename = "vdisk.list")]
    VdiskList,
    /// `path` as `vdisk.list` gave it.
    #[serde(rename = "vdisk.compact")]
    VdiskCompact { path: PathBuf },
    /// `now` asks GitHub even when the check at start is off, and reports a failure.
    #[serde(rename = "update.check")]
    UpdateCheck {
        #[serde(default)]
        now: bool,
    },
    #[serde(rename = "update.auto")]
    UpdateAuto { enabled: bool },
    /// `restart` starts the new version afterwards; the caller should then exit.
    #[serde(rename = "update.install")]
    UpdateInstall {
        #[serde(default)]
        restart: bool,
    },
    #[serde(rename = "update.notes")]
    UpdateNotes,
    #[serde(rename = "cleanup.analyze")]
    CleanupAnalyze,
    #[serde(rename = "cleanup.run")]
    CleanupRun { categories: Vec<String> },
    #[serde(rename = "programs.list")]
    ProgramsList,
    #[serde(rename = "programs.uninstall")]
    ProgramUninstall { key: String },
    #[serde(rename = "leftovers.find")]
    LeftoversFind,
    #[serde(rename = "leftovers.registry")]
    RegistryLeftovers,
    #[serde(rename = "files.trash")]
    FilesTrash { paths: Vec<PathBuf> },
    /// Empty folders the scan found; each is checked again and left alone if no longer empty.
    #[serde(rename = "files.trash_empty")]
    EmptyFoldersTrash { paths: Vec<PathBuf> },
    #[serde(rename = "files.reveal")]
    FilesReveal { path: PathBuf },
    #[serde(rename = "boot.report")]
    BootReport,
    #[serde(rename = "disk.health")]
    DiskHealth,
    #[serde(rename = "disk.optimize")]
    DiskOptimize,
    #[serde(rename = "processes.list")]
    ProcessesList,
    /// `name` and `pids` as `processes.list` gave them; `force` ends it at once.
    #[serde(rename = "processes.close")]
    ProcessesClose {
        name: String,
        pids: Vec<u32>,
        #[serde(default)]
        force: bool,
    },
    #[serde(rename = "power.list")]
    PowerList,
    #[serde(rename = "power.set")]
    PowerSet { id: String },
    #[serde(rename = "power.settings")]
    PowerSettings,
    #[serde(rename = "startup.list")]
    StartupList,
    #[serde(rename = "startup.set")]
    StartupSet { id: String, enabled: bool },
    #[serde(rename = "maintenance.analyze_component_store")]
    AnalyzeComponentStore,
    #[serde(rename = "maintenance.component_cleanup")]
    ComponentCleanup,
    #[serde(rename = "drivers.list")]
    DriversList,
    #[serde(rename = "drivers.problems")]
    DriverProblems,
    #[serde(rename = "drivers.device_manager")]
    DeviceManager,
    /// Published names (`oem12.inf`) as `drivers.list` gave them; empty means every old one.
    #[serde(rename = "drivers.clean")]
    DriversClean {
        #[serde(default)]
        packages: Vec<String>,
    },
    #[serde(rename = "battery.health")]
    BatteryHealth,
    #[serde(rename = "kernels.list")]
    KernelsList,
    /// Releases (`6.8.0-45-generic`) as `kernels.list` gave them; empty means every old one.
    #[serde(rename = "kernels.clean")]
    KernelsClean {
        #[serde(default)]
        releases: Vec<String>,
    },
    #[serde(rename = "restore.list")]
    RestoreList,
    #[serde(rename = "restore.clean")]
    RestoreClean,
    #[serde(rename = "maintenance.hibernation")]
    Hibernation { enabled: bool },
    #[serde(rename = "maintenance.journal_usage")]
    JournalUsage,
    #[serde(rename = "maintenance.journal_vacuum")]
    JournalVacuum,
}

/// What a request produced.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Response {
    Overview(system::Overview),
    Recommendations(Vec<recommend::Recommendation>),
    /// What a request run with administrator rights answered, as `rewind-os-cli --json` prints.
    Elevated(serde_json::Value),
    Tweaks(Vec<tweaks::TweakStatus>),
    Apps(Vec<apps::StoreApp>),
    AppsRemoved(Vec<apps::Removal>),
    Scan(#[serde(serialize_with = "scan::serialize_summary")] Arc<scan::ScanTree>),
    Files(Vec<scan::FileEntry>),
    Duplicates(duplicates::DuplicateReport),
    BuildFolders(projects::Report),
    BuildFoldersCleaned(Vec<projects::Cleaned>),
    VirtualDisks(vdisks::Report),
    Compacted(vdisks::Compacted),
    Drivers(drivers::Report),
    DeviceProblems(Vec<drivers::DeviceProblem>),
    DriversRemoved(drivers::Removed),
    Batteries(Vec<battery::Battery>),
    Kernels(kernels::Report),
    KernelsRemoved(kernels::Removed),
    RestorePoints(restore::Report),
    RestoreCleaned(restore::Cleaned),
    Update(update::Status),
    UpdateInstalled(update::Installed),
    CleanupAnalysis(Vec<cleanup::CategoryReport>),
    CleanupDone(Vec<cleanup::CategoryResult>),
    Programs(Vec<programs::InstalledProgram>),
    Leftovers(Vec<leftovers::Leftover>),
    RegistryLeftovers(Vec<leftovers::RegistryLeftover>),
    Trashed(Vec<TrashResult>),
    Startup(startup::Report),
    Processes(processes::Report),
    Power(power::Report),
    ProcessClosed(processes::Closed),
    Boot(boot::Report),
    DiskHealth(disks::Report),
    /// Output of an external Windows tool, shown as is.
    ToolOutput(String),
    /// Finished; the message (English key) says what happened.
    Done(&'static str),
}

#[derive(Debug, Clone, Serialize)]
pub struct TrashResult {
    pub path: PathBuf,
    pub bytes: u64,
    pub error: Option<String>,
}

/// Entry point for every front end.
#[derive(Debug, Default)]
pub struct Engine;

impl Engine {
    pub fn new() -> Self {
        Self
    }

    /// Runs `request`. Long requests report through `task` and stop early when it is cancelled.
    pub fn run(&self, request: Request, task: &Task) -> Result<Response> {
        let id = request.id();
        if !tool(id).is_some_and(ToolSpec::available) {
            return Err(EngineError::Unsupported {
                tool: id,
                os: Os::current().name(),
            });
        }
        match request {
            Request::SystemOverview => Ok(Response::Overview(system::overview())),
            Request::Recommendations => Ok(Response::Recommendations(recommend::recommend(task))),
            Request::TweaksList => Ok(Response::Tweaks(tweaks::list())),
            Request::AppsList => Ok(Response::Apps(apps::list())),
            Request::AppsRemove { ids } => apps::remove(&ids).map(Response::AppsRemoved),
            Request::TweakSet { id, applied } => {
                tweaks::set(&id, applied)?;
                Ok(Response::Done(if applied {
                    "Setting applied"
                } else {
                    "Setting undone"
                }))
            }
            Request::RunElevated { request } => elevated::run(&request).map(Response::Elevated),
            Request::RelaunchElevated { args } => {
                rewind_os_platform::elevation::relaunch_elevated(&args)?;
                Ok(Response::Done("An elevated copy was started"))
            }
            Request::DiskScan { root } => scan::scan(&root, task).map(|mut tree| {
                if !tree.cancelled {
                    task.set_stage("Looking for empty folders");
                    tree.empty_folders = empty::find(&tree);
                }
                if let Some(dir) = history::folder() {
                    tree.history = history::record(&tree, &dir, chrono::Utc::now());
                }
                Response::Scan(Arc::new(tree))
            }),
            Request::ListFiles { dir } => scan::list_files(&dir).map(Response::Files),
            Request::Duplicates { root, min_bytes } => duplicates::find(
                &root,
                min_bytes.unwrap_or(duplicates::DEFAULT_MIN_BYTES),
                task,
            )
            .map(Response::Duplicates),
            Request::CleanupAnalyze => Ok(Response::CleanupAnalysis(cleanup::analyze(task))),
            Request::CleanupRun { categories } => {
                cleanup::run(&categories, task).map(Response::CleanupDone)
            }
            Request::ProgramsList => Ok(Response::Programs(programs::installed())),
            Request::ProgramUninstall { key } => {
                programs::start_uninstaller(&key)?;
                Ok(Response::Done(match Os::current() {
                    Os::Windows => "The program's uninstaller was started",
                    Os::Linux => "The program was uninstalled",
                    Os::MacOs => "The app was moved to the Trash",
                }))
            }
            Request::LeftoversFind => Ok(Response::Leftovers(leftovers::find(task))),
            Request::RegistryLeftovers => {
                Ok(Response::RegistryLeftovers(leftovers::find_registry(task)))
            }
            Request::FilesTrash { paths } => Ok(Response::Trashed(trash(&paths, task))),
            Request::EmptyFoldersTrash { paths } => {
                Ok(Response::Trashed(empty::trash(&paths, task)))
            }
            Request::DevFind { root } => {
                let root = root
                    .or_else(projects::default_root)
                    .ok_or_else(|| EngineError::NotFound("home folder".to_owned()))?;
                projects::find(&root, &projects::Places::current(), task)
                    .map(Response::BuildFolders)
            }
            Request::DevClean { paths } => Ok(Response::BuildFoldersCleaned(projects::clean(
                &paths,
                &projects::Places::current(),
                task,
            ))),
            Request::VdiskList => vdisks::list().map(Response::VirtualDisks),
            Request::VdiskCompact { path } => vdisks::compact(&path, task).map(Response::Compacted),
            Request::UpdateCheck { now } => update::check(now).map(Response::Update),
            Request::UpdateAuto { enabled } => {
                update::set_auto(enabled)?;
                Ok(Response::Done(if enabled {
                    "Rewind OS will check for a new version each time it starts"
                } else {
                    "Rewind OS will not check for new versions by itself"
                }))
            }
            Request::UpdateInstall { restart } => {
                update::install(restart, task).map(Response::UpdateInstalled)
            }
            Request::UpdateNotes => {
                update::open_notes()?;
                Ok(Response::Done("Opened in the web browser"))
            }
            Request::FilesReveal { path } => {
                use rewind_os_platform::shell::{Revealed, reveal};
                Ok(Response::Done(match reveal(&path)? {
                    Revealed::Exact => "Opened in the file manager",
                    Revealed::Nearest(_) => REVEALED_NEAREST,
                }))
            }
            Request::StartupList => Ok(Response::Startup(startup::report())),
            Request::ProcessesList => processes::list(task).map(Response::Processes),
            Request::PowerList => power::report().map(Response::Power),
            Request::PowerSet { id } => {
                power::set(&id)?;
                Ok(Response::Done("Power plan changed"))
            }
            Request::PowerSettings => {
                power::open_settings()?;
                Ok(Response::Done("Opened the power settings"))
            }
            Request::ProcessesClose { name, pids, force } => {
                processes::close(&name, &pids, force, task).map(Response::ProcessClosed)
            }
            Request::BootReport => boot::report().map(Response::Boot),
            Request::DiskHealth => disks::report().map(Response::DiskHealth),
            Request::DiskOptimize => disks::optimize().map(Response::ToolOutput),
            Request::StartupSet { id, enabled } => {
                startup::set_enabled(&id, enabled)?;
                Ok(Response::Done(if enabled {
                    "Startup program enabled"
                } else {
                    "Startup program disabled"
                }))
            }
            Request::AnalyzeComponentStore => {
                maintenance::analyze_component_store().map(Response::ToolOutput)
            }
            Request::ComponentCleanup => maintenance::component_cleanup().map(Response::ToolOutput),
            Request::DriversList => drivers::list(task).map(Response::Drivers),
            Request::DriverProblems => drivers::problems().map(Response::DeviceProblems),
            Request::DeviceManager => {
                drivers::open_device_manager()?;
                Ok(Response::Done("Opened Device Manager"))
            }
            Request::DriversClean { packages } => {
                drivers::remove(&packages, task).map(Response::DriversRemoved)
            }
            Request::BatteryHealth => battery::report().map(Response::Batteries),
            Request::KernelsList => kernels::list(task).map(Response::Kernels),
            Request::KernelsClean { releases } => {
                kernels::remove(&releases, task).map(Response::KernelsRemoved)
            }
            Request::RestoreList => restore::list().map(Response::RestorePoints),
            Request::RestoreClean => restore::clean(task).map(Response::RestoreCleaned),
            Request::Hibernation { enabled } => {
                maintenance::set_hibernation(enabled)?;
                Ok(Response::Done(if enabled {
                    "Hibernation turned on"
                } else {
                    "Hibernation turned off; hiberfil.sys was removed"
                }))
            }
            Request::JournalUsage => maintenance::journal_usage().map(Response::ToolOutput),
            Request::JournalVacuum => maintenance::journal_vacuum().map(Response::ToolOutput),
        }
    }
}

impl Request {
    /// The tool id, as listed in [`TOOLS`].
    pub fn id(&self) -> &'static str {
        match self {
            Request::SystemOverview => "system.overview",
            Request::Recommendations => "system.recommendations",
            Request::TweaksList => "tweaks.list",
            Request::AppsList => "apps.list",
            Request::AppsRemove { .. } => "apps.remove",
            Request::TweakSet { .. } => "tweaks.set",
            Request::RelaunchElevated { .. } => "system.relaunch_elevated",
            Request::RunElevated { .. } => "system.run_elevated",
            Request::DiskScan { .. } => "disk.scan",
            Request::ListFiles { .. } => "disk.list_files",
            Request::Duplicates { .. } => "disk.duplicates",
            Request::CleanupAnalyze => "cleanup.analyze",
            Request::CleanupRun { .. } => "cleanup.run",
            Request::ProgramsList => "programs.list",
            Request::ProgramUninstall { .. } => "programs.uninstall",
            Request::LeftoversFind => "leftovers.find",
            Request::RegistryLeftovers => "leftovers.registry",
            Request::FilesTrash { .. } => "files.trash",
            Request::EmptyFoldersTrash { .. } => "files.trash_empty",
            Request::DevFind { .. } => "dev.find",
            Request::DevClean { .. } => "dev.clean",
            Request::VdiskList => "vdisk.list",
            Request::VdiskCompact { .. } => "vdisk.compact",
            Request::UpdateCheck { .. } => "update.check",
            Request::UpdateAuto { .. } => "update.auto",
            Request::UpdateInstall { .. } => "update.install",
            Request::UpdateNotes => "update.notes",
            Request::FilesReveal { .. } => "files.reveal",
            Request::StartupList => "startup.list",
            Request::ProcessesList => "processes.list",
            Request::PowerList => "power.list",
            Request::PowerSet { .. } => "power.set",
            Request::PowerSettings => "power.settings",
            Request::ProcessesClose { .. } => "processes.close",
            Request::BootReport => "boot.report",
            Request::DiskHealth => "disk.health",
            Request::DiskOptimize => "disk.optimize",
            Request::StartupSet { .. } => "startup.set",
            Request::AnalyzeComponentStore => "maintenance.analyze_component_store",
            Request::ComponentCleanup => "maintenance.component_cleanup",
            Request::DriversList => "drivers.list",
            Request::DriverProblems => "drivers.problems",
            Request::DeviceManager => "drivers.device_manager",
            Request::DriversClean { .. } => "drivers.clean",
            Request::BatteryHealth => "battery.health",
            Request::KernelsList => "kernels.list",
            Request::KernelsClean { .. } => "kernels.clean",
            Request::RestoreList => "restore.list",
            Request::RestoreClean => "restore.clean",
            Request::Hibernation { .. } => "maintenance.hibernation",
            Request::JournalUsage => "maintenance.journal_usage",
            Request::JournalVacuum => "maintenance.journal_vacuum",
        }
    }
}

/// Moves paths to the recycle bin, measuring each first so the report says what moved.
pub(crate) fn trash(paths: &[PathBuf], task: &Task) -> Vec<TrashResult> {
    task.set_stage("Moving to the recycle bin");
    paths
        .iter()
        .map(|path| {
            if let Some(reason) = safety::removal_refusal(path) {
                return TrashResult {
                    path: path.clone(),
                    bytes: 0,
                    error: Some(reason.to_owned()),
                };
            }
            let bytes = walk::measure(path, &walk::Filter::default(), task).bytes;
            let outcome = rewind_os_platform::shell::move_to_trash(std::slice::from_ref(path));
            let error = outcome.into_iter().next().and_then(|o| o.error);
            task.add(1, if error.is_none() { bytes } else { 0 });
            TrashResult {
                path: path.clone(),
                bytes,
                error,
            }
        })
        .collect()
}
