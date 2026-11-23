//! Runs engine requests on a background thread so the window never freezes.

use rewind_os_engine::{Engine, Request, Response, Task};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

pub type Outcome = Result<Response, String>;

pub struct Job {
    pub task: Arc<Task>,
    pub started: Instant,
    rx: Receiver<Outcome>,
}

impl Job {
    pub fn start(ctx: &egui::Context, engine: &Arc<Engine>, request: Request) -> Self {
        let task = Arc::new(Task::new());
        let (tx, rx) = mpsc::channel();
        {
            let task = Arc::clone(&task);
            let engine = Arc::clone(engine);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let outcome = engine.run(request, &task).map_err(|e| e.to_string());
                let _ = tx.send(outcome);
                ctx.request_repaint();
            });
        }
        Self {
            task,
            started: Instant::now(),
            rx,
        }
    }

    /// The outcome once the request finished. Keeps the window refreshing while it runs, so
    /// progress stays current.
    pub fn poll(&self, ctx: &egui::Context) -> Option<Outcome> {
        match self.rx.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(150));
                None
            }
            Err(TryRecvError::Disconnected) => {
                Some(Err("The background task stopped unexpectedly".to_owned()))
            }
        }
    }
}

/// Polls `slot` and, when its job finished, takes it out and returns the outcome.
pub fn finished(slot: &mut Option<Job>, ctx: &egui::Context) -> Option<Outcome> {
    let outcome = slot.as_ref()?.poll(ctx)?;
    *slot = None;
    Some(outcome)
}
