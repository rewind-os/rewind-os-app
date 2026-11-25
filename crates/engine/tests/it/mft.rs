use rewind_os_engine::Task;
use rewind_os_engine::scan::{self, ScanTree};
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

/// The test volume, unpacked.
fn volume() -> io::Result<Vec<u8>> {
    let packed = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ntfs/testfs1.gz"),
    )?;
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(&packed[..]).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Refuses what a raw Windows volume refuses: reads that do not start and end on a sector.
struct WholeSectors(Cursor<Vec<u8>>);

impl Read for WholeSectors {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.0.position().is_multiple_of(512) || !buf.len().is_multiple_of(512) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        self.0.read(buf)
    }
}

impl Seek for WholeSectors {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.0.seek(to)
    }
}

fn scan_volume(inside: &[&str]) -> Result<ScanTree, Box<dyn std::error::Error>> {
    let mut reader = WholeSectors(Cursor::new(volume()?));
    let inside: Vec<String> = inside.iter().map(|s| s.to_string()).collect();
    Ok(scan::scan_file_table(
        &mut reader,
        Path::new("X:\\"),
        &inside,
        &Task::new(),
    )?)
}

#[test]
fn the_file_table_gives_every_file_and_folder_of_the_volume() {
    let tree = scan_volume(&[]).unwrap();
    assert!(tree.from_file_table);
    let root = tree.root_node().unwrap();
    // empty-file, file-with-12345, 1000-bytes-file, sparse-file.
    assert_eq!(root.own_files, 4);
    let many = tree
        .children(ScanTree::ROOT)
        .find(|(_, n)| &*n.name == "many_subdirs")
        .map(|(id, _)| id)
        .unwrap();
    assert_eq!(tree.children(many).count(), 512);
    let named = |name: &str| {
        tree.top_files
            .iter()
            .find(|f| f.path.file_name().is_some_and(|n| n == name))
            .unwrap_or_else(|| panic!("{name} not found"))
    };
    assert_eq!(named("1000-bytes-file").bytes, 1000);
    assert_eq!(named("file-with-12345").bytes, 5);
    let sparse = named("sparse-file");
    assert!(sparse.disk_bytes < sparse.bytes, "the hole takes no space");
    assert!(root.bytes >= sparse.bytes + 1005);
    assert!(
        tree.top_files
            .iter()
            .all(|f| !f.path.to_string_lossy().contains('$')),
        "NTFS's own files are never shown"
    );
    // Written in 2023, when the volume was made.
    assert!(
        sparse
            .modified
            .is_some_and(|t| t.timestamp() > 1_600_000_000)
    );
}

#[test]
fn a_folder_inside_the_volume_is_found_whatever_the_case() {
    let tree = scan_volume(&["MANY_SUBDIRS"]).unwrap();
    let root = tree.root_node().unwrap();
    assert_eq!(root.own_files, 0);
    assert_eq!(tree.dir_count(), 513);
    assert!(
        tree.children(ScanTree::ROOT)
            .all(|(_, n)| n.parent == Some(ScanTree::ROOT))
    );
}

#[test]
fn a_missing_folder_or_another_file_system_is_refused() {
    let mut reader = WholeSectors(Cursor::new(volume().unwrap()));
    let missing = ["nowhere".to_string()];
    assert!(
        scan::scan_file_table(
            &mut reader,
            Path::new("X:\\nowhere"),
            &missing,
            &Task::new()
        )
        .is_err()
    );
    let mut blank = WholeSectors(Cursor::new(vec![0u8; 1 << 20]));
    assert!(scan::scan_file_table(&mut blank, Path::new("X:\\"), &[], &Task::new()).is_err());
}
