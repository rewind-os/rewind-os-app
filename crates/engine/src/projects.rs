//! Build folders of software projects: `node_modules`, Rust's `target`, Python virtual
//! environments and other build output. They often take gigabytes, and each project's own tool
//! makes them again (`npm install`, `cargo build`), so they are deleted, not moved to the
//! recycle bin.
//!
//! A folder counts only next to the file that proves what it is: `node_modules` beside a
//! `package.json` *and* its lockfile, `target` beside a `Cargo.toml` with Cargo's own cache tag
//! inside. A folder that only shares the name is never offered.
//!
//! The search never enters hidden folders, the apps' data folders (`AppData`, `~/Library`),
//! programs' folders or the operating system, so the `node_modules` an editor extension or an
//! installed app runs from is never offered. Removing checks all of that again, then goes
//! through the safety guard, so a path typed by hand gets no further than the search would.

use crate::cleanup::Risk;
use crate::walk::{self, Filter};
use crate::{EngineError, Result, Task, safety};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use rewind_os_platform::Os;
use rewind_os_platform::paths::Known;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// English reasons; translate with `rewind_os_i18n::tr`.
pub const NOT_RECOGNISED: &str =
    "This is not a build folder Rewind OS recognises, so nothing was deleted.";
pub const HIDDEN_OR_APP_DATA: &str = "This is inside a hidden folder, an app's data or a program's folder, where build folders belong to installed tools.";
pub const SYSTEM_FOLDER: &str =
    "Projects are not searched inside the operating system's or programs' folders.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    NodeModules,
    RustTarget,
    MavenTarget,
    GradleBuild,
    DotnetBuild,
    PythonEnv,
    WebCache,
    DartBuild,
    CmakeBuild,
}

impl Kind {
    /// English name; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Kind::NodeModules => "Node.js packages",
            Kind::RustTarget => "Rust build",
            Kind::MavenTarget => "Maven build",
            Kind::GradleBuild => "Gradle build",
            Kind::DotnetBuild => ".NET build",
            Kind::PythonEnv => "Python environment",
            Kind::WebCache => "Web framework cache",
            Kind::DartBuild => "Flutter build",
            Kind::CmakeBuild => "CMake build",
        }
    }

    /// What makes it again, as typed in the project's folder.
    pub fn rebuild(self) -> &'static str {
        match self {
            Kind::NodeModules => "npm install",
            Kind::RustTarget => "cargo build",
            Kind::MavenTarget => "mvn package",
            Kind::GradleBuild => "gradle build",
            Kind::DotnetBuild => "dotnet build",
            Kind::PythonEnv => "pip install -r requirements.txt",
            Kind::WebCache => "npm run build",
            Kind::DartBuild => "flutter pub get",
            Kind::CmakeBuild => "cmake --build",
        }
    }

    /// A Python environment may hold packages installed by hand that no file lists.
    pub fn risk(self) -> Risk {
        match self {
            Kind::PythonEnv => Risk::Moderate,
            _ => Risk::Safe,
        }
    }
}

const NODE_LOCKFILES: &[&str] = &[
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "bun.lock",
];
const PYTHON_PROJECT: &[&str] = &[
    "pyproject.toml",
    "requirements.txt",
    "Pipfile",
    "setup.py",
    "setup.cfg",
    "poetry.lock",
    "uv.lock",
];
const GRADLE_PROJECT: &[&str] = &[
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
];
const WEB_CACHES: &[&str] = &[
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".angular",
];
const DOTNET_PROJECT: &[&str] = &[".csproj", ".fsproj", ".vbproj"];

fn has(dir: &Path, name: &str) -> bool {
    dir.join(name).is_file()
}

fn has_any(dir: &Path, names: &[&str]) -> bool {
    names.iter().any(|n| has(dir, n))
}

fn has_extension(dir: &Path, endings: &[&str]) -> bool {
    fs::read_dir(dir).into_iter().flatten().flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_lowercase();
        endings.iter().any(|end| name.ends_with(end)) && e.path().is_file()
    })
}

/// What build folder `dir` is, judged by the files beside and inside it.
pub fn identify(dir: &Path) -> Option<Kind> {
    let meta = fs::symlink_metadata(dir).ok()?;
    if !meta.is_dir() {
        return None;
    }
    let name = dir.file_name()?.to_str()?;
    let project = dir.parent()?;
    let kind = match name {
        "node_modules" if has(project, "package.json") && has_any(project, NODE_LOCKFILES) => {
            Kind::NodeModules
        }
        "target" if has(project, "Cargo.toml") && has(dir, "CACHEDIR.TAG") => Kind::RustTarget,
        "target" if has(project, "pom.xml") => Kind::MavenTarget,
        "build" | ".gradle" if has_any(project, GRADLE_PROJECT) => Kind::GradleBuild,
        "build" | ".dart_tool" if has(project, "pubspec.yaml") => Kind::DartBuild,
        "bin" | "obj" if has_extension(project, DOTNET_PROJECT) => Kind::DotnetBuild,
        _ if WEB_CACHES.contains(&name) && has(project, "package.json") => Kind::WebCache,
        _ if (name == "build" || name.starts_with("cmake-build-"))
            && has(dir, "CMakeCache.txt") =>
        {
            Kind::CmakeBuild
        }
        _ if has(dir, "pyvenv.cfg") && has_any(project, PYTHON_PROJECT) => Kind::PythonEnv,
        _ => return None,
    };
    Some(kind)
}

/// Folders the search never enters, besides hidden ones, programs and the system: the apps'
/// data and settings. Taken as a parameter so tests can name their own.
#[derive(Debug, Clone, Default)]
pub struct Places {
    pub off_limits: Vec<PathBuf>,
}

impl Places {
    /// This computer's.
    pub fn current() -> Places {
        let mut off_limits: Vec<PathBuf> = [
            Known::LocalAppData,
            Known::RoamingAppData,
            Known::ProgramData,
            Known::ProgramFiles,
            Known::ProgramFilesX86,
            Known::Windows,
            Known::Cache,
            Known::Config,
            Known::Data,
        ]
        .iter()
        .filter_map(|k| k.path())
        .collect();
        if Os::current() == Os::MacOs
            && let Some(home) = Known::Home.path()
        {
            off_limits.push(home.join("Library"));
        }
        Places { off_limits }
    }
}

fn lower(path: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

/// Whether the search would enter `dir` (it is not hidden, an app's data, a program's folder
/// or the system).
fn enterable(dir: &Path, places: &Places) -> bool {
    let hidden = dir
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'));
    let low = lower(dir);
    !hidden
        && !places.off_limits.iter().any(|z| low.starts_with(lower(z)))
        && !safety::Guard::current().is_program_folder(dir)
        && safety::removal_refusal(dir) != Some(safety::SYSTEM)
}

/// A build folder found, with its size.
#[derive(Debug, Clone, Serialize)]
pub struct BuildFolder {
    pub path: PathBuf,
    pub project: PathBuf,
    pub kind: Kind,
    pub risk: Risk,
    pub bytes: u64,
    pub files: u64,
    /// When anything directly in the project last changed: a hint of when it was last used.
    pub last_used: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub root: PathBuf,
    pub folders: Vec<BuildFolder>,
    /// Folders that could not be listed.
    pub unreadable: u64,
    pub cancelled: bool,
}

impl Report {
    pub fn total(&self) -> u64 {
        self.folders.iter().map(|f| f.bytes).sum()
    }
}

/// Where the search starts unless told otherwise: the home folder.
pub fn default_root() -> Option<PathBuf> {
    Known::Home.path()
}

/// Finds the build folders of every project under `root`.
pub fn find(root: &Path, places: &Places, task: &Task) -> Result<Report> {
    let meta = fs::symlink_metadata(root).map_err(|source| EngineError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    if !meta.is_dir() {
        return Err(EngineError::NotFound(root.display().to_string()));
    }
    let guard = safety::Guard::current();
    let in_system =
        root.parent().is_some() && safety::removal_refusal(root) == Some(safety::SYSTEM);
    if in_system || guard.is_program_folder(root) {
        return Err(EngineError::Refused(SYSTEM_FOLDER));
    }
    task.set_stage("Looking for projects");
    let (found, unreadable) = search(root, places, task);
    task.set_stage("Measuring");
    let mut folders: Vec<BuildFolder> = found
        .into_par_iter()
        .map(|(path, kind)| {
            let tally = walk::measure(&path, &Filter::default(), task);
            let project = path.parent().map(Path::to_path_buf).unwrap_or_default();
            BuildFolder {
                last_used: last_used(&project),
                project,
                kind,
                risk: kind.risk(),
                bytes: tally.bytes,
                files: tally.files,
                path,
            }
        })
        .collect();
    folders.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    Ok(Report {
        root: root.to_path_buf(),
        folders,
        unreadable,
        cancelled: task.is_cancelled(),
    })
}

fn search(dir: &Path, places: &Places, task: &Task) -> (Vec<(PathBuf, Kind)>, u64) {
    if task.is_cancelled() {
        return (Vec::new(), 0);
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return (Vec::new(), 1);
    };
    let mut found = Vec::new();
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        // Links and junctions are never followed.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let path = entry.path();
        if let Some(kind) = identify(&path) {
            found.push((path, kind));
        } else if enterable(&path, places) {
            subdirs.push(path);
        }
    }
    let (more, unreadable): (Vec<_>, Vec<u64>) = subdirs
        .par_iter()
        .map(|sub| search(sub, places, task))
        .unzip();
    found.extend(more.into_iter().flatten());
    (found, unreadable.into_iter().sum())
}

/// The newest change directly inside the project, leaving its build folders out.
fn last_used(project: &Path) -> Option<DateTime<Utc>> {
    fs::read_dir(project)
        .ok()?
        .flatten()
        .filter(|e| identify(&e.path()).is_none())
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
        .map(DateTime::<Utc>::from)
}

/// What deleting one build folder did.
#[derive(Debug, Clone, Serialize)]
pub struct Cleaned {
    pub path: PathBuf,
    pub bytes: u64,
    pub files: u64,
    /// Files that could not be deleted (in use, read-only); they stay.
    pub failed: u64,
    /// Why nothing was deleted (English, translate with `rewind_os_i18n::tr_text`).
    pub refused: Option<&'static str>,
}

/// Why `path` may not be deleted as a build folder, if it may not.
pub fn refusal(path: &Path, places: &Places) -> Option<&'static str> {
    if identify(path).is_none() {
        return Some(NOT_RECOGNISED);
    }
    // Every folder above the project must be one the search would have entered.
    let inside_hidden = path
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .take_while(|a| a.parent().is_some())
        .any(|a| !enterable(a, places));
    if inside_hidden {
        return Some(HIDDEN_OR_APP_DATA);
    }
    safety::removal_refusal(path)
}

/// Deletes the chosen build folders, each checked again first.
pub fn clean(paths: &[PathBuf], places: &Places, task: &Task) -> Vec<Cleaned> {
    task.set_stage("Deleting");
    paths
        .iter()
        .map(|path| {
            if let Some(why) = refusal(path, places) {
                return Cleaned {
                    path: path.clone(),
                    bytes: 0,
                    files: 0,
                    failed: 0,
                    refused: Some(why),
                };
            }
            let tally = walk::delete(path, &Filter::default(), task);
            Cleaned {
                path: path.clone(),
                bytes: tally.bytes,
                files: tally.files,
                failed: tally.failed,
                refused: None,
            }
        })
        .collect()
}
