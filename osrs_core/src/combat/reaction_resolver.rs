use super::player_reaction_state::PlayerReactionState;
use super::targeted_attack::TargetedAttack;

/// Turns a targeted attack's mitigation plus the defender's live state into a damage multiplier
/// (0 = fully avoided, 1 = full damage).
///
/// When the mitigation is met (the matching protection prayer is up, or the attack's own condition
/// holds), `prayer_penetration` of the damage still gets through: 0 is the usual full block, 1 is
/// an attack the prayer does nothing against. Otherwise the damage is whole.
pub fn get_damage_multiplier(attack: &TargetedAttack, reaction_state: Option<&PlayerReactionState>) -> f32 {
    // TODO: implement late-reaction support, e.g. phantom muspah's magic attack
    if attack.mitigation.is_met(attack.style, reaction_state) {
        attack.prayer_penetration
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{AttackStyle, DefenderCondition, Mitigation, ProtectionPrayer};

    fn make_attack(mitigation: Mitigation, prayer_penetration: f32) -> TargetedAttack {
        TargetedAttack::new(AttackStyle::Melee)
            .with_prayer_penetration(prayer_penetration)
            .with_mitigation(mitigation)
    }

    fn state(prayer: ProtectionPrayer) -> PlayerReactionState {
        PlayerReactionState { active_prayer: prayer, ..Default::default() }
    }

    #[test]
    fn unmitigable_always_allows_full_damage() {
        let attack = make_attack(Mitigation::Unmitigable, 0.0);

        assert_eq!(get_damage_multiplier(&attack, Some(&state(ProtectionPrayer::Melee))), 1.0);
    }

    #[test]
    fn an_attack_checks_the_prayer_of_its_style_by_default() {
        let melee = TargetedAttack::new(AttackStyle::Melee);
        let magic = TargetedAttack::new(AttackStyle::Magic);

        assert_eq!(get_damage_multiplier(&melee, Some(&state(ProtectionPrayer::Melee))), 0.0);
        assert_eq!(get_damage_multiplier(&magic, Some(&state(ProtectionPrayer::Melee))), 1.0);
        assert_eq!(get_damage_multiplier(&magic, Some(&state(ProtectionPrayer::Magic))), 0.0);
    }

    #[test]
    fn wrong_or_no_prayer_deals_full_damage_regardless_of_penetration() {
        let attack = make_attack(Mitigation::ProtectionPrayer, 0.0);

        assert_eq!(get_damage_multiplier(&attack, Some(&state(ProtectionPrayer::Magic))), 1.0);
        assert_eq!(get_damage_multiplier(&attack, Some(&state(ProtectionPrayer::None))), 1.0);
        assert_eq!(get_damage_multiplier(&attack, None), 1.0);
    }

    #[test]
    fn a_piercing_attack_correct_prayer_still_lets_partial_damage_through() {
        // Phantom Muspah's ranged attacks: Protect from Missiles helps, but not to zero.
        let attack = TargetedAttack::new(AttackStyle::Ranged).with_prayer_penetration(0.5);

        assert_eq!(get_damage_multiplier(&attack, Some(&state(ProtectionPrayer::Ranged))), 0.5);
    }

    #[test]
    fn an_ignoring_attack_correct_prayer_does_nothing_at_all() {
        // Doom of Mokhaiotl's ground slams: the correct prayer provides no mitigation at all.
        let attack = TargetedAttack::new(AttackStyle::Magic).with_prayer_penetration(1.0);

        assert_eq!(get_damage_multiplier(&attack, Some(&state(ProtectionPrayer::Magic))), 1.0);
    }

    #[test]
    fn a_condition_mitigation_is_met_when_the_condition_holds() {
        // a screech that only lands on a defender with no protection prayer up
        let attack = make_attack(Mitigation::When(DefenderCondition::NoProtectionPrayer), 0.0);

        assert_eq!(get_damage_multiplier(&attack, Some(&state(ProtectionPrayer::None))), 0.0);
        assert_eq!(get_damage_multiplier(&attack, Some(&state(ProtectionPrayer::Melee))), 1.0);
    }
}
