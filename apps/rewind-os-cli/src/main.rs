//! `rewind-os-cli`: every engine tool from the command line. Destructive tools ask for
//! `--yes`; `--json` prints the engine's response for scripts and agents.

mod mcp;

use clap::{Parser, Subcommand};
use rewind_os_engine::format::{bytes, count, local_date, signed_bytes};
use rewind_os_engine::{Engine, Request, Response, TOOLS, Task, cleanup};
use rewind_os_i18n::{tr, trf};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "rewind-os-cli",
    version,
    about = "Disk analysis and cleanup for Windows"
)]
struct Cli {
    /// Print the engine's response as JSON.
    #[arg(long, global = true)]
    json: bool,
    /// Language for messages (for example `es` or `en`).
    #[arg(long, global = true)]
    lang: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List every tool with its id.
    Commands,
    /// Drives, memory and large system files.
    Overview,
    /// What is worth doing on this computer.
    Recommend,
    /// Settings that make Windows lighter (every one can be undone).
    #[command(subcommand)]
    Tweaks(Tweaks),
    /// Serve the engine's tools to an assistant over the Model Context Protocol (stdin/stdout).
    /// Only tools that change nothing are offered unless the flags allow more.
    Mcp {
        /// A folder the assistant may scan and list (repeatable).
        #[arg(long = "automation-read-root")]
        read_roots: Vec<PathBuf>,
        /// A folder where the assistant may also move files to the recycle bin (repeatable).
        #[arg(long = "automation-write-root")]
        write_roots: Vec<PathBuf>,
        /// Also offer cleanup, startup, settings, apps, uninstall and maintenance tools.
        #[arg(long)]
        allow_system_changes: bool,
    },
    /// Preinstalled Store apps that can be removed (and installed again from the Store).
    #[command(subcommand)]
    Apps(Apps),
    /// Measure every folder under PATH.
    Scan {
        path: PathBuf,
        /// How many folders and files to print.
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// Files directly inside DIR, largest first.
    Files { dir: PathBuf },
    /// Files with identical contents under PATH.
    Duplicates {
        path: PathBuf,
        /// Smallest file to compare, in megabytes.
        #[arg(long, default_value_t = 1)]
        min_mb: u64,
        /// How many sets to print.
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// Build folders of software projects (node_modules, target, virtual environments).
    #[command(subcommand)]
    Dev(Dev),
    /// Junk cleanup.
    #[command(subcommand)]
    Clean(Clean),
    /// Installed programs.
    Programs,
    /// Start a program's own uninstaller (KEY from `programs --json`).
    Uninstall {
        key: String,
        #[arg(long)]
        yes: bool,
    },
    /// Folders no installed program refers to.
    Leftovers {
        /// Report registry keys instead of folders (never removes them).
        #[arg(long)]
        registry: bool,
    },
    /// Move files or folders to the recycle bin.
    Trash {
        paths: Vec<PathBuf>,
        #[arg(long)]
        yes: bool,
    },
    /// Programs that start with Windows.
    #[command(subcommand)]
    Startup(Startup),
    /// Windows maintenance tools (need administrator rights).
    #[command(subcommand)]
    Maintenance(Maintenance),
    /// Run a raw request, for example '{"tool":"cleanup.analyze"}'. Always prints JSON.
    Run { request: String },
}

#[derive(Subcommand)]
enum Dev {
    /// Find build folders under PATH (the home folder when left out).
    Find {
        path: Option<PathBuf>,
        /// How many folders to print.
        #[arg(long, default_value_t = 30)]
        top: usize,
    },
    /// Delete build folders (paths from `dev find`); each project's tool makes them again.
    Clean {
        paths: Vec<PathBuf>,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum Clean {
    /// Measure every category.
    Analyze,
    /// Delete the given categories (ids from `clean analyze`); `--defaults` picks the safe set.
    Run {
        categories: Vec<String>,
        #[arg(long)]
        defaults: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum Tweaks {
    List,
    /// Apply settings by id; `--recommended` applies every recommended one.
    Apply {
        ids: Vec<String>,
        #[arg(long)]
        recommended: bool,
        #[arg(long)]
        yes: bool,
    },
    /// Undo a setting, back to Windows' default.
    Undo {
        id: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum Apps {
    List,
    /// Remove apps by package name; `--recommended` removes every suggested one installed.
    Remove {
        ids: Vec<String>,
        #[arg(long)]
        recommended: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum Startup {
    List,
    Enable {
        id: String,
        #[arg(long)]
        yes: bool,
    },
    Disable {
        id: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum Maintenance {
    /// Real size of the Windows component store.
    Store,
    /// Remove superseded Windows components (takes minutes).
    Cleanup {
        #[arg(long)]
        yes: bool,
    },
    /// Turn hibernation on or off.
    Hibernation {
        #[arg(value_parser = ["on", "off"])]
        state: String,
        #[arg(long)]
        yes: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    rewind_os_i18n::init(cli.lang.as_deref());
    let mut json = cli.json;
    let top = match &cli.command {
        Command::Scan { top, .. }
        | Command::Duplicates { top, .. }
        | Command::Dev(Dev::Find { top, .. }) => *top,
        _ => 20,
    };
    let request = match build(cli.command, &mut json) {
        Ok(Some(request)) => request,
        Ok(None) => return ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let task = Arc::new(Task::new());
    let reporter = (!json && std::io::stderr().is_terminal()).then(|| spawn_reporter(&task));
    let result = Engine::new().run(request, &task);
    drop(reporter);
    match result {
        Ok(response) => {
            if json {
                match serde_json::to_string_pretty(&response) {
                    Ok(text) => println!("{text}"),
                    Err(err) => {
                        eprintln!("{err}");
                        return ExitCode::FAILURE;
                    }
                }
            } else {
                print_human(&response, top);
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            let text = match &err {
                rewind_os_engine::EngineError::Refused(why) => rewind_os_i18n::tr_text(why),
                other => other.to_string(),
            };
            eprintln!("{}: {text}", tr("Error"));
            ExitCode::FAILURE
        }
    }
}

/// Turns the command line into a request. `Ok(None)` means the command was handled here.
fn build(command: Command, json: &mut bool) -> Result<Option<Request>, String> {
    let confirm = |yes: bool| {
        if yes {
            Ok(())
        } else {
            Err(tr("This changes your system. Run it again with --yes to confirm.").to_owned())
        }
    };
    Ok(Some(match command {
        Command::Commands => {
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(TOOLS).map_err(|e| e.to_string())?
                );
            } else {
                for tool in TOOLS {
                    let mark = if tool.changes_system { "*" } else { " " };
                    let mut line = format!("{mark} {:<40} {}", tool.id, tr(tool.summary));
                    if !tool.available() {
                        line.push_str(&format!(" ({})", tr("not available on this system yet")));
                    }
                    println!("{line}");
                }
                println!("\n* {}", tr("changes your system"));
            }
            return Ok(None);
        }
        Command::Overview => Request::SystemOverview,
        Command::Recommend => Request::Recommendations,
        Command::Mcp {
            read_roots,
            write_roots,
            allow_system_changes,
        } => {
            let policy = mcp::Policy {
                read_roots,
                write_roots,
                allow_system_changes,
            };
            mcp::serve(&policy).map_err(|e| e.to_string())?;
            return Ok(None);
        }
        Command::Tweaks(Tweaks::List) => Request::TweaksList,
        Command::Apps(Apps::List) => Request::AppsList,
        Command::Apps(Apps::Remove {
            mut ids,
            recommended,
            yes,
        }) => {
            confirm(yes)?;
            if recommended
                && let Ok(Response::Apps(apps)) = rewind_os_engine::Engine::new()
                    .run(Request::AppsList, &rewind_os_engine::Task::new())
            {
                ids.extend(apps.into_iter().filter(|a| a.recommended).map(|a| a.id));
            }
            ids.sort();
            ids.dedup();
            if ids.is_empty() {
                return Err(tr("Nothing to remove.").to_owned());
            }
            Request::AppsRemove { ids }
        }
        Command::Tweaks(Tweaks::Apply {
            ids,
            recommended,
            yes,
        }) => {
            confirm(yes)?;
            let mut ids = ids;
            if recommended {
                ids.extend(
                    rewind_os_engine::tweaks::CATALOG
                        .iter()
                        .filter(|t| t.recommended)
                        .map(|t| t.id.to_owned()),
                );
            }
            if ids.is_empty() {
                return Err(tr("Name at least one setting, or pass --recommended.").to_owned());
            }
            return apply_tweaks(&ids, *json).map(|()| None);
        }
        Command::Tweaks(Tweaks::Undo { id, yes }) => {
            confirm(yes)?;
            Request::TweakSet { id, applied: false }
        }
        Command::Scan { path, .. } => Request::DiskScan { root: path },
        Command::Duplicates { path, min_mb, .. } => Request::Duplicates {
            root: path,
            min_bytes: Some(min_mb.saturating_mul(1024 * 1024)),
        },
        Command::Files { dir } => Request::ListFiles { dir },
        Command::Dev(Dev::Find { path, .. }) => Request::DevFind { root: path },
        Command::Dev(Dev::Clean { paths, yes }) => {
            confirm(yes)?;
            if paths.is_empty() {
                return Err(tr("Name at least one build folder.").to_owned());
            }
            Request::DevClean { paths }
        }
        Command::Clean(Clean::Analyze) => Request::CleanupAnalyze,
        Command::Clean(Clean::Run {
            mut categories,
            defaults,
            yes,
        }) => {
            confirm(yes)?;
            if defaults {
                categories.extend(
                    cleanup::categories()
                        .iter()
                        .filter(|c| c.selected_by_default)
                        .map(|c| c.id.to_owned()),
                );
            }
            if categories.is_empty() {
                return Err(tr("Name at least one category, or pass --defaults.").to_owned());
            }
            Request::CleanupRun { categories }
        }
        Command::Programs => Request::ProgramsList,
        Command::Uninstall { key, yes } => {
            confirm(yes)?;
            Request::ProgramUninstall { key }
        }
        Command::Leftovers { registry: false } => Request::LeftoversFind,
        Command::Leftovers { registry: true } => Request::RegistryLeftovers,
        Command::Trash { paths, yes } => {
            confirm(yes)?;
            Request::FilesTrash { paths }
        }
        Command::Startup(Startup::List) => Request::StartupList,
        Command::Startup(Startup::Enable { id, yes }) => {
            confirm(yes)?;
            Request::StartupSet { id, enabled: true }
        }
        Command::Startup(Startup::Disable { id, yes }) => {
            confirm(yes)?;
            Request::StartupSet { id, enabled: false }
        }
        Command::Maintenance(Maintenance::Store) => Request::AnalyzeComponentStore,
        Command::Maintenance(Maintenance::Cleanup { yes }) => {
            confirm(yes)?;
            Request::ComponentCleanup
        }
        Command::Maintenance(Maintenance::Hibernation { state, yes }) => {
            confirm(yes)?;
            Request::Hibernation {
                enabled: state == "on",
            }
        }
        Command::Run { request } => {
            *json = true;
            serde_json::from_str(&request).map_err(|e| format!("{}: {e}", tr("Invalid request")))?
        }
    }))
}

/// Prints progress to stderr until dropped.
struct Reporter {
    done: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Reporter {
    fn drop(&mut self) {
        self.done.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        eprint!("\r\x1b[2K");
    }
}

fn spawn_reporter(task: &Arc<Task>) -> Reporter {
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handle = {
        let task = Arc::clone(task);
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                let p = task.snapshot();
                if !p.stage.is_empty() {
                    eprint!(
                        "\r\x1b[2K{} · {} · {}",
                        tr(p.stage),
                        trf("{n} items", &[("n", &count(p.items))]),
                        bytes(p.bytes)
                    );
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
    };
    Reporter {
        done,
        handle: Some(handle),
    }
}

/// Applies several settings one by one and reports each, so one failure does not hide the rest.
fn apply_tweaks(ids: &[String], json: bool) -> Result<(), String> {
    let engine = Engine::new();
    let mut failed = false;
    for id in ids {
        let outcome = engine.run(
            Request::TweakSet {
                id: id.clone(),
                applied: true,
            },
            &Task::new(),
        );
        match outcome {
            Ok(_) if json => println!("{{\"id\":{id:?},\"ok\":true}}"),
            Ok(_) => println!("{} {id}", tr("Applied")),
            Err(err) => {
                failed = true;
                if json {
                    println!(
                        "{{\"id\":{id:?},\"ok\":false,\"error\":{:?}}}",
                        err.to_string()
                    );
                } else {
                    println!("{}: {id}: {err}", tr("Error"));
                }
            }
        }
    }
    if failed {
        Err(tr("Some settings could not be applied.").to_owned())
    } else {
        Ok(())
    }
}

fn print_human(response: &Response, top: usize) {
    match response {
        Response::Overview(o) => {
            println!("{}: {}", tr("System"), o.os);
            println!(
                "{}: {} ({} {})",
                tr("Processor"),
                o.cpu,
                o.cpu_threads,
                tr("threads")
            );
            println!(
                "{}: {} / {}",
                tr("Memory in use"),
                bytes(o.memory_used),
                bytes(o.memory_total)
            );
            println!(
                "{}: {}",
                tr("Administrator"),
                if o.elevated { tr("Yes") } else { tr("No") }
            );
            println!();
            for d in &o.drives {
                let pct = if d.total > 0 {
                    d.used() as f64 * 100.0 / d.total as f64
                } else {
                    0.0
                };
                println!(
                    "{:<6} {:>10} {} {:>10} ({pct:.0}%)  {} {}",
                    d.mount.display(),
                    bytes(d.used()),
                    tr("of"),
                    bytes(d.total),
                    d.file_system,
                    d.label
                );
            }
            for f in &o.system_files {
                println!("{:<24} {:>10}", f.path.display(), bytes(f.bytes));
            }
        }
        Response::Scan(tree) => {
            if let Some(root) = tree.root_node() {
                println!(
                    "{}: {} ({}) · {} · {} ms",
                    tree.root.display(),
                    bytes(root.bytes),
                    trf("{size} on disk", &[("size", &bytes(root.disk_bytes))]),
                    trf("{n} files", &[("n", &count(root.files))]),
                    tree.elapsed_ms
                );
            }
            if tree.from_file_table {
                println!("{}", tr("Read from the master file table"));
            }
            if tree.cancelled {
                println!("{}", tr("The scan was stopped; sizes are incomplete."));
            }
            if tree.unreadable_dirs > 0 {
                println!(
                    "{}",
                    trf(
                        "{n} folders could not be read (administrator rights may help).",
                        &[("n", &count(tree.unreadable_dirs))]
                    )
                );
            }
            if tree.hard_linked_bytes > 0 {
                println!(
                    "{}",
                    trf(
                        "{size} of hard links (one file under several names) was counted once.",
                        &[("size", &bytes(tree.hard_linked_bytes))]
                    )
                );
            }
            match &tree.history {
                Some(rewind_os_engine::history::Comparison::Changes {
                    since,
                    before,
                    after,
                    grew,
                    shrank,
                    ..
                }) => {
                    println!(
                        "{}",
                        trf(
                            "Since the scan of {date}: {before} then, {after} now.",
                            &[
                                ("date", &local_date(*since)),
                                ("before", &bytes(*before)),
                                ("after", &bytes(*after)),
                            ]
                        )
                    );
                    for (title, list) in
                        [(tr("Grew the most"), grew), (tr("Shrank the most"), shrank)]
                    {
                        if list.is_empty() {
                            continue;
                        }
                        println!("\n{title}");
                        for change in list {
                            println!(
                                "{:>12}  {}",
                                signed_bytes(change.delta()),
                                change.path.display()
                            );
                        }
                    }
                }
                Some(rewind_os_engine::history::Comparison::Nothing { reason }) => {
                    println!("{}", tr(reason));
                }
                None => {}
            }
            println!("\n{}", tr("Largest folders"));
            for (id, node) in tree
                .children(rewind_os_engine::scan::ScanTree::ROOT)
                .take(top)
            {
                println!("{:>10}  {}", bytes(node.bytes), tree.path(id).display());
            }
            println!("\n{}", tr("Largest files"));
            for f in tree.top_files.iter().take(top) {
                println!("{:>10}  {}", bytes(f.bytes), f.path.display());
            }
        }
        Response::Duplicates(report) => {
            println!(
                "{}",
                trf(
                    "{n} sets of identical files; keeping one copy of each frees {size}.",
                    &[
                        ("n", &count(report.sets.len() as u64)),
                        ("size", &bytes(report.reclaimable())),
                    ]
                )
            );
            if report.cancelled {
                println!(
                    "{}",
                    tr("The search was stopped; some duplicates may be missing.")
                );
            }
            if report.unreadable > 0 {
                println!(
                    "{}",
                    trf(
                        "{n} files or folders could not be read.",
                        &[("n", &count(report.unreadable))]
                    )
                );
            }
            if report.cloud_skipped > 0 {
                println!(
                    "{}",
                    trf(
                        "{n} files that are only in the cloud were not compared.",
                        &[("n", &count(report.cloud_skipped))]
                    )
                );
            }
            if report.program_folders_skipped > 0 {
                println!(
                    "{}",
                    trf(
                        "{n} program folders were not searched: each program needs its own copy of its files.",
                        &[("n", &count(report.program_folders_skipped))]
                    )
                );
            }
            for set in report.sets.iter().take(top) {
                println!(
                    "\n{} × {}",
                    bytes(set.bytes),
                    count(set.copies.len() as u64)
                );
                for copy in &set.copies {
                    println!("  {}", copy.path.display());
                }
            }
        }
        Response::Files(files) => {
            for f in files {
                println!("{:>10}  {}", bytes(f.bytes), f.path.display());
            }
        }
        Response::CleanupAnalysis(reports) => {
            let mut total = 0;
            for r in reports.iter().filter(|r| r.present || r.bytes > 0) {
                let state = if r.blocked {
                    tr("needs administrator").to_owned()
                } else if let Some(note) = &r.note {
                    note.clone()
                } else {
                    bytes(r.bytes)
                };
                total += r.bytes;
                let mark = if r.category.selected_by_default {
                    "*"
                } else {
                    " "
                };
                println!(
                    "{mark} {:<32} {:>18}  {}",
                    r.category.id,
                    state,
                    tr(r.category.name)
                );
            }
            println!("\n{}: {}", tr("Total"), bytes(total));
            println!("* {}", tr("included by --defaults"));
        }
        Response::CleanupDone(results) => {
            let mut freed = 0;
            for r in results {
                freed += r.bytes;
                let mut line = format!("{:<32} {:>10}", r.id, bytes(r.bytes));
                if r.failed > 0 {
                    line.push_str(&format!(
                        "  {}",
                        trf("{n} files in use were kept", &[("n", &count(r.failed))])
                    ));
                }
                if let Some(reason) = r.skipped {
                    line.push_str(&format!("  {}", tr(reason)));
                }
                if let Some(err) = &r.error {
                    line.push_str(&format!("  {}: {err}", tr("Error")));
                }
                println!("{line}");
            }
            println!("\n{}: {}", tr("Freed"), bytes(freed));
        }
        Response::Programs(programs) => {
            for p in programs {
                let size = p.estimated_bytes.map(bytes).unwrap_or_default();
                println!(
                    "{:<50} {:<18} {:>10}  {}",
                    p.name,
                    p.version.as_deref().unwrap_or(""),
                    size,
                    p.publisher.as_deref().unwrap_or("")
                );
            }
        }
        Response::Leftovers(found) => {
            if found.is_empty() {
                println!("{}", tr("No leftover folders found."));
            }
            for l in found {
                println!(
                    "{:>10}  {:<8} {}",
                    bytes(l.bytes),
                    format!("{:?}", l.confidence),
                    l.path.display()
                );
            }
        }
        Response::RegistryLeftovers(keys) => {
            if keys.is_empty() {
                println!("{}", tr("No leftover registry keys found."));
            }
            for k in keys {
                println!("{}", k.key);
            }
            if !keys.is_empty() {
                println!(
                    "\n{}",
                    tr(
                        "Report only: Rewind OS never removes registry keys. Export a key in Registry Editor before removing it by hand."
                    )
                );
            }
        }
        Response::Elevated(value) => match serde_json::to_string_pretty(value) {
            Ok(text) => println!("{text}"),
            Err(err) => eprintln!("{err}"),
        },
        Response::Apps(apps) => {
            if apps.is_empty() {
                println!("{}", tr("No preinstalled apps to remove."));
            }
            for a in apps {
                let mark = if a.recommended { "*" } else { " " };
                println!("{mark} {:<45} {}", a.id, tr(a.name));
            }
            if apps.iter().any(|a| a.recommended) {
                println!("\n* {}", tr("Suggested"));
            }
        }
        Response::AppsRemoved(results) => {
            for r in results {
                match &r.error {
                    None => println!("{}  {}", tr("Removed"), r.id),
                    Some(err) => println!("{}  {}: {err}", tr("Error"), r.id),
                }
            }
            println!(
                "{}",
                tr("Removed apps can be installed again from the Microsoft Store.")
            );
        }
        Response::Trashed(results) => {
            for r in results {
                match &r.error {
                    None => println!("{:>10}  {}", bytes(r.bytes), r.path.display()),
                    Some(err) => println!(
                        "{:>10}  {}  {}: {}",
                        "-",
                        r.path.display(),
                        tr("Error"),
                        rewind_os_i18n::tr_text(err)
                    ),
                }
            }
            if results.iter().any(|r| r.error.is_none()) {
                println!("{}", tr("Space is freed when the recycle bin is emptied."));
            }
        }
        Response::BuildFolders(report) => {
            println!(
                "{}",
                trf(
                    "{n} build folders, {size} in total.",
                    &[
                        ("n", &count(report.folders.len() as u64)),
                        ("size", &bytes(report.total())),
                    ]
                )
            );
            if report.cancelled {
                println!(
                    "{}",
                    tr("The search was stopped; some folders may be missing.")
                );
            }
            if report.unreadable > 0 {
                println!(
                    "{}",
                    trf(
                        "{n} folders could not be read.",
                        &[("n", &count(report.unreadable))]
                    )
                );
            }
            for f in report.folders.iter().take(top) {
                let used = f
                    .last_used
                    .map(rewind_os_engine::format::local_date)
                    .unwrap_or_default();
                println!(
                    "{:>10}  {:<20} {:<10}  {}",
                    bytes(f.bytes),
                    tr(f.kind.label()),
                    used,
                    f.path.display()
                );
            }
        }
        Response::BuildFoldersCleaned(results) => {
            for r in results {
                match r.refused {
                    Some(why) => println!(
                        "{:>10}  {}  {}",
                        "-",
                        r.path.display(),
                        rewind_os_i18n::tr_text(why)
                    ),
                    None if r.failed > 0 => println!(
                        "{:>10}  {}  {}",
                        bytes(r.bytes),
                        r.path.display(),
                        trf("{n} files in use were kept.", &[("n", &count(r.failed))])
                    ),
                    None => println!("{:>10}  {}", bytes(r.bytes), r.path.display()),
                }
            }
        }
        Response::Startup(entries) => {
            for e in entries {
                let state = if e.enabled { tr("On") } else { tr("Off") };
                println!("{:<4} {:<40} {}", state, e.id, e.command);
            }
        }
        Response::ToolOutput(text) => println!("{text}"),
        Response::Done(message) => println!("{}", tr(message)),
        Response::Recommendations(list) => {
            if list.is_empty() {
                println!("{}", tr("Nothing to do: this computer is in good shape."));
            }
            for r in list {
                let args: Vec<(&str, &str)> =
                    r.args.iter().map(|(k, v)| (*k, v.as_str())).collect();
                let level = match r.level {
                    rewind_os_engine::recommend::Level::Important => "!",
                    rewind_os_engine::recommend::Level::Suggested => "*",
                    rewind_os_engine::recommend::Level::Tip => "-",
                };
                println!("{level} {}", trf(r.title, &args));
                println!("  {}", tr(r.detail));
            }
        }
        Response::Tweaks(list) => {
            for s in list {
                let state = match s.state {
                    rewind_os_engine::tweaks::State::Applied => tr("Applied"),
                    rewind_os_engine::tweaks::State::Default => tr("Default"),
                    rewind_os_engine::tweaks::State::Partial => tr("Partly applied"),
                };
                let mark = if s.tweak.recommended { "*" } else { " " };
                println!(
                    "{mark} {:<36} {:<16} {}",
                    s.tweak.id,
                    state,
                    tr(s.tweak.name)
                );
            }
            println!("\n* {}", tr("recommended"));
        }
    }
}
