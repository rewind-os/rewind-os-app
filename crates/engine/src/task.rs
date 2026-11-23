//! Progress and cancellation shared between a running request and its front end.

use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct Task {
    cancelled: AtomicBool,
    items: AtomicU64,
    bytes: AtomicU64,
    stage: Mutex<&'static str>,
    detail: Mutex<String>,
}

/// A copy of a task's counters at one moment.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Progress {
    pub items: u64,
    pub bytes: u64,
    /// English key for the current stage.
    pub stage: &'static str,
    pub detail: String,
}

impl Task {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn add(&self, items: u64, bytes: u64) {
        self.items.fetch_add(items, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// `stage` is English text; front ends translate it.
    pub fn set_stage(&self, stage: &'static str) {
        *self.stage.lock() = stage;
    }

    pub fn set_detail(&self, detail: impl Into<String>) {
        *self.detail.lock() = detail.into();
    }

    pub fn snapshot(&self) -> Progress {
        Progress {
            items: self.items.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            stage: *self.stage.lock(),
            detail: self.detail.lock().clone(),
        }
    }
}
