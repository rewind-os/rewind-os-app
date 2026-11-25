//! NTFS's master file table, read in one pass from a raw volume.
//!
//! Every file and folder on an NTFS volume has a record in the table, with its names (one per
//! hard link, each naming its parent folder's record), its size and its times. Reading the
//! table front to back is far faster than opening every folder, and it also sees folders that
//! even administrators cannot list. Only reading: nothing here writes to the volume.
//!
//! The volume is read in whole clusters at cluster-aligned offsets, as Windows requires of raw
//! volume handles. Every read of the table's bytes is checked, so a damaged record is counted
//! and skipped instead of stopping the scan.

use crate::Task;
use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The record of the volume's root folder.
pub const ROOT: u64 = 5;
/// Records below this number are NTFS's own metadata (`$MFT`, `$LogFile`, `$Extend`, …).
pub const FIRST_USER_RECORD: u64 = 24;

/// What the table says about one file or folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// Every name: the parent folder's record and the name in it. Hard links give several.
    pub names: Vec<(u64, Box<str>)>,
    pub dir: bool,
    /// Logical size of the file's data.
    pub bytes: u64,
    /// Space the data takes on the disk, told apart from `bytes` for compressed, sparse and
    /// cloud files only (as the folder-by-folder scan does).
    pub disk_bytes: u64,
    pub modified: Option<SystemTime>,
}

/// Every file and folder of a volume, by record number.
#[derive(Debug, Default)]
pub struct FileTable {
    pub entries: HashMap<u64, Entry>,
    /// Records marked damaged, or whose sector checks failed.
    pub damaged: u64,
}

const CHUNK: usize = 1 << 20;
const END_OF_ATTRIBUTES: u32 = 0xFFFF_FFFF;
const STANDARD_INFORMATION: u32 = 0x10;
const FILE_NAME: u32 = 0x30;
const DATA: u32 = 0x80;
const REPARSE_POINT: u32 = 0xC0;
const DOS_NAMESPACE: u8 = 2;
/// Reparse tags with this bit stand for another place (symbolic links, junctions, mount
/// points): never followed, like everywhere else.
const NAME_SURROGATE: u32 = 0x2000_0000;
/// File attributes whose size on disk differs from their length.
const SPARSE_LIKE: u32 = 0x0000_0200 // sparse
    | 0x0000_0800 // compressed
    | 0x0000_1000 // offline
    | 0x0004_0000 // recall on open
    | 0x0040_0000; // recall on data access
/// Attribute flags: compressed, sparse.
const PACKED_ATTRIBUTE: u16 = 0x0001 | 0x8000;
/// 100 ns intervals from 1601 to 1970.
const NT_TO_UNIX: u64 = 116_444_736_000_000_000;

fn bad(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        b.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// A file reference's record number (the low 48 bits; the rest is a sequence number).
fn record_number(reference: u64) -> u64 {
    reference & 0x0000_FFFF_FFFF_FFFF
}

/// Converts an NTFS time (100 ns since 1601) to a system time; `None` for zero or before 1970.
pub fn nt_time(ticks: u64) -> Option<SystemTime> {
    let since = ticks.checked_sub(NT_TO_UNIX).filter(|_| ticks != 0)?;
    let nanos = u32::try_from((since % 10_000_000) * 100).ok()?;
    UNIX_EPOCH.checked_add(Duration::new(since / 10_000_000, nanos))
}

struct Boot {
    cluster: u64,
    record: usize,
    mft_offset: u64,
}

fn boot(sector: &[u8]) -> Option<Boot> {
    if sector.get(3..11)? != b"NTFS    " || sector.get(510..512)? != [0x55, 0xAA] {
        return None;
    }
    let sector_size = u64::from(u16_at(sector, 0x0B)?);
    if !matches!(sector_size, 512 | 1024 | 2048 | 4096) {
        return None;
    }
    let raw = *sector.get(0x0D)?;
    let sectors = if raw > 0x80 {
        1u64.checked_shl(256 - u32::from(raw))?
    } else {
        u64::from(raw)
    };
    let cluster = sector_size.checked_mul(sectors)?;
    if !cluster.is_power_of_two() || cluster > 2 << 20 {
        return None;
    }
    let raw = i8::from_le_bytes([*sector.get(0x40)?]);
    let record = if raw > 0 {
        cluster.checked_mul(u64::try_from(raw).ok()?)?
    } else {
        1u64.checked_shl(u32::from(raw.unsigned_abs()))?
    };
    if !(256..=65_536).contains(&record) || !record.is_power_of_two() {
        return None;
    }
    Some(Boot {
        cluster,
        record: usize::try_from(record).ok()?,
        mft_offset: u64_at(sector, 0x30)?.checked_mul(cluster)?,
    })
}

/// Undoes NTFS's sector check: the last two bytes of each sector were swapped for a check
/// value, kept in the record's update sequence. `None` when a sector was torn.
fn fixup(record: &mut [u8]) -> Option<()> {
    let at = usize::from(u16_at(record, 4)?);
    let count = usize::from(u16_at(record, 6)?);
    if count < 2 {
        return None;
    }
    let stride = record.len() / (count - 1);
    if stride < 2 {
        return None;
    }
    let check: [u8; 2] = record.get(at..at + 2)?.try_into().ok()?;
    for i in 1..count {
        let saved: [u8; 2] = record.get(at + 2 * i..at + 2 * i + 2)?.try_into().ok()?;
        let end = i * stride;
        let tail = record.get_mut(end - 2..end)?;
        if tail != check {
            return None;
        }
        tail.copy_from_slice(&saved);
    }
    Some(())
}

fn little_endian(bytes: &[u8], signed: bool) -> Option<i64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    let negative = signed && bytes.last().is_some_and(|b| b & 0x80 != 0);
    let mut out = [if negative { 0xFF } else { 0 }; 8];
    out.get_mut(..bytes.len())?.copy_from_slice(bytes);
    let value = i64::from_le_bytes(out);
    (signed || value >= 0).then_some(value)
}

/// A non-resident value's data runs: (first cluster, or `None` for a sparse run; clusters).
fn data_runs(mut bytes: &[u8]) -> Option<Vec<(Option<u64>, u64)>> {
    let mut runs = Vec::new();
    let mut lcn: i64 = 0;
    loop {
        let (&header, rest) = bytes.split_first()?;
        if header == 0 {
            return Some(runs);
        }
        let (len_size, off_size) = (usize::from(header & 0x0F), usize::from(header >> 4));
        let length = u64::try_from(little_endian(rest.get(..len_size)?, false)?).ok()?;
        let rest = rest.get(len_size..)?;
        if off_size == 0 {
            runs.push((None, length));
        } else {
            lcn = lcn.checked_add(little_endian(rest.get(..off_size)?, true)?)?;
            runs.push((Some(u64::try_from(lcn).ok()?), length));
        }
        bytes = rest.get(off_size..)?;
    }
}

/// The unnamed `$DATA` attribute's sizes, from the attribute that starts the stream.
#[derive(Debug, Clone, Copy, Default)]
struct Data {
    bytes: u64,
    allocated: Option<u64>,
    /// Space really used, given for compressed and sparse streams.
    packed: Option<u64>,
}

/// What one record (a file's base record or one of its extension records) holds.
#[derive(Debug, Default)]
struct Part {
    base: bool,
    dir: bool,
    names: Vec<(u64, Box<str>)>,
    std: Option<(u64, u32)>,
    data: Option<Data>,
    /// A reparse point: its tag, or `None` when the tag could not be read.
    reparse: Option<Option<u32>>,
    /// The `$MFT`'s own data runs (record 0 only).
    runs: Option<Vec<(Option<u64>, u64)>>,
}

enum Record {
    Unused,
    Damaged,
    Used { owner: u64, part: Part },
}

fn resident_value(attribute: &[u8]) -> Option<&[u8]> {
    let length = usize::try_from(u32_at(attribute, 0x10)?).ok()?;
    let offset = usize::from(u16_at(attribute, 0x14)?);
    attribute.get(offset..offset.checked_add(length)?)
}

fn read_attribute(part: &mut Part, attribute: &[u8]) -> Option<()> {
    let ty = u32_at(attribute, 0)?;
    let non_resident = *attribute.get(8)? != 0;
    let unnamed = *attribute.get(9)? == 0;
    let flags = u16_at(attribute, 0x0C)?;
    match (ty, non_resident) {
        (STANDARD_INFORMATION, false) => {
            let value = resident_value(attribute)?;
            part.std = Some((u64_at(value, 0x08)?, u32_at(value, 0x20)?));
        }
        (FILE_NAME, false) => {
            let value = resident_value(attribute)?;
            let length = usize::from(*value.get(0x40)?);
            if *value.get(0x41)? == DOS_NAMESPACE {
                return Some(());
            }
            let units: Vec<u16> = value
                .get(0x42..0x42 + 2 * length)?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            let name = String::from_utf16_lossy(&units).into_boxed_str();
            part.names.push((record_number(u64_at(value, 0)?), name));
        }
        (DATA, false) if unnamed => {
            let bytes = u64::try_from(resident_value(attribute)?.len()).ok()?;
            part.data = Some(Data {
                bytes,
                ..Data::default()
            });
        }
        (DATA, true) if unnamed => {
            // Only the attribute that starts the stream carries its sizes.
            if u64_at(attribute, 0x10)? == 0 {
                part.data = Some(Data {
                    bytes: u64_at(attribute, 0x30)?,
                    allocated: u64_at(attribute, 0x28),
                    packed: if flags & PACKED_ATTRIBUTE != 0 {
                        u64_at(attribute, 0x40)
                    } else {
                        None
                    },
                });
                let runs_at = usize::from(u16_at(attribute, 0x20)?);
                part.runs = attribute.get(runs_at..).and_then(data_runs);
            }
        }
        (REPARSE_POINT, false) => {
            part.reparse = Some(resident_value(attribute).and_then(|v| u32_at(v, 0)));
        }
        (REPARSE_POINT, true) => part.reparse = Some(None),
        _ => {}
    }
    Some(())
}

fn read_record(record: &mut [u8], number: u64) -> Record {
    match record.get(0..4) {
        Some(b"FILE") => {}
        Some(b"BAAD") => return Record::Damaged,
        _ => return Record::Unused,
    }
    if fixup(record).is_none() {
        return Record::Damaged;
    }
    let record: &[u8] = record;
    let (Some(flags), Some(base), Some(first), Some(used)) = (
        u16_at(record, 0x16),
        u64_at(record, 0x20),
        u16_at(record, 0x14),
        u32_at(record, 0x18),
    ) else {
        return Record::Damaged;
    };
    if flags & 0x0001 == 0 {
        return Record::Unused;
    }
    let base = record_number(base);
    let mut part = Part {
        base: base == 0,
        dir: flags & 0x0002 != 0,
        ..Part::default()
    };
    let used = usize::try_from(used)
        .unwrap_or(usize::MAX)
        .min(record.len());
    let mut at = usize::from(first);
    while let Some(ty) = u32_at(record, at) {
        if ty == END_OF_ATTRIBUTES {
            break;
        }
        let Some(length) = u32_at(record, at + 4).and_then(|l| usize::try_from(l).ok()) else {
            break;
        };
        let Some(attribute) = at
            .checked_add(length)
            .filter(|end| length >= 0x10 && *end <= used)
            .and_then(|end| record.get(at..end))
        else {
            return Record::Damaged;
        };
        if read_attribute(&mut part, attribute).is_none() {
            return Record::Damaged;
        }
        at += length;
    }
    Record::Used {
        owner: if base == 0 { number } else { base },
        part,
    }
}

fn read_at<R: Read + Seek>(volume: &mut R, offset: u64, buf: &mut [u8]) -> io::Result<()> {
    volume.seek(SeekFrom::Start(offset))?;
    volume.read_exact(buf)
}

/// Gathers the parts of every file, from its base record and its extension records.
#[derive(Default)]
struct Gathered {
    parts: HashMap<u64, Part>,
    damaged: u64,
}

impl Gathered {
    fn add(&mut self, record: &mut [u8], number: u64) {
        match read_record(record, number) {
            Record::Unused => {}
            Record::Damaged => self.damaged += 1,
            Record::Used { owner, part } => {
                let into = self.parts.entry(owner).or_default();
                into.base |= part.base;
                into.dir |= part.dir;
                into.names.extend(part.names);
                into.std = into.std.or(part.std);
                into.data = into.data.or(part.data);
                into.reparse = into.reparse.or(part.reparse);
            }
        }
    }

    fn finish(self) -> FileTable {
        let entries = self
            .parts
            .into_iter()
            .filter_map(|(number, part)| {
                let link = part
                    .reparse
                    .is_some_and(|tag| tag.is_none_or(|t| t & NAME_SURROGATE != 0));
                if !part.base || link || part.names.is_empty() {
                    return None;
                }
                let mut names = part.names;
                names.dedup();
                let (time, attributes) = part.std.unwrap_or_default();
                let data = part.data.unwrap_or_default();
                let disk_bytes = if attributes & SPARSE_LIKE != 0 || data.packed.is_some() {
                    data.packed.or(data.allocated).unwrap_or(data.bytes)
                } else {
                    data.bytes
                };
                let entry = Entry {
                    names,
                    dir: part.dir,
                    bytes: if part.dir { 0 } else { data.bytes },
                    disk_bytes: if part.dir { 0 } else { disk_bytes },
                    modified: nt_time(time),
                };
                Some((number, entry))
            })
            .collect();
        FileTable {
            entries,
            damaged: self.damaged,
        }
    }
}

/// Reads the master file table of the NTFS volume `volume` covers, from its first byte.
/// Stops early, keeping what it read, when `task` is cancelled.
pub fn read<R: Read + Seek>(volume: &mut R, task: &Task) -> io::Result<FileTable> {
    // 4 KiB is whole sectors for every sector size NTFS uses.
    let mut first = vec![0u8; 4096];
    read_at(volume, 0, &mut first)?;
    let boot = boot(&first).ok_or_else(|| bad("not an NTFS volume"))?;
    let cluster = usize::try_from(boot.cluster).map_err(|_| bad("cluster size"))?;
    let record_len = boot.record;

    // Record 0 describes the table itself: where its pieces are and how long it is.
    let mut head = vec![0u8; record_len.div_ceil(cluster) * cluster];
    read_at(volume, boot.mft_offset, &mut head)?;
    let record0 = head
        .get_mut(..record_len)
        .ok_or_else(|| bad("file table record"))?;
    let (table_bytes, runs) = match read_record(record0, 0) {
        Record::Used { part, .. } => {
            let size = part.data.map(|d| d.bytes);
            (size, part.runs)
        }
        _ => (None, None),
    };
    let (Some(table_bytes), Some(runs)) = (table_bytes, runs) else {
        return Err(bad("the file table does not describe itself"));
    };
    let covered = runs
        .iter()
        .try_fold(0u64, |sum, (_, clusters)| {
            sum.checked_add(clusters.checked_mul(boot.cluster)?)
        })
        .ok_or_else(|| bad("file table size"))?;
    if covered < table_bytes {
        // The table's own description is split over several records: not read here.
        return Err(bad("the file table is too fragmented"));
    }
    let records = table_bytes / record_len as u64;

    let mut gathered = Gathered::default();
    let mut number: u64 = 0;
    let mut pending: Vec<u8> = Vec::with_capacity(CHUNK + record_len);
    let chunk = CHUNK.max(cluster);
    'runs: for (lcn, clusters) in runs {
        let mut left = clusters.saturating_mul(boot.cluster);
        let Some(lcn) = lcn else {
            // A hole in the table: its records were never written.
            number += left / record_len as u64;
            pending.clear();
            continue;
        };
        let mut offset = lcn.saturating_mul(boot.cluster);
        while left > 0 {
            if task.is_cancelled() || number >= records {
                break 'runs;
            }
            let take = usize::try_from(left).unwrap_or(chunk).min(chunk);
            let start = pending.len();
            pending.resize(start + take, 0);
            if let Some(buf) = pending.get_mut(start..) {
                read_at(volume, offset, buf)?;
            }
            offset += take as u64;
            left -= take as u64;
            let whole = pending.len() / record_len * record_len;
            for record in pending
                .get_mut(..whole)
                .into_iter()
                .flat_map(|b| b.chunks_exact_mut(record_len))
            {
                if number >= records {
                    break;
                }
                gathered.add(record, number);
                number += 1;
            }
            pending.drain(..whole);
        }
    }
    Ok(gathered.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_runs_follow_relative_offsets_and_holes() {
        // 4 clusters at 0x10, a hole of 2, then 1 cluster 3 clusters before the first run.
        let runs = data_runs(&[0x11, 0x04, 0x10, 0x01, 0x02, 0x11, 0x01, 0xFD, 0x00]).unwrap();
        assert_eq!(runs, vec![(Some(0x10), 4), (None, 2), (Some(0x0D), 1)]);
        assert!(data_runs(&[0x11, 0x04]).is_none(), "a cut run is refused");
    }

    #[test]
    fn a_torn_sector_marks_the_record_damaged() {
        let mut record = vec![0u8; 1024];
        record[0..4].copy_from_slice(b"FILE");
        record[4..6].copy_from_slice(&0x30u16.to_le_bytes());
        record[6..8].copy_from_slice(&3u16.to_le_bytes());
        record[0x30..0x32].copy_from_slice(&[7, 7]);
        record[510..512].copy_from_slice(&[7, 7]);
        record[1022..1024].copy_from_slice(&[9, 9]);
        assert!(matches!(read_record(&mut record, 30), Record::Damaged));
    }

    #[test]
    fn nt_times_before_1970_are_unknown() {
        assert!(nt_time(0).is_none());
        assert!(nt_time(NT_TO_UNIX - 1).is_none());
        assert!(nt_time(NT_TO_UNIX + 10_000_000).is_some_and(|t| t > UNIX_EPOCH));
    }
}
