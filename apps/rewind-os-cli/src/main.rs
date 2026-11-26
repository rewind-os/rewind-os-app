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
    /// How long the computer takes to start, and what slowed it down.
    Boot,
    /// Running programs: what each uses of the processor and memory, and closing one.
    #[command(subcommand)]
    Processes(Processes),
    /// The power plan.
    #[command(subcommand)]
    Power(Power),
    /// Virtual disks of WSL and Docker Desktop.
    #[command(subcommand)]
    Vdisks(Vdisks),
    /// Drive health, and optimizing the drives.
    #[command(subcommand)]
    Disks(Disks),
    /// Windows maintenance tools (need administrator rights).
    #[command(subcommand)]
    Maintenance(Maintenance),
    /// New versions of Rewind OS.
    #[command(subcommand)]
    Update(Update),
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
enum Update {
    /// Ask GitHub now whether a newer version is out.
    Check,
    /// Let the desktop app check each time it starts (the default).
    On,
    /// Stop the desktop app from checking by itself.
    Off,
    /// Download the newer version, check its signature, and install it (MSI or portable copy).
    Install {
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
enum Processes {
    /// Measure for a second what each running program uses, heaviest first.
    List {
        /// How many programs to print.
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// Close a running program by the name `processes list` shows (unsaved work can be lost).
    Close {
        name: String,
        /// End it at once instead of asking it to close.
        #[arg(long)]
        force: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum Vdisks {
    /// The virtual disks and what they take on Windows.
    List,
    /// Give Windows back the space a disk no longer uses (stops WSL; needs administrator rights).
    Compact {
        /// The distribution's name, "Docker Desktop", or the disk's path from `vdisks list`.
        disk: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum Power {
    /// The power plans and which one is in use.
    List,
    /// Switch plan: its id from `power list`, or saver, balanced or performance.
    Set {
        plan: String,
        #[arg(long)]
        yes: bool,
    },
    /// Open Windows' power settings, where the power mode is chosen.
    Settings,
}

#[derive(Subcommand)]
enum Disks {
    /// Whether the drives report problems: health, temperature, wear and errors.
    Health,
    /// Optimize every drive now, as the system does on its own (TRIM or defragmentation).
    Optimize {
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
    rewind_os_engine::update::tidy();
    let mut json = cli.json;
    let top = match &cli.command {
        Command::Scan { top, .. }
        | Command::Duplicates { top, .. }
        | Command::Dev(Dev::Find { top, .. })
        | Command::Processes(Processes::List { top }) => *top,
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
            let text = match err.message() {
                Some((key, args)) => {
                    let args: Vec<(&str, &str)> =
                        args.iter().map(|(k, v)| (*k, v.as_str())).collect();
                    trf(key, &args)
                }
                None => err.to_string(),
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
        Command::Boot => Request::BootReport,
        Command::Processes(Processes::List { .. }) => Request::ProcessesList,
        Command::Processes(Processes::Close { name, force, yes }) => {
            confirm(yes)?;
            let listed = Engine::new()
                .run(Request::ProcessesList, &Task::new())
                .map_err(|e| e.to_string())?;
            let Response::Processes(report) = listed else {
                return Err(tr("No running program has that name.").to_owned());
            };
            // One the app may close if there is one; otherwise the engine says why not.
            let mut named: Vec<_> = report
                .programs
                .into_iter()
                .filter(|p| p.name.eq_ignore_ascii_case(&name))
                .collect();
            named.sort_by_key(|p| p.refusal.is_some());
            let program = named
                .into_iter()
                .next()
                .ok_or_else(|| tr("No running program has that name.").to_owned())?;
            Request::ProcessesClose {
                name: program.name,
                pids: program.pids,
                force,
            }
        }
        Command::Vdisks(Vdisks::List) => Request::VdiskList,
        Command::Vdisks(Vdisks::Compact { disk, yes }) => {
            confirm(yes)?;
            let listed = Engine::new()
                .run(Request::VdiskList, &Task::new())
                .map_err(|e| e.to_string())?;
            let Response::VirtualDisks(report) = listed else {
                return Err(tr("This is not one of the virtual disks Rewind OS found.").to_owned());
            };
            let found = report
                .disks
                .into_iter()
                .find(|d| {
                    d.name.eq_ignore_ascii_case(&disk)
                        || d.path.to_string_lossy().eq_ignore_ascii_case(&disk)
                })
                .ok_or_else(|| {
                    tr("This is not one of the virtual disks Rewind OS found.").to_owned()
                })?;
            Request::VdiskCompact { path: found.path }
        }
        Command::Power(Power::List) => Request::PowerList,
        Command::Power(Power::Settings) => Request::PowerSettings,
        Command::Power(Power::Set { plan, yes }) => {
            confirm(yes)?;
            let listed = Engine::new()
                .run(Request::PowerList, &Task::new())
                .map_err(|e| e.to_string())?;
            let Response::Power(report) = listed else {
                return Err(tr("This power plan is not on this computer.").to_owned());
            };
            let wanted = plan.to_ascii_lowercase();
            let kind = match wanted.as_str() {
                "saver" | "power-saver" => Some(rewind_os_engine::power::Kind::Saver),
                "balanced" => Some(rewind_os_engine::power::Kind::Balanced),
                "performance" | "high" => Some(rewind_os_engine::power::Kind::Performance),
                _ => None,
            };
            let found = report
                .plans
                .iter()
                .find(|p| p.id == wanted || Some(p.kind) == kind)
                .ok_or_else(|| tr("This power plan is not on this computer.").to_owned())?;
            Request::PowerSet {
                id: found.id.clone(),
            }
        }
        Command::Disks(Disks::Health) => Request::DiskHealth,
        Command::Disks(Disks::Optimize { yes }) => {
            confirm(yes)?;
            Request::DiskOptimize
        }
        Command::Update(Update::Check) => Request::UpdateCheck { now: true },
        Command::Update(Update::On) => Request::UpdateAuto { enabled: true },
        Command::Update(Update::Off) => Request::UpdateAuto { enabled: false },
        Command::Update(Update::Install { yes }) => {
            confirm(yes)?;
            Request::UpdateInstall { restart: false }
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
        Response::Update(status) => {
            match (&status.latest, status.newer) {
                (Some(latest), true) => println!(
                    "{}",
                    trf(
                        "Rewind OS {latest} is out (this is {current}): {url}",
                        &[
                            ("latest", &latest.version),
                            ("current", status.current),
                            ("url", &latest.url),
                        ],
                    )
                ),
                (Some(_), false) => println!(
                    "{}",
                    trf(
                        "Rewind OS {current} is the newest version.",
                        &[("current", status.current)]
                    )
                ),
                (None, _) => println!("{}", tr("Rewind OS has not checked for new versions yet.")),
            }
            if !status.auto {
                println!(
                    "{}",
                    tr("The check at start is off; turn it on with: rewind-os-cli update on")
                );
            }
        }
        Response::UpdateInstalled(done) => match &done.folder {
            Some(folder) => println!(
                "{}",
                trf(
                    "Rewind OS {version} is installed in {folder}.",
                    &[
                        ("version", &done.version),
                        ("folder", &folder.display().to_string())
                    ],
                )
            ),
            None => println!(
                "{}",
                trf(
                    "The installer of Rewind OS {version} was started; follow its steps.",
                    &[("version", &done.version)],
                )
            ),
        },
        Response::Boot(report) => {
            use rewind_os_engine::format::{duration_ms, local_date};
            match report.boots.first() {
                Some(last) => {
                    let mut line = trf(
                        "Last start: {time}",
                        &[("time", &duration_ms(last.total_ms))],
                    );
                    if let Some(desktop) = last.desktop_ms {
                        line.push_str(&format!(
                            " ({})",
                            trf(
                                "desktop ready after {time}",
                                &[("time", &duration_ms(desktop))]
                            )
                        ));
                    }
                    if let Some(at) = last.at {
                        line.push_str(&format!(", {}", local_date(at)));
                    }
                    println!("{line}");
                }
                None => println!("{}", tr("The system has no record of its starts yet.")),
            }
            if report.boots.len() > 1 {
                println!("\n{}", tr("Recent starts"));
                for b in &report.boots {
                    let date = b.at.map(local_date).unwrap_or_default();
                    println!("{:>12}  {date}", duration_ms(b.total_ms));
                }
            }
            if !report.slow.is_empty() {
                println!(
                    "\n{}",
                    if report.history {
                        tr("What made starts slower than usual")
                    } else {
                        tr("What took longest to start since the last start")
                    }
                );
                for s in report.slow.iter().take(top) {
                    let times = if report.history {
                        format!(
                            "  {}",
                            trf("{n} starts", &[("n", &count(u64::from(s.times)))])
                        )
                    } else {
                        String::new()
                    };
                    println!(
                        "{:>12}  {:<8} {}{times}",
                        duration_ms(s.delay_ms),
                        tr(s.kind.label()),
                        if s.name == s.file {
                            s.name.clone()
                        } else {
                            format!("{} ({})", s.name, s.file)
                        }
                    );
                }
            }
        }
        Response::Processes(report) => {
            println!(
                "{}: {:.0} %   {}: {} / {}   {}",
                tr("Processor"),
                report.cpu,
                tr("Memory in use"),
                bytes(report.memory_used),
                bytes(report.memory_total),
                trf("{n} processes", &[("n", &count(report.processes as u64))])
            );
            println!();
            for p in report.programs.iter().take(top) {
                let mut notes = Vec::new();
                if p.pids.len() > 1 {
                    notes.push(trf("{n} processes", &[("n", &count(p.pids.len() as u64))]));
                }
                if p.startup.is_some() {
                    notes.push(tr("starts with the system").to_owned());
                }
                if p.refusal.is_some() {
                    notes.push(tr("not closed by Rewind OS").to_owned());
                }
                println!(
                    "{:>6.1} %  {:>10}  {}{}",
                    p.cpu,
                    bytes(p.memory),
                    p.name,
                    if notes.is_empty() {
                        String::new()
                    } else {
                        format!("  ({})", notes.join(", "))
                    }
                );
            }
        }
        Response::VirtualDisks(report) => {
            if report.disks.is_empty() {
                println!(
                    "{}",
                    tr("No virtual disks of WSL or Docker Desktop were found.")
                );
            }
            for d in &report.disks {
                let inside = match (d.used_inside, d.reclaimable()) {
                    (Some(used), Some(free)) => trf(
                        "{used} used inside, about {free} to give back",
                        &[("used", &bytes(used)), ("free", &bytes(free))],
                    ),
                    _ if d.owner == rewind_os_engine::vdisks::Owner::Docker => {
                        tr("what Docker uses inside is not known").to_owned()
                    }
                    _ => tr("start it to see what it uses inside").to_owned(),
                };
                println!("{:>10}  {:<20} {inside}", bytes(d.bytes), d.name);
                println!("            {}", d.path.display());
            }
            if report.docker_running {
                println!(
                    "\n{}",
                    tr(
                        "Quit Docker Desktop first: it keeps WSL running, and the disk must not be in use."
                    )
                );
            }
        }
        Response::Compacted(done) => {
            println!(
                "{}",
                trf(
                    "{name}: {before} → {after}",
                    &[
                        ("name", &done.name),
                        ("before", &bytes(done.before)),
                        ("after", &bytes(done.after))
                    ]
                )
            );
            if !done.output.is_empty() {
                println!("\n{}", done.output);
            }
        }
        Response::Power(report) => {
            for p in &report.plans {
                let name = if p.kind == rewind_os_engine::power::Kind::Custom {
                    p.name.clone()
                } else {
                    tr(p.kind.label()).to_owned()
                };
                let mut line =
                    format!("{} {:<24} {}", if p.active { "*" } else { " " }, name, p.id);
                if let Some(why) = &p.degraded {
                    line.push_str(&format!("  ({}: {why})", tr("held back")));
                }
                println!("{line}");
            }
            if let Some(mode) = report.mode_plugged_in {
                println!(
                    "\n{}: {}",
                    tr("Power mode when plugged in"),
                    tr(mode.label())
                );
            }
            if let Some(mode) = report.mode_on_battery {
                println!("{}: {}", tr("Power mode on battery"), tr(mode.label()));
            }
        }
        Response::ProcessClosed(done) => {
            if let Some(why) = done.refusal {
                println!("{}", tr(why));
            } else if done.running == 0 {
                println!("{}", trf("{name} was closed.", &[("name", &done.name)]));
            } else {
                println!(
                    "{}",
                    trf(
                        "{name} did not close: it may be asking to save your work, or it only ends when forced (--force).",
                        &[("name", &done.name)]
                    )
                );
            }
        }
        Response::DiskHealth(report) => {
            if report.disks.is_empty() {
                println!("{}", tr("No drives were found."));
            }
            for d in &report.disks {
                let mut kind = tr(d.media.label()).to_owned();
                if !d.bus.is_empty() {
                    kind.push_str(&format!(" {}", d.bus));
                }
                println!(
                    "{}  ({kind}, {}{})",
                    d.name,
                    bytes(d.size),
                    if d.volumes.is_empty() {
                        String::new()
                    } else {
                        format!(", {}", d.volumes.join(" "))
                    }
                );
                println!("  {:<22} {}", tr("Health"), tr(d.health.label()));
                if let Some(c) = d.temperature_c {
                    println!("  {:<22} {c} °C", tr("Temperature"));
                }
                if let Some(p) = d.used_life {
                    println!("  {:<22} {p} %", tr("Rated life used"));
                }
                if let Some(h) = d.power_on_hours {
                    println!("  {:<22} {} h", tr("Powered on"), count(h));
                }
                if let Some(n) = d.errors {
                    println!("  {:<22} {}", tr("Uncorrected errors"), count(n));
                }
                if let Some(n) = d.bad_sectors {
                    println!("  {:<22} {}", tr("Damaged sectors"), count(n));
                }
                for p in &d.problems {
                    println!("  ! {}", tr(p.label()));
                }
            }
            if let Some(why) = report.missing {
                println!("\n{}", tr(why));
            }
            match report.schedule.enabled {
                Some(true) => println!(
                    "\n{}",
                    match report.schedule.last {
                        Some(at) => trf(
                            "The system optimizes the drives on its own; last time on {date}.",
                            &[("date", &local_date(at))]
                        ),
                        None => tr("The system optimizes the drives on its own.").to_owned(),
                    }
                ),
                Some(false) => println!(
                    "\n{}",
                    tr("The system's regular optimization of the drives is turned off.")
                ),
                None => {}
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
