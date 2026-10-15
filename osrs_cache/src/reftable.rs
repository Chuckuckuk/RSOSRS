//! An archive's reference table (group 255/`archive`), decoded without losing anything so it can be
//! written back: the map editor changes a few groups, then re-encodes the table with their new
//! CRCs and revisions (see [`crate::writer`]).
//!
//! Layout (protocols 5-7, 7 using "big smart" ids and counts):
//! `[protocol u8][revision u32 if >= 6][flags u8][group count]`, then column by column: group id
//! deltas, name hashes (flag 1), CRCs, an unknown u32 (flag 8), whirlpool digests (flag 2, 64
//! bytes), compressed and uncompressed sizes (flag 4), revisions, file counts, file id deltas, file
//! name hashes (flag 1).

use crate::buf::Buf;
use crate::cache::{FileEntry, Group, Index};
use crate::{fmt_err, Result};
use std::collections::{BTreeMap, HashMap};

pub const FLAG_NAMES: u8 = 1;
pub const FLAG_WHIRLPOOL: u8 = 2;
pub const FLAG_SIZES: u8 = 4;
pub const FLAG_UNKNOWN: u8 = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct RefTable {
    pub protocol: u8,
    pub revision: u32,
    pub flags: u8,
    /// In table order (ascending ids).
    pub groups: Vec<RefGroup>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefGroup {
    pub id: u32,
    pub name_hash: i32,
    /// CRC-32 of the group's container, without its trailing version.
    pub crc: u32,
    /// Flag 8: one u32 per group, carried through unread.
    pub unknown: [u8; 4],
    /// Flag 2: whirlpool digest of the container.
    pub whirlpool: [u8; 64],
    /// Flag 4: compressed and uncompressed sizes.
    pub sizes: [u8; 8],
    pub revision: u32,
    /// `(file id, name hash)`, ascending ids.
    pub files: Vec<(u32, i32)>,
}

impl RefGroup {
    pub fn new(id: u32) -> RefGroup {
        RefGroup { id, name_hash: 0, crc: 0, unknown: [0; 4], whirlpool: [0; 64], sizes: [0; 8], revision: 0, files: Vec::new() }
    }
}

impl RefTable {
    pub fn decode(data: &[u8]) -> Result<RefTable> {
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
        let mut groups: Vec<RefGroup> = Vec::with_capacity(count);
        let mut last = 0u32;
        for _ in 0..count {
            last = last.wrapping_add(rd(&mut b));
            groups.push(RefGroup::new(last));
        }
        if flags & FLAG_NAMES != 0 {
            for g in &mut groups {
                g.name_hash = b.i32();
            }
        }
        for g in &mut groups {
            g.crc = b.u32();
        }
        let take = |b: &mut Buf, out: &mut [u8]| {
            for o in out.iter_mut() {
                *o = b.u8();
            }
        };
        if flags & FLAG_UNKNOWN != 0 {
            for g in &mut groups {
                take(&mut b, &mut g.unknown);
            }
        }
        if flags & FLAG_WHIRLPOOL != 0 {
            for g in &mut groups {
                take(&mut b, &mut g.whirlpool);
            }
        }
        if flags & FLAG_SIZES != 0 {
            for g in &mut groups {
                take(&mut b, &mut g.sizes);
            }
        }
        for g in &mut groups {
            g.revision = b.u32();
        }
        let counts: Vec<usize> = (0..count).map(|_| rd(&mut b) as usize).collect();
        for (g, &n) in groups.iter_mut().zip(&counts) {
            let mut last = 0u32;
            for _ in 0..n {
                last = last.wrapping_add(rd(&mut b));
                g.files.push((last, 0));
            }
        }
        if flags & FLAG_NAMES != 0 {
            for g in &mut groups {
                for f in &mut g.files {
                    f.1 = b.i32();
                }
            }
        }
        if b.pos > data.len() {
            return fmt_err("reference table truncated");
        }
        Ok(RefTable { protocol, revision, flags, groups })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let wide = self.protocol >= 7;
        let wr = |out: &mut Vec<u8>, v: u32| {
            if !wide {
                out.extend_from_slice(&(v as u16).to_be_bytes());
            } else if v < 0x8000 {
                out.extend_from_slice(&(v as u16).to_be_bytes());
            } else {
                out.extend_from_slice(&(v | 0x8000_0000).to_be_bytes());
            }
        };
        out.push(self.protocol);
        if self.protocol >= 6 {
            out.extend_from_slice(&self.revision.to_be_bytes());
        }
        out.push(self.flags);
        wr(&mut out, self.groups.len() as u32);
        let mut last = 0u32;
        for g in &self.groups {
            wr(&mut out, g.id.wrapping_sub(last));
            last = g.id;
        }
        if self.flags & FLAG_NAMES != 0 {
            for g in &self.groups {
                out.extend_from_slice(&g.name_hash.to_be_bytes());
            }
        }
        for g in &self.groups {
            out.extend_from_slice(&g.crc.to_be_bytes());
        }
        if self.flags & FLAG_UNKNOWN != 0 {
            for g in &self.groups {
                out.extend_from_slice(&g.unknown);
            }
        }
        if self.flags & FLAG_WHIRLPOOL != 0 {
            for g in &self.groups {
                out.extend_from_slice(&g.whirlpool);
            }
        }
        if self.flags & FLAG_SIZES != 0 {
            for g in &self.groups {
                out.extend_from_slice(&g.sizes);
            }
        }
        for g in &self.groups {
            out.extend_from_slice(&g.revision.to_be_bytes());
        }
        for g in &self.groups {
            wr(&mut out, g.files.len() as u32);
        }
        for g in &self.groups {
            let mut last = 0u32;
            for &(id, _) in &g.files {
                wr(&mut out, id.wrapping_sub(last));
                last = id;
            }
        }
        if self.flags & FLAG_NAMES != 0 {
            for g in &self.groups {
                for &(_, h) in &g.files {
                    out.extend_from_slice(&h.to_be_bytes());
                }
            }
        }
        out
    }

    pub fn group(&self, id: u32) -> Option<&RefGroup> {
        self.groups.binary_search_by_key(&id, |g| g.id).ok().map(|i| &self.groups[i])
    }

    pub fn group_mut(&mut self, id: u32) -> Option<&mut RefGroup> {
        self.groups.binary_search_by_key(&id, |g| g.id).ok().map(|i| &mut self.groups[i])
    }

    /// The group `id`, added (empty, in order) if the table does not list it yet.
    pub fn group_or_insert(&mut self, id: u32) -> &mut RefGroup {
        let i = match self.groups.binary_search_by_key(&id, |g| g.id) {
            Ok(i) => i,
            Err(i) => {
                self.groups.insert(i, RefGroup::new(id));
                i
            }
        };
        &mut self.groups[i]
    }

    /// Records that group `id` now holds `container` (as written to the data file): its CRC, sizes
    /// and a revision one past the table's newest, and a blanked whirlpool digest, which nothing
    /// here checks and which would otherwise be stale.
    pub fn touch(&mut self, id: u32, container: &[u8], uncompressed_len: usize) {
        let crc = crc32(container);
        let revision = self.groups.iter().map(|g| g.revision).max().unwrap_or(0).wrapping_add(1);
        let g = self.group_or_insert(id);
        g.crc = crc;
        g.revision = revision;
        g.whirlpool = [0; 64];
        g.sizes[..4].copy_from_slice(&(container.len() as u32).to_be_bytes());
        g.sizes[4..].copy_from_slice(&(uncompressed_len as u32).to_be_bytes());
    }

    /// The read-side view of this table.
    pub fn to_index(&self, archive: u8) -> Index {
        let named = self.flags & FLAG_NAMES != 0;
        let mut by_name = HashMap::new();
        let mut groups = BTreeMap::new();
        for g in &self.groups {
            if named {
                by_name.insert(g.name_hash, g.id);
            }
            let files = g.files.iter().map(|&(id, name_hash)| FileEntry { id, name_hash }).collect();
            groups.insert(g.id, Group { id: g.id, name_hash: g.name_hash, crc: g.crc, revision: g.revision, files });
        }
        Index::new(archive, self.revision, groups, by_name)
    }
}

/// CRC-32 (IEEE), as the reference tables use.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(data);
    crc.sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(protocol: u8, flags: u8) -> RefTable {
        let mut a = RefGroup::new(3);
        a.name_hash = -5;
        a.crc = 0xDEAD_BEEF;
        a.revision = 9;
        a.whirlpool[0] = 7;
        a.files = vec![(0, 11), (1, 12), (40, 13)];
        let mut b = RefGroup::new(70_000);
        b.files = vec![(0, 0), (35_000, 1)];
        RefTable { protocol, revision: 4, flags, groups: vec![a, b] }
    }

    #[test]
    fn a_table_round_trips_in_every_protocol() {
        for (protocol, flags) in [(7, 0b1111), (7, 0), (6, FLAG_NAMES)] {
            let mut t = sample(protocol, flags);
            if protocol < 7 {
                // narrow ids
                t.groups[1].id = 900;
                t.groups[1].files[1].0 = 300;
            }
            let mut back = RefTable::decode(&t.encode()).unwrap();
            if flags & FLAG_NAMES == 0 {
                for g in &mut t.groups {
                    g.name_hash = 0;
                    g.files.iter_mut().for_each(|f| f.1 = 0);
                }
            }
            if flags & FLAG_WHIRLPOOL == 0 {
                t.groups.iter_mut().for_each(|g| g.whirlpool = [0; 64]);
            }
            back.groups.iter_mut().zip(&t.groups).for_each(|(b, t)| assert_eq!(b, t));
            assert_eq!(back, t);
        }
    }

    #[test]
    fn touching_a_new_group_inserts_it_in_order() {
        let mut t = sample(7, 0b1111);
        t.touch(10, b"hello", 99);
        let ids: Vec<u32> = t.groups.iter().map(|g| g.id).collect();
        assert_eq!(ids, [3, 10, 70_000]);
        let g = t.group(10).unwrap();
        assert_eq!(g.crc, crc32(b"hello"));
        assert_eq!(g.revision, 10);
        assert_eq!(&g.sizes[4..], &99u32.to_be_bytes());
    }
}
