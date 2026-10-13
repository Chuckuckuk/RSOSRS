//! Spot animations / graphics (config group 13): the small models drawn on and between characters
//! (arrows in flight, spell effects). A definition is a model, its colours and optionally the
//! sequence that animates it.

use crate::buf::Buf;
use crate::config::Configs;
use crate::model::Model;
use crate::scene::{build_models, LocMesh, Shape};
use crate::{idx, Cache, Result};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpotAnim {
    pub id: u32,
    pub model: u32,
    pub sequence: Option<u32>,
    /// Scale in 128ths, horizontally and vertically.
    pub resize_x: u16,
    pub resize_y: u16,
    pub ambient: i32,
    pub contrast: i32,
    pub recolor_from: Vec<u16>,
    pub recolor_to: Vec<u16>,
}

impl SpotAnim {
    pub fn decode(id: u32, data: &[u8]) -> SpotAnim {
        let mut def = SpotAnim { id, resize_x: 128, resize_y: 128, ..Default::default() };
        let mut b = Buf::new(data);
        while b.remaining() > 0 {
            match b.u8() {
                0 => break,
                // the model is an int in this revision (opcode 1 is the older short)
                1 => def.model = b.u16() as u32,
                3 => def.model = b.u32(),
                2 => {
                    let sequence = b.u16();
                    def.sequence = (sequence != 0xFFFF).then_some(sequence as u32);
                }
                4 => def.resize_x = b.u16(),
                5 => def.resize_y = b.u16(),
                6 => {
                    b.u16();
                }
                7 => def.ambient = b.u8() as i32,
                8 => def.contrast = b.u8() as i32,
                40 => {
                    for _ in 0..b.u8() {
                        def.recolor_from.push(b.u16());
                        def.recolor_to.push(b.u16());
                    }
                }
                41 => {
                    for _ in 0..b.u8() {
                        b.u16();
                        b.u16();
                    }
                }
                _ => break,
            }
        }
        def
    }
}

impl Configs<'_> {
    pub fn spotanims(&self) -> Result<HashMap<u32, SpotAnim>> {
        Ok(self.group_files(crate::cfg::SPOTANIM)?.into_iter().map(|(id, d)| (id, SpotAnim::decode(id, &d))).collect())
    }
}

/// The lit model of a graphic in its bind pose (units are tiles, like every scene mesh).
pub fn build_spotanim(cache: &Cache, def: &SpotAnim, texture_hsl: &HashMap<u32, u16>) -> Option<LocMesh> {
    build_spotanim_with_rig(cache, def, texture_hsl, false)
}

/// Like [`build_spotanim`]; with `keep_rig` the vertices and labels are kept so the graphic's own
/// sequence can be baked against it.
pub fn build_spotanim_with_rig(
    cache: &Cache,
    def: &SpotAnim,
    texture_hsl: &HashMap<u32, u16>,
    keep_rig: bool,
) -> Option<LocMesh> {
    if def.model == 0 {
        return None;
    }
    let mut load = |id: u32| -> Option<Rc<Model>> {
        let data = cache.read_group(idx::MODELS, id).ok()?;
        Model::decode(id, &data).ok().map(Rc::new)
    };
    let shape = Shape {
        recolor_from: &def.recolor_from,
        recolor_to: &def.recolor_to,
        size: [def.resize_x, def.resize_y, def.resize_x],
        offset: [0, 0, 0],
        ambient: 64.0 + def.ambient as f32,
        contrast: 850.0 + def.contrast as f32 * 5.0,
        light: (-50.0, -10.0, -50.0),
        keep_rig,
    };
    build_models(&mut load, &[def.model], &shape, false, false, 0, 10, (0, 0), texture_hsl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arrows_graphic_is_a_model_with_a_tier_colour() {
        // graphic 10 of the cache: model 3136, two recolours
        let data: Vec<u8> = vec![
            0x03, 0x00, 0x00, 0x0c, 0x40, 0x28, 0x02, 0x00, 0x39, 0x16, 0x14, 0x00, 0x3d, 0x16, 0x14, 0x00,
        ];
        let def = SpotAnim::decode(10, &data);
        assert_eq!(def.model, 3136);
        assert_eq!(def.recolor_from, vec![0x39, 0x3d]);
        assert_eq!(def.recolor_to, vec![0x1614, 0x1614]);
        assert_eq!(def.sequence, None);
    }
}
