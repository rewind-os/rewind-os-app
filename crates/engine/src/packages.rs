//! Installed programs on Linux, from the package managers that are present: dpkg (only what
//! was installed on purpose, per `apt-mark`), rpm, pacman (explicitly installed), Flatpak apps
//! and Snaps. Every parser is a pure function of the tool's output so it can be tested anywhere.

use crate::programs::{InstalledProgram, Scope};
use chrono::{DateTime, NaiveDate, Utc};
use rewind_os_platform::process;
use std::collections::HashSet;
use std::path::Path;

/// `12.3 MiB`, `1,2 GB`, `512 kB`, `800 B` -> bytes. Binary and decimal units are both read as
/// their own size.
pub fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim().replace(',', ".").replace('\u{a0}', " ");
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.trim().parse().ok()?;
    let factor: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" | "bytes" => 1.0,
        "kb" => 1e3,
        "kib" | "k" => 1024.0,
        "mb" => 1e6,
        "mib" | "m" => 1024.0 * 1024.0,
        "gb" => 1e9,
        "gib" | "g" => 1024.0 * 1024.0 * 1024.0,
        "tb" => 1e12,
        "tib" => 1024f64.powi(4),
        _ => return None,
    };
    let bytes = number * factor;
    (bytes.is_finite() && bytes >= 0.0).then_some(bytes as u64)
}

fn program(manager: &'static str, name: &str) -> InstalledProgram {
    InstalledProgram {
        key: format!("{}|{name}", manager.to_ascii_lowercase()),
        name: name.to_owned(),
        publisher: None,
        version: None,
        install_location: None,
        display_icon: None,
        uninstall_command: None,
        estimated_bytes: None,
        installed_on: None,
        scope: Scope::Machine,
        manager: Some(manager),
    }
}

fn text(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

/// `Name <mail>` -> `Name`.
fn without_mail(s: &str) -> Option<String> {
    text(s.split('<').next().unwrap_or(s))
}

/// Lines of `dpkg-query -W -f='${binary:Package}\t${Version}\t${Installed-Size}\t${Maintainer}\t${db:Status-Abbrev}\t${Priority}\t${Section}\n'`,
/// keeping installed packages in `manual` (all of them when `manual` is `None`). Packages of
/// priority `required` or `important` are the base system and libraries are parts of programs;
/// both are left out.
pub fn parse_dpkg(out: &str, manual: Option<&HashSet<String>>) -> Vec<InstalledProgram> {
    out.lines()
        .filter_map(|line| {
            let mut f = line.split('\t');
            let full = f.next()?.trim();
            let version = f.next()?;
            let kib = f.next()?;
            let maintainer = f.next()?;
            let status = f.next()?;
            let priority = f.next().unwrap_or("").trim();
            let section = f.next().unwrap_or("").trim();
            let library = section.ends_with("libs") || section.ends_with("libdevel");
            if !status.starts_with("ii")
                || priority == "required"
                || priority == "important"
                || library
            {
                return None;
            }
            let name = full.split(':').next().unwrap_or(full);
            if manual.is_some_and(|m| !m.contains(name) && !m.contains(full)) {
                return None;
            }
            let mut p = program("APT", name);
            p.version = text(version);
            p.estimated_bytes = kib.trim().parse::<u64>().ok().map(|k| k * 1024);
            p.publisher = without_mail(maintainer);
            Some(p)
        })
        .collect()
}

/// Lines of `rpm -qa --qf '%{NAME}\t%{VERSION}-%{RELEASE}\t%{SIZE}\t%{VENDOR}\t%{INSTALLTIME}\n'`.
pub fn parse_rpm(out: &str) -> Vec<InstalledProgram> {
    out.lines()
        .filter_map(|line| {
            let mut f = line.split('\t');
            let name = text(f.next()?)?;
            let mut p = program("RPM", &name);
            p.version = f.next().and_then(text);
            p.estimated_bytes = f.next().and_then(|s| s.trim().parse().ok());
            p.publisher = f.next().and_then(text).filter(|v| v != "(none)");
            p.installed_on = f
                .next()
                .and_then(|s| s.trim().parse::<i64>().ok())
                .and_then(|secs| DateTime::<Utc>::from_timestamp(secs, 0))
                .map(|d| d.date_naive());
            Some(p)
        })
        .collect()
}

/// Output of `pacman -Qei` (English): blocks of `Key : value` separated by blank lines.
pub fn parse_pacman(out: &str) -> Vec<InstalledProgram> {
    out.split("\n\n")
        .filter_map(|block| {
            let field = |key: &str| {
                block.lines().find_map(|l| {
                    let (k, v) = l.split_once(" : ")?;
                    (k.trim() == key).then(|| v.trim().to_owned())
                })
            };
            let name = field("Name")?;
            let mut p = program("pacman", &name);
            p.version = field("Version");
            p.estimated_bytes = field("Installed Size").and_then(|s| parse_size(&s));
            p.publisher = field("Packager").and_then(|s| without_mail(&s));
            Some(p)
        })
        .collect()
}

/// Lines of `flatpak list --app --columns=application,name,version,size,origin,installation`.
pub fn parse_flatpak(out: &str) -> Vec<InstalledProgram> {
    out.lines()
        .filter_map(|line| {
            let mut f = line.split('\t');
            let id = text(f.next()?)?;
            let name = f.next().and_then(text).unwrap_or_else(|| id.clone());
            let mut p = program("Flatpak", &name);
            p.key = format!("flatpak|{id}");
            p.version = f.next().and_then(text);
            p.estimated_bytes = f.next().and_then(parse_size);
            p.publisher = f.next().and_then(text);
            if f.next().is_some_and(|i| i.trim() == "user") {
                p.scope = Scope::User;
            }
            Some(p)
        })
        .collect()
}

/// Output of `snap list`: a header line, then `Name Version Rev Tracking Publisher Notes`.
pub fn parse_snap(out: &str) -> Vec<InstalledProgram> {
    out.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let name = f.first()?;
            let notes = f.get(5).copied().unwrap_or("");
            // Bases and snapd itself are parts of the system, not programs.
            if notes.contains("base") || notes.contains("snapd") || *name == "snapd" {
                return None;
            }
            let mut p = program("Snap", name);
            p.version = f.get(1).and_then(|s| text(s));
            p.publisher = f
                .get(4)
                .map(|s| s.trim_end_matches(['✓', '*', '✪']).to_owned())
                .filter(|s| !s.is_empty() && s != "-");
            Some(p)
        })
        .collect()
}

/// Runs a package tool in the C locale, so its output is in English whatever the system
/// language. `None` when the tool is missing or fails.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut all = vec!["LC_ALL=C", program];
    all.extend_from_slice(args);
    let done = process::run("env", &all).ok()?;
    done.success().then_some(done.stdout)
}

/// Day a dpkg package was installed or last upgraded: when its file list was written.
fn dpkg_date(name: &str, arch: &str) -> Option<NaiveDate> {
    let info = Path::new("/var/lib/dpkg/info");
    let meta = std::fs::metadata(info.join(format!("{name}.list")))
        .or_else(|_| std::fs::metadata(info.join(format!("{name}:{arch}.list"))))
        .ok()?;
    Some(DateTime::<Utc>::from(meta.modified().ok()?).date_naive())
}

/// Every program the package managers on this computer report, sorted by name.
pub fn installed() -> Vec<InstalledProgram> {
    let mut all = Vec::new();
    if let Some(out) = run(
        "dpkg-query",
        &[
            "-W",
            "-f=${binary:Package}\\t${Version}\\t${Installed-Size}\\t${Maintainer}\\t${db:Status-Abbrev}\\t${Priority}\\t${Section}\\n",
        ],
    ) {
        let manual: Option<HashSet<String>> = run("apt-mark", &["showmanual"])
            .map(|m| m.lines().map(|l| l.trim().to_owned()).collect());
        let mut found = parse_dpkg(&out, manual.as_ref());
        let arch = run("dpkg", &["--print-architecture"]).unwrap_or_default();
        for p in &mut found {
            p.installed_on = dpkg_date(&p.name, arch.trim());
        }
        all.extend(found);
    } else if let Some(out) = run(
        "rpm",
        &[
            "-qa",
            "--qf",
            "%{NAME}\\t%{VERSION}-%{RELEASE}\\t%{SIZE}\\t%{VENDOR}\\t%{INSTALLTIME}\\n",
        ],
    ) {
        all.extend(parse_rpm(&out));
    } else if let Some(out) = run("pacman", &["-Qei"]) {
        all.extend(parse_pacman(&out));
    }
    if let Some(out) = run(
        "flatpak",
        &[
            "list",
            "--app",
            "--columns=application,name,version,size,origin,installation",
        ],
    ) {
        all.extend(parse_flatpak(&out));
    }
    if let Some(out) = run("snap", &["list"]) {
        all.extend(parse_snap(&out));
    }
    all.sort_by_key(|p| p.name.to_lowercase());
    all
}
