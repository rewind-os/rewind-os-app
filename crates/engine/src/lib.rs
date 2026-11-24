//! The Rewind OS engine. Every user-visible action is a [`Request`] with a stable id listed in
//! [`TOOLS`]; the desktop app and the CLI both go through [`Engine::run`], so anything the GUI
//! can do the CLI can do too.

pub mod cleanup;
pub mod duplicates;
pub mod format;
pub mod icons;
pub mod leftovers;
pub mod maintenance;
pub mod packages;
pub mod programs;
pub mod recommend;
pub mod safety;
pub mod scan;
pub mod startup;
pub mod system;
pub mod task;
pub mod tweaks;
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
        "cleanup.analyze",
        "Measure temporary files, caches and other junk by category",
    )
    .admin(AdminNeed::Partly),
    ToolSpec::new("cleanup.run", "Delete the chosen junk categories")
        .admin(AdminNeed::Partly)
        .changes(),
    ToolSpec::new("programs.list", "List installed programs").only(WINDOWS_LINUX),
    ToolSpec::new("programs.uninstall", "Start a program's own uninstaller")
        .changes()
        .only(WINDOWS),
    ToolSpec::new(
        "leftovers.find",
        "Find folders that no installed program, service or startup entry refers to",
    )
    .only(WINDOWS),
    ToolSpec::new(
        "leftovers.registry",
        "Report registry keys of programs that are no longer installed (never removes them)",
    )
    .only(WINDOWS),
    ToolSpec::new("files.trash", "Move files or folders to the recycle bin")
        .admin(AdminNeed::Partly)
        .changes(),
    ToolSpec::new("files.reveal", "Show a file or folder in the file manager"),
    ToolSpec::new("startup.list", "List programs that start with the system").only(WINDOWS_LINUX),
    ToolSpec::new("startup.set", "Enable or disable a startup program")
        .admin(AdminNeed::Partly)
        .changes()
        .only(WINDOWS_LINUX),
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
    #[serde(rename = "tweaks.set")]
    TweakSet { id: String, applied: bool },
    #[serde(rename = "system.relaunch_elevated")]
    RelaunchElevated { args: Vec<String> },
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
    #[serde(rename = "files.reveal")]
    FilesReveal { path: PathBuf },
    #[serde(rename = "startup.list")]
    StartupList,
    #[serde(rename = "startup.set")]
    StartupSet { id: String, enabled: bool },
    #[serde(rename = "maintenance.analyze_component_store")]
    AnalyzeComponentStore,
    #[serde(rename = "maintenance.component_cleanup")]
    ComponentCleanup,
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
    Tweaks(Vec<tweaks::TweakStatus>),
    Scan(#[serde(serialize_with = "scan::serialize_summary")] Arc<scan::ScanTree>),
    Files(Vec<scan::FileEntry>),
    Duplicates(duplicates::DuplicateReport),
    CleanupAnalysis(Vec<cleanup::CategoryReport>),
    CleanupDone(Vec<cleanup::CategoryResult>),
    Programs(Vec<programs::InstalledProgram>),
    Leftovers(Vec<leftovers::Leftover>),
    RegistryLeftovers(Vec<leftovers::RegistryLeftover>),
    Trashed(Vec<TrashResult>),
    Startup(Vec<startup::StartupEntry>),
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
            Request::TweakSet { id, applied } => {
                tweaks::set(&id, applied)?;
                Ok(Response::Done(if applied {
                    "Setting applied"
                } else {
                    "Setting undone"
                }))
            }
            Request::RelaunchElevated { args } => {
                rewind_os_platform::elevation::relaunch_elevated(&args)?;
                Ok(Response::Done("An elevated copy was started"))
            }
            Request::DiskScan { root } => {
                scan::scan(&root, task).map(|t| Response::Scan(Arc::new(t)))
            }
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
                Ok(Response::Done("The program's uninstaller was started"))
            }
            Request::LeftoversFind => Ok(Response::Leftovers(leftovers::find(task))),
            Request::RegistryLeftovers => {
                Ok(Response::RegistryLeftovers(leftovers::find_registry(task)))
            }
            Request::FilesTrash { paths } => Ok(Response::Trashed(trash(&paths, task))),
            Request::FilesReveal { path } => {
                rewind_os_platform::shell::reveal(&path)?;
                Ok(Response::Done("Opened in the file manager"))
            }
            Request::StartupList => Ok(Response::Startup(startup::list())),
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
            Request::TweakSet { .. } => "tweaks.set",
            Request::RelaunchElevated { .. } => "system.relaunch_elevated",
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
            Request::FilesReveal { .. } => "files.reveal",
            Request::StartupList => "startup.list",
            Request::StartupSet { .. } => "startup.set",
            Request::AnalyzeComponentStore => "maintenance.analyze_component_store",
            Request::ComponentCleanup => "maintenance.component_cleanup",
            Request::Hibernation { .. } => "maintenance.hibernation",
            Request::JournalUsage => "maintenance.journal_usage",
            Request::JournalVacuum => "maintenance.journal_vacuum",
        }
    }
}

/// Moves paths to the recycle bin, measuring each first so the report says what moved.
fn trash(paths: &[PathBuf], task: &Task) -> Vec<TrashResult> {
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
