//! The minimap orbs (group 160; scripts 446, 82, 447, 2069 and the shared 449).
//!
//! Each orb has a value text (font 494, centred), a base disc and a 26-wide clipping layer holding
//! the "empty" disc (sprite 1059) at the top of it. Script 449 sets the layer's height to
//! `26 * (max - cur) / max`, so it grows down over the coloured base as the value drops, and the
//! text colour runs red at 0 through yellow at half to green when full.
//!
//! The status variants follow scripts 446 (poison, venom), 82 (prayer lit), 454 (run) and 2792
//! (special attack). TODO: disease and parasite discs, the stamina-potion run icon, weapon-specific
//! special attack counters, hover highlights.

use super::{comp, PaneInput};
use osrs_cache::ui::{Override, UiState};

/// The text colour script 449 gives `cur` of `max`.
pub fn value_colour(cur: i32, max: i32) -> u32 {
    let max = max.max(1);
    let cur = cur.clamp(0, max);
    let half = max / 2;
    let rgb = |r: i32, g: i32| ((r.clamp(0, 255) as u32) << 16) | ((g.clamp(0, 255) as u32) << 8);
    if half <= 0 {
        return if cur >= max { 0x00ff00 } else { 0xff0000 };
    }
    if cur > half {
        rgb(255 - 255 * (cur - half) / half, 255)
    } else {
        rgb(255, 255 * cur / half)
    }
}

/// The height of the empty-disc layer.
pub fn fill_height(cur: i32, max: i32) -> i32 {
    let max = max.max(1);
    26 * (max - cur.clamp(0, max)) / max
}

fn set_value(state: &mut UiState, text: u32, fill: u32, cur: i32, max: i32) {
    let g = 160;
    state.overrides.insert(
        comp(g, text),
        Override { text: Some(cur.to_string()), colour: Some(value_colour(cur, max)), ..Default::default() },
    );
    state.overrides.insert(comp(g, fill), Override { size: Some((26, fill_height(cur, max))), ..Default::default() });
}

fn set_sprite(state: &mut UiState, file: u32, sprite: i32) {
    state.overrides.entry(comp(160, file)).or_default().sprite = Some(sprite);
}

/// The hitpoints disc: normal, poisoned (green) or venomed (purple).
pub fn hp_disc(poisoned: bool, venomed: bool) -> i32 {
    if venomed {
        1102
    } else if poisoned {
        1061
    } else {
        1060
    }
}

pub fn sync(state: &mut UiState, input: &PaneInput) {
    let Some(p) = input.player else { return };
    set_value(state, 10, 14, p.health, p.max_health);
    set_value(state, 21, 23, p.prayer, p.max_prayer);
    set_value(state, 29, 31, p.run_energy, 100);
    set_value(state, 37, 39, p.special_energy, 100);

    // hitpoints (script 446): the disc turns poison green or venom purple, the empty-disc layer
    // fades, the "Cure" button appears
    let status = p.poisoned || p.venomed;
    let disc = hp_disc(p.poisoned, p.venomed);
    set_sprite(state, 11, disc);
    set_sprite(state, 13, disc);
    state.overrides.entry(comp(160, 15)).or_default().transparency = Some(if status { 100 } else { 0 });
    state.overrides.entry(comp(160, 12)).or_default().hidden = Some(!status);
    state.overrides.entry(comp(160, 9)).or_default().hidden = Some(!status);

    // prayer (script 82/456): lit while a prayer is on
    let praying = !p.active_prayers.is_empty();
    set_sprite(state, 22, if praying { 1066 } else { 1063 });
    set_sprite(state, 25, if praying { 1058 } else { 1068 });

    // run (script 454): walking and running discs and boots
    set_sprite(state, 30, if p.is_running { 1065 } else { 1064 });
    set_sprite(state, 33, if p.is_running { 1070 } else { 1069 });

    // special attack (script 2792): lit while switched on; greyed out, and not clickable, without
    // a weapon that has a special
    let spec = state.overrides.entry(comp(160, 38)).or_default();
    if p.special_cost.is_some() {
        spec.sprite = Some(if p.special_enabled { 1608 } else { 1607 });
        spec.transparency = Some(25);
    } else {
        spec.sprite = Some(1064);
        spec.transparency = Some(50);
    }
    state.overrides.entry(comp(160, 36)).or_default().hidden = Some(p.special_cost.is_none());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poison_and_venom_change_the_hitpoints_disc() {
        assert_eq!(hp_disc(false, false), 1060);
        assert_eq!(hp_disc(true, false), 1061);
        assert_eq!(hp_disc(true, true), 1102);
    }

    #[test]
    fn the_fill_drains_from_the_top() {
        assert_eq!(fill_height(10, 10), 0);
        assert_eq!(fill_height(0, 10), 26);
        assert_eq!(fill_height(5, 10), 13);
    }

    #[test]
    fn the_value_runs_red_yellow_green() {
        assert_eq!(value_colour(0, 10), 0xff0000);
        assert_eq!(value_colour(5, 10), 0xffff00);
        assert_eq!(value_colour(10, 10), 0x00ff00);
    }
}
