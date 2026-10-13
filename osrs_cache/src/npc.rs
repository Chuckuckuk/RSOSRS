//! NPC definitions (config archive group 9): the models an NPC is built from, its size and the
//! animations it plays.

use crate::buf::Buf;
use crate::config::Configs;
use crate::model::Model;
use crate::scene::{build_models, LocMesh, Shape};
use crate::{cfg, idx, Cache, Result};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, Default)]
pub struct NpcDef {
    pub id: u32,
    pub name: String,
    pub models: Vec<u32>,
    /// Footprint in tiles (square).
    pub size: u8,
    pub combat_level: Option<u16>,
    /// Sequence ids; `None` where the definition has none.
    pub stand: Option<u32>,
    pub walk: Option<u32>,
    pub walk_back: Option<u32>,
    pub walk_left: Option<u32>,
    pub walk_right: Option<u32>,
    pub run: Option<u32>,
    pub turn_left: Option<u32>,
    pub turn_right: Option<u32>,
    pub actions: Vec<String>,
    pub recolor_from: Vec<u16>,
    pub recolor_to: Vec<u16>,
    pub retexture_from: Vec<u16>,
    pub retexture_to: Vec<u16>,
    /// Model scale in 1/128ths (128 = as modelled).
    pub width_scale: u16,
    pub height_scale: u16,
    pub ambient: i8,
    pub contrast: i8,
    /// An NPC that is only drawn when a varbit or varp says so lists its real forms here.
    pub transforms: Vec<i32>,
    /// Set when decoding stopped at an opcode this decoder does not know.
    pub unknown_opcode: Option<u8>,
}

fn seq(v: u16) -> Option<u32> {
    (v != 0xFFFF).then_some(v as u32)
}

impl NpcDef {
    pub fn decode(id: u32, data: &[u8]) -> NpcDef {
        let mut d = NpcDef { id, size: 1, width_scale: 128, height_scale: 128, ..Default::default() };
        let mut b = Buf::new(data);
        while b.remaining() > 0 {
            match b.u8() {
                0 => break,
                1 => d.models = (0..b.u8()).map(|_| b.u16() as u32).collect(),
                // this revision widened model ids to u32 (as it did for objects and kits): 61 is the
                // body model list and 62 the chat-head list
                61 => d.models = (0..b.u8()).map(|_| b.u32()).collect(),
                62 => {
                    for _ in 0..b.u8() {
                        b.u32();
                    }
                }
                2 => d.name = b.string(),
                3 => {
                    b.string();
                }
                12 => d.size = b.u8(),
                13 => d.stand = seq(b.u16()),
                14 => d.walk = seq(b.u16()),
                15 => d.turn_left = seq(b.u16()),
                16 => d.turn_right = seq(b.u16()),
                17 => {
                    d.walk = seq(b.u16());
                    // 180 degree turn
                    d.walk_back = seq(b.u16());
                    d.walk_left = seq(b.u16());
                    d.walk_right = seq(b.u16());
                }
                18 => {
                    b.u16();
                }
                op @ 30..=34 => {
                    let slot = (op - 30) as usize;
                    if d.actions.len() <= slot {
                        d.actions.resize(slot + 1, String::new());
                    }
                    d.actions[slot] = b.string();
                }
                40 => {
                    for _ in 0..b.u8() {
                        d.recolor_from.push(b.u16());
                        d.recolor_to.push(b.u16());
                    }
                }
                41 => {
                    for _ in 0..b.u8() {
                        d.retexture_from.push(b.u16());
                        d.retexture_to.push(b.u16());
                    }
                }
                60 => {
                    for _ in 0..b.u8() {
                        b.u16();
                    }
                }
                // base levels: attack, defence, strength, hitpoints, ranged, magic
                74..=79 => {
                    b.u16();
                }
                93 => {}
                95 => d.combat_level = Some(b.u16()),
                97 => d.width_scale = b.u16(),
                98 => d.height_scale = b.u16(),
                99 => {}
                100 => d.ambient = b.i8(),
                101 => d.contrast = b.i8(),
                102 => {
                    // head icons: a bit set says which of up to 8 icons follow, each an archive
                    // id and a sprite index
                    let mask = b.u8();
                    for bit in 0..8 {
                        if mask & (1 << bit) != 0 {
                            b.u16();
                            b.u16();
                        }
                    }
                }
                103 => {
                    b.u16();
                }
                // run animation, then run + the three turned ones
                114 => d.run = seq(b.u16()),
                115 => {
                    d.run = seq(b.u16());
                    for _ in 0..3 {
                        b.u16();
                    }
                }
                // crawl animation, then crawl + the three turned ones
                116 => {
                    b.u16();
                }
                117 => {
                    for _ in 0..4 {
                        b.u16();
                    }
                }
                op @ (106 | 118) => {
                    // varbit, varp (and for 118 a default form), then the forms to pick between
                    b.u16();
                    b.u16();
                    if op == 118 {
                        b.u16();
                    }
                    let n = b.u8();
                    for _ in 0..=n {
                        d.transforms.push(b.u16() as i32);
                    }
                }
                107 => {}
                109 => {}
                111 => {}
                249 => {
                    for _ in 0..b.u8() {
                        let is_string = b.u8() == 1;
                        b.u8();
                        b.u8();
                        b.u8();
                        if is_string {
                            b.string();
                        } else {
                            b.u32();
                        }
                    }
                }
                other => {
                    d.unknown_opcode = Some(other);
                    break;
                }
            }
        }
        d
    }
}

impl Configs<'_> {
    pub fn npcs(&self) -> Result<HashMap<u32, NpcDef>> {
        Ok(self.group_files(cfg::NPC)?.into_iter().map(|(id, d)| (id, NpcDef::decode(id, &d))).collect())
    }
}

/// The client lights NPCs with ambient 64 and contrast 850 (plus the definition's own), from the
/// same direction as players.
const NPC_AMBIENT: f32 = 64.0;
const NPC_CONTRAST: f32 = 850.0;
const NPC_LIGHT: (f32, f32, f32) = (-30.0, -50.0, -30.0);

/// Lit mesh of an NPC in the bind pose, origin at the feet, front towards +z (OSRS orientation 0),
/// scaled by the definition. Units are tiles, like every other scene mesh. Keeps the rig so the
/// stand and walk animations can be baked against it.
pub fn build_npc(cache: &Cache, def: &NpcDef, texture_hsl: &HashMap<u32, u16>) -> Option<LocMesh> {
    let mut loaded: HashMap<u32, Option<Rc<Model>>> = HashMap::new();
    let mut load_model = |id: u32| -> Option<Rc<Model>> {
        loaded
            .entry(id)
            .or_insert_with(|| {
                let data = cache.read_group(idx::MODELS, id).ok()?;
                Model::decode(id, &data).ok().map(Rc::new)
            })
            .clone()
    };
    let shape = Shape {
        recolor_from: &def.recolor_from,
        recolor_to: &def.recolor_to,
        size: [def.width_scale, def.height_scale, def.width_scale],
        offset: [0, 0, 0],
        ambient: NPC_AMBIENT + def.ambient as f32,
        contrast: NPC_CONTRAST + def.contrast as f32 * 5.0,
        light: NPC_LIGHT,
        keep_rig: true,
    };
    build_models(&mut load_model, &def.models, &shape, false, false, 0, 10, (0, 0), texture_hsl)
}
