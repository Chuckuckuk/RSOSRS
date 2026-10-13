//! Health bar definitions (config group 33): the two sprites a bar is drawn from. The back sprite
//! (the red part) is drawn whole; the front sprite (the green) is cut off at the fraction of
//! health left.

use crate::buf::Buf;
use crate::config::Configs;
use crate::textures::{decode_sprite, Texture};
use crate::{idx, Cache, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct HealthBarDef {
    pub id: u32,
    pub front: Option<u32>,
    pub back: Option<u32>,
    /// Pixels wide.
    pub width: u32,
    /// Client cycles (20 ms) it stays after the last hit.
    pub duration: u32,
}

impl HealthBarDef {
    pub fn decode(id: u32, data: &[u8]) -> HealthBarDef {
        let mut def = HealthBarDef { id, front: None, back: None, width: 30, duration: 300 };
        let mut b = Buf::new(data);
        while b.remaining() > 0 {
            match b.u8() {
                0 => break,
                5 => def.duration = b.u16() as u32,
                1 | 11 => {
                    b.u16();
                }
                2 | 3 | 6 | 15 => {
                    b.u8();
                }
                4 => {}
                7 => def.front = Some(b.u16() as u32),
                8 => def.back = Some(b.u16() as u32),
                14 => def.width = b.u8() as u32,
                _ => break,
            }
        }
        def
    }
}

/// A bar drawn from the cache.
#[derive(Clone)]
pub struct HealthBarArt {
    pub front: Texture,
    pub back: Texture,
    pub width: u32,
}

/// The health bar of type `id` (0 is the standard one).
pub fn load_healthbar(cache: &Cache, configs: &Configs, id: u32) -> Result<Option<HealthBarArt>> {
    let Some((_, data)) = configs.group_files(33)?.into_iter().find(|(file, _)| *file == id) else {
        return Ok(None);
    };
    let def = HealthBarDef::decode(id, &data);
    let (Some(front), Some(back)) = (def.front, def.back) else { return Ok(None) };
    let sprites = cache.index(idx::SPRITES)?;
    let load = |group: u32| -> Result<Option<Texture>> {
        Ok(cache.read_files(&sprites, group)?.first().and_then(|(_, f)| decode_sprite(f)))
    };
    let (Some(front), Some(back)) = (load(front)?, load(back)?) else { return Ok(None) };
    Ok(Some(HealthBarArt { front, back, width: def.width }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_bar_has_a_front_and_back_sprite() {
        let def = HealthBarDef::decode(0, &[0x07, 0x08, 0x80, 0x08, 0x08, 0x81, 0x02, 0xfa, 0x03, 0xfa, 0x05, 0x01, 0x2c, 0x00]);
        assert_eq!((def.front, def.back, def.width), (Some(2176), Some(2177), 30));
    }

    #[test]
    fn a_wider_bar_says_so() {
        let def = HealthBarDef::decode(1, &hex("070b97080b9802fa03fa05012c0b01180e3200"));
        assert_eq!(def.width, 50);
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }
}
