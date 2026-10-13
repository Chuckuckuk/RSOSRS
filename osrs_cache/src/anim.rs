//! Skeletal-label animation: sequences (config group 12), frames (archive 0) and the frame bases
//! (archive 1) that say which vertex labels each frame transform moves.
//!
//! A frame is a list of transforms; each transform has a type from its frame base (origin, translate,
//! rotate, scale, alpha) and the vertex labels it applies to. [`pose`] applies one frame to a
//! model's vertices exactly like the client's `Model.animate`.

use crate::buf::Buf;
use crate::{cfg, fmt_err, idx, Cache, Index, Result};
use std::collections::HashMap;

/// Transform kinds in a frame base.
pub mod kind {
    pub const ORIGIN: u8 = 0;
    pub const TRANSLATE: u8 = 1;
    pub const ROTATE: u8 = 2;
    pub const SCALE: u8 = 3;
    pub const ALPHA: u8 = 5;
}

#[derive(Debug, Clone, Default)]
pub struct FrameBase {
    pub id: u32,
    /// Per transform slot: its [`kind`].
    pub types: Vec<u8>,
    /// Per transform slot: the vertex labels it moves.
    pub labels: Vec<Vec<u8>>,
}

impl FrameBase {
    pub fn decode(id: u32, data: &[u8]) -> Result<FrameBase> {
        let mut b = Buf::new(data);
        let n = b.u8() as usize;
        let types: Vec<u8> = (0..n).map(|_| b.u8()).collect();
        let counts: Vec<usize> = (0..n).map(|_| b.u8() as usize).collect();
        let labels: Vec<Vec<u8>> = counts.iter().map(|c| (0..*c).map(|_| b.u8()).collect()).collect();
        // this revision appends two more bytes (0, 0 in every base) that nothing here uses
        if b.pos + 2 != data.len() {
            return fmt_err(format!("frame base {id}: {} bytes left over", data.len() as isize - b.pos as isize));
        }
        Ok(FrameBase { id, types, labels })
    }
}

/// One transform of a frame: slot in the frame base plus its x/y/z operands.
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    pub slot: u16,
    pub d: [i32; 3],
}

#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// Archive-1 group of this frame's base.
    pub base: u32,
    pub transforms: Vec<Transform>,
    pub has_alpha: bool,
}

impl Frame {
    pub fn decode(data: &[u8], bases: &mut impl FnMut(u32) -> Option<FrameBase>) -> Result<Frame> {
        let mut head = Buf::new(data);
        let base_id = head.u16() as u32;
        let count = head.u8() as usize;
        let Some(base) = bases(base_id) else { return fmt_err(format!("missing frame base {base_id}")) };
        if count > base.types.len() {
            return fmt_err(format!("frame has {count} slots, base {base_id} only {}", base.types.len()));
        }
        // flags are one byte per slot, operands follow them
        let mut ops = Buf::at(data, head.pos + count);
        let mut f = Frame { base: base_id, ..Default::default() };
        let mut last: i32 = -1;
        for i in 0..count {
            let flags = head.u8();
            if flags == 0 {
                continue;
            }
            let ty = base.types[i];
            if ty != kind::ORIGIN {
                // a transform that follows an origin slot needs that origin: insert a neutral one
                for j in ((last + 1)..i as i32).rev() {
                    if base.types[j as usize] == kind::ORIGIN {
                        f.transforms.push(Transform { slot: j as u16, d: [0, 0, 0] });
                        break;
                    }
                }
            }
            let default = if ty == kind::SCALE { 128 } else { 0 };
            let mut d = [default; 3];
            for (axis, v) in d.iter_mut().enumerate() {
                if flags & (1 << axis) != 0 {
                    *v = ops.smart_s();
                }
            }
            f.transforms.push(Transform { slot: i as u16, d });
            last = i as i32;
            f.has_alpha |= ty == kind::ALPHA;
        }
        if ops.pos != data.len() {
            return fmt_err(format!("frame of base {base_id}: {} operand bytes unread", data.len() as isize - ops.pos as isize));
        }
        Ok(f)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Sequence {
    pub id: u32,
    /// Duration of each frame in client ticks (20 ms).
    pub delays: Vec<u16>,
    /// `(frame group, file)` of each frame.
    pub frames: Vec<(u32, u32)>,
    pub loop_offset: Option<u16>,
    pub max_loops: u8,
    pub priority: u8,
    /// What the left (shield) hand holds while this plays: see [`Sequence::held_item`].
    pub left_hand: Option<u16>,
    /// What the right (weapon) hand holds while this plays.
    pub right_hand: Option<u16>,
}

/// What a sequence puts in one hand while it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldItem {
    /// Nothing: whatever is worn there is put away (a woodcutter's weapon).
    Empty,
    /// This item (a woodcutter's axe).
    Item(u32),
}

impl Sequence {
    pub fn decode(id: u32, data: &[u8]) -> Sequence {
        let mut s = Sequence { id, ..Default::default() };
        let mut b = Buf::new(data);
        while b.remaining() > 0 {
            match b.u8() {
                0 => break,
                1 => {
                    let n = b.u16() as usize;
                    s.delays = (0..n).map(|_| b.u16()).collect();
                    let files: Vec<u32> = (0..n).map(|_| b.u16() as u32).collect();
                    let groups: Vec<u32> = (0..n).map(|_| b.u16() as u32).collect();
                    s.frames = groups.into_iter().zip(files).collect();
                }
                2 => s.loop_offset = Some(b.u16()),
                3 => {
                    for _ in 0..b.u8() {
                        b.u8();
                    }
                }
                4 => {}
                5 | 9 | 11 => {
                    b.u8();
                }
                6 => s.left_hand = Some(b.u16()),
                7 => s.right_hand = Some(b.u16()),
                8 => s.max_loops = b.u8(),
                10 => s.priority = b.u8(),
                12 => {
                    let n = b.u8();
                    for _ in 0..n * 2 {
                        b.u16();
                    }
                }
                // this revision prefixes sequences with a one-byte opcode 16
                16 => {
                    b.u8();
                }
                // 13 and up are sound-effect tables with variable-length entries that nothing
                // here needs; they close the definition
                13.. => break,
            }
        }
        s
    }

    /// What the left and right hand hold while this plays, where it says: the client stores an
    /// item as its id plus 512, and 0 for an empty hand.
    pub fn held_items(&self) -> [Option<HeldItem>; 2] {
        let held = |raw: Option<u16>| {
            raw.and_then(|v| match v {
                0 => Some(HeldItem::Empty),
                512.. => Some(HeldItem::Item(v as u32 - 512)),
                _ => None,
            })
        };
        [held(self.left_hand), held(self.right_hand)]
    }

    /// Total length in client ticks.
    pub fn duration(&self) -> u32 {
        self.delays.iter().map(|d| *d as u32).sum()
    }
}

/// Lazily decoded sequences, frames and frame bases.
pub struct Animations<'a> {
    cache: &'a Cache,
    seq_index: Index,
    frame_index: Index,
    base_index: Index,
    bases: HashMap<u32, Option<FrameBase>>,
}

impl<'a> Animations<'a> {
    pub fn new(cache: &'a Cache) -> Result<Animations<'a>> {
        Ok(Animations {
            cache,
            seq_index: cache.index(idx::CONFIG)?,
            frame_index: cache.index(idx::ANIM_FRAMES)?,
            base_index: cache.index(idx::SKELETONS)?,
            bases: HashMap::new(),
        })
    }

    pub fn sequence(&self, id: u32) -> Result<Sequence> {
        let data = self.cache.read_file(&self.seq_index, cfg::SEQUENCE, id)?;
        Ok(Sequence::decode(id, &data))
    }

    pub fn base(&mut self, id: u32) -> Option<FrameBase> {
        if !self.bases.contains_key(&id) {
            let decoded = self
                .cache
                .read_file(&self.base_index, id, 0)
                .ok()
                .and_then(|d| FrameBase::decode(id, &d).ok());
            self.bases.insert(id, decoded);
        }
        self.bases[&id].clone()
    }

    pub fn frame(&mut self, group: u32, file: u32) -> Result<Frame> {
        let data = self.cache.read_file(&self.frame_index, group, file)?;
        Frame::decode(&data, &mut |id| self.base(id))
    }

    /// All frames of a sequence, in order.
    pub fn frames_of(&mut self, seq: &Sequence) -> Result<Vec<Frame>> {
        seq.frames.iter().map(|(g, f)| self.frame(*g, *f)).collect()
    }
}

/// Every frame file of the cache decodes against its base: used by the format sweep.
pub fn frame_groups(cache: &Cache) -> Result<Vec<(u32, Vec<u32>)>> {
    let ix = cache.index(idx::ANIM_FRAMES)?;
    Ok(ix.groups.iter().map(|(g, grp)| (*g, grp.files.iter().map(|f| f.id).collect())).collect())
}

/// Vertex positions in model units; mutated by [`pose`].
pub type Positions = Vec<[f32; 3]>;

/// Apply `frame` to `pos` (vertices of a model whose per-vertex labels are `skin`), as the client's
/// `Model.animate` does: origin / translate / rotate / scale transforms over label groups.
pub fn pose(pos: &mut Positions, skin: &[u8], base: &FrameBase, frame: &Frame) {
    let mut origin = [0.0f32; 3];
    for t in &frame.transforms {
        let ty = base.types.get(t.slot as usize).copied().unwrap_or(255);
        let Some(labels) = base.labels.get(t.slot as usize) else { continue };
        let d = [t.d[0] as f32, t.d[1] as f32, t.d[2] as f32];
        let in_group = |v: usize| skin.get(v).is_some_and(|s| labels.contains(s));
        match ty {
            kind::ORIGIN => {
                let (mut sum, mut n) = ([0.0f32; 3], 0u32);
                for v in 0..pos.len() {
                    if in_group(v) {
                        for k in 0..3 {
                            sum[k] += pos[v][k];
                        }
                        n += 1;
                    }
                }
                origin = if n > 0 {
                    [sum[0] / n as f32 + d[0], sum[1] / n as f32 + d[1], sum[2] / n as f32 + d[2]]
                } else {
                    d
                };
            }
            kind::TRANSLATE => {
                for v in 0..pos.len() {
                    if in_group(v) {
                        for k in 0..3 {
                            pos[v][k] += d[k];
                        }
                    }
                }
            }
            kind::ROTATE => {
                // angles are 1/256 turns of the 2048-step table, i.e. (value & 255) * 8
                let angle = |v: i32| (v & 255) as f32 * 8.0 * std::f32::consts::TAU / 2048.0;
                let (ax, ay, az) = (angle(t.d[0]), angle(t.d[1]), angle(t.d[2]));
                for v in 0..pos.len() {
                    if !in_group(v) {
                        continue;
                    }
                    let mut p = [pos[v][0] - origin[0], pos[v][1] - origin[1], pos[v][2] - origin[2]];
                    if az != 0.0 {
                        let (s, c) = az.sin_cos();
                        let x = p[1] * s + p[0] * c;
                        p[1] = p[1] * c - p[0] * s;
                        p[0] = x;
                    }
                    if ax != 0.0 {
                        let (s, c) = ax.sin_cos();
                        let y = p[1] * c - p[2] * s;
                        p[2] = p[1] * s + p[2] * c;
                        p[1] = y;
                    }
                    if ay != 0.0 {
                        let (s, c) = ay.sin_cos();
                        let x = p[2] * s + p[0] * c;
                        p[2] = p[2] * c - p[0] * s;
                        p[0] = x;
                    }
                    pos[v] = [p[0] + origin[0], p[1] + origin[1], p[2] + origin[2]];
                }
            }
            kind::SCALE => {
                for v in 0..pos.len() {
                    if in_group(v) {
                        for k in 0..3 {
                            pos[v][k] = (pos[v][k] - origin[k]) * d[k] / 128.0 + origin[k];
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// A sequence baked against one rig: the posed vertices of every frame and how long each lasts.
#[derive(Debug, Clone, Default)]
pub struct Clip {
    pub sequence: u32,
    /// Frame durations in client ticks (20 ms).
    pub delays: Vec<u16>,
    /// Posed vertex positions of each frame, parallel to the rig's vertices.
    pub frames: Vec<Positions>,
}

impl Clip {
    /// Pose `rig` with every frame of `sequence_id`. Frames reference a frame base; each frame is
    /// applied to the bind pose, like the client's per-frame `animate`.
    pub fn bake(anims: &mut Animations, sequence_id: u32, rig: &crate::scene::Rig) -> Result<Clip> {
        let seq = anims.sequence(sequence_id)?;
        if seq.frames.is_empty() {
            return fmt_err(format!("sequence {sequence_id} has no frames"));
        }
        let mut clip = Clip { sequence: sequence_id, ..Default::default() };
        for (delay, (group, file)) in seq.delays.iter().zip(&seq.frames) {
            let frame = anims.frame(*group, *file)?;
            let Some(base) = anims.base(frame.base) else {
                return fmt_err(format!("sequence {sequence_id}: frame base {} unavailable", frame.base));
            };
            let mut pos = rig.positions.clone();
            pose(&mut pos, &rig.skin, &base, &frame);
            clip.delays.push(*delay);
            clip.frames.push(pos);
        }
        Ok(clip)
    }

    /// Total length in client ticks.
    pub fn duration(&self) -> u32 {
        self.delays.iter().map(|d| *d as u32).sum()
    }

    /// Index of the frame showing at `ticks` into the (looping) clip.
    pub fn frame_at(&self, ticks: f32) -> usize {
        let total = self.duration().max(1) as f32;
        let mut t = ticks.rem_euclid(total);
        for (i, d) in self.delays.iter().enumerate() {
            if t < *d as f32 {
                return i;
            }
            t -= *d as f32;
        }
        self.delays.len().saturating_sub(1)
    }
}
