//! Config definitions (archive 2): floor underlays/overlays and object (loc) definitions.

use crate::buf::Buf;
use crate::{cfg, idx, Cache, Result};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Underlay {
    pub rgb: u32,
    pub texture: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct Overlay {
    pub rgb: u32,
    pub texture: Option<u8>,
    pub hide_underlay: bool,
    pub secondary_rgb: Option<u32>,
    /// Average colour (16-bit HSL) of `texture`, filled in by [`Configs::overlays`].
    pub texture_hsl: Option<u16>,
}

impl Default for Overlay {
    fn default() -> Self {
        Overlay { rgb: 0, texture: None, hide_underlay: true, secondary_rgb: None, texture_hsl: None }
    }
}

impl Underlay {
    pub fn decode(data: &[u8]) -> Underlay {
        let mut b = Buf::new(data);
        let mut d = Underlay::default();
        loop {
            match b.u8() {
                0 => break,
                1 => d.rgb = b.u24(),
                2 => {
                    let t = b.u8();
                    d.texture = if t == 255 { None } else { Some(t) };
                }
                3 => {
                    b.u16();
                }
                4 => {}
                5 => {}
                _ => break,
            }
            if b.pos >= data.len() {
                break;
            }
        }
        d
    }
}

impl Overlay {
    pub fn decode(data: &[u8]) -> Overlay {
        let mut b = Buf::new(data);
        let mut d = Overlay::default();
        loop {
            if b.pos >= data.len() {
                break;
            }
            match b.u8() {
                0 => break,
                1 => d.rgb = b.u24(),
                2 => {
                    let t = b.u8();
                    d.texture = if t == 255 { None } else { Some(t) };
                }
                5 => d.hide_underlay = false,
                7 => d.secondary_rgb = Some(b.u24()),
                8 => {}
                9 => {
                    b.u16();
                }
                10 | 12 => {}
                11 | 14 | 16 | 21 => {
                    b.u8();
                }
                13 => {
                    b.u24();
                }
                15 | 20 | 22 => {
                    b.u16();
                }
                _ => break,
            }
        }
        d
    }
}

/// Newer opcodes whose payload is skipped (size inferred by checking that definitions parse to
/// their exact end).
const KNOWN_SKIPS: [(u8, usize); 4] = [(90, 0), (93, 6), (95, 1), (96, 1)];

/// Object (loc) definition, trimmed to what rendering and placement need.
#[derive(Debug, Clone, Default)]
pub struct ObjectDef {
    pub id: u32,
    pub name: String,
    pub models: Vec<u32>,
    /// Parallel to `models` when the def lists per-type models (opcode 1).
    pub model_types: Vec<u8>,
    pub size_x: u8,
    pub size_y: u8,
    pub solid: bool,
    pub impenetrable: bool,
    /// The client's clip type: 0 = does not block movement, 1 and 2 (default) = blocks.
    pub interact_type: i32,
    /// Opcode 19 (-1 when absent): floor decorations only block movement when this is 1.
    pub interactive_flag: i32,
    pub anim: Option<u32>,
    pub ambient: i8,
    pub contrast: i8,
    /// How far (1/128 tile) decorations on this wall stand off from it; the client's default is 16.
    pub decor_displacement: Option<u8>,
    /// Menu options by slot (index 0 is the left-click option); empty where there is none.
    pub actions: Vec<String>,
    pub recolor_from: Vec<u16>,
    pub recolor_to: Vec<u16>,
    pub retexture_from: Vec<u16>,
    pub retexture_to: Vec<u16>,
    pub model_size_x: u16,
    pub model_size_y: u16,
    pub model_size_z: u16,
    pub offset_x: i16,
    pub offset_y: i16,
    pub offset_z: i16,
    pub rotated: bool,
    pub clip_type: i32,
    pub map_icon: Option<u16>,
    /// Opcode 68: frame of the `mapscene` sprite group drawn for this object on the minimap.
    pub map_scene: Option<u16>,
    /// The client's `contouredGround`: `Some(0)` (opcode 21) bends the whole model onto the terrain
    /// under it; `Some(n)` (opcode 81, n = byte * 256) bends only the part below `n / 65536` of the
    /// model's height. `None` keeps the model rigid at the placement height.
    pub contoured_ground: Option<i32>,
    pub transforms: Vec<i32>,
    pub varbit: Option<u32>,
    pub varp: Option<u32>,
    pub category: Option<u16>,
    /// Set when decoding stopped at an opcode this decoder does not know.
    pub unknown_opcode: Option<u8>,
}

impl ObjectDef {
    /// What a left-click on this object does, if it does anything that is not just walking: the
    /// first option, unless it is missing, hidden or the plain "Walk here".
    pub fn left_click(&self) -> Option<&str> {
        let first = self.actions.first()?.trim();
        let plain = first.is_empty() || first.eq_ignore_ascii_case("hidden") || first.eq_ignore_ascii_case("walk here");
        (!plain).then_some(first)
    }

    pub fn decode(id: u32, data: &[u8]) -> ObjectDef {
        Self::decode_with(id, data, &KNOWN_SKIPS).0
    }

    /// Decode with extra `(opcode, payload bytes)` skips; also returns whether the whole
    /// definition was consumed exactly (terminator at the end of the data).
    pub fn decode_with(id: u32, data: &[u8], skips: &[(u8, usize)]) -> (ObjectDef, bool) {
        let mut clean = false;
        let mut d = ObjectDef {
            id,
            size_x: 1,
            size_y: 1,
            solid: true,
            impenetrable: true,
            interact_type: 2,
            model_size_x: 128,
            model_size_y: 128,
            model_size_z: 128,
            clip_type: -1,
            interactive_flag: -1,
            ..Default::default()
        };
        let mut b = Buf::new(data);
        loop {
            if b.pos >= data.len() {
                break;
            }
            let op = b.u8();
            match op {
                0 => {
                    clean = b.pos == data.len();
                    break;
                }
                1 => {
                    let n = b.u8();
                    for _ in 0..n {
                        let m = b.u16() as u32;
                        let t = b.u8();
                        d.models.push(m);
                        d.model_types.push(t);
                    }
                }
                // this revision widened model ids to u32: 6 = typed models, 7 = untyped models
                6 => {
                    let n = b.u8();
                    for _ in 0..n {
                        let m = b.u32();
                        let t = b.u8();
                        d.models.push(m);
                        d.model_types.push(t);
                    }
                }
                7 => {
                    let n = b.u8();
                    for _ in 0..n {
                        d.models.push(b.u32());
                    }
                }
                2 => d.name = b.string(),
                3 => {
                    b.string();
                }
                5 => {
                    let n = b.u8();
                    for _ in 0..n {
                        d.models.push(b.u16() as u32);
                    }
                }
                14 => d.size_x = b.u8(),
                15 => d.size_y = b.u8(),
                17 => {
                    d.interact_type = 0;
                    d.solid = false;
                }
                18 => d.impenetrable = false,
                19 => d.interactive_flag = b.u8() as i32,
                21 => d.contoured_ground = Some(0),
                22 | 23 => {}
                24 => {
                    let a = b.u16();
                    d.anim = if a == 0xFFFF { None } else { Some(a as u32) };
                }
                27 => d.interact_type = 1,
                28 => d.decor_displacement = Some(b.u8()),
                29 => d.ambient = b.i8(),
                30..=34 => {
                    // slot by opcode: the first (left-click) option is opcode 30 even if it is absent
                    let slot = (op - 30) as usize;
                    if d.actions.len() <= slot {
                        d.actions.resize(slot + 1, String::new());
                    }
                    d.actions[slot] = b.string();
                }
                39 => d.contrast = b.i8(),
                40 => {
                    let n = b.u8();
                    for _ in 0..n {
                        d.recolor_from.push(b.u16());
                        d.recolor_to.push(b.u16());
                    }
                }
                41 => {
                    let n = b.u8();
                    for _ in 0..n {
                        d.retexture_from.push(b.u16());
                        d.retexture_to.push(b.u16());
                    }
                }
                42 => {
                    let n = b.u8();
                    b.pos += n as usize;
                }
                61 => d.category = Some(b.u16()),
                62 => d.rotated = true,
                64 => {}
                65 => d.model_size_x = b.u16(),
                66 => d.model_size_y = b.u16(),
                67 => d.model_size_z = b.u16(),
                68 => d.map_scene = Some(b.u16()),
                69 => {
                    b.u8();
                }
                70 => d.offset_x = b.i16(),
                71 => d.offset_y = b.i16(),
                72 => d.offset_z = b.i16(),
                73 | 74 => {}
                75 => {
                    b.u8();
                }
                77 | 92 => {
                    let varbit = b.u16();
                    d.varbit = if varbit == 0xFFFF { None } else { Some(varbit as u32) };
                    let varp = b.u16();
                    d.varp = if varp == 0xFFFF { None } else { Some(varp as u32) };
                    let default = if op == 92 {
                        let v = b.u16();
                        if v == 0xFFFF { -1 } else { v as i32 }
                    } else {
                        -1
                    };
                    let n = b.u8();
                    for _ in 0..=n {
                        let v = b.u16();
                        d.transforms.push(if v == 0xFFFF { -1 } else { v as i32 });
                    }
                    if op == 92 {
                        d.transforms.push(default);
                    }
                }
                78 => {
                    b.u16();
                    b.u8();
                    b.u8();
                }
                79 => {
                    b.u16();
                    b.u16();
                    b.u8();
                    b.u8();
                    let n = b.u8();
                    for _ in 0..n {
                        b.u16();
                    }
                }
                81 => d.contoured_ground = Some(b.u8() as i32 * 256),
                82 => d.map_icon = Some(b.u16()),
                89 => {}
                // multi-option menu entry: u8, u16, u16, u32, u32, label
                101 => {
                    b.u8();
                    b.u16();
                    b.u16();
                    b.u32();
                    b.u32();
                    b.string();
                }
                249 => {
                    let n = b.u8();
                    for _ in 0..n {
                        let is_str = b.u8() == 1;
                        b.u24();
                        if is_str {
                            b.string();
                        } else {
                            b.u32();
                        }
                    }
                }
                other => {
                    if let Some((_, n)) = skips.iter().find(|(o, _)| *o == other) {
                        b.pos += n;
                    } else {
                        d.unknown_opcode = Some(other);
                        break; // unknown opcode: stop rather than misparse
                    }
                }
            }
        }
        (d, clean)
    }
}

/// Lazily loaded config tables.
pub struct Configs<'a> {
    cache: &'a Cache,
    index: crate::Index,
}

impl<'a> Configs<'a> {
    pub fn new(cache: &'a Cache) -> Result<Configs<'a>> {
        Ok(Configs { cache, index: cache.index(idx::CONFIG)? })
    }

    /// Raw `(id, bytes)` files of one config group.
    pub fn group_files(&self, group: u32) -> Result<Vec<(u32, Vec<u8>)>> {
        self.cache.read_files(&self.index, group)
    }

    pub fn underlays(&self) -> Result<HashMap<u32, Underlay>> {
        Ok(self
            .cache
            .read_files(&self.index, cfg::UNDERLAY)?
            .into_iter()
            .map(|(id, d)| (id, Underlay::decode(&d)))
            .collect())
    }

    /// Average colour (16-bit HSL) of every texture, stored at bytes 2..4 of its definition.
    pub fn texture_average_hsl(&self) -> Result<HashMap<u32, u16>> {
        let index = self.cache.index(idx::TEXTURES)?;
        let mut out = HashMap::new();
        for group in index.groups.keys() {
            for (id, d) in self.cache.read_files(&index, *group)? {
                if d.len() >= 4 {
                    out.insert(id, u16::from_be_bytes([d[2], d[3]]));
                }
            }
        }
        Ok(out)
    }

    pub fn overlays(&self) -> Result<HashMap<u32, Overlay>> {
        let tex = self.texture_average_hsl()?;
        Ok(self
            .cache
            .read_files(&self.index, cfg::OVERLAY)?
            .into_iter()
            .map(|(id, d)| {
                let mut o = Overlay::decode(&d);
                o.texture_hsl = o.texture.and_then(|t| tex.get(&(t as u32)).copied());
                (id, o)
            })
            .collect())
    }

    pub fn objects(&self) -> Result<HashMap<u32, ObjectDef>> {
        Ok(self
            .cache
            .read_files(&self.index, cfg::OBJECT)?
            .into_iter()
            .map(|(id, d)| (id, ObjectDef::decode(id, &d)))
            .collect())
    }
}
