//! Hit splat definitions (config group 32): which sprite the damage number sits on.
//!
//! The game picks a splat by a type id (12 = a block of yours, 16 = damage of yours, ...). Most of
//! those are just a pointer to the definition that holds the sprites, chosen by a client setting
//! (varbit); [`HitSplatDef::resolve`] follows the pointer's default.

use crate::buf::Buf;
use crate::config::Configs;
use crate::textures::{decode_sprite, Texture};
use crate::{idx, Cache, Result};
use std::collections::HashMap;

/// The splat type ids the game uses for what the player does and suffers.
pub mod kind {
    /// A hit of 0 on or by you.
    pub const BLOCK_ME: u32 = 12;
    /// A hit that did damage, on or by you.
    pub const DAMAGE_ME: u32 = 16;
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HitSplatDef {
    pub id: u32,
    pub font: Option<u32>,
    /// 0xRRGGBB of the number, when it is not the default white.
    pub text_color: Option<u32>,
    pub icon: Option<u32>,
    pub left: Option<u32>,
    pub middle: Option<u32>,
    pub right: Option<u32>,
    pub offset_x: i32,
    pub offset_y: i32,
    /// The text, `%1` standing for the amount.
    pub format: String,
    /// Client cycles (20 ms each) it stays up.
    pub duration: u32,
    /// The definition to use instead, when this one is only a pointer.
    pub redirect: Option<u32>,
}

/// A "nullable large smart": 2 bytes, or 4 with the top bit set; 32767 is none.
fn large_smart(b: &mut Buf) -> Option<u32> {
    let first = b.peek();
    let value = if first & 0x80 != 0 { b.u32() & 0x7FFF_FFFF } else { b.u16() as u32 };
    (value != 0x7FFF).then_some(value)
}

impl HitSplatDef {
    pub fn decode(id: u32, data: &[u8]) -> HitSplatDef {
        let mut def = HitSplatDef { id, duration: 70, ..Default::default() };
        let mut b = Buf::new(data);
        while b.remaining() > 0 {
            let opcode = b.u8();
            match opcode {
                0 => break,
                1 => def.font = large_smart(&mut b),
                2 => def.text_color = Some(b.u24()),
                3 => def.icon = large_smart(&mut b),
                4 => def.left = large_smart(&mut b),
                5 => def.middle = large_smart(&mut b),
                6 => def.right = large_smart(&mut b),
                7 => def.offset_x = b.i16() as i32,
                8 => {
                    // circumfixed: a 0, the text, a 0
                    b.u8();
                    def.format = b.string();
                }
                9 => def.duration = b.u16() as u32,
                10 => def.offset_y = b.i16() as i32,
                11 => {}
                12 => {
                    b.u8();
                }
                13 => {
                    b.i16();
                }
                14 => {
                    b.u16();
                }
                17 | 18 => {
                    // which definition to use depends on a client setting (varbit/varp); setting
                    // value 0 is the first entry
                    let _varbit = b.u16();
                    let _varp = b.u16();
                    let default = if opcode == 18 { Some(b.u16() as u32) } else { None };
                    let count = b.u8() as usize;
                    let targets: Vec<u32> = (0..=count).map(|_| b.u16() as u32).collect();
                    def.redirect = targets.first().copied().or(default).filter(|id| *id != 0xFFFF);
                }
                _ => break,
            }
        }
        def
    }

    /// The definition that has the sprites: this one, or what it points at.
    pub fn resolve<'a>(&'a self, all: &'a HashMap<u32, HitSplatDef>) -> &'a HitSplatDef {
        let mut current = self;
        for _ in 0..4 {
            match current.redirect.and_then(|id| all.get(&id)) {
                Some(next) => current = next,
                None => break,
            }
        }
        current
    }
}

/// A splat drawn from the cache: the middle sprite the number is centred on, and the number's colour.
#[derive(Clone)]
pub struct HitSplatArt {
    pub sprite: Texture,
    pub text_color: u32,
    pub offset_y: i32,
    /// Seconds it stays up.
    pub seconds: f32,
}

impl Configs<'_> {
    pub fn hitsplats(&self) -> Result<HashMap<u32, HitSplatDef>> {
        Ok(self.group_files(32)?.into_iter().map(|(id, d)| (id, HitSplatDef::decode(id, &d))).collect())
    }
}

/// The picture of splat type `kind` (see [`kind`]).
pub fn load_hitsplat(cache: &Cache, configs: &Configs, kind: u32) -> Result<Option<HitSplatArt>> {
    let all = configs.hitsplats()?;
    let Some(def) = all.get(&kind) else { return Ok(None) };
    let def = def.resolve(&all);
    let Some(sprite_id) = def.middle else { return Ok(None) };
    let sprites = cache.index(idx::SPRITES)?;
    let files = cache.read_files(&sprites, sprite_id)?;
    let Some(sprite) = files.first().and_then(|(_, f)| decode_sprite(f)) else { return Ok(None) };
    Ok(Some(HitSplatArt {
        sprite,
        text_color: def.text_color.unwrap_or(0xFFFFFF),
        offset_y: def.offset_y,
        seconds: def.duration as f32 * 0.02,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn a_splat_definition_has_its_sprite_text_and_duration() {
        // type 26 of the game's cache: "%1" on sprite 1358 for 50 cycles
        let def = HitSplatDef::decode(26, &hex("080025310005054e0900320d000100"));
        assert_eq!(def.middle, Some(1358));
        assert_eq!(def.format, "%1");
        assert_eq!(def.duration, 50);
        assert_eq!(def.redirect, None);
    }

    #[test]
    fn a_pointer_definition_names_the_one_with_the_sprites() {
        // type 17 points at 29, or at 28 under the other setting
        let pointer = HitSplatDef::decode(17, &hex("0900321227fcffff001d01001d001c00"));
        assert_eq!(pointer.redirect, Some(29));
        let target = HitSplatDef { id: 29, middle: Some(1631), ..Default::default() };
        let all = HashMap::from([(17, pointer.clone()), (29, target)]);
        assert_eq!(pointer.resolve(&all).middle, Some(1631));
    }
}
