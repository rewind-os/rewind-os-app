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

/// One line of `/proc/self/mountinfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub id: u64,
    /// `major:minor` of the file system.
    pub device: String,
    /// The folder of that file system shown here (`/` for all of it).
    pub root: std::path::PathBuf,
    /// Where it is shown.
    pub point: std::path::PathBuf,
    pub kind: String,
}

/// Reads `/proc/self/mountinfo` text: id, parent, device, root and mount point come first
/// (paths with spaces and other bytes written as `\040` and the like), the kind follows the
/// ` - ` separator. Lines that do not fit are left out.
pub fn parse_mountinfo(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let mut fields = before.split_whitespace();
            let id = fields.next()?.parse().ok()?;
            let device = fields.nth(1)?.to_owned();
            let root = std::path::PathBuf::from(unescape_octal(fields.next()?));
            let point = std::path::PathBuf::from(unescape_octal(fields.next()?));
            let kind = after.split_whitespace().next()?.to_owned();
            Some(Mount {
                id,
                device,
                root,
                point,
                kind,
            })
        })
        .collect()
}

/// The mount points of virtual file systems among `mounts`.
pub fn virtual_mounts_in(mounts: &[Mount]) -> Vec<std::path::PathBuf> {
    mounts
        .iter()
        .filter(|m| VIRTUAL_KINDS.contains(&m.kind.as_str()))
        .map(|m| m.point.clone())
        .collect()
}

/// Mounts below `root` that show again files another mount below `root` already shows: the
/// same file system, and a folder of it inside the one the other shows (a bind mount; on WSL
/// `/mnt/wslg/distro` is `/` again). Each comes with the mount point of the one that shows
/// them first: the one showing more of the file system or, for the same folder, the older
/// mount. A repeat is only named while its original is counted: inside `root`, not inside it,
/// and not inside another repeat, so no file ends up counted nowhere. Virtual file systems are
/// left out; they are skipped anyway.
pub fn repeated_mounts_in(
    mounts: &[Mount],
    root: &std::path::Path,
) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    let real: Vec<&Mount> = mounts
        .iter()
        .filter(|m| !VIRTUAL_KINDS.contains(&m.kind.as_str()))
        .collect();
    let shows_first = |a: &Mount, b: &Mount| {
        let (da, db) = (a.root.components().count(), b.root.components().count());
        da < db || (da == db && a.id < b.id)
    };
    // Nothing inside a virtual file system is entered: neither a repeat nor an original there.
    let virtual_points = virtual_mounts_in(mounts);
    let counted = |point: &std::path::Path| {
        point.starts_with(root) && !virtual_points.iter().any(|v| point.starts_with(v))
    };
    let mut repeats: Vec<(std::path::PathBuf, std::path::PathBuf)> = real
        .iter()
        .filter(|m| counted(&m.point) && m.point != root)
        .filter_map(|m| {
            let original = real
                .iter()
                .filter(|o| o.id != m.id && o.device == m.device)
                .filter(|o| m.root.starts_with(&o.root) && shows_first(o, m))
                .filter(|o| counted(&o.point))
                .find(|o| !o.point.starts_with(&m.point))?;
            Some((m.point.clone(), original.point.clone()))
        })
        .collect();
    // An original inside another repeat is not counted: keep its repeat, then look again.
    loop {
        let hidden = repeats.iter().position(|(_, original)| {
            repeats
                .iter()
                .any(|(other, _)| other != original && original.starts_with(other))
        });
        match hidden {
            Some(index) => {
                repeats.remove(index);
            }
            None => return repeats,
        }
    }
}

fn mounts() -> Vec<Mount> {
    std::fs::read_to_string("/proc/self/mountinfo")
        .map(|t| parse_mountinfo(&t))
        .unwrap_or_default()
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
        virtual_mounts_in(&mounts())
    } else if cfg!(target_os = "macos") {
        vec![std::path::PathBuf::from("/dev")]
    } else {
        Vec::new()
    }
}

/// Mounts below `root` that repeat files another mount shows, with that mount (Linux; empty
/// elsewhere).
pub fn repeated_mounts(root: &std::path::Path) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    if cfg!(target_os = "linux") {
        repeated_mounts_in(&mounts(), root)
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
        let mounts = virtual_mounts_in(&parse_mountinfo(text));
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

    #[test]
    fn a_mount_that_shows_files_again_is_named_with_the_one_shown_first() {
        // WSL: the distribution's drive at `/` and again at `/mnt/wslg/distro`; a bind mount of
        // a folder of `/home`; Btrfs subvolumes of one file system; a second drive.
        let text = "\
60 1 8:48 / / rw - ext4 /dev/sdd rw
70 60 8:48 / /mnt/wslg/distro rw - ext4 /dev/sdd rw
71 60 8:49 / /home rw - ext4 /dev/sde rw
72 60 8:49 /rdvl/shared /srv/shared rw - ext4 /dev/sde rw
73 60 0:31 /@data /data rw - btrfs /dev/sdf rw
74 60 0:31 /@media /media/big rw - btrfs /dev/sdf rw
75 60 8:64 / /mnt/backup rw - ext4 /dev/sdg rw
76 60 0:21 / /proc rw - proc proc rw
77 60 0:21 / /mnt/proc-again rw - proc proc rw
";
        let repeated = repeated_mounts_in(&parse_mountinfo(text), Path::new("/"));
        let has = |dup: &str, original: &str| {
            repeated
                .iter()
                .any(|(d, o)| d == Path::new(dup) && o == Path::new(original))
        };
        assert!(has("/mnt/wslg/distro", "/"));
        assert!(has("/srv/shared", "/home"));
        // The root is shown first, never the repeat.
        assert!(!repeated.iter().any(|(d, _)| d == Path::new("/")));
        // Different folders of one file system, other drives and virtual ones are kept.
        for kept in [
            "/data",
            "/media/big",
            "/mnt/backup",
            "/home",
            "/mnt/proc-again",
        ] {
            assert!(
                !repeated.iter().any(|(d, _)| d == Path::new(kept)),
                "{kept}"
            );
        }
    }

    #[test]
    fn a_repeat_is_only_skipped_while_its_original_is_counted() {
        // WSL: `/run/user` repeats `/mnt/wslg`, and `/mnt/wslg/run/user/1000` repeats
        // `/run/user/1000`, which sits inside `/run/user`.
        let text = "\
82 1 8:48 / / rw - ext4 /dev/sdd rw
83 82 0:40 / /mnt/wslg rw - tmpfs none rw
132 82 0:40 /run/user /run/user rw - tmpfs none rw
212 132 0:77 / /run/user/1000 rw - tmpfs tmpfs rw
226 83 0:77 / /mnt/wslg/run/user/1000 rw - tmpfs tmpfs rw
";
        let mounts = parse_mountinfo(text);
        let skipped = |root: &str| -> Vec<std::path::PathBuf> {
            repeated_mounts_in(&mounts, Path::new(root))
                .into_iter()
                .map(|(d, _)| d)
                .collect()
        };
        let all = skipped("/");
        assert!(all.iter().any(|d| d == Path::new("/run/user")));
        // Its original is inside a skipped folder, so this copy is the one counted.
        assert!(
            !all.iter()
                .any(|d| d == Path::new("/mnt/wslg/run/user/1000"))
        );
        // Scanning `/mnt` alone: the originals are not in it, so nothing is skipped.
        assert!(skipped("/mnt").is_empty());
        let wsl = parse_mountinfo(
            "82 1 8:48 / / rw - ext4 /dev/sdd rw\n84 82 8:48 / /mnt/wslg/distro rw - ext4 /dev/sdd rw\n",
        );
        assert!(!repeated_mounts_in(&wsl, Path::new("/")).is_empty());
        // Inside `/dev`, which is never entered, nothing is named and no original counts.
        let shm = parse_mountinfo(
            "1 0 8:1 / / rw - ext4 x rw\n2 1 0:5 / /dev rw - devtmpfs x rw\n3 1 0:6 / /run/shm rw - tmpfs x rw\n4 2 0:6 / /dev/shm rw - tmpfs x rw\n5 2 0:7 / /dev/old rw - tmpfs x rw\n6 1 0:7 / /srv/new rw - tmpfs x rw\n",
        );
        assert!(repeated_mounts_in(&shm, Path::new("/")).is_empty());
        // Inside `/mnt` only part of `/` is counted: its full copy is kept.
        assert!(repeated_mounts_in(&wsl, Path::new("/mnt")).is_empty());
        // Scanning `/run`: `/run/user` stays, since `/mnt/wslg` is not in the scan.
        assert!(!skipped("/run").iter().any(|d| d == Path::new("/run/user")));
    }
}
