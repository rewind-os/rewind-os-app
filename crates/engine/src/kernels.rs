//! Linux: old kernels. Every kernel update on Debian, Ubuntu and their family installs the new
//! kernel next to the old ones, each with its files in `/boot` (often a small partition) and its
//! modules in `/lib/modules`.
//!
//! Kernels are read from dpkg, the packages named after a kernel release
//! (`linux-image-6.8.0-45-generic`, its modules and headers). The kernel running now and the
//! newest installed one always stay (`safety::is_removable_kernel`). The others are removed with
//! `apt-get remove`, and only when apt's own dry run removes exactly those packages, so a
//! package that depends on them is never taken along. Fedora's dnf and Arch's pacman already
//! keep only a few kernels, so Rewind OS does this only with APT.

use crate::packages;
use crate::safety;
use crate::task::Task;
use crate::{EngineError, Result};
use rewind_os_platform::Os;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

/// English reasons; translate with `rewind_os_i18n::tr`.
pub const NO_APT: &str = "Rewind OS removes old kernels only with APT, on Debian, Ubuntu and their family. Other package managers already keep only a few.";
pub const RUNNING_UNKNOWN: &str =
    "The system did not say which kernel is running, so no kernel is offered.";
pub const NO_PACKAGES: &str = "No kernel is installed as a package: this system's kernel comes from elsewhere, as in WSL or a container.";
pub const REMOVES_OTHERS: &str = "Removing these kernels would also remove other packages, so Rewind OS does not do it. Use your package manager if you are sure.";
pub const NOT_OLD: &str =
    "This is not an old kernel: it is the one running, the newest one, or it is gone.";

/// Prefixes of the packages that install a kernel itself.
const IMAGES: &[&str] = &["linux-image-unsigned-", "linux-image-"];
/// Prefixes of the packages that belong to one kernel release, longest first.
const PARTS: &[&str] = &[
    "linux-modules-extra-",
    "linux-modules-",
    "linux-headers-",
    "linux-cloud-tools-",
    "linux-tools-",
];

/// One installed kernel.
#[derive(Debug, Clone, Serialize)]
pub struct Kernel {
    /// As `uname -r` says it (`6.8.0-45-generic`).
    pub release: String,
    /// Its packages, as dpkg names them.
    pub packages: Vec<String>,
    /// Space its packages and its startup image in `/boot` take.
    pub bytes: u64,
    pub running: bool,
    pub newest: bool,
}

impl Kernel {
    pub fn removable(&self) -> bool {
        !self.running && !self.newest
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Every installed kernel, newest first.
    pub kernels: Vec<Kernel>,
    /// The release running now.
    pub running: String,
}

impl Report {
    pub fn old(&self) -> impl Iterator<Item = &Kernel> {
        self.kernels.iter().filter(|k| k.removable())
    }

    pub fn reclaimable(&self) -> u64 {
        self.old().map(|k| k.bytes).sum()
    }
}

/// What removing did.
#[derive(Debug, Clone, Serialize)]
pub struct Removed {
    pub removed: Vec<String>,
    pub bytes: u64,
    /// Releases not removed, with why (English).
    pub failed: Vec<(String, String)>,
}

/// `6.8.0-45-generic` -> `[6, 8, 0, 45]`, to compare releases as numbers.
pub fn release_numbers(release: &str) -> Vec<u64> {
    release
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// The release without its flavour (`6.8.0-45-generic` -> `6.8.0-45`): the name of the
/// headers several flavours of one release share.
fn abi(release: &str) -> &str {
    match release.rsplit_once('-') {
        Some((head, flavour)) if !flavour.starts_with(|c: char| c.is_ascii_digit()) => head,
        _ => release,
    }
}

/// The kernels in `dpkg-query -W -f='${Package}\t${Installed-Size}\t${db:Status-Abbrev}\n'`,
/// newest first, each with its installed packages and their size. `running` and the newest are
/// marked; the sizes of startup images are not added here.
pub fn parse_kernels(out: &str, running: &str) -> Vec<Kernel> {
    let mut images: Vec<(String, String, u64)> = Vec::new();
    let mut parts: Vec<(String, String, u64)> = Vec::new();
    for line in out.lines() {
        let mut f = line.split('\t');
        let (Some(name), Some(kib), Some(status)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let name = name.trim();
        if !status.starts_with("ii") || !packages::safe_package_name(name) {
            continue;
        }
        let bytes = kib.trim().parse::<u64>().unwrap_or(0) * 1024;
        let release = |prefixes: &[&str]| {
            prefixes.iter().find_map(|p| {
                name.strip_prefix(p)
                    .filter(|r| r.starts_with(|c: char| c.is_ascii_digit()))
                    .map(str::to_owned)
            })
        };
        if let Some(r) = release(IMAGES) {
            images.push((r, name.to_owned(), bytes));
        } else if let Some(r) = release(PARTS) {
            parts.push((r, name.to_owned(), bytes));
        }
    }
    let mut releases: Vec<String> = images.iter().map(|(r, _, _)| r.clone()).collect();
    releases.sort_by_key(|r| std::cmp::Reverse(release_numbers(r)));
    releases.dedup();
    let mut kernels: Vec<Kernel> = releases
        .iter()
        .map(|release| {
            // Headers shared by flavours of one release belong to this kernel only when no
            // other installed kernel has the same release.
            let shared_alone = releases.iter().filter(|r| abi(r) == abi(release)).count() == 1;
            let shared = |r: &str| {
                shared_alone
                    && r != release
                    && (r == abi(release) || r.strip_suffix("-common") == Some(abi(release)))
            };
            let mut packages = Vec::new();
            let mut bytes = 0;
            for (r, name, size) in images.iter().chain(&parts) {
                if r == release || shared(r) {
                    packages.push(name.clone());
                    bytes += size;
                }
            }
            Kernel {
                release: release.clone(),
                packages,
                bytes,
                running: release == running,
                newest: false,
            }
        })
        .collect();
    if let Some(first) = kernels.first_mut() {
        first.newest = true;
    }
    kernels
}

fn require_linux(tool: &'static str) -> Result<()> {
    if Os::current() != Os::Linux {
        return Err(EngineError::Unsupported {
            tool,
            os: Os::current().name(),
        });
    }
    Ok(())
}

/// Every installed kernel, which can go and the space each takes.
pub fn list(task: &Task) -> Result<Report> {
    require_linux("kernels.list")?;
    task.set_stage("Reading the installed kernels");
    let running = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_owned())
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or(EngineError::Refused(RUNNING_UNKNOWN))?;
    let out = packages::run(
        "dpkg-query",
        &[
            "-W",
            "-f=${Package}\\t${Installed-Size}\\t${db:Status-Abbrev}\\n",
        ],
    )
    .ok_or(EngineError::Refused(NO_APT))?;
    let mut kernels = parse_kernels(&out, &running);
    // The startup image is made on the computer, so dpkg does not count it.
    for k in &mut kernels {
        let initrd = Path::new("/boot").join(format!("initrd.img-{}", k.release));
        if let Ok(meta) = std::fs::symlink_metadata(initrd)
            && meta.is_file()
        {
            k.bytes += meta.len();
        }
    }
    Ok(Report { kernels, running })
}

/// Removes the named old kernels (every old one when `releases` is empty). Each is checked again
/// against a fresh list first, and apt's dry run must remove exactly their packages.
pub fn remove(releases: &[String], task: &Task) -> Result<Removed> {
    require_linux("kernels.clean")?;
    let report = list(task)?;
    let newest = report
        .kernels
        .iter()
        .find(|k| k.newest)
        .map(|k| k.release.clone())
        .unwrap_or_default();
    let wanted: Vec<String> = if releases.is_empty() {
        report.old().map(|k| k.release.clone()).collect()
    } else {
        releases.to_vec()
    };
    let mut done = Removed {
        removed: Vec::new(),
        bytes: 0,
        failed: Vec::new(),
    };
    let mut chosen: Vec<&Kernel> = Vec::new();
    for release in wanted {
        match report.kernels.iter().find(|k| {
            k.release == release
                && safety::is_removable_kernel(&k.release, &report.running, &newest)
        }) {
            Some(k) => chosen.push(k),
            None => done.failed.push((release, NOT_OLD.to_owned())),
        }
    }
    let names: Vec<&str> = chosen
        .iter()
        .flat_map(|k| k.packages.iter().map(String::as_str))
        .collect();
    if names.is_empty() {
        return Ok(done);
    }
    task.set_stage("Removing old kernels");
    let mut simulate = vec!["-s", "remove"];
    simulate.extend(&names);
    let simulation = packages::run("apt-get", &simulate).unwrap_or_default();
    let removals: HashSet<String> = packages::apt_removals(&simulation).into_iter().collect();
    let expected: HashSet<String> = names.iter().map(|n| (*n).to_owned()).collect();
    if removals != expected {
        return Err(EngineError::Refused(REMOVES_OTHERS));
    }
    // `remove`, not `purge`, as for programs.
    let mut args = vec!["remove", "-y"];
    args.extend(&names);
    packages::run_as_admin("apt-get", &args)?;
    for k in chosen {
        task.add(1, k.bytes);
        done.bytes += k.bytes;
        done.removed.push(k.release.clone());
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DPKG: &str = "\
linux-image-6.8.0-31-generic\t14000\tii \n\
linux-modules-6.8.0-31-generic\t100000\tii \n\
linux-headers-6.8.0-31-generic\t30000\tii \n\
linux-headers-6.8.0-31\t80000\tii \n\
linux-image-6.8.0-100-generic\t15000\tii \n\
linux-modules-6.8.0-100-generic\t110000\tii \n\
linux-modules-extra-6.8.0-100-generic\t90000\tii \n\
linux-image-6.8.0-45-generic\t14500\tii \n\
linux-modules-6.8.0-45-generic\t105000\tii \n\
linux-image-6.5.0-9-generic\t14000\trc \n\
linux-image-generic\t20\tii \n\
linux-headers-generic\t20\tii \n\
linux-libc-dev\t7000\tii \n\
linux-firmware\t900000\tii \n";

    #[test]
    fn kernels_are_read_newest_first_with_their_own_packages() {
        let kernels = parse_kernels(DPKG, "6.8.0-45-generic");
        let releases: Vec<&str> = kernels.iter().map(|k| k.release.as_str()).collect();
        // 100 is newer than 45 and 31 although it sorts first as text.
        assert_eq!(
            releases,
            ["6.8.0-100-generic", "6.8.0-45-generic", "6.8.0-31-generic"]
        );
        let oldest = &kernels[2];
        assert!(
            oldest
                .packages
                .contains(&"linux-headers-6.8.0-31".to_owned())
        );
        assert_eq!(oldest.packages.len(), 4);
        // Metapackages, firmware, libc headers and removed kernels are no one's.
        for k in &kernels {
            assert!(
                k.packages
                    .iter()
                    .all(|p| p.ends_with(abi(&k.release)) || p.ends_with(&k.release))
            );
        }
        assert!(kernels.iter().all(|k| k.release != "6.5.0-9-generic"));
    }

    #[test]
    fn the_running_and_the_newest_kernel_always_stay() {
        let kernels = parse_kernels(DPKG, "6.8.0-45-generic");
        let old: Vec<&str> = kernels
            .iter()
            .filter(|k| k.removable())
            .map(|k| k.release.as_str())
            .collect();
        assert_eq!(old, ["6.8.0-31-generic"]);
        // Running an older one keeps it, as well as the newest.
        let kernels = parse_kernels(DPKG, "6.8.0-31-generic");
        let kept: Vec<&str> = kernels
            .iter()
            .filter(|k| !k.removable())
            .map(|k| k.release.as_str())
            .collect();
        assert_eq!(kept, ["6.8.0-100-generic", "6.8.0-31-generic"]);
        // A lone kernel is never offered.
        let lone = parse_kernels("linux-image-6.1.0-18-amd64\t1\tii \n", "other");
        assert!(lone.iter().all(|k| !k.removable()));
    }

    #[test]
    fn headers_shared_with_another_flavour_stay_with_it() {
        let dpkg = "\
linux-image-6.8.0-31-generic\t1\tii \n\
linux-image-6.8.0-31-lowlatency\t1\tii \n\
linux-headers-6.8.0-31\t1\tii \n\
linux-headers-6.1.0-18-common\t1\tii \n\
linux-image-6.1.0-18-amd64\t1\tii \n";
        let kernels = parse_kernels(dpkg, "none");
        for k in kernels.iter().filter(|k| k.release.starts_with("6.8.0-31")) {
            assert!(!k.packages.contains(&"linux-headers-6.8.0-31".to_owned()));
        }
        let debian = kernels.iter().find(|k| k.release == "6.1.0-18-amd64");
        assert!(debian.is_some_and(|k| {
            k.packages
                .contains(&"linux-headers-6.1.0-18-common".to_owned())
        }));
    }
}
