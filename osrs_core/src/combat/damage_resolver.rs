use super::attack_definition::AttackDefinition;
use super::attack_style::AttackStyle;
use super::combat_stats::CombatStats;
use super::max_hit_source::MaxHitSource;
use crate::actors::Health;
use crate::model::{ConditionContext, Modifier, ModifierTarget};
use crate::rng::Rng;
use crate::rules::{self, Ratio};

/// Everything about one attack-versus-defender matchup that does not need randomness: the two
/// rolls, the resulting hit chance and the max hit. This is what a DPS calculator or a data
/// validation pass wants, and `roll` below is just this plus the dice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RollAnalysis {
    pub attack_roll: i64,
    pub defence_roll: i64,
    pub hit_chance: f64,
    pub max_hit: i32,
}

impl RollAnalysis {
    /// Expected damage of one hit: `chance * (max/2 + 1 - 1/(max+1))` (reference §5, wiki DPS page).
    pub fn average_damage(&self) -> f64 {
        let max = self.max_hit as f64;
        self.hit_chance * (max / 2.0 + 1.0 - 1.0 / (max + 1.0))
    }
}

fn condition_context<'a>(attack: &'a AttackDefinition, defender: &CombatStats) -> ConditionContext<'a> {
    ConditionContext {
        style: attack.style,
        damage_type: attack.damage_type,
        target_attributes: defender.attributes(),
        flags: &attack.flags,
    }
}

fn apply_all(mut value: i64, modifiers: &[&Modifier]) -> i64 {
    for modifier in modifiers {
        value = modifier.ratio.apply(value);
    }
    value
}

/// The max hit of `attack` from `attacker`'s stats, after every applicable modifier.
fn max_hit_of(attack: &AttackDefinition, attacker: &CombatStats, ctx: &ConditionContext) -> i32 {
    let style = attack.style;
    let level_mods = Modifier::applicable(&attack.modifiers, ModifierTarget::DamageLevel, ctx);

    let base = match attack.max_hit {
        MaxHitSource::Fixed(value) => value,
        MaxHitSource::Derived => {
            let level = apply_all(attacker.effective_damage_level(style) as i64, &level_mods) as i32;
            rules::base_max_hit(level, attacker.strength_bonus_for(style))
        }
        MaxHitSource::Spell { base } => magic_scaled(base, attacker, 1, &attack.modifiers, ctx),
        MaxHitSource::PoweredStaff { divisor, offset, gear_bonus_scale } => {
            // Visible Magic level (current, boosted), not the effective level.
            let base = rules::powered_staff_base_max_hit(attacker.accuracy_level_base(AttackStyle::Magic), divisor, offset);
            magic_scaled(base, attacker, gear_bonus_scale, &attack.modifiers, ctx)
        }
    };

    let multipliers: Vec<Ratio> =
        Modifier::applicable(&attack.modifiers, ModifierTarget::MaxHit, ctx).into_iter().map(|m| m.ratio).collect();
    let mut max_hit = rules::apply_multipliers(base, &multipliers);
    for modifier in Modifier::applicable(&attack.modifiers, ModifierTarget::MaxHitAdd, ctx) {
        max_hit += modifier.add;
    }
    max_hit.clamp(0, rules::MAX_HIT_CAP)
}

/// Magic Damage % = capped (gear x scale) + prayer + modifiers, summed, then floored once.
fn magic_scaled(base: i32, attacker: &CombatStats, gear_scale: i32, modifiers: &[Modifier], ctx: &ConditionContext) -> i32 {
    let gear = (attacker.bonuses().magic_damage_permille * gear_scale).min(1000);
    let extra: i32 = Modifier::applicable(modifiers, ModifierTarget::MagicDamagePermille, ctx).iter().map(|m| m.add).sum();
    rules::magic_max_hit(base, gear + attacker.prayer_effects().magic_damage_permille + extra)
}

/// The deterministic half of a roll (see `RollAnalysis`).
pub fn analyze(attack: &AttackDefinition, attacker: &CombatStats, defender: &CombatStats, defender_is_monster: bool) -> RollAnalysis {
    let ctx = condition_context(attack, defender);

    let accuracy_mods = Modifier::applicable(&attack.modifiers, ModifierTarget::AccuracyLevel, &ctx);
    let effective = apply_all(attacker.effective_accuracy_level(attack.style) as i64, &accuracy_mods) as i32;
    let mut attack_roll = rules::roll(effective, attacker.bonuses().attack.get(attack.damage_type));
    attack_roll = apply_all(attack_roll, &Modifier::applicable(&attack.modifiers, ModifierTarget::AttackRoll, &ctx));

    let mut defence_roll = defender.defence_roll_against(attack.damage_type, defender_is_monster);
    defence_roll = apply_all(defence_roll, &Modifier::applicable(&attack.modifiers, ModifierTarget::TargetDefenceRoll, &ctx));

    RollAnalysis {
        attack_roll,
        defence_roll,
        hit_chance: if attack.always_hits { 1.0 } else { rules::hit_chance(attack_roll, defence_roll) },
        max_hit: max_hit_of(attack, attacker, &ctx),
    }
}

/// Rolls one hit of an attack (accuracy roll first, then a 0..=max_hit roll only on a hit, then
/// any post-roll `DamageRoll` modifiers) and applies `damage_multiplier` afterwards. Returns the
/// damage without touching any `Health`. A multi-hit attack (`hit_count > 1`) calls this once per
/// hit.
///
/// RNG draw order matters for seed parity with the C#: exactly one `next_double` for accuracy
/// (none for an `always_hits` attack), then - only if that hit - exactly one `next_range`, then one
/// more per chance-based post-roll modifier.
pub fn roll(
    attack: &AttackDefinition,
    attacker: &CombatStats,
    defender: &CombatStats,
    defender_is_monster: bool,
    rng: &mut dyn Rng,
    damage_multiplier: f32,
) -> i32 {
    roll_with_floor(attack, attacker, defender, defender_is_monster, rng, damage_multiplier, 0)
}

/// `roll`, except that a hit that gets past the accuracy roll deals at least `successful_floor`
/// damage: with 1, a successful hit that rolls 0 becomes 1 and only a miss is 0 (what a player's
/// attacks do). The RNG draws are exactly the same as `roll`'s.
pub fn roll_with_floor(
    attack: &AttackDefinition,
    attacker: &CombatStats,
    defender: &CombatStats,
    defender_is_monster: bool,
    rng: &mut dyn Rng,
    damage_multiplier: f32,
    successful_floor: i32,
) -> i32 {
    let analysis = analyze(attack, attacker, defender, defender_is_monster);

    if !attack.always_hits && rng.next_double() >= analysis.hit_chance {
        return 0; // miss: accuracy failed, nothing else rolls
    }

    // a successful hit can still roll 0, same as real OSRS, unless the attacker's hits have a floor
    let mut rolled = rng.next_range(0, analysis.max_hit + 1).max(successful_floor);

    let ctx = condition_context(attack, defender);
    for modifier in Modifier::applicable(&attack.modifiers, ModifierTarget::DamageRoll, &ctx) {
        if modifier.chance_one_in > 1 && rng.next_range(0, modifier.chance_one_in) != 0 {
            continue;
        }
        rolled = modifier.ratio.apply(rolled as i64) as i32;
    }
    let rolled = rolled.min(rules::MAX_HIT_CAP);

    // C# multiplies an int by a float (single precision) before rounding half away from zero.
    ((rolled as f32 * damage_multiplier) as f64).round() as i32
}

/// `roll`, then applies the result to `target`. Returns the damage dealt.
pub fn resolve(
    attack: &AttackDefinition,
    attacker: &CombatStats,
    defender: &CombatStats,
    defender_is_monster: bool,
    target: &mut Health,
    rng: &mut dyn Rng,
    damage_multiplier: f32,
) -> i32 {
    let damage = roll(attack, attacker, defender, defender_is_monster, rng, damage_multiplier);
    target.damage(damage);
    damage
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{AttackRangeKind, AttackStyle};
    use crate::rng::{DotNetRandom, ScriptedRng};

    fn make_attack() -> AttackDefinition {
        AttackDefinition::new("Test", AttackRangeKind::Melee, 0, 1, AttackStyle::Melee)
    }

    fn modest_stats() -> CombatStats {
        CombatStats::new(10, 10, 10, 10, 10).unwrap()
    }

    #[test]
    fn roll_on_a_miss_returns_zero_without_consuming_a_damage_roll() {
        // Scripting the accuracy draw to 0.99 forces a miss, and an EMPTY ints list means the
        // test panics if roll ever tries to roll a damage amount after a miss.
        let mut rng = ScriptedRng::from_doubles(vec![0.99]);

        let damage = roll(&make_attack(), &modest_stats(), &modest_stats(), false, &mut rng, 1.0);

        assert_eq!(damage, 0);
    }

    #[test]
    fn a_floor_of_one_turns_a_successful_zero_into_one_but_leaves_a_miss_at_zero() {
        // accuracy passes (0.0), the damage roll comes up 0
        let mut rng = ScriptedRng::new(vec![0.0], vec![0]);
        let hit = roll_with_floor(&make_attack(), &modest_stats(), &modest_stats(), false, &mut rng, 1.0, 1);
        assert_eq!(hit, 1);

        // without the floor the same draws are a 0
        let mut rng = ScriptedRng::new(vec![0.0], vec![0]);
        assert_eq!(roll(&make_attack(), &modest_stats(), &modest_stats(), false, &mut rng, 1.0), 0);

        // accuracy fails: still 0, and no damage draw is consumed
        let mut rng = ScriptedRng::from_doubles(vec![0.99]);
        let miss = roll_with_floor(&make_attack(), &modest_stats(), &modest_stats(), false, &mut rng, 1.0, 1);
        assert_eq!(miss, 0);
    }

    #[test]
    fn roll_on_a_hit_returns_the_scripted_damage_amount() {
        // 0.0 is less than any positive hit chance, guaranteeing a hit.
        let mut rng = ScriptedRng::new(vec![0.0], vec![7]);

        let damage = roll(&make_attack(), &modest_stats(), &modest_stats(), false, &mut rng, 1.0);

        assert_eq!(damage, 7);
    }

    #[test]
    fn roll_applies_the_damage_multiplier_after_the_damage_roll() {
        let mut rng = ScriptedRng::new(vec![0.0], vec![10]);

        let damage = roll(&make_attack(), &modest_stats(), &modest_stats(), false, &mut rng, 0.5);

        assert_eq!(damage, 5);
    }

    #[test]
    fn roll_rounds_half_away_from_zero() {
        let mut rng = ScriptedRng::new(vec![0.0], vec![5]);

        let damage = roll(&make_attack(), &modest_stats(), &modest_stats(), false, &mut rng, 0.5); // 2.5

        assert_eq!(damage, 3);
    }

    #[test]
    fn roll_with_zero_multiplier_returns_zero_even_on_a_hit_with_non_zero_damage() {
        let mut rng = ScriptedRng::new(vec![0.0], vec![10]);

        let damage = roll(&make_attack(), &modest_stats(), &modest_stats(), false, &mut rng, 0.0);

        assert_eq!(damage, 0);
    }

    #[test]
    fn resolve_on_a_hit_applies_the_rolled_damage_to_health() {
        let mut target = Health::new(20);
        let mut rng = ScriptedRng::new(vec![0.0], vec![6]);

        let damage = resolve(&make_attack(), &modest_stats(), &modest_stats(), false, &mut target, &mut rng, 1.0);

        assert_eq!(damage, 6);
        assert_eq!(target.current(), 14);
    }

    #[test]
    fn resolve_on_a_miss_never_touches_health() {
        let mut target = Health::new(20);
        let mut rng = ScriptedRng::from_doubles(vec![0.99]);

        let damage = resolve(&make_attack(), &modest_stats(), &modest_stats(), false, &mut target, &mut rng, 1.0);

        assert_eq!(damage, 0);
        assert_eq!(target.current(), 20);
    }

    #[test]
    fn roll_is_deterministic_for_the_same_seed() {
        // A relative check: two independently-seeded streams against EACH OTHER.
        let attack = make_attack();
        let mut rng_a = DotNetRandom::new(42);
        let mut rng_b = DotNetRandom::new(42);

        for _ in 0..20 {
            let a = roll(&attack, &modest_stats(), &modest_stats(), false, &mut rng_a, 1.0);
            let b = roll(&attack, &modest_stats(), &modest_stats(), false, &mut rng_b, 1.0);
            assert_eq!(a, b, "Same seed must produce the same roll sequence.");
        }
    }

    #[test]
    fn roll_never_exceeds_max_hit_and_never_goes_negative() {
        let attack = make_attack();
        let stats = modest_stats();
        let mut rng = DotNetRandom::new(1234);
        let max_hit = analyze(&attack, &stats, &stats, false).max_hit;

        for _ in 0..500 {
            let damage = roll(&attack, &stats, &stats, false, &mut rng, 1.0);
            assert!(damage >= 0);
            assert!(damage <= max_hit);
        }
    }

    // ---- full-fidelity pipeline ----

    use crate::combat::MaxHitSource;
    use crate::model::{
        Attribute, AttributeSet, Condition, DamageType, EquipmentBonuses, Modifier, ModifierTarget, PrayerEffects, StanceBonuses,
    };

    fn maxed_melee_player() -> CombatStats {
        let mut bonuses = EquipmentBonuses { melee_strength: 86, ..Default::default() };
        bonuses.attack.slash = 120;
        CombatStats::new(99, 118, 99, 1, 99) // 118 = 99 + super strength
            .unwrap()
            .with_bonuses(bonuses)
            .with_prayer_effects(PrayerEffects { attack_percent: 120, strength_percent: 123, defence_percent: 125, ..Default::default() })
            .with_stance(StanceBonuses { strength: 3, ..Default::default() })
    }

    fn golem() -> CombatStats {
        let mut bonuses = EquipmentBonuses::default();
        bonuses.defence.slash = 20;
        CombatStats::new(1, 1, 100, 1, 1).unwrap().with_bonuses(bonuses)
    }

    #[test]
    fn analysis_uses_the_whole_pipeline_for_a_maxed_melee_player_against_a_monster() {
        let attack = make_attack().with_damage_type(DamageType::Slash);

        let analysis = analyze(&attack, &maxed_melee_player(), &golem(), true);

        // attack: floor(99 * 1.20) = 118, +0 stance, +8 = 126; * (120 + 64) = 23184
        assert_eq!(analysis.attack_roll, 126 * 184);
        // defence: (100 + 9) * (20 + 64)
        assert_eq!(analysis.defence_roll, 109 * 84);
        // strength: floor(118 * 1.23) = 145, +3, +8 = 156; (156 * 150 + 320) / 640 = 37
        assert_eq!(analysis.max_hit, 37);
        assert!(analysis.hit_chance > 0.5);
    }

    #[test]
    fn the_defence_roll_follows_the_incoming_damage_type() {
        let mut bonuses = EquipmentBonuses::default();
        bonuses.defence.stab = 200;
        bonuses.defence.magic = -10;
        let defender = CombatStats::new(1, 1, 50, 1, 80).unwrap().with_bonuses(bonuses);

        assert_eq!(defender.defence_roll_against(DamageType::Stab, true), 59 * 264);
        assert_eq!(defender.defence_roll_against(DamageType::Crush, true), 59 * 64);
        // a monster defends magic with its MAGIC level
        assert_eq!(defender.defence_roll_against(DamageType::Magic, true), 89 * 54);
    }

    #[test]
    fn a_player_defends_magic_with_seventy_percent_magic_and_thirty_percent_defence() {
        let defender = CombatStats::new(1, 1, 99, 1, 99).unwrap();
        // effective magic 107, effective defence 107 -> 107
        assert_eq!(defender.defence_roll_against(DamageType::Magic, false), 107 * 64);

        let low_magic = CombatStats::new(1, 1, 99, 1, 1).unwrap();
        // effective magic 9, effective defence 107 -> floor(6.3 + 32.1) = 38
        assert_eq!(low_magic.defence_roll_against(DamageType::Magic, false), 38 * 64);
    }

    #[test]
    fn a_fixed_max_hit_ignores_the_attackers_strength() {
        let attack = AttackDefinition::new("Bite", AttackRangeKind::Melee, 0, 1, AttackStyle::Melee).with_max_hit(MaxHitSource::Fixed(8));

        assert_eq!(analyze(&attack, &modest_stats(), &modest_stats(), false).max_hit, 8);
    }

    #[test]
    fn spells_scale_a_fixed_base_by_summed_magic_damage_bonuses() {
        let attack = AttackDefinition::new("Ice Barrage", AttackRangeKind::Ranged, 10, 1, AttackStyle::Magic)
            .with_max_hit(MaxHitSource::Spell { base: 30 });
        let gear = EquipmentBonuses { magic_damage_permille: 100, ..Default::default() };
        let mage = CombatStats::new(1, 1, 1, 1, 99)
            .unwrap()
            .with_bonuses(gear)
            .with_prayer_effects(PrayerEffects { magic_damage_permille: 40, ..Default::default() });

        // 30 * (1 + 0.10 + 0.04) = 34.2 -> 34
        assert_eq!(analyze(&attack, &mage, &modest_stats(), true).max_hit, 34);
    }

    #[test]
    fn powered_staves_scale_from_the_visible_magic_level_and_cap_the_gear_bonus() {
        let trident = AttackDefinition::new("Trident", AttackRangeKind::Ranged, 8, 1, AttackStyle::Magic)
            .with_max_hit(MaxHitSource::powered_staff(3, -5));
        let shadow = AttackDefinition::new("Shadow", AttackRangeKind::Ranged, 8, 1, AttackStyle::Magic)
            .with_max_hit(MaxHitSource::PoweredStaff { divisor: 3, offset: 1, gear_bonus_scale: 3 });
        let mage = CombatStats::new(1, 1, 1, 1, 99).unwrap();
        let geared = mage.with_bonuses(EquipmentBonuses { magic_damage_permille: 500, ..Default::default() });

        assert_eq!(analyze(&trident, &mage, &modest_stats(), true).max_hit, 28);
        // 99/3 + 1 = 34; gear 50% x3 = 150% capped at 100% -> 68
        assert_eq!(analyze(&shadow, &geared, &modest_stats(), true).max_hit, 68);
    }

    #[test]
    fn only_the_better_of_slayer_helm_and_salve_applies() {
        let attack = AttackDefinition::new("Slash", AttackRangeKind::Melee, 0, 1, AttackStyle::Melee)
            .with_max_hit(MaxHitSource::Fixed(36))
            .with_flags(vec!["on_task".to_owned()])
            .with_modifiers(vec![
                Modifier::multiplier(ModifierTarget::MaxHit, 7, 6).when(Condition::Flag("on_task".into())).in_group("slayer_or_salve", 1),
                Modifier::multiplier(ModifierTarget::MaxHit, 6, 5).when(Condition::TargetHas(Attribute::Undead)).in_group("slayer_or_salve", 2),
            ]);
        let undead = modest_stats().with_attributes(AttributeSet::default().with(Attribute::Undead));

        assert_eq!(analyze(&attack, &modest_stats(), &modest_stats(), true).max_hit, 42); // slayer 36 * 7/6
        assert_eq!(analyze(&attack, &modest_stats(), &undead, true).max_hit, 43); // salve 36 * 6/5 = 43.2
    }

    #[test]
    fn accuracy_and_defence_modifiers_change_the_rolls() {
        let base = make_attack();
        let boosted = make_attack().with_modifiers(vec![
            Modifier::multiplier(ModifierTarget::AttackRoll, 3, 2),
            Modifier::multiplier(ModifierTarget::TargetDefenceRoll, 1, 2),
        ]);

        let a = analyze(&base, &modest_stats(), &modest_stats(), true);
        let b = analyze(&boosted, &modest_stats(), &modest_stats(), true);

        assert_eq!(b.attack_roll, a.attack_roll * 3 / 2);
        assert_eq!(b.defence_roll, a.defence_roll / 2);
        assert!(b.hit_chance > a.hit_chance);
    }

    #[test]
    fn an_always_hit_attack_skips_the_accuracy_draw() {
        let attack = make_attack().with_always_hits(true);
        // No doubles scripted at all: drawing one would panic.
        let mut rng = ScriptedRng::new(vec![], vec![4]);

        assert_eq!(roll(&attack, &modest_stats(), &modest_stats(), true, &mut rng, 1.0), 4);
    }

    #[test]
    fn a_chance_based_post_roll_modifier_only_procs_on_its_draw() {
        let keris = make_attack()
            .with_modifiers(vec![Modifier::multiplier(ModifierTarget::DamageRoll, 3, 1).with_chance_one_in(51)]);

        let mut procs = ScriptedRng::new(vec![0.0], vec![5, 0]);
        assert_eq!(roll(&keris, &modest_stats(), &modest_stats(), true, &mut procs, 1.0), 15);

        let mut no_proc = ScriptedRng::new(vec![0.0], vec![5, 7]);
        assert_eq!(roll(&keris, &modest_stats(), &modest_stats(), true, &mut no_proc, 1.0), 5);
    }

    #[test]
    fn average_damage_matches_the_wiki_expression() {
        let analysis = RollAnalysis { attack_roll: 0, defence_roll: 0, hit_chance: 0.5, max_hit: 10 };

        assert!((analysis.average_damage() - 0.5 * (6.0 - 1.0 / 11.0)).abs() < 1e-12);
    }
}
