//! Object placements (file 1 of a map group).

use crate::buf::Buf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Placement {
    pub id: u32,
    pub plane: u8,
    pub x: u8,
    pub z: u8,
    /// Object type / shape (0 = wall, 10 = scenery, 22 = ground decoration, ...).
    pub kind: u8,
    pub orientation: u8,
}

pub fn decode(data: &[u8]) -> Vec<Placement> {
    let mut b = Buf::new(data);
    let mut out = Vec::new();
    let mut id: i64 = -1;
    while b.pos < data.len() {
        let delta = b.smart_chain();
        if delta == 0 {
            break;
        }
        id += delta as i64;
        let mut pos: i32 = 0;
        loop {
            if b.pos >= data.len() {
                return out;
            }
            let d = b.smart_u() as i32;
            if d == 0 {
                break;
            }
            pos += d - 1;
            let attr = b.u8();
            out.push(Placement {
                id: id as u32,
                plane: ((pos >> 12) & 3) as u8,
                x: ((pos >> 6) & 63) as u8,
                z: (pos & 63) as u8,
                kind: attr >> 2,
                orientation: attr & 3,
            });
        }
    }
    out
}

impl Placement {
    /// Plane, x and z packed the way the file stores a position: `plane << 12 | x << 6 | z`.
    pub fn packed_position(&self) -> u32 {
        (self.plane as u32 & 3) << 12 | (self.x as u32 & 63) << 6 | (self.z as u32 & 63)
    }
}

/// Encodes placements as file 1 of a map group, the inverse of [`decode`]: grouped by object id
/// (ascending, as `smart_chain` deltas), each group's positions ascending (as `smart_u` deltas),
/// each followed by its attribute byte `kind << 2 | orientation`. Placements on the same spot keep
/// their order.
pub fn encode(placements: &[Placement]) -> Vec<u8> {
    let mut sorted = placements.to_vec();
    sorted.sort_by_key(|p| (p.id, p.packed_position()));
    let mut out = Vec::new();
    let mut last_id: i64 = -1;
    let mut i = 0;
    while i < sorted.len() {
        let id = sorted[i].id;
        write_smart_chain(&mut out, (id as i64 - last_id) as u32);
        last_id = id as i64;
        let mut last_pos = 0u32;
        while i < sorted.len() && sorted[i].id == id {
            let p = sorted[i];
            let pos = p.packed_position();
            write_smart_u(&mut out, pos - last_pos + 1);
            last_pos = pos;
            out.push((p.kind & 63) << 2 | (p.orientation & 3));
            i += 1;
        }
        write_smart_u(&mut out, 0);
    }
    write_smart_u(&mut out, 0);
    out
}

/// 1 byte below 128, else 2 bytes plus 32768 (the inverse of [`Buf::smart_u`]); `v < 32768`.
fn write_smart_u(out: &mut Vec<u8>, v: u32) {
    debug_assert!(v < 32768);
    if v < 128 {
        out.push(v as u8);
    } else {
        out.extend_from_slice(&((v + 32768) as u16).to_be_bytes());
    }
}

/// The inverse of [`Buf::smart_chain`]: 32767s, then the rest.
fn write_smart_chain(out: &mut Vec<u8>, mut v: u32) {
    while v >= 32767 {
        write_smart_u(out, 32767);
        v -= 32767;
    }
    write_smart_u(out, v);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(id: u32, plane: u8, x: u8, z: u8, kind: u8, orientation: u8) -> Placement {
        Placement { id, plane, x, z, kind, orientation }
    }

    #[test]
    fn encoded_placements_decode_back_sorted() {
        let ps = vec![
            at(70_000, 3, 63, 63, 22, 3),
            at(1276, 0, 10, 12, 10, 0),
            at(0, 0, 0, 0, 0, 0),
            at(1276, 0, 10, 12, 4, 1),
            at(1276, 0, 2, 40, 10, 2),
            at(32767, 1, 5, 5, 11, 1),
        ];
        let back = decode(&encode(&ps));
        let mut want = ps.clone();
        want.sort_by_key(|p| (p.id, p.packed_position()));
        assert_eq!(back, want);
        assert!(decode(&encode(&[])).is_empty());
    }
}
