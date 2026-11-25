//! Installed programs, read from the uninstall entries Windows keeps in the registry.

use crate::{EngineError, Result};
use chrono::NaiveDate;
use rewind_os_platform::registry::{self, Hive, Value};
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Installed for every user.
    Machine,
    /// Installed for the current user only.
    User,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledProgram {
    /// Stable id: `<hive>\<view>\<subkey>`.
    pub key: String,
    pub name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub install_location: Option<PathBuf>,
    pub display_icon: Option<String>,
    #[serde(skip)]
    pub uninstall_command: Option<String>,
    /// Size the installer declared; often missing or outdated.
    pub estimated_bytes: Option<u64>,
    pub installed_on: Option<NaiveDate>,
    pub scope: Scope,
    /// The package manager that installed it, on Linux (`APT`, `Flatpak`…).
    pub manager: Option<&'static str>,
}

fn sources() -> [(Hive, &'static str, &'static str); 3] {
    [
        (Hive::LocalMachine, UNINSTALL, "hklm64"),
        (Hive::LocalMachine, UNINSTALL_32, "hklm32"),
        (Hive::CurrentUser, UNINSTALL, "hkcu"),
    ]
}

/// Every installed program: Windows' own list, or the package managers on Linux. Sorted by
/// name.
pub fn installed() -> Vec<InstalledProgram> {
    match rewind_os_platform::Os::current() {
        rewind_os_platform::Os::Windows => windows_installed(),
        rewind_os_platform::Os::Linux => crate::packages::installed(),
        rewind_os_platform::Os::MacOs => crate::macapps::installed(),
    }
}

fn windows_installed() -> Vec<InstalledProgram> {
    let mut seen = HashSet::new();
    let mut programs: Vec<InstalledProgram> = sources()
        .into_iter()
        .flat_map(|(hive, base, tag)| {
            registry::subkeys(hive, base)
                .into_iter()
                .filter_map(move |sub| read(hive, base, tag, &sub))
        })
        .filter(|p| seen.insert((p.name.to_lowercase(), p.version.clone())))
        .collect();
    programs.sort_by_key(|p| p.name.to_lowercase());
    programs
}

fn read(hive: Hive, base: &str, tag: &str, sub: &str) -> Option<InstalledProgram> {
    let path = format!(r"{base}\{sub}");
    let values = registry::values(hive, &path);
    let text = |name: &str| {
        values
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .and_then(|(_, v)| v.as_text())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let number = |name: &str| {
        values
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .and_then(|(_, v)| match v {
                Value::Number(n) => Some(*n),
                _ => None,
            })
    };
    let name = text("DisplayName")?;
    // Hidden components and updates of another entry are not separate programs.
    if number("SystemComponent") == Some(1) || text("ParentKeyName").is_some() {
        return None;
    }
    Some(InstalledProgram {
        key: format!(r"{tag}\{sub}"),
        name,
        publisher: text("Publisher"),
        version: text("DisplayVersion"),
        install_location: text("InstallLocation").map(|s| PathBuf::from(s.trim_matches('"'))),
        display_icon: text("DisplayIcon"),
        uninstall_command: text("UninstallString"),
        estimated_bytes: number("EstimatedSize").map(|kb| kb * 1024),
        installed_on: text("InstallDate")
            .and_then(|d| NaiveDate::parse_from_str(&d, "%Y%m%d").ok()),
        scope: if hive == Hive::CurrentUser {
            Scope::User
        } else {
            Scope::Machine
        },
        manager: None,
    })
}

/// Starts the uninstaller the program registered. It shows its own window; this returns as
/// soon as it started.
pub fn start_uninstaller(key: &str) -> Result<()> {
    match rewind_os_platform::Os::current() {
        rewind_os_platform::Os::Linux => return crate::packages::uninstall(key),
        rewind_os_platform::Os::MacOs => return crate::macapps::uninstall(key),
        rewind_os_platform::Os::Windows => {}
    }
    let program = installed()
        .into_iter()
        .find(|p| p.key == key)
        .ok_or_else(|| EngineError::NotFound(key.to_owned()))?;
    let command = program
        .uninstall_command
        .ok_or_else(|| EngineError::NotFound(format!("{key}: no uninstall command")))?;
    launch(&command)
}

/// Refusal when the person declined the administrator prompt of the uninstaller.
#[cfg(windows)]
const DECLINED: &str = "The uninstaller was not started: the administrator prompt was declined.";
/// Refusal when the registered uninstaller is no longer on disk.
#[cfg(windows)]
const MISSING: &str = "The uninstaller this program registered is no longer on this computer. Open its folder to remove it by hand, then search for leftovers.";

/// The program and the arguments of a registered uninstall command line.
#[cfg(any(windows, test))]
#[derive(Debug, PartialEq, Eq)]
struct Launch {
    program: String,
    args: String,
}

/// Splits an `UninstallString` into the program and its arguments. Installers write these in
/// many shapes: quoted (`"C:\Program Files\App\unins000.exe" /x`), unquoted with spaces in the
/// path (`C:\Program Files\App\uninstall.exe /S`) or a bare program (`MsiExec.exe /I{…}`).
/// For an unquoted line, the first prefix that names an existing file wins, then the first
/// prefix ending in `.exe`, then the first word. Windows Installer entries registered as
/// `/I{product}` (change or repair) are turned into `/X{product}` (remove).
#[cfg(any(windows, test))]
fn split_command(command: &str, exists: impl Fn(&str) -> bool) -> Option<Launch> {
    let command = command.trim();
    let (program, args) = if let Some(rest) = command.strip_prefix('"') {
        let end = rest.find('"')?;
        (rest.get(..end)?, rest.get(end + 1..)?)
    } else {
        let cuts: Vec<usize> = command
            .char_indices()
            .filter(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i)
            .chain(std::iter::once(command.len()))
            .collect();
        let prefix = |i: usize| command.get(..i).unwrap_or_default();
        let cut = cuts
            .iter()
            .copied()
            .find(|&i| exists(prefix(i)))
            .or_else(|| {
                cuts.iter()
                    .copied()
                    .find(|&i| prefix(i).to_ascii_lowercase().ends_with(".exe"))
            })
            .or_else(|| cuts.first().copied())?;
        (prefix(cut), command.get(cut..).unwrap_or_default())
    };
    let program = program.trim();
    if program.is_empty() {
        return None;
    }
    let mut args = args.trim().to_owned();
    let name = program
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    if name == "msiexec" || name == "msiexec.exe" {
        args = msi_remove(&args);
    }
    Some(Launch {
        program: program.to_owned(),
        args,
    })
}

/// `/I{product}` (install, change or repair) becomes `/X{product}` (remove).
#[cfg(any(windows, test))]
fn msi_remove(args: &str) -> String {
    let mut out = String::with_capacity(args.len());
    let mut rest = args;
    while let Some(at) = rest.find(['/', '-']) {
        let (before, from) = rest.split_at(at);
        out.push_str(before);
        let mut chars = from.chars();
        let slash = chars.next().unwrap_or('/');
        let after = chars.as_str();
        let is_install = after.get(..1).is_some_and(|c| c.eq_ignore_ascii_case("i"))
            && after
                .get(1..)
                .is_some_and(|r| r.trim_start().starts_with('{'));
        out.push(slash);
        if is_install {
            out.push('X');
            rest = after.get(1..).unwrap_or_default();
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Starts the uninstaller the way Explorer would (ShellExecute, through `Start-Process`), so
/// Windows shows its administrator prompt when the uninstaller asks for one. `cmd /C` could not:
/// a program that needs elevation failed inside cmd without a word, and cmd's quote handling
/// broke command lines with more than one quoted part.
#[cfg(windows)]
fn launch(command: &str) -> Result<()> {
    let exists = |p: &str| std::path::Path::new(p).is_file();
    let launch = split_command(command, exists)
        .ok_or_else(|| EngineError::NotFound(format!("uninstall command: {command}")))?;
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let mut start = format!(
        "Start-Process -FilePath {} -ErrorAction Stop",
        quote(&launch.program)
    );
    if !launch.args.is_empty() {
        // One string: it reaches the uninstaller as its command line, untouched.
        start.push_str(&format!(" -ArgumentList {}", quote(&launch.args)));
    }
    // Exit with the Windows error code, so a declined prompt (1223) and a missing file (2, 3)
    // can be told apart from other failures without reading localized messages.
    let script = format!(
        "try {{ {start} }} catch {{ $e = $_.Exception; \
         while ($e -and -not ($e -is [System.ComponentModel.Win32Exception])) {{ $e = $e.InnerException }}; \
         if ($e) {{ [Console]::Error.WriteLine($e.Message); exit $e.NativeErrorCode }}; \
         [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}"
    );
    let finished = rewind_os_platform::process::run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )?;
    match finished.code {
        Some(0) => Ok(()),
        Some(1223) => Err(EngineError::Refused(DECLINED)),
        Some(2 | 3) => Err(EngineError::Refused(MISSING)),
        code => Err(rewind_os_platform::PlatformError::Command {
            program: launch.program,
            code,
            detail: finished.stderr.trim().to_owned(),
        }
        .into()),
    }
}

#[cfg(not(windows))]
fn launch(_command: &str) -> Result<()> {
    Err(rewind_os_platform::PlatformError::Unsupported("program uninstallers").into())
}

#[cfg(test)]
mod tests {
    use super::{Launch, split_command};

    fn split(command: &str, existing: &[&str]) -> Option<Launch> {
        split_command(command, |p| existing.contains(&p))
    }

    #[test]
    fn a_quoted_program_keeps_every_quoted_argument() {
        let l = split(
            r#""C:\Program Files\App\unins000.exe" /uninstall "C:\Program Files\App\data""#,
            &[],
        )
        .unwrap();
        assert_eq!(l.program, r"C:\Program Files\App\unins000.exe");
        assert!(l.args.contains(r#""C:\Program Files\App\data""#));
    }

    #[test]
    fn an_unquoted_path_with_spaces_is_not_cut_at_the_first_space() {
        let l = split(r"C:\Program Files\My App\uninstall.exe /S", &[]).unwrap();
        assert_eq!(l.program, r"C:\Program Files\My App\uninstall.exe");
        assert_eq!(l.args, "/S");
    }

    #[test]
    fn an_existing_file_wins_over_the_exe_guess() {
        let l = split(
            r"C:\Program Files\Tool\remove tool.bat now",
            &[r"C:\Program Files\Tool\remove tool.bat"],
        )
        .unwrap();
        assert_eq!(l.program, r"C:\Program Files\Tool\remove tool.bat");
        assert_eq!(l.args, "now");
    }

    #[test]
    fn windows_installer_entries_remove_instead_of_repair() {
        let l = split("MsiExec.exe /I{1234-ABCD}", &[]).unwrap();
        assert!(l.args.contains("{1234-ABCD}"));
        assert!(l.args.to_ascii_uppercase().starts_with("/X"));
        // Other programs keep their arguments as they are.
        let other = split(r"C:\App\setup.exe /I{1234-ABCD}", &[]).unwrap();
        assert_eq!(other.args, "/I{1234-ABCD}");
    }

    #[test]
    fn an_empty_or_broken_command_is_not_launched() {
        assert!(split("   ", &[]).is_none());
        assert!(split(r#""C:\App\unins.exe"#, &[]).is_none());
    }
}
