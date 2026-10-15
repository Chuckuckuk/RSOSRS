//! Turns data definitions (`NpcDef` and its attacks) into the runtime types the combat code uses.

use std::rc::Rc;

use crate::actors::{
    ChaseToMeleeRangeMovementPolicy, MaintainDistanceMovementPolicy, MovementPolicy, StationaryMovementPolicy,
    StyleReactiveMovementPolicy,
};
use crate::combat::{
    AttackDefinition, AttackStyle, CombatStats, MaxHitSource, ProjectileSpeed, ProtectionPrayer, RangeAwarePattern,
    ReactionRequirement, RotationMode,
};
use crate::data::{NpcAttackDef, NpcDef, NpcMovementDef, RotationDef};
use crate::model::{DamageType, StanceBonuses};

fn protection_for(style: AttackStyle) -> ProtectionPrayer {
    match style {
        AttackStyle::Melee => ProtectionPrayer::Melee,
        AttackStyle::Ranged => ProtectionPrayer::Ranged,
        AttackStyle::Magic => ProtectionPrayer::Magic,
    }
}

/// One of a monster's attacks as a runtime `AttackDefinition`. Assumes the definition has been
/// through `Registry::validate`.
pub fn attack_from_def(def: &NpcAttackDef) -> Rc<AttackDefinition> {
    attack_sounding(def, None)
}

/// [`attack_from_def`], heard as `fallback_sound` when the attack names no sound of its own.
fn attack_sounding(def: &NpcAttackDef, fallback_sound: Option<u32>) -> Rc<AttackDefinition> {
    // the attack's timing archetype comes from its numbers: a delay until it lands is Doom-style,
    // a wind-up is Jad-style, neither is traditional
    let mut attack = if def.impact_delay_ticks > 0 {
        AttackDefinition::on_contact(def.name.clone(), def.range, def.impact_delay_ticks, def.style)
    } else {
        AttackDefinition::targeted(def.name.clone(), def.range, def.telegraph_ticks, def.style)
    }
    .with_damage_type(def.damage_type.unwrap_or_else(|| DamageType::default_for(def.style)))
    .with_max_hit(MaxHitSource::Fixed(def.max_hit))
    .with_prayer_penetration(def.prayer_penetration)
    .with_modifiers(def.modifiers.clone())
    .with_hit_count(def.hit_count)
    .with_animation(def.animation)
    .with_graphics(def.projectile, def.target_graphic, def.impact_graphic)
    .with_sounds(def.sound.or(fallback_sound), def.resolve_sound);
    if let Some(speed) = def.speed {
        attack = attack.with_attack_speed(speed);
    }
    if let Some(min_range) = def.min_range {
        attack = attack.with_min_range(min_range);
    }
    if let (Some(speed), 0) = (def.projectile_speed, def.impact_delay_ticks) {
        attack = attack.with_projectile_speed(ProjectileSpeed::tiles_per_tick(speed));
    }
    if def.protectable {
        attack = attack.with_reaction(ReactionRequirement::prayer_match(protection_for(def.style)));
    }
    if let Some(status) = def.on_hit.and_then(|effect| effect.to_status().ok()) {
        attack = attack.with_on_hit_status_effect(status);
    }
    attack.shared()
}

pub fn movement_policy_from_def(movement: NpcMovementDef) -> Rc<dyn MovementPolicy> {
    match movement {
        NpcMovementDef::Chase => Rc::new(ChaseToMeleeRangeMovementPolicy),
        NpcMovementDef::Stationary => Rc::new(StationaryMovementPolicy),
        NpcMovementDef::MaintainDistance { distance } => Rc::new(MaintainDistanceMovementPolicy::new(distance)),
        NpcMovementDef::StyleReactive { kite_distance } => Rc::new(StyleReactiveMovementPolicy::new(kite_distance)),
    }
}

/// The attack pattern of a monster with at least one attack.
pub fn pattern_from_def(def: &NpcDef) -> RangeAwarePattern {
    let attacks = def.attacks.iter().map(|attack| (attack_sounding(attack, def.sounds.attack), attack.weight)).collect();
    let mode = match def.rotation {
        RotationDef::Fixed => RotationMode::Fixed,
        RotationDef::Weighted => RotationMode::Weighted,
    };
    RangeAwarePattern::new(attacks, mode, def.size, def.attack_range)
}

/// A monster's combat stats: its levels, its attack/defence bonuses and its attribute tags. A
/// monster attacks with an effective level of `level + 9` (reference section 4's monster
/// asymmetry, seen from the attacking side: the usual +8 plus a +1 stance bonus).
pub fn stats_from_def(def: &NpcDef) -> CombatStats {
    let levels = def.levels;
    CombatStats::new(levels.attack, levels.strength, levels.defence, levels.ranged, levels.magic)
        .expect("validated monster levels are at least 1")
        .with_bonuses(def.bonuses)
        .with_attributes(def.attributes)
        .with_stance(StanceBonuses { attack: 1, strength: 1, ranged: 1, magic: 1, defence: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Registry;

    #[test]
    fn jads_attacks_carry_their_listed_numbers() {
        let registry = Registry::builtin();
        let jad = registry.npc("tztok_jad").unwrap();

        let magic = attack_from_def(&jad.attacks[2]);

        let targeted = magic.expect_targeted();
        assert_eq!(targeted.style, AttackStyle::Magic);
        assert_eq!(targeted.max_hit, MaxHitSource::Fixed(95));
        assert_eq!(targeted.reaction, ReactionRequirement::prayer_match(ProtectionPrayer::Magic));
        assert_eq!(magic.animation, Some(2656));
        // a three-tick wind-up, then it rolls and flies at the engine's default speed
        assert_eq!(magic.telegraph_ticks(), 3);
        assert_eq!(magic.flight_ticks(10), crate::combat::ProjectileSpeed::DEFAULT.flight_ticks(10));
        // the rate is Jad's own, not repeated on each attack
        assert_eq!(jad.attack_speed, 8);
        assert_eq!(magic.attack_speed, None);
    }

    #[test]
    fn an_attack_reaches_as_far_as_its_monsters_attack_range_unless_it_names_its_own() {
        let registry = Registry::builtin();

        // the aviansie's one attack names no range, so it uses the monster's own 7
        let aviansie = registry.npc("aviansie").unwrap();
        assert_eq!(aviansie.attack_range, 7);
        assert_eq!(attack_from_def(&aviansie.attacks[0]).range, None);
        assert_eq!(attack_from_def(&aviansie.attacks[0]).reach(aviansie.attack_range), 7);

        // Jad is a 15 tile monster whose melee swing says it needs adjacency
        let jad = registry.npc("tztok_jad").unwrap();
        assert_eq!(jad.attack_range, 15);
        let melee = attack_from_def(&jad.attacks[0]);
        assert!(melee.is_melee_reach(jad.attack_range));
        assert!(!attack_from_def(&jad.attacks[2]).is_melee_reach(jad.attack_range));
    }

    #[test]
    fn a_monsters_stats_include_bonuses_and_attributes() {
        let registry = Registry::builtin();
        let demon = registry.npc("abyssal_demon").unwrap();

        let stats = stats_from_def(demon);

        assert_eq!(stats.defence(), 135);
        assert_eq!(stats.effective_accuracy_level(AttackStyle::Melee), 97 + 9, "monsters attack at level + 9");
        assert_eq!(stats.bonuses().defence.slash, 20);
        assert!(stats.attributes().contains(crate::model::Attribute::Demon));
    }

    #[test]
    fn every_bundled_monster_builds() {
        let registry = Registry::builtin();
        for npc in registry.npcs() {
            let _ = stats_from_def(npc);
            let _ = movement_policy_from_def(npc.movement);
            if !npc.attacks.is_empty() {
                let _ = pattern_from_def(npc);
            }
        }
    }
}
