//! The ReWindows engine. Every user-visible action is a [`Request`] with a stable id listed in
//! [`TOOLS`]; the desktop app and the CLI both go through [`Engine::run`], so anything the GUI
//! can do the CLI can do too.

pub mod cleanup;
pub mod format;
pub mod leftovers;
pub mod maintenance;
pub mod programs;
pub mod scan;
pub mod startup;
pub mod system;
pub mod task;
mod walk;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

pub use rewindows_platform::PlatformError;
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

/// Description of one tool, for `rewindows-cli commands` and for documentation.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ToolSpec {
    pub id: &'static str,
    /// English text; translate with `rewindows_i18n::tr`.
    pub summary: &'static str,
    pub admin: AdminNeed,
    /// Deletes, moves or changes something on the computer.
    pub changes_system: bool,
}

pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        id: "system.overview",
        summary: "Show drives, memory and large system files",
        admin: AdminNeed::No,
        changes_system: false,
    },
    ToolSpec {
        id: "system.relaunch_elevated",
        summary: "Restart the app with administrator rights",
        admin: AdminNeed::No,
        changes_system: false,
    },
    ToolSpec {
        id: "disk.scan",
        summary: "Measure every folder under a path and find the largest files",
        admin: AdminNeed::Partly,
        changes_system: false,
    },
    ToolSpec {
        id: "disk.list_files",
        summary: "List the files directly inside a folder, largest first",
        admin: AdminNeed::No,
        changes_system: false,
    },
    ToolSpec {
        id: "cleanup.analyze",
        summary: "Measure temporary files, caches and other junk by category",
        admin: AdminNeed::Partly,
        changes_system: false,
    },
    ToolSpec {
        id: "cleanup.run",
        summary: "Delete the chosen junk categories",
        admin: AdminNeed::Partly,
        changes_system: true,
    },
    ToolSpec {
        id: "programs.list",
        summary: "List installed programs",
        admin: AdminNeed::No,
        changes_system: false,
    },
    ToolSpec {
        id: "programs.uninstall",
        summary: "Start a program's own uninstaller",
        admin: AdminNeed::No,
        changes_system: true,
    },
    ToolSpec {
        id: "leftovers.find",
        summary: "Find folders that no installed program, service or startup entry refers to",
        admin: AdminNeed::No,
        changes_system: false,
    },
    ToolSpec {
        id: "files.trash",
        summary: "Move files or folders to the recycle bin",
        admin: AdminNeed::Partly,
        changes_system: true,
    },
    ToolSpec {
        id: "files.reveal",
        summary: "Show a file or folder in the file manager",
        admin: AdminNeed::No,
        changes_system: false,
    },
    ToolSpec {
        id: "startup.list",
        summary: "List programs that start with Windows",
        admin: AdminNeed::No,
        changes_system: false,
    },
    ToolSpec {
        id: "startup.set",
        summary: "Enable or disable a startup program",
        admin: AdminNeed::Partly,
        changes_system: true,
    },
    ToolSpec {
        id: "maintenance.analyze_component_store",
        summary: "Report how much space the Windows component store really uses",
        admin: AdminNeed::Yes,
        changes_system: false,
    },
    ToolSpec {
        id: "maintenance.component_cleanup",
        summary: "Remove superseded Windows components",
        admin: AdminNeed::Yes,
        changes_system: true,
    },
    ToolSpec {
        id: "maintenance.hibernation",
        summary: "Turn hibernation (and its hiberfil.sys file) on or off",
        admin: AdminNeed::Yes,
        changes_system: true,
    },
];

/// One call into the engine. Serialized as `{"tool": "<id>", ...arguments}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool")]
pub enum Request {
    #[serde(rename = "system.overview")]
    SystemOverview,
    #[serde(rename = "system.relaunch_elevated")]
    RelaunchElevated { args: Vec<String> },
    #[serde(rename = "disk.scan")]
    DiskScan { root: PathBuf },
    #[serde(rename = "disk.list_files")]
    ListFiles { dir: PathBuf },
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
}

/// What a request produced.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Response {
    Overview(system::Overview),
    Scan(#[serde(serialize_with = "scan::serialize_summary")] Arc<scan::ScanTree>),
    Files(Vec<scan::FileEntry>),
    CleanupAnalysis(Vec<cleanup::CategoryReport>),
    CleanupDone(Vec<cleanup::CategoryResult>),
    Programs(Vec<programs::InstalledProgram>),
    Leftovers(Vec<leftovers::Leftover>),
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
        match request {
            Request::SystemOverview => Ok(Response::Overview(system::overview())),
            Request::RelaunchElevated { args } => {
                rewindows_platform::elevation::relaunch_elevated(&args)?;
                Ok(Response::Done("An elevated copy was started"))
            }
            Request::DiskScan { root } => {
                scan::scan(&root, task).map(|t| Response::Scan(Arc::new(t)))
            }
            Request::ListFiles { dir } => scan::list_files(&dir).map(Response::Files),
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
            Request::FilesTrash { paths } => Ok(Response::Trashed(trash(&paths, task))),
            Request::FilesReveal { path } => {
                rewindows_platform::shell::reveal(&path)?;
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
        }
    }
}

impl Request {
    /// The tool id, as listed in [`TOOLS`].
    pub fn id(&self) -> &'static str {
        match self {
            Request::SystemOverview => "system.overview",
            Request::RelaunchElevated { .. } => "system.relaunch_elevated",
            Request::DiskScan { .. } => "disk.scan",
            Request::ListFiles { .. } => "disk.list_files",
            Request::CleanupAnalyze => "cleanup.analyze",
            Request::CleanupRun { .. } => "cleanup.run",
            Request::ProgramsList => "programs.list",
            Request::ProgramUninstall { .. } => "programs.uninstall",
            Request::LeftoversFind => "leftovers.find",
            Request::FilesTrash { .. } => "files.trash",
            Request::FilesReveal { .. } => "files.reveal",
            Request::StartupList => "startup.list",
            Request::StartupSet { .. } => "startup.set",
            Request::AnalyzeComponentStore => "maintenance.analyze_component_store",
            Request::ComponentCleanup => "maintenance.component_cleanup",
            Request::Hibernation { .. } => "maintenance.hibernation",
        }
    }
}

/// Moves paths to the recycle bin, measuring each first so the report says what moved.
fn trash(paths: &[PathBuf], task: &Task) -> Vec<TrashResult> {
    task.set_stage("Moving to the recycle bin");
    paths
        .iter()
        .map(|path| {
            let bytes = walk::measure(path, &walk::Filter::default(), task).bytes;
            let outcome = rewindows_platform::shell::move_to_trash(std::slice::from_ref(path));
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
