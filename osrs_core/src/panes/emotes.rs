//! The Emotes tab (group 216; scripts 699, 701, 31).
//!
//! Script 699 fills the layer 216:2 with one 48x48 icon per emote: enum 1001 maps the emote's
//! number (0..55) to its sprite (a number with no entry has no emote). The icons are placed in the
//! order of the numbers that exist, four to a row: `x = (n % 4) * (width / 4)` with the width of
//! 216:2 (171, so 42), `y = (n / 4) * 49 + 6`. The pane scrolls (the list is taller than the
//! panel), and script 31 builds the scroll bar in 216:4 (`super::scrollbar`).
//!
//! Script 701 swaps the icon of an emote the player has not unlocked for a dimmed one (enum 1002);
//! the simulation has no unlocks, so every emote is drawn unlocked. Clicking an emote does
//! nothing: the server has no emotes.

use super::{comp, scrollbar, sprite_child, PaneInput};
use osrs_cache::clientscript::{load_enum, Operand};
use osrs_cache::ui::UiState;
use osrs_cache::Cache;

/// The enum from an emote's number to its icon sprite.
const ICONS: u32 = 1001;

/// The icon of each emote that exists, in order of number.
#[derive(Default)]
pub struct EmoteTable {
    pub icons: Vec<i32>,
}

impl EmoteTable {
    pub fn load(cache: &Cache) -> osrs_cache::Result<EmoteTable> {
        let e = load_enum(cache, ICONS)?;
        let mut by_number: Vec<(i32, i32)> = e
            .entries
            .iter()
            .filter_map(|(k, v)| if let Operand::Int(sprite) = v { Some((*k, *sprite)) } else { None })
            .filter(|(_, sprite)| *sprite != -1)
            .collect();
        by_number.sort_unstable();
        Ok(EmoteTable { icons: by_number.into_iter().map(|(_, s)| s).collect() })
    }
}

/// The width of the pane the icons sit in (216:2 fills the 171 px scroll layer).
const PANE_WIDTH: i32 = 171;
/// The height of the scroll layer (216:1) and of the scroll bar's layer (216:4).
const PANE_HEIGHT: i32 = 261;

/// Where the `n`th icon goes (`(x, y)` in 216:2).
pub fn place(n: usize) -> (i32, i32) {
    ((n as i32 % 4) * (PANE_WIDTH / 4), (n as i32 / 4) * 49 + 6)
}

pub fn sync(state: &mut UiState, input: &PaneInput) {
    let icons = &input.icons.emotes.icons;
    if icons.is_empty() {
        return;
    }
    let children = icons
        .iter()
        .enumerate()
        .map(|(n, sprite)| {
            let (x, y) = place(n);
            sprite_child(x, y, 48, 48, *sprite)
        })
        .collect();
    state.created.insert(comp(216, 2), children);
    let (_, last_y) = place(icons.len() - 1);
    state.created.insert(comp(216, 4), scrollbar(PANE_HEIGHT, PANE_HEIGHT, last_y + 48));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_go_four_to_a_row() {
        assert_eq!(place(0), (0, 6));
        assert_eq!(place(3), (126, 6));
        assert_eq!(place(4), (0, 55));
        assert_eq!(place(55), (126, 13 * 49 + 6));
    }
}
