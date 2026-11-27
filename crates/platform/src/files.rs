//! Facts about single files that differ per system.

use std::fs::Metadata;

/// The file lives in the cloud (OneDrive "files on demand" and similar): reading it would
/// download it, so tools that read contents must skip it.
#[cfg(windows)]
pub fn is_cloud_placeholder(meta: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x0000_1000;
    const RECALL_ON_OPEN: u32 = 0x0004_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    meta.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

/// The file lives in the cloud: reading it would download it.
#[cfg(not(windows))]
pub fn is_cloud_placeholder(_meta: &Metadata) -> bool {
    false
}

/// The space `meta`'s file takes on the disk. On Linux and macOS: the blocks allocated to it.
/// On Windows: the allocated size of compressed, sparse and cloud files (which can be far less
/// than their length, or nothing), asked only for those so a scan stays as fast; other files
/// count their length.
#[cfg(unix)]
pub fn size_on_disk(_path: &std::path::Path, meta: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks().saturating_mul(512)
}

/// The space `meta`'s file takes on the disk. On Linux and macOS: the blocks allocated to it.
/// On Windows: the allocated size of compressed, sparse and cloud files (which can be far less
/// than their length, or nothing), asked only for those so a scan stays as fast; other files
/// count their length.
#[cfg(windows)]
pub fn size_on_disk(path: &std::path::Path, meta: &Metadata) -> u64 {
    use std::os::windows::fs::MetadataExt;
    const SPARSE: u32 = 0x0000_0200;
    const COMPRESSED: u32 = 0x0000_0800;
    if meta.file_attributes() & (SPARSE | COMPRESSED) != 0 || is_cloud_placeholder(meta) {
        filesize::file_real_size(path).unwrap_or(meta.len())
    } else {
        meta.len()
    }
}

/// The space `meta`'s file takes on the disk: its length, where the system cannot tell more.
#[cfg(not(any(unix, windows)))]
pub fn size_on_disk(_path: &std::path::Path, meta: &Metadata) -> u64 {
    meta.len()
}

/// Identifies a file with more than one name (hard links): the volume and the file's number on
/// it. `None` for a file with a single name, or when the system cannot tell.
///
/// On Linux and macOS this comes from `meta`; on Windows the file is opened to ask, which costs
/// a little per file, so callers ask only where hard links are expected.
#[cfg(unix)]
pub fn link_identity(_path: &std::path::Path, meta: &Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    (meta.nlink() > 1).then(|| (meta.dev(), meta.ino()))
}

/// Identifies a file with more than one name (hard links): the volume and the file's number on
/// it. `None` for a file with a single name, or when the system cannot tell.
#[cfg(windows)]
pub fn link_identity(path: &std::path::Path, _meta: &Metadata) -> Option<(u64, u64)> {
    let handle = winapi_util::Handle::from_path_any(path).ok()?;
    let info = winapi_util::file::information(&handle).ok()?;
    (info.number_of_links() > 1).then(|| (info.volume_serial_number(), info.file_index()))
}

/// Identifies a file with more than one name. Not known on this system.
#[cfg(not(any(unix, windows)))]
pub fn link_identity(_path: &std::path::Path, _meta: &Metadata) -> Option<(u64, u64)> {
    None
}

/// Whether the folder is one where hard links are common enough to be worth asking about on
/// systems where asking costs (the Windows folder, with `WinSxS` and `System32`).
pub fn hard_links_expected(dir: &std::path::Path) -> bool {
    if cfg!(unix) {
        return true;
    }
    let lower = |p: &std::path::Path| std::path::PathBuf::from(p.to_string_lossy().to_lowercase());
    crate::paths::Known::Windows
        .path()
        .is_some_and(|windows| lower(dir).starts_with(lower(&windows)))
}

/// File systems the kernel makes up (processes, devices, settings): nothing on them uses disk
/// space, and some report files of many terabytes (`/proc/kcore`).
const VIRTUAL_KINDS: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "devfs",
    "cgroup",
    "cgroup2",
    "debugfs",
    "tracefs",
    "securityfs",
    "pstore",
    "bpf",
    "configfs",
    "fusectl",
    "mqueue",
    "hugetlbfs",
    "binfmt_misc",
    "autofs",
    "efivarfs",
    "selinuxfs",
    "rpc_pipefs",
    "nsfs",
];

/// The mount points of virtual file systems in `/proc/self/mountinfo` text. The mount point is
/// the fifth field (with spaces and other bytes written as `\040` and the like); the kind
/// follows the ` - ` separator.
pub fn virtual_mounts_in(mountinfo: &str) -> Vec<std::path::PathBuf> {
    mountinfo
        .lines()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let kind = after.split_whitespace().next()?;
            if !VIRTUAL_KINDS.contains(&kind) {
                return None;
            }
            let point = before.split_whitespace().nth(4)?;
            Some(std::path::PathBuf::from(unescape_octal(point)))
        })
        .collect()
}

/// Undoes the `\ooo` escapes the kernel writes in mount tables.
fn unescape_octal(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let code = bytes
            .get(i + 1..i + 4)
            .filter(|_| b == b'\\')
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u8::from_str_radix(d, 8).ok());
        match code {
            Some(c) => {
                out.push(c);
                i += 4;
            }
            None => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where this system mounts virtual file systems, which measuring never enters. Empty on
/// Windows; on macOS `/dev` (devfs), which has no mount table to read without a program.
pub fn virtual_mounts() -> Vec<std::path::PathBuf> {
    if cfg!(target_os = "linux") {
        std::fs::read_to_string("/proc/self/mountinfo")
            .map(|t| virtual_mounts_in(&t))
            .unwrap_or_default()
    } else if cfg!(target_os = "macos") {
        vec![std::path::PathBuf::from("/dev")]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod virtual_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn only_virtual_file_systems_are_named_and_escapes_are_undone() {
        let text = "\
22 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw
23 22 0:21 / /proc rw,nosuid - proc proc rw
24 22 0:22 / /sys rw,nosuid - sysfs sysfs rw
25 24 0:23 / /sys/fs/cgroup rw - cgroup2 cgroup2 rw
26 22 0:5 / /dev rw - devtmpfs udev rw
27 22 8:3 / /home rw shared:2 - ext4 /dev/sda3 rw
28 22 0:40 / /mnt/my\\040proc rw - proc proc rw
29 22 0:41 / /run rw - tmpfs tmpfs rw
";
        let mounts = virtual_mounts_in(text);
        for virtual_point in ["/proc", "/sys", "/sys/fs/cgroup", "/dev", "/mnt/my proc"] {
            assert!(
                mounts.iter().any(|m| m == Path::new(virtual_point)),
                "{virtual_point}"
            );
        }
        for real in ["/", "/home", "/run"] {
            assert!(!mounts.iter().any(|m| m == Path::new(real)), "{real}");
        }
    }
}
