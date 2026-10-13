//! Item definitions (config group 10): name, the model and camera of the inventory picture, and the
//! options in the inventory menu.

use crate::buf::Buf;
use crate::config::Configs;
use crate::Result;
use std::collections::HashMap;

/// A value of an item parameter (opcode 249), read by client scripts with `oc_param`.
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    Int(i32),
    Str(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemDef {
    pub id: u32,
    pub name: String,
    pub inventory_model: u32,
    /// Camera of the inventory picture: distance, pitch, yaw, roll (2048 = a turn), offsets.
    pub zoom: i32,
    pub pitch: i32,
    pub yaw: i32,
    pub roll: i32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub stackable: bool,
    pub cost: i32,
    pub members: bool,
    /// Options on the ground and in the inventory, by slot; `None` is no option.
    pub ground_options: [Option<String>; 5],
    pub options: [Option<String>; 5],
    pub recolor_from: Vec<u16>,
    pub recolor_to: Vec<u16>,
    /// A model of a stackable as a pile of `count`: (count, model).
    pub counts: Vec<(u32, u32)>,
    /// The linked noted/unnoted item (opcode 97); with `is_noted_template` this item is a noted copy.
    pub noted_link: Option<u32>,
    pub is_noted_template: bool,
    /// Bank placeholders have this set.
    pub is_placeholder: bool,
    /// Models worn on a male / female character (up to three each).
    pub male_models: Vec<u32>,
    pub female_models: Vec<u32>,
    /// The body slot it is worn in (0 head, 1 cape, 2 amulet, 3 weapon, 4 torso, 5 shield, 6 arms,
    /// 7 legs, 8 hair, 9 hands, 10 feet, 11 jaw, 12 ring, 13 ammunition) and the slots it covers.
    pub wear_slot: Option<u8>,
    pub hides_slots: Vec<u8>,
    pub resize: [u16; 3],
    pub ambient: i32,
    pub contrast: i32,
    /// Parameters (opcode 249): key -> value.
    pub params: Vec<(u32, Param)>,
}

impl ItemDef {
    pub fn decode(id: u32, data: &[u8]) -> ItemDef {
        Self::decode_checked(id, data).0
    }

    /// Decodes, and says which opcode it did not understand (and where) if it had to stop early.
    pub fn decode_checked(id: u32, data: &[u8]) -> (ItemDef, Option<(u8, usize)>) {
        let mut unknown = None;
        let mut def = ItemDef { id, resize: [128, 128, 128], ..Default::default() };
        let mut b = Buf::new(data);
        let mut count_models = [0u32; 10];
        let mut count_counts = [0u32; 10];
        let signed16 = |v: u16| if v > 32767 { v as i32 - 65536 } else { v as i32 };
        while b.remaining() > 0 {
            let opcode = b.u8();
            match opcode {
                0 => break,
                1 => def.inventory_model = b.u16() as u32,
                // this revision stores the inventory model as an int, and the worn models as an
                // int and an offset byte
                44 => def.inventory_model = b.u32(),
                45 => {
                    def.male_models.push(b.u32());
                    b.u8();
                }
                46 | 47 => def.male_models.push(b.u32()),
                48 => {
                    def.female_models.push(b.u32());
                    b.u8();
                }
                49 | 50 => def.female_models.push(b.u32()),
                // chat-head models
                51..=54 => {
                    b.u32();
                }
                // flags with nothing after them
                15 | 65 | 160 | 251 => {}
                43 => {
                    // sub-options of an inventory option: the option, then (slot + 1, text) until 0
                    b.u8();
                    loop {
                        let slot = b.u8();
                        if slot == 0 || slot > 5 || b.remaining() == 0 {
                            break;
                        }
                        b.string();
                    }
                }
                75 | 94 => {
                    b.u16();
                }
                3 => {
                    b.string();
                }
                2 => def.name = b.string(),
                4 => def.zoom = b.u16() as i32,
                5 => def.pitch = b.u16() as i32,
                6 => def.yaw = b.u16() as i32,
                7 => def.offset_x = signed16(b.u16()),
                8 => def.offset_y = signed16(b.u16()),
                9 => {
                    b.string();
                }
                11 => def.stackable = true,
                12 => def.cost = b.i32(),
                13 => def.wear_slot = Some(b.u8()),
                14 | 27 => {
                    let covered = b.u8();
                    // 255 is "nothing"
                    if covered != 255 {
                        def.hides_slots.push(covered);
                    }
                }
                42 | 115 => {
                    b.u8();
                }
                16 => def.members = true,
                23..=26 | 78 | 79 | 90..=93 | 139 | 140 | 148 => {
                    b.u16();
                }
                30..=34 => {
                    let text = b.string();
                    def.ground_options[(opcode - 30) as usize] = (!text.eq_ignore_ascii_case("hidden")).then_some(text);
                }
                35..=39 => {
                    def.options[(opcode - 35) as usize] = Some(b.string());
                }
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
                95 => def.roll = b.u16() as i32,
                97 => def.noted_link = Some(b.u16() as u32),
                98 => {
                    b.u16();
                    def.is_noted_template = true;
                }
                100..=109 => {
                    let i = (opcode - 100) as usize;
                    count_models[i] = b.u16() as u32;
                    count_counts[i] = b.u16() as u32;
                }
                110 => def.resize[0] = b.u16(),
                111 => def.resize[1] = b.u16(),
                112 => def.resize[2] = b.u16(),
                113 => def.ambient = b.i8() as i32,
                114 => def.contrast = b.i8() as i32 * 5,
                149 => {
                    b.u16();
                    def.is_placeholder = true;
                }
                249 => {
                    for _ in 0..b.u8() {
                        let is_string = b.u8() == 1;
                        let key = b.u24();
                        let value = if is_string { Param::Str(b.string()) } else { Param::Int(b.i32()) };
                        def.params.push((key, value));
                    }
                }
                _ => {
                    unknown = Some((opcode, b.pos - 1));
                    break;
                }
            }
        }
        def.counts = (0..10).filter(|i| count_counts[*i] > 0).map(|i| (count_counts[i], count_models[i])).collect();
        (def, unknown)
    }

    /// The item whose picture shows a stack of `quantity`: a stackable has other items that look
    /// like a pile of 2, 3, 4, 5 ... (its `counts`); the largest pile not above `quantity` wins.
    /// `None` means the item's own picture.
    pub fn pile_item(&self, quantity: u32) -> Option<u32> {
        if !self.stackable || quantity <= 1 {
            return None;
        }
        self.counts
            .iter()
            .filter(|(count, item)| *count <= quantity && *item != 0)
            .max_by_key(|(count, _)| *count)
            .map(|(_, item)| *item)
    }

    /// What a left-click does: the first inventory option ("Wield", "Eat", ...), else "Use".
    pub fn first_option(&self) -> &str {
        self.options[..4].iter().flatten().next().map_or("Use", String::as_str)
    }

    /// The inventory menu's options, top to bottom, with "Use" and "Drop" in the game's places.
    pub fn menu_options(&self) -> Vec<String> {
        let mut out: Vec<String> = self.options[..4].iter().flatten().cloned().collect();
        out.push("Use".to_owned());
        out.push(self.options[4].clone().unwrap_or_else(|| "Drop".to_owned()));
        out
    }
}

impl Configs<'_> {
    pub fn items(&self) -> Result<HashMap<u32, ItemDef>> {
        Ok(self.group_files(crate::cfg::ITEM)?.into_iter().map(|(id, d)| (id, ItemDef::decode(id, &d))).collect())
    }
}

/// Item name (lower case) -> the id of the plain item of that name: not a noted copy or a
/// placeholder, the lowest id when several share a name.
pub fn ids_by_name(items: &HashMap<u32, ItemDef>) -> HashMap<String, u32> {
    let mut out: HashMap<String, u32> = HashMap::new();
    let mut all: Vec<&ItemDef> = items.values().filter(|d| !d.name.is_empty() && d.name != "null").collect();
    all.sort_by_key(|d| d.id);
    for def in all {
        if def.is_noted_template || def.is_placeholder || def.inventory_model == 0 {
            continue;
        }
        out.entry(def.name.to_lowercase()).or_insert(def.id);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_and_camera_decode() {
        // model 5, name "Sword", zoom 1500, pitch 400, yaw 1800, option 0 "Wield"
        let mut data = vec![1, 0, 5, 2];
        data.extend_from_slice(b"Sword\0");
        data.extend_from_slice(&[4, 0x05, 0xDC, 5, 0x01, 0x90, 6, 0x07, 0x08, 35]);
        data.extend_from_slice(b"Wield\0");
        data.push(0);
        let def = ItemDef::decode(7, &data);
        assert_eq!((def.inventory_model, def.zoom, def.pitch, def.yaw), (5, 1500, 400, 1800));
        assert_eq!(def.name, "Sword");
        assert_eq!(def.first_option(), "Wield");
        assert_eq!(def.menu_options(), vec!["Wield", "Use", "Drop"]);
    }
}
