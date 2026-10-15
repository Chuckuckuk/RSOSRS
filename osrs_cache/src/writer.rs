//! Writing groups into a copy of a cache.
//!
//! [`CacheWriter::create`] copies a cache's files (`main_file_cache.dat2` and every `.idxN`) to a
//! new directory; [`CacheWriter::put`] then appends a group's container to the copy's data file as
//! a fresh sector chain and points the group's index entry at it. The old sectors stay behind,
//! unreferenced, so a write never disturbs another group. The source cache is only ever read.
//!
//! Sector layout (as [`Cache::read_raw`](crate::Cache::read_raw) reads it): 520 bytes, a header of
//! `[group u16 (u32 when the id needs more than 16 bits)][chunk u16][next sector u24][archive u8]`,
//! then data. An index entry is `[length u24][first sector u24]` at `group * 6`.

use crate::{fmt_err, Result};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const SECTOR: u64 = crate::cache::SECTOR as u64;

/// Whether `name` is one of a cache's own files (the data file or an index).
pub fn is_cache_file(name: &str) -> bool {
    name == "main_file_cache.dat2" || name.strip_prefix("main_file_cache.idx").is_some_and(|n| n.parse::<u8>().is_ok())
}

pub struct CacheWriter {
    dir: PathBuf,
    dat: File,
}

impl CacheWriter {
    /// Copies the cache in `from` into the new directory `to` (which must not exist yet) and opens
    /// the copy for writing.
    pub fn create(from: &Path, to: &Path) -> Result<CacheWriter> {
        if to.exists() {
            return fmt_err(format!("{} already exists", to.display()));
        }
        if !crate::active::is_cache_dir(from) {
            return fmt_err(format!("{} holds no cache", from.display()));
        }
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_cache_file(&name) {
                std::fs::copy(entry.path(), to.join(&name))?;
            }
        }
        CacheWriter::open(to)
    }

    /// Opens an existing cache directory for writing.
    fn open(dir: &Path) -> Result<CacheWriter> {
        let dat = OpenOptions::new().read(true).write(true).open(dir.join("main_file_cache.dat2"))?;
        Ok(CacheWriter { dir: dir.to_path_buf(), dat })
    }

    /// Stores `container` (raw, as [`crate::container::encode`] makes it) as `archive/group`.
    pub fn put(&mut self, archive: u8, group: u32, container: &[u8]) -> Result<()> {
        if container.len() >= 1 << 24 {
            return fmt_err(format!("group {archive}/{group} is too large for an index entry"));
        }
        let wide = group > 0xFFFF;
        let header = if wide { 10 } else { 8 };
        let per_sector = SECTOR as usize - header;
        let end = self.dat.seek(SeekFrom::End(0))?;
        // sector 0 is never used: a next-sector of 0 ends a chain
        let first = end.div_ceil(SECTOR).max(1);
        let chunks = container.len().div_ceil(per_sector).max(1);
        if first + chunks as u64 >= 1 << 24 {
            return fmt_err("the data file is full (24-bit sector numbers)");
        }
        let mut out = Vec::with_capacity(chunks * SECTOR as usize);
        for (chunk, piece) in (0..chunks).map(|c| (c, &container[(c * per_sector).min(container.len())..((c + 1) * per_sector).min(container.len())])) {
            let next = if chunk + 1 == chunks { 0 } else { first as u32 + chunk as u32 + 1 };
            if wide {
                out.extend_from_slice(&group.to_be_bytes());
            } else {
                out.extend_from_slice(&(group as u16).to_be_bytes());
            }
            out.extend_from_slice(&(chunk as u16).to_be_bytes());
            out.extend_from_slice(&next.to_be_bytes()[1..]);
            out.push(archive);
            out.extend_from_slice(piece);
            out.resize((chunk + 1) * SECTOR as usize, 0);
        }
        self.dat.seek(SeekFrom::Start(first * SECTOR))?;
        self.dat.write_all(&out)?;

        let mut idx = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.dir.join(format!("main_file_cache.idx{archive}")))?;
        let at = group as u64 * 6;
        let len = idx.metadata()?.len();
        if len < at {
            // groups between the old end and this one are absent (zero entries)
            idx.set_len(at)?;
        }
        let mut entry = [0u8; 6];
        entry[..3].copy_from_slice(&(container.len() as u32).to_be_bytes()[1..]);
        entry[3..].copy_from_slice(&(first as u32).to_be_bytes()[1..]);
        idx.seek(SeekFrom::Start(at))?;
        idx.write_all(&entry)?;
        Ok(())
    }

    /// Flushes the data file to disk.
    pub fn finish(mut self) -> Result<()> {
        self.dat.flush()?;
        self.dat.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cache;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("osrs_cache_writer_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn groups_written_into_a_copy_read_back_and_leave_the_source_alone() {
        let src = temp_dir("src");
        std::fs::create_dir_all(&src).unwrap();
        // a one-sector data file and an empty index: an otherwise empty cache
        std::fs::write(src.join("main_file_cache.dat2"), vec![0u8; 520]).unwrap();
        std::fs::write(src.join("main_file_cache.idx5"), []).unwrap();
        std::fs::write(src.join("notes.txt"), b"not a cache file").unwrap();

        let dst = temp_dir("dst");
        let mut w = CacheWriter::create(&src, &dst).unwrap();
        assert!(!dst.join("notes.txt").exists());
        let small = crate::container::encode(b"hello", crate::container::Compression::None);
        let big: Vec<u8> = (0..3000u32).map(|i| i as u8).collect();
        let big = crate::container::encode(&big, crate::container::Compression::Gzip);
        w.put(5, 12850, &small).unwrap();
        w.put(5, 70_000, &big).unwrap();
        w.put(5, 3, &crate::container::encode(&vec![9u8; 1200], crate::container::Compression::None)).unwrap();
        w.finish().unwrap();

        let c = Cache::open(&dst).unwrap();
        assert_eq!(c.read_raw(5, 12850).unwrap(), small);
        assert_eq!(c.read_raw(5, 70_000).unwrap(), big);
        assert_eq!(c.read_group(5, 3).unwrap(), vec![9u8; 1200]);
        assert!(c.read_raw(5, 4).is_err());
        assert_eq!(std::fs::read(src.join("main_file_cache.dat2")).unwrap(), vec![0u8; 520]);
        assert!(std::fs::read(src.join("main_file_cache.idx5")).unwrap().is_empty());
        assert!(CacheWriter::create(&src, &dst).is_err(), "never writes over a directory");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }
}
