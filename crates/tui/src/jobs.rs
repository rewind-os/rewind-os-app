//! Engine requests running on their own threads, so the screen keeps answering keys and shows
//! progress while a scan or a cleanup runs.

use rewind_os_engine::{Engine, EngineError, Progress, Request, Response, Task};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// What a request's answer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    Overview,
    Recommendations,
    Scan,
    Analyze,
    Clean,
    Startup,
    StartupSet,
    Programs,
    Uninstall,
    Duplicates,
    DuplicatesTrash,
    Leftovers,
    LeftoversTrash,
    BuildFolders,
    BuildClean,
}

/// What a running request reports.
#[derive(Debug, Clone, Default)]
pub struct Running {
    pub progress: Progress,
    pub elapsed: Duration,
    pub stopping: bool,
}

/// The text of an error, translated.
pub fn error_text(err: &EngineError) -> String {
    match err.message() {
        Some((key, args)) => {
            let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
            rewind_os_i18n::trf(key, &args)
        }
        None => err.to_string(),
    }
}

struct Job {
    purpose: Purpose,
    task: Arc<Task>,
    started: Instant,
    rx: Receiver<Result<Response, String>>,
}

#[derive(Default)]
pub struct Jobs {
    running: Vec<Job>,
}

impl Jobs {
    /// Starts `request`; one already running for the same purpose is stopped first.
    pub fn start(&mut self, purpose: Purpose, request: Request) {
        self.stop(purpose);
        let task = Arc::new(Task::new());
        let (tx, rx) = mpsc::channel();
        {
            let task = Arc::clone(&task);
            std::thread::spawn(move || {
                let outcome = Engine::new()
                    .run(request, &task)
                    .map_err(|e| error_text(&e));
                let _ = tx.send(outcome);
            });
        }
        self.running.push(Job {
            purpose,
            task,
            started: Instant::now(),
            rx,
        });
    }

    /// Asks the request for `purpose` to stop; it still answers, with what it did so far.
    pub fn stop(&self, purpose: Purpose) {
        for job in self.running.iter().filter(|j| j.purpose == purpose) {
            job.task.cancel();
        }
    }

    pub fn stop_all(&self) {
        for job in &self.running {
            job.task.cancel();
        }
    }

    pub fn running(&self, purpose: Purpose) -> Option<Running> {
        self.running
            .iter()
            .rev()
            .find(|j| j.purpose == purpose)
            .map(|j| Running {
                progress: j.task.snapshot(),
                elapsed: j.started.elapsed(),
                stopping: j.task.is_cancelled(),
            })
    }

    pub fn any(&self) -> bool {
        !self.running.is_empty()
    }

    /// The answers that came in since the last call. A request stopped because another one for
    /// the same purpose replaced it is dropped.
    pub fn finished(&mut self) -> Vec<(Purpose, Result<Response, String>)> {
        let mut done = Vec::new();
        let latest: Vec<usize> = self
            .running
            .iter()
            .enumerate()
            .filter(|(i, j)| {
                !self
                    .running
                    .iter()
                    .skip(i + 1)
                    .any(|later| later.purpose == j.purpose)
            })
            .map(|(i, _)| i)
            .collect();
        let mut index = 0;
        self.running.retain(|job| {
            let keep = match job.rx.try_recv() {
                Ok(outcome) => {
                    if latest.contains(&index) {
                        done.push((job.purpose, outcome));
                    }
                    false
                }
                Err(TryRecvError::Empty) => true,
                Err(TryRecvError::Disconnected) => false,
            };
            index += 1;
            keep
        });
        done
    }
}
