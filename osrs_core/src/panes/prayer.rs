//! The prayer book (scripts 458, 547, 461, 463).
//!
//! The 29 cells 541:9..37 are laid out five to a row, 37 pixels apart, in level order (which is
//! the order of `PRAYER_BOOK`). Each cell is rebuilt with two children: the yellow glow disc
//! (sprite 4892, 34x34), shown only while the prayer is on, and the 30x30 icon, the grey copy while
//! the player's Prayer level is too low. Cell 541:38 has no prayer and stays hidden.
//!
//! TODO: the quick-prayer set-up mode (purple glow 4893), the filters and Info buttons.

use super::{comp, sprite_child, PaneInput};
use crate::interface::PRAYER_BOOK;
use osrs_cache::ui::{Assets, Override, UiState};

/// The first of the cells (`541:9`); book prayer `i` is cell `FIRST_CELL + i`.
pub const FIRST_CELL: u32 = 9;
pub const GLOW: i32 = 4892;

pub fn sync(state: &mut UiState, input: &PaneInput, assets: &mut dyn Assets) {
    // a player's Prayer level is his most prayer points
    let level = input.player.map_or(1, |p| p.max_prayer);
    for (i, book) in PRAYER_BOOK.iter().enumerate() {
        let cell = comp(541, FIRST_CELL + i as u32);
        let (col, row) = ((i % 5) as i32, (i / 5) as i32);
        state.overrides.insert(cell, Override { pos: Some((col * 37, row * 37)), ..Default::default() });
        let usable = input.levels.get(i).copied().flatten().is_some_and(|needed| needed <= level);
        let icon = if usable { book.icon } else { book.greyed } as i32;
        let (w, h) = assets.sprite(icon).map_or((30, 30), |t| (t.width as i32, t.height as i32));
        let on = input.player.is_some_and(|p| p.active_prayers.iter().any(|id| id == book.id));
        state.created.insert(cell, vec![sprite_child(0, 0, 34, 34, GLOW), sprite_child((34 - w) / 2, (34 - h) / 2, w, h, icon)]);
        state.created_overrides.insert((cell, 0), Override { hidden: Some(!on), ..Default::default() });
    }
    // the cell the book has no prayer for
    for i in PRAYER_BOOK.len() as u32..30 {
        state.overrides.entry(comp(541, FIRST_CELL + i)).or_default().hidden = Some(true);
    }
}
