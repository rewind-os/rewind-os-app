//! Well-known folders and `%VARIABLE%` expansion.

use std::path::PathBuf;

/// Folders the engine needs, resolved from the environment Windows sets for every process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Known {
    UserProfile,
    RoamingAppData,
    LocalAppData,
    ProgramData,
    ProgramFiles,
    ProgramFilesX86,
    Windows,
    Temp,
}

impl Known {
    fn variable(self) -> &'static str {
        match self {
            Known::UserProfile => "USERPROFILE",
            Known::RoamingAppData => "APPDATA",
            Known::LocalAppData => "LOCALAPPDATA",
            Known::ProgramData => "PROGRAMDATA",
            Known::ProgramFiles => "PROGRAMFILES",
            Known::ProgramFilesX86 => "PROGRAMFILES(X86)",
            Known::Windows => "WINDIR",
            Known::Temp => "TEMP",
        }
    }

    /// The folder, or `None` when the variable is not set (for example on another OS).
    pub fn path(self) -> Option<PathBuf> {
        std::env::var_os(self.variable())
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    }
}

/// Replaces every `%NAME%` with the value of environment variable `NAME`, passing each value
/// through `escape` (so callers can escape glob characters). Returns `None` if any variable is
/// missing, so a pattern never silently turns into a relative path.
pub fn expand(template: &str, escape: impl Fn(&str) -> String) -> Option<String> {
    let mut out = String::with_capacity(template.len() + 64);
    let mut rest = template;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        let name = &after[..end];
        let value = std::env::var(name).ok().filter(|v| !v.is_empty())?;
        out.push_str(&escape(&value));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}
