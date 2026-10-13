use crate::buf::Buf;
use crate::{container, fmt_err, name_hash, xtea, Error, Result};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SECTOR: usize = 520;

pub struct Cache {
    dir: PathBuf,
    /// The game's data file; `None` for a cache that reads from a [`bundle`](crate::bundle).
    dat: Option<Mutex<File>>,
    idx: Mutex<HashMap<u8, Vec<(u32, u32)>>>,
    bundle: Option<std::sync::Arc<crate::bundle::Bundle>>,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub id: u32,
    pub name_hash: i32,
    pub crc: u32,
    pub revision: u32,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Copy)]
pub struct FileEntry {
    pub id: u32,
    pub name_hash: i32,
}

/// The reference table of one archive (index).
pub struct Index {
    pub archive: u8,
    pub revision: u32,
    pub groups: BTreeMap<u32, Group>,
    by_name: HashMap<i32, u32>,
}

impl Index {
    pub fn group_by_name(&self, name: &str) -> Option<&Group> {
        self.by_name.get(&name_hash(name)).and_then(|id| self.groups.get(id))
    }
}

impl Cache {
    pub fn open(dir: impl AsRef<Path>) -> Result<Cache> {
        let dir = dir.as_ref().to_path_buf();
        let dat = File::open(dir.join("main_file_cache.dat2"))?;
        Ok(Cache { dir, dat: Some(Mutex::new(dat)), idx: Mutex::new(HashMap::new()), bundle: None })
    }

    /// A cache that reads from `bundle` only.
    pub fn from_bundle(bundle: std::sync::Arc<crate::bundle::Bundle>) -> Cache {
        Cache { dir: PathBuf::new(), dat: None, idx: Mutex::new(HashMap::new()), bundle: Some(bundle) }
    }

    /// `%USERPROFILE%\.runelite\jagexcache\oldschool\LIVE`, falling back to `~/jagexcache`.
    pub fn open_default() -> Result<Cache> {
        // a program carrying its own cache (see `bundle`) reads that, not the installed game
        if let Some(bundle) = crate::bundle::installed() {
            return Ok(Cache::from_bundle(bundle));
        }
        let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
        for rel in [".runelite/jagexcache/oldschool/LIVE", "jagexcache/oldschool/LIVE"] {
            let p = Path::new(&home).join(rel);
            if p.join("main_file_cache.dat2").exists() {
                return Cache::open(p);
            }
        }
        Err(Error::Missing("no OSRS cache found under the home directory".into()))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn idx_entry(&self, index: u8, group: u32) -> Result<(u32, u32)> {
        let mut map = self.idx.lock().unwrap();
        if !map.contains_key(&index) {
            let bytes = std::fs::read(self.dir.join(format!("main_file_cache.idx{index}")))?;
            let v = bytes
                .chunks_exact(6)
                .map(|c| {
                    let len = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
                    let sec = (c[3] as u32) << 16 | (c[4] as u32) << 8 | c[5] as u32;
                    (len, sec)
                })
                .collect();
            map.insert(index, v);
        }
        match map[&index].get(group as usize) {
            Some(&(len, sec)) if len > 0 => Ok((len, sec)),
            _ => Err(Error::Missing(format!("group {index}/{group}"))),
        }
    }

    /// Raw (still compressed/encrypted) container bytes of `index/group`.
    pub fn read_raw(&self, index: u8, group: u32) -> Result<Vec<u8>> {
        crate::bundle::record(index, group);
        if let Some(bundle) = &self.bundle {
            return bundle.get(index, group).map(<[u8]>::to_vec).ok_or_else(|| crate::bundle::missing(index, group));
        }
        let (len, mut sector) = self.idx_entry(index, group)?;
        let len = len as usize;
        let mut out = Vec::with_capacity(len);
        let mut dat = self.dat.as_ref().expect("a file-backed cache has its data file").lock().unwrap();
        let mut chunk = 0u32;
        let mut sec = [0u8; SECTOR];
        while out.len() < len {
            dat.seek(SeekFrom::Start(sector as u64 * SECTOR as u64))?;
            dat.read_exact(&mut sec)?;
            let ext = group > 0xFFFF;
            let (hdr, id) = if ext {
                (10, u32::from_be_bytes(sec[0..4].try_into().unwrap()))
            } else {
                (8, u16::from_be_bytes(sec[0..2].try_into().unwrap()) as u32)
            };
            let o = if ext { 4 } else { 2 };
            let ch = u16::from_be_bytes(sec[o..o + 2].try_into().unwrap()) as u32;
            let next = (sec[o + 2] as u32) << 16 | (sec[o + 3] as u32) << 8 | sec[o + 4] as u32;
            let ix = sec[o + 5];
            if id != group || ch != chunk || ix != index {
                return fmt_err(format!("sector mismatch in {index}/{group} chunk {chunk}"));
            }
            let take = (len - out.len()).min(SECTOR - hdr);
            out.extend_from_slice(&sec[hdr..hdr + take]);
            sector = next;
            chunk += 1;
        }
        Ok(out)
    }

    /// Decoded container bytes of `index/group`.
    pub fn read_group(&self, index: u8, group: u32) -> Result<Vec<u8>> {
        container::decode(&self.read_raw(index, group)?, None)
    }

    pub fn read_group_keyed(&self, index: u8, group: u32, key: Option<&xtea::Key>) -> Result<Vec<u8>> {
        container::decode(&self.read_raw(index, group)?, key)
    }

    /// Split a multi-file group into its files.
    pub fn read_files(&self, index: &Index, group: u32) -> Result<Vec<(u32, Vec<u8>)>> {
        let g = index.groups.get(&group).ok_or_else(|| Error::Missing(format!("group {group}")))?;
        let data = self.read_group(index.archive, group)?;
        split_files(&data, &g.files)
    }

    /// One file of a group (e.g. an object definition inside the config archive).
    pub fn read_file(&self, index: &Index, group: u32, file: u32) -> Result<Vec<u8>> {
        self.read_files(index, group)?
            .into_iter()
            .find(|(id, _)| *id == file)
            .map(|(_, d)| d)
            .ok_or_else(|| Error::Missing(format!("file {group}/{file}")))
    }

    pub fn index(&self, archive: u8) -> Result<Index> {
        let data = self.read_group(255, archive as u32)?;
        parse_reference_table(archive, &data)
    }
}

fn split_files(data: &[u8], files: &[FileEntry]) -> Result<Vec<(u32, Vec<u8>)>> {
    let n = files.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    if n == 1 {
        return Ok(vec![(files[0].id, data.to_vec())]);
    }
    let chunks = *data.last().unwrap() as usize;
    let trailer = chunks * n * 4;
    if data.len() < trailer + 1 {
        return fmt_err("group trailer larger than data");
    }
    let mut b = Buf::at(data, data.len() - 1 - trailer);
    let mut sizes = vec![vec![0usize; chunks]; n];
    for c in 0..chunks {
        let mut delta = 0i32;
        for f in 0..n {
            delta = delta.wrapping_add(b.i32());
            sizes[f][c] = delta as usize;
        }
    }
    let mut out: Vec<Vec<u8>> = vec![Vec::new(); n];
    let mut pos = 0;
    for c in 0..chunks {
        for f in 0..n {
            let s = sizes[f][c];
            if pos + s > data.len() {
                return fmt_err("group chunk overruns data");
            }
            out[f].extend_from_slice(&data[pos..pos + s]);
            pos += s;
        }
    }
    Ok(files.iter().zip(out).map(|(f, d)| (f.id, d)).collect())
}

fn parse_reference_table(archive: u8, data: &[u8]) -> Result<Index> {
    let mut b = Buf::new(data);
    let protocol = b.u8();
    if !(5..=7).contains(&protocol) {
        return fmt_err(format!("reference table protocol {protocol}"));
    }
    let revision = if protocol >= 6 { b.u32() } else { 0 };
    let flags = b.u8();
    let wide = protocol >= 7;
    let rd = |b: &mut Buf| -> u32 {
        if wide {
            b.big_smart() as u32
        } else {
            b.u16() as u32
        }
    };
    let count = rd(&mut b) as usize;
    let mut groups: Vec<Group> = Vec::with_capacity(count);
    let mut last = 0u32;
    for _ in 0..count {
        last = last.wrapping_add(rd(&mut b));
        groups.push(Group { id: last, name_hash: 0, crc: 0, revision: 0, files: Vec::new() });
    }
    if flags & 1 != 0 {
        for g in &mut groups {
            g.name_hash = b.i32();
        }
    }
    for g in &mut groups {
        g.crc = b.u32();
    }
    if flags & 8 != 0 {
        b.pos += 4 * count;
    }
    if flags & 2 != 0 {
        b.pos += 64 * count;
    }
    if flags & 4 != 0 {
        b.pos += 8 * count;
    }
    for g in &mut groups {
        g.revision = b.u32();
    }
    let mut counts = Vec::with_capacity(count);
    for _ in 0..count {
        counts.push(rd(&mut b) as usize);
    }
    for (g, &n) in groups.iter_mut().zip(&counts) {
        let mut last = 0u32;
        for _ in 0..n {
            last = last.wrapping_add(rd(&mut b));
            g.files.push(FileEntry { id: last, name_hash: 0 });
        }
    }
    if flags & 1 != 0 {
        for g in &mut groups {
            for f in &mut g.files {
                f.name_hash = b.i32();
            }
        }
    }
    let mut by_name = HashMap::new();
    let mut map = BTreeMap::new();
    for g in groups {
        if flags & 1 != 0 {
            by_name.insert(g.name_hash, g.id);
        }
        map.insert(g.id, g);
    }
    Ok(Index { archive, revision, groups: map, by_name })
}
