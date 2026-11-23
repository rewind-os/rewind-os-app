//! Everything that talks to the operating system directly lives here, so the engine stays
//! testable on any platform. Functions that only make sense on Windows return
//! [`PlatformError::Unsupported`] elsewhere instead of pretending to succeed.

pub mod elevation;
pub mod paths;
pub mod process;
pub mod registry;
pub mod shell;

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("not supported on this operating system: {0}")]
    Unsupported(&'static str),
    #[error("administrator rights are required")]
    NeedsAdmin,
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} failed (exit code {code:?}): {detail}")]
    Command {
        program: String,
        code: Option<i32>,
        detail: String,
    },
    #[error("could not start {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("recycle bin: {0}")]
    Trash(String),
}

pub type Result<T> = std::result::Result<T, PlatformError>;
