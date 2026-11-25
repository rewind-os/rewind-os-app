//! Everything that talks to the operating system directly lives here, so the engine stays
//! testable on any platform. Functions that only make sense on Windows return
//! [`PlatformError::Unsupported`] elsewhere instead of pretending to succeed.

pub mod elevation;
pub mod files;
pub mod installer;
pub mod paths;
pub mod process;
pub mod registry;
pub mod shell;
pub mod volume;
pub mod web;

use std::path::PathBuf;

/// The operating system families Rewind OS knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Os {
    Windows,
    Linux,
    MacOs,
}

impl Os {
    /// The system this program was built for. Other Unix systems are treated like Linux.
    pub const fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }

    /// The system's name, the same in every language.
    pub fn name(self) -> &'static str {
        match self {
            Os::Windows => "Windows",
            Os::Linux => "Linux",
            Os::MacOs => "macOS",
        }
    }
}

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
