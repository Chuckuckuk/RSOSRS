//! The Combat Options tab (group 593; scripts 7592, 7593, 2486, 324, 325, 187, 189, 329).
//!
//! * The weapon's name (font 497, "Unarmed" with nothing wielded) and the "Combat Lvl: N" line under
//!   it, which moves down 6 pixels when the name needs two lines (script 2486).
//! * One button per attack style of the wielded weapon. What a weapon type offers is the cache's
//!   database table 78 (read by `db_find` in script 7603): per weapon type up to four entries
//!   `(slot, name, tooltip, icon sprite)`. The buttons are laid out two by two (71x47 at x = 20 and
//!   99, y = 45 and 99); the slot is the number the game's `varp 43` holds, so button `a` is
//!   component `593:6 + 4a`. Staves and wands (types 18, 21, 22) use a column of 71x32 buttons with
//!   the two spell buttons beside it (593:22..31).
//! * The selected style gets the highlighted frame (script 836: sprites 1150..1158) and the others
//!   the plain one (script 835: 1141..1149).
//! * Auto retaliate (593:32). The simulation has no such setting; the button is drawn "On", as a
//!   new account has it, and does nothing.
//! * The special attack bar (593:38): shown only when the weapon has a special, filled to the
//!   player's special energy, its text yellow while the special is switched on (script 187).
//!
//! The server names its weapons by its own categories and the styles of each in its own order;
//! [`weapon_type`] says which of the game's weapon types a category is, and [`assign`] matches the
//! server's styles to the type's slots by name (then by place), which is what a click needs to
//! turn "the button in slot 3" back into `SetStance { index }`.

use super::{comp, sprite_child, PaneInput};
use osrs_cache::dbtable::DbTables;
use osrs_cache::interface::Component;
use osrs_cache::ui::{Assets, Override, UiState};
use osrs_cache::Cache;
use std::collections::HashMap;

/// The database table that lists each weapon type's attack styles.
const STYLE_TABLE: u32 = 78;

/// One attack style a weapon type offers.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleEntry {
    /// The slot (0..3) the game's `varp 43` holds when this style is chosen.
    pub slot: usize,
    pub name: String,
    pub tooltip: String,
    pub sprite: i32,
}

/// The attack styles of every weapon type, from the cache.
#[derive(Default)]
pub struct WeaponStyles {
    types: HashMap<i32, Vec<StyleEntry>>,
}

impl WeaponStyles {
    pub fn load(cache: &Cache) -> osrs_cache::Result<WeaponStyles> {
        let tables = DbTables::load(cache)?;
        let mut types = HashMap::new();
        for row in tables.rows_of(STYLE_TABLE) {
            let Some(ty) = row.tuple(0, 0).and_then(|t| t.first()).and_then(|v| v.int()) else { continue };
            let mut entries = Vec::new();
            for i in 0.. {
                let Some(t) = row.tuple(1, i) else { break };
                if let [slot, name, tooltip, sprite] = t {
                    entries.push(StyleEntry {
                        slot: slot.int().unwrap_or(0).clamp(0, 3) as usize,
                        name: name.str().unwrap_or("").to_owned(),
                        tooltip: tooltip.str().unwrap_or("").to_owned(),
                        sprite: sprite.int().unwrap_or(-1),
                    });
                }
            }
            types.insert(ty, entries);
        }
        Ok(WeaponStyles { types })
    }

    /// The styles of game weapon type `ty`.
    pub fn of(&self, ty: i32) -> &[StyleEntry] {
        self.types.get(&ty).map_or(&[], Vec::as_slice)
    }
}

/// Which of the game's weapon types (varbit 357) a category of the server's weapon data is.
pub fn weapon_type(category: &str) -> i32 {
    match category {
        "axe" | "axe_2h" => 1,
        "blunt_2h" => 2,
        "bow" => 3,
        "crossbow" => 5,
        "godsword" => 10,
        "halberd" => 26,
        "staff" => 18,
        "powered_staff" => 24,
        "blowpipe" => 19,
        "whip" => 20,
        "scimitar" | "slash_sword" => 9,
        "dagger" | "stab_sword" => 17,
        "mace" => 16,
        // "unarmed" and anything the table does not know
        _ => 0,
    }
}

/// Weapon types whose buttons are the staff layout (a column, with the spell buttons beside it).
fn is_staff(ty: i32) -> bool {
    matches!(ty, 18 | 21 | 22)
}

/// For each of the four slots, the server's style index that sits in it. Styles are matched to
/// entries by name first, then the rest in order; spell styles have no slot.
pub fn assign(entries: &[StyleEntry], stances: &[String]) -> [Option<usize>; 4] {
    let mut out = [None; 4];
    let mut taken = vec![false; entries.len()];
    let mut placed = vec![false; stances.len()];
    let is_spell = |name: &str| name.to_lowercase().contains("spell");
    for (si, name) in stances.iter().enumerate() {
        if is_spell(name) {
            placed[si] = true;
            continue;
        }
        if let Some(e) = (0..entries.len()).find(|&e| !taken[e] && entries[e].name.eq_ignore_ascii_case(name)) {
            taken[e] = true;
            placed[si] = true;
            out[entries[e].slot] = Some(si);
        }
    }
    for si in 0..stances.len() {
        if placed[si] {
            continue;
        }
        if let Some(e) = (0..entries.len()).find(|&e| !taken[e]) {
            taken[e] = true;
            out[entries[e].slot] = Some(si);
        }
    }
    out
}

/// The server's style index for the button in `slot`, for the player as the snapshot shows it.
pub fn stance_for_slot(styles: &WeaponStyles, p: &osrs_core::sim::PlayerSnapshot, slot: usize) -> Option<usize> {
    let ty = weapon_type(&p.weapon_category);
    assign(styles.of(ty), &p.stances).get(slot).copied().flatten()
}

/// Component file of the button for slot `a` (its frame layer is `+1`, sprite `+2`, text `+3`).
pub const fn button(a: usize) -> u32 {
    6 + 4 * a as u32
}

/// The frame script 835 (plain, sprites 1141..) or 836 (selected, 1150..) draws inside a layer:
/// four corners, four tiled edges and a tiled middle.
pub fn frame(selected: bool) -> Vec<Component> {
    let base = if selected { 1150 } else { 1141 };
    // (x, y, w, h, x mode, y mode, w mode, h mode, sprite offset, tiled)
    type Part = (i16, i16, u16, i32, i8, i8, i8, i8, i32, bool);
    let parts: [Part; 9] = [
        (0, 0, 6, 6, 0, 0, 0, 0, 0, false),
        (6, 0, 12, 6, 0, 0, 1, 0, 1, true),
        (0, 0, 6, 6, 2, 0, 0, 0, 2, false),
        (0, 6, 6, 12, 0, 0, 0, 1, 3, true),
        (6, 6, 12, 12, 0, 0, 1, 1, 4, true),
        (0, 6, 6, 12, 2, 0, 0, 1, 5, true),
        (0, 0, 6, 6, 0, 2, 0, 0, 6, false),
        (6, 0, 12, 6, 0, 2, 1, 0, 7, true),
        (0, 0, 6, 6, 2, 2, 0, 0, 8, false),
    ];
    parts
        .into_iter()
        .map(|(x, y, w, h, xm, ym, wm, hm, off, tiling)| Component {
            x_mode: xm,
            y_mode: ym,
            width_mode: wm,
            height_mode: hm,
            tiling,
            ..sprite_child(x as i32, y as i32, w as i32, h, base + off)
        })
        .collect()
}

fn set(state: &mut UiState, file: u32) -> &mut Override {
    state.overrides.entry(comp(593, file)).or_default()
}

pub fn sync(state: &mut UiState, input: &PaneInput, assets: &mut dyn Assets) {
    let Some(p) = input.player else { return };
    let styles = &input.icons.styles;

    // the weapon's name and the combat level line under it
    let weapon = p
        .equipped
        .iter()
        .find(|(slot, _)| *slot == osrs_core::data::EquipSlot::Weapon)
        .map(|(_, item)| item.name.clone());
    let name = weapon.unwrap_or_else(|| "Unarmed".to_owned());
    let two_lines = assets.font(497).is_some_and(|f| f.width(&name) as i32 > 169);
    set(state, 3).text = Some(name);
    set(state, 4).text = Some(format!("Combat Lvl: {}", p.combat_level));
    set(state, 4).pos = Some((0, if two_lines { 32 } else { 26 }));
    // the sailing banner is not in this game: the weapon block sits at the left
    set(state, 2).pos = Some((0, 0));
    set(state, 46).hidden = Some(true);
    set(state, 37).hidden = Some(true);

    // the style buttons
    let ty = weapon_type(&p.weapon_category);
    let entries = styles.of(ty);
    let staff = is_staff(ty);
    let slots = assign(entries, &p.stances);
    let selected = p.stance_index.or((!p.stances.is_empty()).then_some(0));
    let selected_slot = selected.and_then(|s| slots.iter().position(|x| *x == Some(s)));
    let spell_selected = selected.and_then(|s| p.stances.get(s)).map(|n| n.to_lowercase());
    let (xs, ys): ([i32; 4], [i32; 4]) =
        if staff { ([20; 4], [45, 81, 117, 153]) } else { ([20, 99, 20, 99], [45, 45, 99, 99]) };
    let (bw, bh) = if staff { (71, 32) } else { (71, 47) };
    let mut rank = 0;
    for a in 0..4 {
        let file = button(a);
        let Some(entry) = entries.iter().find(|e| e.slot == a && e.sprite != -1) else {
            set(state, file).hidden = Some(true);
            continue;
        };
        {
            let o = set(state, file);
            o.hidden = Some(false);
            o.pos = Some((xs[rank], ys[rank]));
            o.size = Some((bw, bh));
        }
        let icon = set(state, file + 2);
        icon.sprite = Some(entry.sprite);
        icon.pos = Some((0, if staff { -1 } else { -6 }));
        if a == 0 && staff {
            icon.size = Some((33, 23));
        } else if a == 0 && ty == 8 {
            icon.size = Some((30, 30));
        }
        let text = set(state, file + 3);
        text.text = Some(entry.name.clone());
        text.pos = Some((0, if staff { 10 } else { 13 }));
        state.created.insert(comp(593, file + 1), frame(selected_slot == Some(a)));
        rank += 1;
    }

    // the spell buttons of the staff layout
    set(state, 22).hidden = Some(!staff);
    if staff {
        set(state, 22).size = Some((71, 104));
        set(state, 22).pos = Some((20 + 71 + 8, 45));
        for file in [23, 24, 28, 29] {
            // these layers are as wide as their parent (width mode 1, raw 0)
            set(state, file).size = Some((0, 50));
        }
        let defensive = spell_selected.as_deref().is_some_and(|n| n.contains("defensive"));
        let plain = spell_selected.as_deref().is_some_and(|n| n.contains("spell") && !n.contains("defensive"));
        state.created.insert(comp(593, 23), frame(defensive));
        state.created.insert(comp(593, 28), frame(plain));
        for (file, x) in [(25, 14), (30, 0)] {
            let o = set(state, file);
            o.size = Some((33, 36));
            o.pos = Some((x, -5));
        }
    }

    // auto retaliate: drawn on, as a new account has it
    set(state, 32).size = Some((150, 44));
    set(state, 32).pos = Some((0, 153));
    state.created.insert(comp(593, 34), frame(true));
    set(state, 35).sprite = Some(1749);
    set(state, 36).text = Some("Auto Retaliate<br>(On)".to_owned());

    // the special attack bar
    match p.special_cost {
        None => {
            set(state, 38).hidden = Some(true);
        }
        Some(cost) => {
            let bar = 146;
            let energy = p.special_energy.clamp(0, 100);
            set(state, 38).hidden = Some(false);
            set(state, 38).size = Some((150, 26));
            set(state, 38).pos = Some((0, 153 + 44 + 7));
            set(state, 40).size = Some((4, 12));
            set(state, 44).size = Some((4, 14));
            // the fill layer's width is `parent - raw`
            set(state, 41).size = Some((4 + bar - bar * energy / 100, 12));
            set(state, 42).colour = Some(if energy >= cost { 0x397d3b } else { 0x00326b });
            let text = set(state, 43);
            text.text = Some(format!("Special Attack: {energy}%"));
            text.colour = Some(if p.special_enabled { 0xffff00 } else { 0x000010 });
            state.created.insert(comp(593, 39), frame(false));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(slot: usize, name: &str) -> StyleEntry {
        StyleEntry { slot, name: name.to_owned(), tooltip: String::new(), sprite: 1 }
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn styles_are_matched_by_name_then_by_place() {
        // the whip: three entries in slots 0, 1 and 3
        let whip = [entry(0, "Flick"), entry(1, "Lash"), entry(3, "Deflect")];
        assert_eq!(assign(&whip, &names(&["Flick", "Lash", "Deflect"])), [Some(0), Some(1), None, Some(2)]);
        // a powered staff offers Accurate twice; the server's Longrange finds its own slot
        let powered = [entry(0, "Accurate"), entry(1, "Accurate"), entry(3, "Longrange")];
        assert_eq!(assign(&powered, &names(&["Accurate", "Longrange"])), [Some(0), None, None, Some(1)]);
        // a staff: the server's "Block" is the game's "Focus"; the spell styles have no button
        let staff = [entry(0, "Bash"), entry(1, "Pound"), entry(3, "Focus")];
        let server = names(&["Bash", "Pound", "Block", "Spell", "Defensive spell"]);
        assert_eq!(assign(&staff, &server), [Some(0), Some(1), None, Some(2)]);
    }

    #[test]
    fn every_category_of_the_servers_data_has_a_type_with_its_styles() {
        let Ok(cache) = Cache::open_default() else { return };
        let Ok(styles) = WeaponStyles::load(&cache) else { return };
        let registry = osrs_core::data::Registry::builtin();
        let mut categories: Vec<(String, Vec<String>)> = registry
            .items()
            .filter_map(|i| i.weapon.as_ref())
            .map(|w| (w.category.clone(), w.stances.iter().map(|s| s.name.clone()).collect()))
            .collect();
        categories.push(("unarmed".to_owned(), names(&["Punch", "Kick", "Block"])));
        for (category, stances) in categories {
            let entries = styles.of(weapon_type(&category));
            assert!(!entries.is_empty(), "{category} has no weapon type");
            let slots = assign(entries, &stances);
            let placed = slots.iter().flatten().count();
            let melee = stances.iter().filter(|s| !s.to_lowercase().contains("spell")).count();
            assert_eq!(placed, melee, "{category}: {stances:?} do not fit {entries:?}");
        }
    }
}
