//! `rewind-os-cli`: every engine tool from the command line. Destructive tools ask for
//! `--yes`; `--json` prints the engine's response for scripts and agents.

use clap::{Parser, Subcommand};
use rewind_os_engine::format::{bytes, count};
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
    /// Measure every folder under PATH.
    Scan {
        path: PathBuf,
        /// How many folders and files to print.
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// Files directly inside DIR, largest first.
    Files { dir: PathBuf },
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
    Leftovers,
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
enum Startup {
    List,
    Enable { id: String },
    Disable { id: String },
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
        Command::Scan { top, .. } => *top,
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
            eprintln!("{}: {err}", tr("Error"));
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
        Command::Scan { path, .. } => Request::DiskScan { root: path },
        Command::Files { dir } => Request::ListFiles { dir },
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
        Command::Leftovers => Request::LeftoversFind,
        Command::Trash { paths, yes } => {
            confirm(yes)?;
            Request::FilesTrash { paths }
        }
        Command::Startup(Startup::List) => Request::StartupList,
        Command::Startup(Startup::Enable { id }) => Request::StartupSet { id, enabled: true },
        Command::Startup(Startup::Disable { id }) => Request::StartupSet { id, enabled: false },
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
                    "{}: {} · {} · {} ms",
                    tree.root.display(),
                    bytes(root.bytes),
                    trf("{n} files", &[("n", &count(root.files))]),
                    tree.elapsed_ms
                );
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
        Response::Trashed(results) => {
            for r in results {
                match &r.error {
                    None => println!("{:>10}  {}", bytes(r.bytes), r.path.display()),
                    Some(err) => {
                        println!("{:>10}  {}  {}: {err}", "-", r.path.display(), tr("Error"))
                    }
                }
            }
            println!("{}", tr("Space is freed when the recycle bin is emptied."));
        }
        Response::Startup(entries) => {
            for e in entries {
                let state = if e.enabled { tr("On") } else { tr("Off") };
                println!("{:<4} {:<40} {}", state, e.id, e.command);
            }
        }
        Response::ToolOutput(text) => println!("{text}"),
        Response::Done(message) => println!("{}", tr(message)),
    }
}
