//! Which animations the player's own attacks and blocks play. They depend on what is wielded
//! (a whip flicks, a scimitar slashes, a bow draws) and on the stance's damage type; a shield
//! changes the block. These are cache sequence ids, sent to the client in the snapshot.

use crate::model::DamageType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerAnimations {
    pub attack: u32,
    pub defend: u32,
}

/// Bare fists: a punch, or a kick for a damage type of crush.
const PUNCH: u32 = 422;
const KICK: u32 = 423;
const UNARMED_BLOCK: u32 = 424;
const SHIELD_BLOCK: u32 = 1156;

/// `kind` is the weapon's category (or, for weapons the data has no category for, its name), lower
/// case; `damage_type` is the selected stance's.
pub fn player_animations(kind: Option<&str>, damage_type: Option<DamageType>, has_shield: bool) -> PlayerAnimations {
    let stab = damage_type == Some(DamageType::Stab);
    let crush = damage_type == Some(DamageType::Crush);
    let has = |words: &[&str]| kind.is_some_and(|k| words.iter().any(|w| k.contains(w)));
    let (attack, defend) = if kind.is_none() {
        (if crush { KICK } else { PUNCH }, UNARMED_BLOCK)
    } else if has(&["whip"]) {
        (1658, 1659)
    } else if has(&["crossbow"]) {
        (4230, UNARMED_BLOCK)
    } else if has(&["blowpipe"]) {
        (5061, UNARMED_BLOCK)
    } else if has(&["bow"]) {
        (426, UNARMED_BLOCK)
    } else if has(&["staff", "wand"]) {
        (if crush { 419 } else { 1167 }, 420)
    } else if has(&["halberd"]) {
        (if stab { 428 } else { 440 }, 430)
    } else if has(&["spear", "hasta"]) {
        (if crush { 2080 } else { 381 }, 430)
    } else if has(&["godsword", "2h", "two"]) {
        (if stab { 7048 } else { 7041 }, 7056)
    } else if has(&["dagger"]) {
        (if stab { 376 } else { 377 }, 378)
    } else if has(&["mace", "club", "hammer", "pickaxe", "blunt", "maul"]) {
        (if stab { 400 } else { 401 }, 403)
    } else if has(&["axe"]) {
        (if crush { 401 } else { 395 }, 397)
    } else if has(&["scimitar", "sword", "slash"]) {
        (if stab { 386 } else { 390 }, 388)
    } else {
        (if crush { KICK } else { PUNCH }, UNARMED_BLOCK)
    };
    // behind a shield the block is the shield's, whatever is wielded (bows and staves excepted: those
    // keep theirs)
    let defend = if has_shield && defend == UNARMED_BLOCK { SHIELD_BLOCK } else { defend };
    PlayerAnimations { attack, defend }
}

/// The cache graphic an arrow or bolt of this kind flies as. Arrows are one model recoloured by
/// tier; bolts are another.
pub fn projectile_graphic(ammo_item: &str) -> u32 {
    let id = ammo_item.to_lowercase();
    let tier = |name: &str| id.starts_with(name);
    if id.contains("bolt") {
        27
    } else if id.contains("dragon") {
        1120
    } else if tier("iron") {
        9
    } else if tier("steel") {
        11
    } else if tier("mithril") {
        12
    } else if tier("adamant") {
        13
    } else if tier("rune") {
        15
    } else {
        // bronze and anything else
        10
    }
}

/// The graphic shown on the archer while it nocks and draws, for arrows (bolts have none).
pub fn draw_graphic(ammo_item: &str) -> Option<u32> {
    let id = ammo_item.to_lowercase();
    if id.contains("bolt") {
        return None;
    }
    Some(if id.contains("dragon") {
        1111
    } else if id.starts_with("iron") {
        18
    } else if id.starts_with("steel") {
        20
    } else if id.starts_with("mithril") {
        21
    } else if id.starts_with("adamant") {
        22
    } else if id.starts_with("rune") {
        24
    } else {
        19
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_fly_as_their_tiers_graphic() {
        assert_eq!(projectile_graphic("bronze_arrow"), 10);
        assert_eq!(projectile_graphic("iron_arrow"), 9);
        assert_eq!(projectile_graphic("rune_arrow"), 15);
        assert_eq!(projectile_graphic("dragon_arrow"), 1120);
        assert_eq!(projectile_graphic("steel_bolts"), 27);
    }

    #[test]
    fn bare_hands_punch_and_a_whip_flicks() {
        assert_eq!(player_animations(None, None, false), PlayerAnimations { attack: 422, defend: 424 });
        assert_eq!(player_animations(Some("whip"), Some(DamageType::Slash), false).attack, 1658);
    }

    #[test]
    fn a_swords_animation_follows_the_stance() {
        let slash = player_animations(Some("scimitar"), Some(DamageType::Slash), false);
        let stab = player_animations(Some("scimitar"), Some(DamageType::Stab), false);
        assert_ne!(slash.attack, stab.attack);
        // weapons without a category are found by name
        assert_eq!(player_animations(Some("bronze sword"), Some(DamageType::Slash), false).attack, slash.attack);
    }

    #[test]
    fn a_shield_changes_the_block_of_fists() {
        assert_eq!(player_animations(None, None, true).defend, 1156);
        assert_eq!(player_animations(Some("whip"), None, true).defend, 1659);
    }
}
