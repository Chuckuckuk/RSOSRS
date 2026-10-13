//! Identity kits (config archive group 3): the body-part models a player character is built from,
//! and the default character assembled from them.

use crate::buf::Buf;
use crate::config::Configs;
use crate::model::Model;
use crate::scene::{build_models, LocMesh, Shape};
use crate::{cfg, idx, Cache, Result};
use std::collections::HashMap;
use std::rc::Rc;

/// Body parts in the order the client numbers them (female parts are these + 7).
pub mod part {
    pub const HAIR: u8 = 0;
    pub const JAW: u8 = 1;
    pub const TORSO: u8 = 2;
    pub const ARMS: u8 = 3;
    pub const HANDS: u8 = 4;
    pub const LEGS: u8 = 5;
    pub const FEET: u8 = 6;
}

#[derive(Debug, Clone, Default)]
pub struct IdentKit {
    pub id: u32,
    /// 0..=6 male, 7..=13 female.
    pub body_part: Option<u8>,
    pub models: Vec<u32>,
    /// Chat-head models (opcode 70; classic 60..=69 are u16 lists).
    pub head_models: Vec<u32>,
    pub recolor_from: Vec<u16>,
    pub recolor_to: Vec<u16>,
    pub retexture_from: Vec<u16>,
    pub retexture_to: Vec<u16>,
    /// Opcode 3 marks kits the character-design screen does not offer.
    pub hidden_in_designer: bool,
    /// Set when decoding stopped at an opcode this decoder does not know.
    pub unknown_opcode: Option<u8>,
}

impl IdentKit {
    pub fn decode(id: u32, data: &[u8]) -> IdentKit {
        let mut k = IdentKit { id, ..Default::default() };
        let mut b = Buf::new(data);
        loop {
            if b.remaining() == 0 {
                break;
            }
            match b.u8() {
                0 => break,
                1 => k.body_part = Some(b.u8()),
                2 => k.models = (0..b.u8()).map(|_| b.u16() as u32).collect(),
                3 => k.hidden_in_designer = true,
                // this revision's u32-id model list (the same widening object defs went through)
                5 => k.models = (0..b.u8()).map(|_| b.u32()).collect(),
                40 => {
                    for _ in 0..b.u8() {
                        k.recolor_from.push(b.u16());
                        k.recolor_to.push(b.u16());
                    }
                }
                41 => {
                    for _ in 0..b.u8() {
                        k.retexture_from.push(b.u16());
                        k.retexture_to.push(b.u16());
                    }
                }
                60..=69 => k.head_models.push(b.u16() as u32),
                70 => k.head_models.push(b.u32()),
                op => {
                    k.unknown_opcode = Some(op);
                    break;
                }
            }
        }
        k
    }
}

impl Configs<'_> {
    pub fn identkits(&self) -> Result<HashMap<u32, IdentKit>> {
        Ok(self
            .group_files(cfg::IDENTKIT)?
            .into_iter()
            .map(|(id, d)| (id, IdentKit::decode(id, &d)))
            .collect())
    }
}

/// A character's look: which kit fills each body part (None = nothing worn there, e.g. a
/// hidden beard or the torso covered by armour).
#[derive(Debug, Clone)]
pub struct Appearance {
    pub female: bool,
    /// Kit id per body part 0..=6 (see [`part`]).
    pub kits: [Option<u32>; 7],
}

impl Appearance {
    /// The character a new account starts with: bald head, goatee, and the first torso, arms,
    /// hands, legs and feet of the designer, all in each kit's own colours.
    pub fn default_male() -> Appearance {
        Appearance { female: false, kits: [Some(0), Some(10), Some(18), Some(26), Some(33), Some(36), Some(42)] }
    }
}

/// The wear slot of what is held in the right hand (the weapon).
pub const SLOT_WEAPON: u8 = 3;
/// The wear slot of what is held in the left hand (the shield).
pub const SLOT_SHIELD: u8 = 5;

/// Something worn: the body slot it takes, its models and colours, and the slots it covers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WornPart {
    pub slot: u8,
    pub models: Vec<u32>,
    pub recolor_from: Vec<u16>,
    pub recolor_to: Vec<u16>,
    pub covers: Vec<u8>,
}

impl WornPart {
    /// What wearing `item` puts on a character, if it can be worn.
    pub fn of(item: &crate::item::ItemDef, female: bool) -> Option<WornPart> {
        let models = if female && !item.female_models.is_empty() { &item.female_models } else { &item.male_models };
        let slot = item.wear_slot?;
        (!models.is_empty()).then(|| WornPart {
            slot,
            models: models.clone(),
            recolor_from: item.recolor_from.clone(),
            recolor_to: item.recolor_to.clone(),
            covers: item.hides_slots.clone(),
        })
    }
}

/// The body slot (as the client numbers worn slots) each kit part fills.
fn slot_of_part(part: usize) -> u8 {
    match part as u8 {
        part::HAIR => 8,
        part::JAW => 11,
        part::TORSO => 4,
        part::ARMS => 6,
        part::HANDS => 9,
        part::LEGS => 7,
        _ => 10,
    }
}

/// The client lights players with ambient 64, contrast 850 and this light direction.
const PLAYER_AMBIENT: f32 = 64.0;
const PLAYER_CONTRAST: f32 = 850.0;
const PLAYER_LIGHT: (f32, f32, f32) = (-30.0, -50.0, -30.0);

/// Lit mesh of a character in the bind pose, origin at the feet, model front towards +z south
/// (OSRS orientation 0), wearing `worn`. Units are tiles, like every other scene mesh.
///
/// As the game assembles a player: slot by slot (head, cape, amulet, weapon, torso, shield, arms,
/// legs, hair, hands, feet, jaw) the worn item's models, or else the kit that fills that slot, except
/// for slots something worn covers (a helmet covers the hair and jaw, a top the arms).
pub fn build_player(
    cache: &Cache,
    appearance: &Appearance,
    worn: &[WornPart],
    texture_hsl: &HashMap<u32, u16>,
) -> Result<Option<LocMesh>> {
    let configs = Configs::new(cache)?;
    let kits = configs.identkits()?;

    let mut covered = [false; 14];
    for part in worn {
        for slot in &part.covers {
            if let Some(c) = covered.get_mut(*slot as usize) {
                *c = true;
            }
        }
    }

    // every group of models with the colours that belong to it, in slot order
    struct Group {
        models: Vec<u32>,
        from: Vec<u16>,
        to: Vec<u16>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for slot in 0u8..12 {
        if covered[slot as usize] {
            continue;
        }
        if let Some(item) = worn.iter().find(|w| w.slot == slot) {
            groups.push(Group { models: item.models.clone(), from: item.recolor_from.clone(), to: item.recolor_to.clone() });
            continue;
        }
        let kit = (0..7)
            .find(|p| slot_of_part(*p) == slot)
            .and_then(|p| appearance.kits[p])
            .and_then(|id| kits.get(&id));
        if let Some(kit) = kit {
            groups.push(Group { models: kit.models.clone(), from: kit.recolor_from.clone(), to: kit.recolor_to.clone() });
        }
    }

    // each group's models are recoloured with that group's own table, then merged
    let mut prepared: Vec<Rc<Model>> = Vec::new();
    for group in &groups {
        for id in &group.models {
            let Ok(data) = cache.read_group(idx::MODELS, *id) else { continue };
            let Ok(mut model) = Model::decode(*id, &data) else { continue };
            model.recolor(&group.from, &group.to);
            prepared.push(Rc::new(model));
        }
    }
    let ids: Vec<u32> = (0..prepared.len() as u32).collect();
    let mut load_model = |id: u32| -> Option<Rc<Model>> { prepared.get(id as usize).cloned() };
    let shape = Shape {
        recolor_from: &[],
        recolor_to: &[],
        size: [128, 128, 128],
        offset: [0, 0, 0],
        ambient: PLAYER_AMBIENT,
        contrast: PLAYER_CONTRAST,
        light: PLAYER_LIGHT,
        keep_rig: true,
    };
    Ok(build_models(&mut load_model, &ids, &shape, false, false, 0, 10, (0, 0), texture_hsl))
}
