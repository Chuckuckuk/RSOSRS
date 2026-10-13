//! Object placements (file 1 of a map group).

use crate::buf::Buf;

#[derive(Debug, Clone, Copy)]
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
