//! The Magic tab, standard spellbook (group 218; scripts 2262, 2611, 2614, 9447, 9448, 2603).
//!
//! The cache holds one hidden 24x24 sprite widget per spell (218:4..200, all inside the layer
//! 218:3). Script 2611 walks enum 1982 (the standard spellbook: a list of "spell items") and, for
//! every spell that passes the filters (all of them, with the default settings), shows its widget
//! (item param 596 names the widget), sizes it, puts it in the grid and gives it its icon: the
//! lit sprite (param 597) or, when the Magic level is below the spell's level (param 604), the
//! grey one (param 598). Script 2614 also greys a spell the player lacks the runes for; the
//! simulation has no runes, so here a spell is lit whenever the level suffices.
//!
//! The grid (script 2611, the desktop path with the default filters) for `n` spells of 24 px:
//! `cols = clamp((n + 8) / 9, 4, 7)` columns, `rows = ceil(n / cols)`, the horizontal gap the
//! space left in the 184 px wide pane (at most 5 * 24 / 7), the vertical gap what is left of the
//! 240 px height (never more than the horizontal one), and the whole block centred in the pane.
//! The 69 spells of the standard book come out as 7 columns by 10 rows with a 2 px gap across.
//!
//! The "Filters" button at the bottom (script 2603): the grey frame of script 835 (the same one as
//! the combat buttons) with "Filters" in orange; the "Info" button is the mobile layout's.
//!
//! The selected spell gets a white outline. The game draws that outline itself for the spell a
//! player has picked to cast; here it marks the spell the server will cast with the wielded staff
//! (`PlayerSnapshot.spell`, set by `ClientCommand::SelectSpell`).
//!
//! TODO: the hover box (script 2623), the rune check, the filters panel, the other spellbooks.

use super::{combat, comp, PaneInput};
use osrs_cache::clientscript::{load_enum, Operand};
use osrs_cache::item::{ItemDef, Param};
use osrs_cache::ui::{Assets, Override, UiState};
use osrs_cache::Cache;
use osrs_core::model::Skill;
use std::collections::HashMap;

/// The enum that lists the standard spellbook (key: place, value: the spell's item).
const STANDARD_BOOK: u32 = 1982;
/// The container layer the spell widgets sit in.
const GRID: u32 = comp(218, 3);
/// The pane the grid is centred in (`218:1`: 190 minus the 6 px scroll bar, by 240).
const PANE: (i32, i32) = (184, 240);
/// The size of a spell icon.
const SIZE: i32 = 24;

/// One spell of the cache's spellbook.
#[derive(Debug, Clone, PartialEq)]
pub struct Spell {
    /// The widget showing it (item param 596).
    pub comp: u32,
    /// Item param 601.
    pub name: String,
    /// Magic level needed (param 604).
    pub level: i32,
    /// The lit icon (param 597) and the grey one (param 598).
    pub icon: i32,
    pub greyed: i32,
    /// Param 605: 0 combat, 1 utility, 2 teleport.
    pub kind: i32,
}

impl Spell {
    /// The id the server knows this spell by: its name in lower case with underscores.
    pub fn server_id(&self) -> String {
        self.name.to_lowercase().replace(' ', "_")
    }
}

/// The standard spellbook, in the order the game lays it out.
#[derive(Default)]
pub struct SpellBook {
    pub spells: Vec<Spell>,
    /// Per spell: whether the server has it (only combat spells it knows can be selected).
    pub known: Vec<bool>,
}

impl SpellBook {
    pub fn load(cache: &Cache, items: &HashMap<u32, ItemDef>) -> osrs_cache::Result<SpellBook> {
        let book = load_enum(cache, STANDARD_BOOK)?;
        let mut spells = Vec::new();
        for (_, value) in &book.entries {
            let Operand::Int(item) = value else { continue };
            let Some(def) = items.get(&(*item as u32)) else { continue };
            let int = |key: u32| {
                def.params.iter().find(|(k, _)| *k == key).and_then(|(_, v)| if let Param::Int(i) = v { Some(*i) } else { None })
            };
            let name = def.params.iter().find_map(|(k, v)| match (k, v) {
                (601, Param::Str(s)) => Some(s.clone()),
                _ => None,
            });
            let (Some(name), Some(widget)) = (name, int(596)) else { continue };
            spells.push(Spell {
                comp: widget as u32,
                name,
                level: int(604).unwrap_or(1),
                icon: int(597).unwrap_or(-1),
                greyed: int(598).unwrap_or(-1),
                kind: int(605).unwrap_or(0),
            });
        }
        let registry = osrs_core::data::Registry::builtin();
        let known = spells.iter().map(|s| registry.spell(&s.server_id()).is_some_and(|d| d.spellbook == "standard")).collect();
        Ok(SpellBook { spells, known })
    }
}

/// How `n` spells are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub cols: i32,
    pub rows: i32,
    pub gap_x: i32,
    pub gap_y: i32,
    /// Where the block starts in the pane (it is centred).
    pub origin: (i32, i32),
}

/// Script 2611's arrangement of `n` spells in the pane.
pub fn grid(n: i32) -> Grid {
    let cols = ((n + 8) / 9).clamp(4, 7);
    let rows = ((n + cols - 1) / cols).max(1);
    let gap_x = ((PANE.0 - SIZE * cols) / (cols - 1)).min(5 * SIZE / 7).max(0);
    let gap_y = if rows >= 2 { ((PANE.1 - SIZE * rows) / (rows - 1)).min(gap_x).max(0) } else { 0 };
    let w = cols * SIZE + (cols - 1) * gap_x;
    let h = rows * SIZE + (rows - 1) * gap_y;
    Grid { cols, rows, gap_x, gap_y, origin: ((PANE.0 - w) / 2, (PANE.1 - h) / 2) }
}

impl Grid {
    /// Top left of the `i`th spell inside the pane.
    pub fn at(&self, i: i32) -> (i32, i32) {
        (self.origin.0 + (i % self.cols) * (SIZE + self.gap_x), self.origin.1 + (i / self.cols) * (SIZE + self.gap_y))
    }
}

/// The player's current Magic level (1 without a snapshot).
pub fn magic_level(input: &PaneInput) -> i32 {
    input.player.and_then(|p| p.skills.iter().find(|s| s.skill == Skill::Magic)).map_or(1, |s| s.current)
}

pub fn sync(state: &mut UiState, input: &PaneInput, assets: &mut dyn Assets) {
    let book = &input.icons.spells.spells;
    if book.is_empty() {
        return;
    }
    let level = magic_level(input);
    let chosen = input.player.and_then(|p| p.spell.as_deref());
    let g = grid(book.len() as i32);
    let mut outline = Vec::new();
    for (i, spell) in book.iter().enumerate() {
        let (x, y) = g.at(i as i32);
        let lit = level >= spell.level;
        state.overrides.insert(
            spell.comp,
            Override {
                hidden: Some(false),
                pos: Some((x, y)),
                size: Some((SIZE, SIZE)),
                sprite: Some(if lit { spell.icon } else { spell.greyed }),
                transparency: Some(0),
                ..Default::default()
            },
        );
        if chosen == Some(spell.server_id().as_str()) {
            outline.push(osrs_cache::interface::Component {
                is_if3: true,
                kind: 3,
                x: x as i16,
                y: y as i16,
                width: SIZE as u16,
                height: SIZE,
                colour: 0xffffff,
                filled: false,
                ..Default::default()
            });
        }
    }
    state.created.insert(GRID, outline);

    // the Filters button (script 2603): a frame the width of the word plus 14, in the middle of the
    // bottom strip; the Info button belongs to the mobile layout
    state.overrides.entry(comp(218, 208)).or_default().hidden = Some(true);
    let width = assets.font(494).map_or(40, |f| f.width("Filters") as i32) + 14;
    state.overrides.insert(comp(218, 209), Override { size: Some((width, 0)), pos: Some(((190 - width) / 2, 0)), ..Default::default() });
    let mut button = combat::frame(false);
    button.push(osrs_cache::interface::Component {
        is_if3: true,
        kind: 4,
        width_mode: 1,
        height_mode: 1,
        x_mode: 1,
        y_mode: 1,
        font_id: 494,
        colour: 0xff981f,
        text: "Filters".to_owned(),
        x_align: 1,
        y_align: 1,
        shadowed: true,
        ..Default::default()
    });
    state.created.insert(comp(218, 209), button);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_book_is_seven_columns_by_ten_rows() {
        let g = grid(69);
        assert_eq!((g.cols, g.rows, g.gap_x, g.gap_y), (7, 10, 2, 0));
        // 7 * 24 + 6 * 2 = 180 wide in 184, so the block starts 2 px in
        assert_eq!(g.origin, (2, 0));
        assert_eq!(g.at(0), (2, 0));
        assert_eq!(g.at(8), (2 + 26, 24));
    }

    #[test]
    fn small_books_use_fewer_columns() {
        assert_eq!(grid(21).cols, 4);
        assert_eq!(grid(40).cols, 5);
    }

    #[test]
    fn spell_ids_are_the_lower_case_name() {
        let s = Spell { comp: 0, name: "Crumble Undead".into(), level: 39, icon: 1, greyed: 2, kind: 0 };
        assert_eq!(s.server_id(), "crumble_undead");
    }

    #[test]
    fn every_standard_spell_of_the_server_is_in_the_cache_book() {
        let Ok(cache) = Cache::open_default() else { return };
        let Ok(configs) = osrs_cache::config::Configs::new(&cache) else { return };
        let Ok(items) = configs.items() else { return };
        let book = SpellBook::load(&cache, &items).unwrap();
        assert!(book.spells.len() > 60);
        let registry = osrs_core::data::Registry::builtin();
        for def in registry.spells().filter(|s| s.spellbook == "standard") {
            let found = book.spells.iter().find(|s| s.server_id() == def.id);
            let found = found.unwrap_or_else(|| panic!("{} is not in the cache's spellbook", def.id));
            assert_eq!(found.level, def.level, "{}", def.id);
        }
    }
}
