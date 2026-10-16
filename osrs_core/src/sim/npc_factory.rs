//! Turns data definitions (`NpcDef` and its attacks) into the runtime types the combat code uses.

use std::rc::Rc;

use crate::actors::{
    AdvanceMovementPolicy, ChaseToMeleeRangeMovementPolicy, MaintainDistanceMovementPolicy, MovementPolicy, StationaryMovementPolicy,
    StyleReactiveMovementPolicy,
};
use crate::combat::{
    AttackDefinition, AttackPattern, AttackStyle, CombatStats, GroundDamage, GroundTarget, HazardAnchor, HazardDef,
    HazardPlacement, MaxHitSource, Mitigation, ObstructedShot, PositionResolver, ProjectileSpeed, ProtectionPrayer,
    RangeAwarePattern, RotationMode, SpecialAttack, TargetedAttack,
};
use crate::data::{
    AnchorData, DamageData, HazardData, NpcAttackDef, NpcDef, NpcMovementDef, PlacementData, PositionData, PrayerData,
    RotationDef, SpecialData, TilesData,
};
use crate::encounters::{PhaseTrigger, Phase, TriggerAction, TriggerBinding};
use crate::model::{DamageType, StanceBonuses};

/// A standard attack that rolls the tick it is chosen when `telegraph_ticks` is 0, else after that
/// many ticks of wind-up: how data and config, which only have the number, name a timing.
pub fn timed_attack(name: impl Into<String>, range: Option<i32>, telegraph_ticks: i32, style: AttackStyle) -> AttackDefinition {
    assert!(telegraph_ticks >= 0, "telegraph_ticks must not be negative");
    if telegraph_ticks == 0 {
        AttackDefinition::instant(name, range, style)
    } else {
        AttackDefinition::telegraphed(name, range, telegraph_ticks, style)
    }
}

/// One of a monster's attacks as a runtime `AttackDefinition`. Assumes the definition has been
/// through `Registry::validate`. `hazards` is the monster's table of hazards its specials leave.
pub fn attack_from_def(def: &NpcAttackDef, hazards: &[HazardData]) -> Rc<AttackDefinition> {
    attack_sounding(def, None, hazards)
}

/// [`attack_from_def`], heard as `fallback_sound` when the attack names no sound of its own.
fn attack_sounding(def: &NpcAttackDef, fallback_sound: Option<u32>, hazards: &[HazardData]) -> Rc<AttackDefinition> {
    let damage_type = def.damage_type.unwrap_or_else(|| DamageType::default_for(def.style));
    let mut attack = match &def.special {
        Some(SpecialData::Position(position)) => {
            AttackDefinition::special(def.name.clone(), def.range, def.telegraph_ticks, position_from_data(position, hazards))
        }
        Some(SpecialData::ObstructedShot(shot)) => {
            let mut hit = TargetedAttack::new(def.style)
                .with_damage_type(damage_type)
                .with_max_hit(MaxHitSource::Fixed(def.max_hit))
                .with_prayer_penetration(def.prayer_penetration)
                .with_modifiers(def.modifiers.clone());
            if !def.protectable {
                hit = hit.with_mitigation(Mitigation::Unmitigable);
            }
            let speed = def.projectile_speed.map_or(ProjectileSpeed::DEFAULT, ProjectileSpeed::tiles_per_tick);
            let on_blocked = position_from_data(shot.on_blocked.as_ref().expect("validated: a shot says what a block does"), hazards);
            let mut resolver = ObstructedShot::new(hit, speed, on_blocked);
            if shot.binds_until_landing {
                resolver = resolver.binding_until_landing();
            }
            if let Some(landing) = &shot.landing_hazards {
                let min = landing.min as usize;
                resolver = resolver.leaving_hazards(hazard_named(hazards, &landing.hazard), landing.radius, min, landing.max as usize);
            }
            AttackDefinition::special(def.name.clone(), def.range, def.telegraph_ticks, resolver)
        }
        None => {
            // the attack's timing archetype comes from its numbers: a delay until it lands is
            // Doom-style, a wind-up is Jad-style, neither is traditional
            let mut standard = if def.impact_delay_ticks > 0 {
                AttackDefinition::on_contact(def.name.clone(), def.range, def.impact_delay_ticks, def.style)
            } else {
                timed_attack(def.name.clone(), def.range, def.telegraph_ticks, def.style)
            }
            .with_damage_type(damage_type)
            .with_max_hit(MaxHitSource::Fixed(def.max_hit))
            .with_prayer_penetration(def.prayer_penetration)
            .with_modifiers(def.modifiers.clone())
            .with_hit_count(def.hit_count);
            if let (Some(speed), 0) = (def.projectile_speed, def.impact_delay_ticks) {
                standard = standard.with_projectile_speed(ProjectileSpeed::tiles_per_tick(speed));
            }
            if !def.protectable {
                standard = standard.with_mitigation(Mitigation::Unmitigable);
            }
            standard
        }
    }
    .with_animation(def.animation)
    .with_graphics(def.projectile, def.target_graphic, def.impact_graphic)
    .with_sounds(def.sound.or(fallback_sound), def.resolve_sound);
    if let Some(speed) = def.speed {
        attack = attack.with_attack_speed(speed);
    }
    if let Some(min_range) = def.min_range {
        attack = attack.with_min_range(min_range);
    }
    if let Some(status) = def.on_hit.and_then(|effect| effect.to_status().ok()) {
        attack = attack.with_on_hit_status_effect(status);
    }
    for rider in &def.riders {
        attack = attack.with_rider(SpecialAttack::new(rider.delay_ticks, position_from_data(&rider.special, hazards)));
    }
    attack.shared()
}

fn hazard_named(hazards: &[HazardData], id: &str) -> Rc<HazardDef> {
    let data = hazards.iter().find(|h| h.id == id).unwrap_or_else(|| panic!("validated: hazard '{id}' exists"));
    let mut def = HazardDef::new(data.name.clone()).hurting(data.damage_per_tick);
    if data.blocks_line_of_sight {
        def = def.blocking_line_of_sight();
    }
    if data.blocks_players {
        def = def.blocking_players();
    }
    if let Some(ticks) = data.lifetime_ticks {
        def = def.lasting(ticks);
    }
    if data.cleared_by_owner_walking {
        def = def.cleared_by_owner_walking();
    }
    if let Some(object) = data.object {
        def = def.drawn_as(object);
    }
    if let Some(graphic) = data.graphic {
        def = def.drawn_as_graphic(graphic);
    }
    Rc::new(def)
}

fn tiles_from_data(tiles: &TilesData) -> GroundTarget {
    let anchor = match tiles.anchor {
        AnchorData::SelfTile => HazardAnchor::SelfTile,
        AnchorData::Target => HazardAnchor::Target,
    };
    let target = match (tiles.square, &tiles.offsets, &tiles.scatter) {
        (Some(radius), None, None) => GroundTarget::square(anchor, radius),
        (None, Some(offsets), None) => GroundTarget::offsets(anchor, offsets.iter().map(|[dx, dz]| (*dx, *dz)).collect()),
        (None, None, Some(scatter)) => GroundTarget::scatter_between(anchor, scatter.radius, scatter.min, scatter.max),
        _ => panic!("validated: tiles name exactly one of square, offsets and scatter"),
    };
    if tiles.always_include_anchor {
        target.including_anchor()
    } else {
        target
    }
}

fn position_from_data(position: &PositionData, hazards: &[HazardData]) -> PositionResolver {
    let damage = match position.damage {
        DamageData::Fixed(amount) => GroundDamage::Fixed(amount),
        DamageData::Uniform { max } => GroundDamage::Uniform { max },
    };
    let mut resolver = PositionResolver::new(tiles_from_data(&position.tiles), damage);
    if let Some(reduction) = position.prayer_reduction {
        let prayer = match reduction.prayer {
            PrayerData::Melee => ProtectionPrayer::Melee,
            PrayerData::Ranged => ProtectionPrayer::Ranged,
            PrayerData::Magic => ProtectionPrayer::Magic,
        };
        resolver = resolver.with_prayer_reduction(prayer, reduction.penetration);
    }
    if let Some(leaves) = &position.leaves {
        let placement = match leaves.placement {
            PlacementData::Anchor => HazardPlacement::AnchorTile,
            PlacementData::EveryCoveredTile => HazardPlacement::EveryCoveredTile,
        };
        resolver = resolver.leaving(hazard_named(hazards, &leaves.hazard), placement);
    }
    resolver
}

pub fn movement_policy_from_def(movement: NpcMovementDef) -> Rc<dyn MovementPolicy> {
    match movement {
        NpcMovementDef::Chase => Rc::new(ChaseToMeleeRangeMovementPolicy),
        NpcMovementDef::Stationary => Rc::new(StationaryMovementPolicy),
        NpcMovementDef::MaintainDistance { distance } => Rc::new(MaintainDistanceMovementPolicy::new(distance)),
        NpcMovementDef::StyleReactive { kite_distance } => Rc::new(StyleReactiveMovementPolicy::new(kite_distance)),
        NpcMovementDef::Advance => Rc::new(AdvanceMovementPolicy),
    }
}

/// The attack pattern of a monster with at least one attack.
pub fn pattern_from_def(def: &NpcDef) -> RangeAwarePattern {
    let attacks = def.attacks.iter().map(|attack| (attack_sounding(attack, def.sounds.attack, &def.hazards), attack.weight)).collect();
    let mode = match def.rotation {
        RotationDef::Fixed => RotationMode::Fixed,
        RotationDef::Weighted => RotationMode::Weighted,
    };
    RangeAwarePattern::new(attacks, mode, def.size, def.attack_range)
}

/// The stages of a monster's fight and the triggers that move it from one to the next: each phase
/// after the first begins when its health falls to `when_hp_below` of its maximum. A phase uses the
/// attacks it names (a use may set its own attack speed, for a volley such as 3-2-2), moves as it
/// says (else as the monster does), and fires its periodic specials on their own clock.
pub fn phases_from_def(def: &NpcDef) -> (Vec<Phase>, Vec<TriggerBinding>) {
    let mut phases = Vec::new();
    let mut triggers = Vec::new();
    for data in &def.phases {
        let attacks = data
            .attacks
            .iter()
            .map(|use_| {
                let base = def.attacks.iter().find(|a| a.name == use_.attack).expect("validated: the phase's attack exists");
                let attack = attack_sounding(base, def.sounds.attack, &def.hazards);
                let attack = match use_.speed {
                    Some(speed) => Rc::new(attack.as_ref().clone().with_attack_speed(speed)),
                    None => attack,
                };
                (attack, use_.weight.unwrap_or(base.weight))
            })
            .collect();
        let mode = match data.rotation.unwrap_or(def.rotation) {
            RotationDef::Fixed => RotationMode::Fixed,
            RotationDef::Weighted => RotationMode::Weighted,
        };
        let pattern: Rc<dyn AttackPattern> = Rc::new(RangeAwarePattern::new(attacks, mode, def.size, def.attack_range));
        let movement = movement_policy_from_def(data.movement.unwrap_or(def.movement));
        let mut phase = Phase::new(data.name.clone(), pattern, Some(movement));
        for periodic in &data.periodic {
            let attack = AttackDefinition::special(periodic.name.clone(), None, 0, position_from_data(&periodic.special, &def.hazards));
            phase = phase.with_periodic(periodic.every_ticks, attack.shared());
        }
        if let Some(fraction) = data.when_hp_below {
            triggers.push(TriggerBinding::new(PhaseTrigger::hp_threshold(fraction), TriggerAction::swap_phase(data.name.clone())));
        }
        phases.push(phase);
    }
    (phases, triggers)
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
    fn the_wind_up_number_picks_instant_or_telegraphed() {
        assert!(matches!(timed_attack("A", None, 0, AttackStyle::Melee).kind, crate::combat::AttackKind::Instant(_)));
        assert!(matches!(timed_attack("A", None, 2, AttackStyle::Melee).kind, crate::combat::AttackKind::Telegraphed(_)));
    }

    #[test]
    fn jads_attacks_carry_their_listed_numbers() {
        let registry = Registry::builtin();
        let jad = registry.npc("tztok_jad").unwrap();

        let magic = attack_from_def(&jad.attacks[2], &jad.hazards);

        let targeted = magic.expect_targeted();
        assert_eq!(targeted.style, AttackStyle::Magic);
        assert_eq!(targeted.max_hit, MaxHitSource::Fixed(95));
        assert_eq!(targeted.mitigation, Mitigation::ProtectionPrayer);
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
        assert_eq!(attack_from_def(&aviansie.attacks[0], &aviansie.hazards).range, None);
        assert_eq!(attack_from_def(&aviansie.attacks[0], &aviansie.hazards).reach(aviansie.attack_range), 7);

        // Jad is a 15 tile monster whose melee swing says it needs adjacency
        let jad = registry.npc("tztok_jad").unwrap();
        assert_eq!(jad.attack_range, 15);
        let melee = attack_from_def(&jad.attacks[0], &jad.hazards);
        assert!(melee.is_melee_reach(jad.attack_range));
        assert!(!attack_from_def(&jad.attacks[2], &jad.hazards).is_melee_reach(jad.attack_range));
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
