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
