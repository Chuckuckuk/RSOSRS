//! The inventory pane (scripts 6007, 6010, 6011).
//!
//! 6007 creates 28 type-5 children of 149:0, 36x32 each, with a one pixel outline and a 0x333333
//! shadow; 6010 puts slot `i` at (16 + (i % 4) * 42, 8 + (i / 4) * 36); 6011 sets the item and
//! count of a full slot and hides an empty one. There is no slot background. The outline and
//! shadow come with the item picture (`osrs_cache::item_icon`).

use super::{comp, sprite_child, PaneInput};
use osrs_cache::ui::{Override, UiState};

/// The widget the 28 slots are children of.
pub const INVENTORY: u32 = comp(149, 0);

pub fn sync(state: &mut UiState, input: &PaneInput) {
    let mut slots = Vec::with_capacity(28);
    state.created_overrides.retain(|(parent, _), _| *parent != INVENTORY);
    for i in 0..28usize {
        let mut slot = sprite_child(16 + (i as i32 % 4) * 42, 8 + (i as i32 / 4) * 36, 36, 32, -1);
        // an op makes the slot a widget the click can land on
        slot.ops = vec!["*".to_owned()];
        slots.push(slot);
        let stack = input.player.and_then(|p| p.inventory.get(i)).and_then(Option::as_ref);
        let object = stack.and_then(|s| input.icons.item_named(&s.name).map(|d| (d.id, s.quantity)));
        let ov = match object {
            Some(item) => Override { item: Some(item), selected: input.use_selected == Some(i), ..Default::default() },
            None => Override { hidden: Some(true), ..Default::default() },
        };
        state.created_overrides.insert((INVENTORY, i), ov);
    }
    state.created.insert(INVENTORY, slots);
}
