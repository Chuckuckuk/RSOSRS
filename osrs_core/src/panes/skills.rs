//! The Skills tab (group 320; scripts 2366, 393, 395, 396).
//!
//! The 24 cells 320:1..24 are 62x30 layers in three columns. Script 393 rebuilds each with the
//! frame in two halves (sprite 187 at (0, 0) and 188 at (31, 0), both 36x36), the skill's 25x25
//! icon at `(3 + nudge, 4)`, the current level at (32, 4) and the base level at (44, 16) (15x12
//! each, font 494, yellow, centred, shadowed). The icon of a stat is enum 255 and the stat a cell
//! shows is enum 681 (keyed by the cell's number in the game's own order), both read from the cache.
//! `320:32` says "Total level: N", the sum of every skill's base level (script 396 and 1007).
//!
//! The server models seven skills (`Skill`); every other skill shows level 1. TODO: the XP tooltip
//! (needs XP in the snapshot), "View guide" and the members markers.

use super::{comp, sprite_child, text_child, PaneInput};
use osrs_cache::clientscript::load_enum;
use osrs_cache::ui::{Override, UiState};
use osrs_cache::Cache;
use osrs_core::model::Skill;

/// A cell of the tab: the component file in group 320, its number in enum 681 and the nudge script
/// 393 is given for its icon (read from the `onLoad` arguments in the cache).
pub const CELLS: [(u32, i32, i32); 24] = [
    (1, 1, 0),
    (2, 2, 0),
    (3, 5, 0),
    (4, 3, 1),
    (5, 7, 1),
    (6, 4, 1),
    (7, 12, 1),
    (8, 22, 0),
    (9, 6, 1),
    (10, 8, 1),
    (11, 9, 1),
    (12, 10, 2),
    (13, 11, 2),
    (14, 19, 2),
    (15, 20, -1),
    (16, 23, 0),
    (17, 13, 2),
    (18, 14, 2),
    (19, 15, 1),
    (20, 16, 3),
    (21, 17, 2),
    (22, 18, 2),
    (23, 21, 1),
    (24, 24, 1),
];

/// The game's stat numbers for the skills the server has.
pub fn skill_of_stat(stat: i32) -> Option<Skill> {
    Some(match stat {
        0 => Skill::Attack,
        1 => Skill::Defence,
        2 => Skill::Strength,
        3 => Skill::Hitpoints,
        4 => Skill::Ranged,
        5 => Skill::Prayer,
        6 => Skill::Magic,
        _ => return None,
    })
}

/// What a cell shows, from the cache's enums.
#[derive(Default)]
pub struct SkillTable {
    /// Per entry of [`CELLS`]: the stat and its icon sprite.
    cells: Vec<(i32, i32)>,
}

impl SkillTable {
    pub fn load(cache: &Cache) -> osrs_cache::Result<SkillTable> {
        let stat_of = load_enum(cache, 681)?;
        let icon_of = load_enum(cache, 255)?;
        let int = |e: &osrs_cache::clientscript::EnumDef, key: i32| {
            e.entries.iter().find(|(k, _)| *k == key).and_then(|(_, v)| match v {
                osrs_cache::clientscript::Operand::Int(i) => Some(*i),
                _ => None,
            })
        };
        let mut cells = Vec::new();
        for (_, key, _) in CELLS {
            let stat = int(&stat_of, key).unwrap_or(-1);
            cells.push((stat, int(&icon_of, stat).unwrap_or(-1)));
        }
        Ok(SkillTable { cells })
    }
}

/// The level shown for a stat: the server's, else the game's starting level (10 for Hitpoints).
fn levels(input: &PaneInput, stat: i32) -> (i32, i32) {
    let start = if stat == 3 { 10 } else { 1 };
    let found = input
        .player
        .zip(skill_of_stat(stat))
        .and_then(|(p, skill)| p.skills.iter().find(|s| s.skill == skill));
    found.map_or((start, start), |s| (s.current, s.base))
}

pub fn sync(state: &mut UiState, input: &PaneInput) {
    let table = &input.icons.skills;
    if table.cells.is_empty() {
        return;
    }
    let mut total = 0;
    for ((file, _, nudge), (stat, icon)) in CELLS.into_iter().zip(&table.cells) {
        let (current, base) = levels(input, *stat);
        total += base;
        state.created.insert(
            comp(320, file),
            vec![
                sprite_child(0, 0, 36, 36, 187),
                sprite_child(31, 0, 36, 36, 188),
                sprite_child(3 + nudge, 4, 25, 25, *icon),
                text_child(32, 4, 15, 12, 494, 0xffff00, &current.to_string()),
                text_child(44, 16, 15, 12, 494, 0xffff00, &base.to_string()),
            ],
        );
    }
    state.overrides.insert(
        comp(320, 32),
        Override { text: Some(format!("Total level: {total}")), ..Default::default() },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cells_cover_every_stat_once() {
        let mut keys: Vec<i32> = CELLS.iter().map(|c| c.1).collect();
        keys.sort_unstable();
        assert_eq!(keys, (1..=24).collect::<Vec<_>>());
    }

    #[test]
    fn the_servers_skills_have_a_stat() {
        for skill in Skill::ALL {
            assert!((0..24).any(|s| skill_of_stat(s) == Some(skill)), "{skill:?}");
        }
    }
}
