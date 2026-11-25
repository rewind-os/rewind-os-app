//! Reading a whole local volume as raw bytes (`\\.\C:` on Windows), so the disk analyzer can
//! read NTFS's master file table in one pass instead of opening every folder. Only for reading:
//! nothing here can write to a volume.

use std::fs::File;
use std::path::{Component, Path, Prefix};

/// A volume opened for reading, and where the scanned folder sits inside it.
#[derive(Debug)]
pub struct RawVolume {
    pub file: File,
    /// Folder names from the volume's root down to the scanned folder; empty for the root.
    pub inside: Vec<String>,
}

/// The drive letter of `path` and the folder names below the drive's root. `None` for paths
/// that are not on a lettered drive (network shares, relative paths) or that go up (`..`).
pub fn split_drive_path(path: &Path) -> Option<(char, Vec<String>)> {
    let mut components = path.components();
    let letter = match components.next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => char::from(letter),
            _ => return None,
        },
        _ => return None,
    };
    let mut inside = Vec::new();
    for component in components {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => inside.push(name.to_str()?.to_owned()),
            Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    letter.is_ascii_alphabetic().then_some((letter, inside))
}

/// Opens the local volume holding `path` for reading raw sectors. Needs administrator rights;
/// `None` when they are missing, on other systems, or for paths not on a lettered drive. The
/// caller still has to check that the volume really holds `path` (a `subst` drive opens the
/// volume of the folder it stands for).
#[cfg(windows)]
pub fn open_raw(path: &Path) -> Option<RawVolume> {
    let (letter, inside) = split_drive_path(path)?;
    let file = File::open(format!(r"\\.\{letter}:")).ok()?;
    Some(RawVolume { file, inside })
}

/// Opens the local volume holding `path` for reading raw sectors. Only on Windows.
#[cfg(not(windows))]
pub fn open_raw(_path: &Path) -> Option<RawVolume> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn only_lettered_drives_are_split() {
        let (letter, inside) = split_drive_path(Path::new(r"C:\Users\me")).unwrap();
        assert_eq!((letter, inside), ('C', vec!["Users".into(), "me".into()]));
        assert!(split_drive_path(Path::new(r"\\?\D:\")).is_some_and(|(_, i)| i.is_empty()));
        assert!(split_drive_path(Path::new(r"\\server\share\x")).is_none());
        assert!(split_drive_path(Path::new(r"C:\a\..\b")).is_none());
    }
}
