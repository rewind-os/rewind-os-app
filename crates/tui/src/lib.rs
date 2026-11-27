//! A terminal interface for computers without a desktop or reached over SSH: full-screen views
//! driven by keys, like k9s, through the same engine requests as the app and the CLI. Started
//! with `rewind-os-cli tui`.

pub mod app;
pub mod build_folders;
pub mod cleanup;
mod dialog;
pub mod duplicates;
pub mod home;
pub mod jobs;
pub mod leftovers;
mod list;
pub mod maintenance;
pub mod processes;
pub mod programs;
pub mod scan;
pub mod startup;
mod style;

pub use app::{App, Effect, View};
pub use jobs::{Jobs, Purpose};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
};
use ratatui::crossterm::execute;
use std::path::PathBuf;
use std::time::Duration;

/// Takes over the terminal until the person quits, then gives it back as it was.
pub fn run(scan_root: Option<PathBuf>) -> std::io::Result<()> {
    let root = scan_root
        .or_else(std::env::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut terminal = ratatui::try_init()?;
    // Clicks and the wheel reach the app; most terminals still select text with Shift held.
    let mouse = execute!(std::io::stdout(), EnableMouseCapture);
    let outcome = mouse.and_then(|()| event_loop(&mut terminal, root));
    let released = execute!(std::io::stdout(), DisableMouseCapture);
    let restored = ratatui::try_restore();
    outcome.and(released).and(restored)
}

fn apply(jobs: &mut Jobs, effects: Vec<Effect>) {
    for effect in effects {
        match effect {
            Effect::Run(purpose, request) => jobs.start(purpose, request),
            Effect::Stop(purpose) => jobs.stop(purpose),
        }
    }
}

fn event_loop(terminal: &mut DefaultTerminal, root: PathBuf) -> std::io::Result<()> {
    let mut app = App::new(root);
    let mut jobs = Jobs::default();
    let first = app.start();
    apply(&mut jobs, first);
    while !app.quit {
        terminal.draw(|frame| app.draw(frame, &jobs))?;
        // Often enough for progress to move, rarely enough to stay idle.
        if event::poll(Duration::from_millis(200))? {
            let effects = match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.key(key, &jobs),
                Event::Mouse(mouse) => app.mouse(mouse, &jobs),
                _ => Vec::new(),
            };
            apply(&mut jobs, effects);
        }
        let effects = app.tick(&jobs);
        apply(&mut jobs, effects);
        for (purpose, outcome) in jobs.finished() {
            let effects = app.finished(purpose, outcome);
            apply(&mut jobs, effects);
        }
    }
    // A cleanup stops between files; what it removed stays removed.
    jobs.stop_all();
    Ok(())
}
