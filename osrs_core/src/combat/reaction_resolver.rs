use super::attack_definition::AttackDefinition;
use super::player_reaction_state::PlayerReactionState;
use super::protection_prayer::ProtectionPrayer;
use super::reaction_requirement::ReactionKind;
use super::reaction_requirement::HazardAnchor;
use crate::tiles::TileCoord;

/// Turns an attack's reaction requirement plus the player's live state into a damage multiplier
/// (0 = fully avoided, 1 = full damage).
pub fn get_damage_multiplier(
    attack: &AttackDefinition,
    reaction_state: Option<&PlayerReactionState>,
    hazard_origin_tile: TileCoord,
    target_tile: TileCoord,
) -> f32 {
    match attack.reaction.kind {
        ReactionKind::PrayerMatch(required_prayer) => {
            // TODO: implement late-reaction support, e.g. phantom muspah's magic attack
            let prayer_correct = reaction_state.is_some_and(|state| {
                state.active_prayer != ProtectionPrayer::None && state.active_prayer == required_prayer
            });
            // prayer_penetration is the fraction that gets through the CORRECT prayer
            // regardless (0 = normal, fully blocked; 1 = "ignore"). Wrong prayer is
            // always full damage.
            if prayer_correct {
                attack.prayer_penetration
            } else {
                1.0
            }
        },
        ReactionKind::DisablePrayer => {
            // Attacks that require protection prayers to be disabled (e.g. Maggot King's screech, Echo Hunllef's black orb)
            let prayer_disabled = reaction_state.is_some_and(|state| state.active_prayer == ProtectionPrayer::None);
            if prayer_disabled {
                attack.prayer_penetration
            } else {
                1.0
            }
        },
        ReactionKind::MoveOffTile(anchor) => {
            // Placeholder logic: attacks targeting tile(s) should check for attackable entities standing on tiles and resolve damage accordingly. 
            // For now, just check if the target is still on the hazard tile.
            let hazard_tile = match anchor {
                HazardAnchor::SelfTile => hazard_origin_tile,
                HazardAnchor::Target => target_tile,
            };
            let distance = target_tile.chebyshev_distance_to(hazard_tile);
            if distance > 0 {
                0.0
            } else {
                1.0
            }
        },
        ReactionKind::Interrupt => 1.0, // placeholder logic: interruptable attacks should never resolve if the interrupt condition is met. For now, just always resolve damage.
        ReactionKind::None => 1.0 // full damage, always; Fragment of Seren's max health nuke, for example, has fixed health-based damage with no mitigation method
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{AttackStyle, ReactionRequirement};

    fn make_attack(reaction: ReactionRequirement, prayer_penetration: f32) -> AttackDefinition {
        AttackDefinition::new("Test", 0, 1, AttackStyle::Melee)
            .with_prayer_penetration(prayer_penetration)
            .with_reaction(reaction)
    }

    fn state(prayer: ProtectionPrayer) -> PlayerReactionState {
        PlayerReactionState { active_prayer: prayer, ..Default::default() }
    }

    fn multiplier(attack: &AttackDefinition, state: Option<&PlayerReactionState>) -> f32 {
        get_damage_multiplier(attack, state, TileCoord::default(), TileCoord::default())
    }

    #[test]
    fn none_always_allows_full_damage() {
        let attack = make_attack(ReactionRequirement::none(), 0.0);

        assert_eq!(multiplier(&attack, Some(&state(ProtectionPrayer::Melee))), 1.0);
    }

    #[test]
    fn prayer_match_correct_prayer_fully_blocks_a_zero_penetration_attack() {
        let attack = make_attack(ReactionRequirement::prayer_match(ProtectionPrayer::Melee), 0.0);

        assert_eq!(multiplier(&attack, Some(&state(ProtectionPrayer::Melee))), 0.0);
    }

    #[test]
    fn prayer_match_wrong_prayer_deals_full_damage_regardless_of_penetration() {
        let attack = make_attack(ReactionRequirement::prayer_match(ProtectionPrayer::Melee), 0.0);

        assert_eq!(multiplier(&attack, Some(&state(ProtectionPrayer::Magic))), 1.0);
    }

    #[test]
    fn prayer_match_no_prayer_active_deals_full_damage() {
        let attack = make_attack(ReactionRequirement::prayer_match(ProtectionPrayer::Melee), 0.0);

        assert_eq!(multiplier(&attack, Some(&state(ProtectionPrayer::None))), 1.0);
    }

    #[test]
    fn prayer_match_with_no_reaction_state_deals_full_damage() {
        let attack = make_attack(ReactionRequirement::prayer_match(ProtectionPrayer::Melee), 0.0);

        assert_eq!(multiplier(&attack, None), 1.0);
    }

    #[test]
    fn prayer_match_piercing_attack_correct_prayer_still_lets_partial_damage_through() {
        // Phantom Muspah's ranged attacks: Protect from Missiles helps, but not to zero.
        let attack = make_attack(ReactionRequirement::prayer_match(ProtectionPrayer::Ranged), 0.5);

        assert_eq!(multiplier(&attack, Some(&state(ProtectionPrayer::Ranged))), 0.5);
    }

    #[test]
    fn prayer_match_ignoring_attack_correct_prayer_does_nothing_at_all() {
        // Doom of Mokhaiotl's ground slams: the correct prayer provides no mitigation at all.
        let attack = make_attack(ReactionRequirement::prayer_match(ProtectionPrayer::Magic), 1.0);

        assert_eq!(multiplier(&attack, Some(&state(ProtectionPrayer::Magic))), 1.0);
    }

}
