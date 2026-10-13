//! The worn-equipment pane (scripts 3281, 3282 and 546).
//!
//! Each slot layer 387:15..25 (36x36) gets three children: the slot frame (sprite 170, 36x36), the
//! worn item (36x32 at (2, 2)) and the empty-slot silhouette (32x32 at (2, 2), shown only while the
//! slot is empty). Which layer is which slot follows the cache's own positions of the layers
//! (enum 2775): head 77,4; cape 36,43; neck 77,43; ammo 118,43; weapon 21,82; body 77,82;
//! shield 133,82; legs 77,122; hands 21,162; feet 77,162; ring 133,162.

use super::{comp, sprite_child, PaneInput};
use osrs_cache::ui::{Override, UiState};
use osrs_core::data::EquipSlot;

/// Each slot, the layer of group 387 it is, and its empty-slot sprite (enum 904).
pub const SLOTS: [(EquipSlot, u32, i32); 11] = [
    (EquipSlot::Head, 15, 156),
    (EquipSlot::Cape, 16, 157),
    (EquipSlot::Neck, 17, 158),
    (EquipSlot::Weapon, 18, 159),
    (EquipSlot::Body, 19, 161),
    (EquipSlot::Shield, 20, 162),
    (EquipSlot::Legs, 21, 163),
    (EquipSlot::Hands, 22, 164),
    (EquipSlot::Feet, 23, 165),
    (EquipSlot::Ring, 24, 160),
    (EquipSlot::Ammo, 25, 166),
];

/// The slot a layer of group 387 is, by its file number.
#[allow(dead_code)]
pub fn slot_of_file(file: u32) -> Option<EquipSlot> {
    SLOTS.iter().find(|(_, f, _)| *f == file).map(|(s, _, _)| *s)
}

pub fn sync(state: &mut UiState, input: &PaneInput) {
    for (slot, file, placeholder) in SLOTS {
        let layer = comp(387, file);
        state.created_overrides.retain(|(parent, _), _| *parent != layer);
        let worn = input.player.and_then(|p| p.equipped.iter().find(|(s, _)| *s == slot)).map(|(_, item)| item);
        let object = worn.and_then(|item| input.icons.item_named(&item.name).map(|d| (d.id, item.quantity)));
        state.created.insert(
            layer,
            vec![
                sprite_child(0, 0, 36, 36, 170),
                sprite_child(2, 2, 36, 32, -1),
                sprite_child(2, 2, 32, 32, placeholder),
            ],
        );
        match object {
            Some(item) => {
                state.created_overrides.insert((layer, 1), Override { item: Some(item), ..Default::default() });
                state.created_overrides.insert((layer, 2), Override { hidden: Some(true), ..Default::default() });
            }
            None => {
                state.created_overrides.insert((layer, 1), Override { hidden: Some(true), ..Default::default() });
            }
        }
    }
}
