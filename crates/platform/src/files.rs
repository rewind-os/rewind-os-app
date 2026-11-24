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
<<<<<<< HEAD

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
=======
>>>>>>> origin/main
