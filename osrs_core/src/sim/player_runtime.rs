//! The registry-driven half of the player: equipping, stances, prayers, spells, special attacks,
//! consumables, the tick-driven resources, and building the `AttackDefinition` of whatever swing
//! the current setup would make. `Player`'s struct and its movement live in `player.rs`.

use std::borrow::Cow;
use std::rc::Rc;

use super::player::Player;
use crate::combat::{
    AttackDefinition, AttackRangeKind, AttackStyle, CombatStats, MaxHitSource, ProtectionPrayer,
};
use crate::data::{ConsumableEffect, EquipSlot, Registry, StanceDef, StanceKind, WeaponDef};
use crate::model::{Attribute, DamageType, EquipmentBonuses, Modifier, PrayerEffects, ScalingContext, Skill, StanceBonuses};
use crate::rules;

/// Attack speed of an unarmed player whose `PlayerConfig` did not say otherwise.
pub const UNARMED_SPEED_FALLBACK: i32 = 4;

/// Longest range a ranged weapon can reach however it is configured (reference §3, Longrange).
const MAX_RANGED_ATTACK_RANGE: i32 = 10;

/// The result of `Player::expected_dps`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dps {
    pub analysis: crate::combat::RollAnalysis,
    pub hits_per_swing: i32,
    pub swing_ticks: i32,
    pub per_swing: f64,
    pub per_second: f64,
}

/// A swing the player's current setup would make against a target with certain attributes.
pub struct AttackPlan {
    pub definition: AttackDefinition,
    /// Ticks until the next swing.
    pub speed: i32,
    /// Tiles the target may be away; 1 means adjacent melee.
    pub range: i32,
    pub style: AttackStyle,
    /// Special energy this swing will cost, if it is a special attack.
    pub special_cost: Option<i32>,
}

impl AttackPlan {
    /// Fixes the projectile travel time for a shot at `distance` tiles and shares the definition.
    pub fn finish(mut self, distance: i32) -> Rc<AttackDefinition> {
        self.definition.projectile_travel_ticks = rules::projectile_travel_ticks(self.style, distance);
        self.definition.shared()
    }
}

fn snake(name: &str) -> String {
    name.trim().to_lowercase().replace(' ', "_")
}

fn protection_for(style: AttackStyle) -> ProtectionPrayer {
    match style {
        AttackStyle::Melee => ProtectionPrayer::Melee,
        AttackStyle::Ranged => ProtectionPrayer::Ranged,
        AttackStyle::Magic => ProtectionPrayer::Magic,
    }
}

fn unarmed_weapon(attack_speed: i32) -> WeaponDef {
    let stance = |name: &str, kind: StanceKind, bonuses: StanceBonuses| StanceDef {
        name: name.into(),
        kind,
        damage_type: Some(DamageType::Crush),
        bonuses,
        ..Default::default()
    };
    WeaponDef {
        category: "unarmed".into(),
        attack_speed,
        stances: vec![
            stance("Punch", StanceKind::Accurate, StanceBonuses { attack: 3, ..Default::default() }),
            stance("Kick", StanceKind::Aggressive, StanceBonuses { strength: 3, ..Default::default() }),
            stance("Block", StanceKind::Defensive, StanceBonuses { defence: 3, ..Default::default() }),
        ],
        attack_sound: Some(super::sounds::UNARMED_ATTACK),
        ..Default::default()
    }
}

/// The Combat Options tab's view of the wielded weapon, as the snapshot carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatOptions {
    /// The weapon's data category (`"unarmed"` without a weapon).
    pub weapon_category: String,
    /// The names of its combat styles, in the order `SetStance { index }` counts them.
    pub stances: Vec<String>,
    pub stance_index: Option<usize>,
    /// Percent of the special attack bar the weapon's special uses, or `None` when it has none.
    pub special_cost: Option<i32>,
}

impl Player {
    // ---- gear, stance, prayers, spell, special ----

    fn effective_weapon<'a>(&self, registry: &'a Registry) -> Cow<'a, WeaponDef> {
        match self.loadout.weapon(registry) {
            Some(weapon) => Cow::Borrowed(weapon),
            None => Cow::Owned(unarmed_weapon(self.attack_interval_ticks.max(1))),
        }
    }

    /// The stance currently selected on the wielded weapon (or on bare fists).
    pub fn current_stance(&self, registry: &Registry) -> Option<StanceDef> {
        let index = self.stance_index?;
        self.effective_weapon(registry).stances.get(index).cloned()
    }

    /// What the Combat Options tab needs to know about the wielded weapon (or bare fists).
    pub fn combat_options(&self, registry: &Registry) -> CombatOptions {
        let weapon = self.effective_weapon(registry);
        CombatOptions {
            weapon_category: weapon.category.clone(),
            stances: weapon.stances.iter().map(|s| s.name.clone()).collect(),
            stance_index: self.stance_index,
            special_cost: self.loadout.weapon(registry).and_then(|w| w.special.as_ref()).map(|s| s.energy_cost),
        }
    }

    pub fn equip(&mut self, registry: &Registry, item_id: &str) -> Result<(), String> {
        let slot = registry.item(item_id).ok_or_else(|| format!("unknown item '{item_id}'"))?.slot;
        self.loadout.equip(registry, item_id)?;
        if slot == Some(EquipSlot::Weapon) {
            self.special_enabled = false;
            self.stance_index = self.loadout.weapon(registry).filter(|w| !w.stances.is_empty()).map(|_| 0);
        }
        self.refresh_stats(registry);
        Ok(())
    }

    pub fn unequip(&mut self, registry: &Registry, slot: EquipSlot) {
        if self.loadout.unequip(slot).is_some() && slot == EquipSlot::Weapon {
            self.special_enabled = false;
            self.stance_index = None;
        }
        self.refresh_stats(registry);
    }

    pub fn set_stance(&mut self, registry: &Registry, index: usize) -> Result<(), String> {
        if index >= self.effective_weapon(registry).stances.len() {
            return Err(format!("this weapon has no stance {index}"));
        }
        self.stance_index = Some(index);
        self.refresh_stats(registry);
        Ok(())
    }

    /// Selects (or with `None`, clears) the spell cast when attacking with a casting weapon.
    pub fn select_spell(&mut self, registry: &Registry, spell_id: Option<&str>) -> Result<(), String> {
        let Some(id) = spell_id else {
            self.selected_spell = None;
            return Ok(());
        };
        let spell = registry.spell(id).ok_or_else(|| format!("unknown spell '{id}'"))?;
        if self.skills.current(Skill::Magic) < spell.level {
            return Err(format!("{} needs level {} Magic", spell.name, spell.level));
        }
        self.selected_spell = Some(spell.id.clone());
        Ok(())
    }

    /// Arms or disarms the special attack of the wielded weapon. Arming needs the weapon to have
    /// one and enough energy.
    pub fn set_special_enabled(&mut self, registry: &Registry, enabled: bool) -> Result<(), String> {
        if !enabled {
            self.special_enabled = false;
            return Ok(());
        }
        let special = self
            .loadout
            .weapon(registry)
            .and_then(|weapon| weapon.special.as_ref())
            .ok_or_else(|| "this weapon has no special attack".to_owned())?;
        if self.special_energy < special.energy_cost {
            return Err(format!("{} needs {}% special energy", special.name, special.energy_cost));
        }
        self.special_enabled = true;
        Ok(())
    }

    /// Switches a prayer on or off; returns whether it is now active. Turning one on switches off
    /// every active prayer that shares a group with it.
    pub fn toggle_prayer(&mut self, registry: &Registry, prayer_id: &str) -> Result<bool, String> {
        let prayer = registry.prayer(prayer_id).ok_or_else(|| format!("unknown prayer '{prayer_id}'"))?;
        if let Some(position) = self.active_prayers.iter().position(|id| id == &prayer.id) {
            self.active_prayers.remove(position);
            self.after_prayers_changed(registry);
            return Ok(false);
        }
        if self.skills.base(Skill::Prayer) < prayer.level {
            return Err(format!("{} needs level {} Prayer", prayer.name, prayer.level));
        }
        if self.prayer.current() <= 0 {
            return Err("you have no prayer points left".to_owned());
        }
        self.active_prayers.retain(|active| {
            let other = registry.prayer(active);
            !other.is_some_and(|other| other.groups.iter().any(|group| prayer.groups.contains(group)))
        });
        self.active_prayers.push(prayer.id.clone());
        self.after_prayers_changed(registry);
        Ok(true)
    }

    pub fn deactivate_all_prayers(&mut self, registry: &Registry) {
        self.active_prayers.clear();
        self.after_prayers_changed(registry);
    }

    fn after_prayers_changed(&mut self, registry: &Registry) {
        self.reaction_state.active_prayer = self
            .active_prayers
            .iter()
            .filter_map(|id| registry.prayer(id))
            .find_map(|prayer| prayer.protects)
            .map_or(ProtectionPrayer::None, protection_for);
        if self.active_prayers.is_empty() {
            self.prayer_drain = rules::PrayerDrainCounter::default();
        }
        self.refresh_stats(registry);
    }

    fn prayer_has_tag(&self, registry: &Registry, tag: &str) -> bool {
        self.active_prayers.iter().filter_map(|id| registry.prayer(id)).any(|prayer| prayer.has_tag(tag))
    }

    // ---- derived stats ----

    /// Recomputes the `CombatStats` every roll reads from the skills' current levels, the worn
    /// gear, the active prayers and the stance. Call after anything that changes one of those.
    pub fn refresh_stats(&mut self, registry: &Registry) {
        let level = |skill| self.skills.current(skill).max(1);
        let prayer = self
            .active_prayers
            .iter()
            .filter_map(|id| registry.prayer(id))
            .fold(PrayerEffects::default(), |total, prayer| total.combine(prayer.effects));
        let stance = self.current_stance(registry).map(|stance| stance.bonuses).unwrap_or_default();
        let bonuses: EquipmentBonuses = self.loadout.totals(registry);

        let stats = CombatStats::new(
            level(Skill::Attack),
            level(Skill::Strength),
            level(Skill::Defence),
            level(Skill::Ranged),
            level(Skill::Magic),
        )
        .expect("levels are clamped to at least 1")
        .with_bonuses(bonuses)
        .with_prayer_effects(prayer)
        .with_stance(stance);
        self.mover.actor.set_stats(stats);
    }

    // ---- building an attack ----

    /// Builds the swing the current setup would make at a target with `target_attributes`: the
    /// spell if one is selected on a casting weapon, the powered staff's own attack, or the
    /// weapon's attack in its current stance - with the special attack folded in if it is armed
    /// and affordable. `Err` says why no attack is possible (no ammo, a spell the target is
    /// not valid for).
    pub fn plan_attack(&self, registry: &Registry, target: &CombatStats) -> Result<AttackPlan, String> {
        let target_attributes = target.attributes();
        let weapon = self.effective_weapon(registry);
        let stance = self.current_stance(registry);
        let stance_range = stance.as_ref().map_or(0, |s| s.range_delta);
        let stance_speed = stance.as_ref().map_or(0, |s| s.speed_delta);
        let health = self.mover.actor.health();
        let (hp_now, hp_max) = (health.current(), health.max());

        let mut modifiers: Vec<Modifier> = self.loadout.modifiers(registry);
        for prayer in self.active_prayers.iter().filter_map(|id| registry.prayer(id)) {
            modifiers.extend(prayer.modifiers.iter().cloned());
        }

        let mut flags: Vec<String> = Vec::new();
        if self.on_slayer_task {
            flags.push("on_task".into());
        }
        flags.push(format!("weapon_category:{}", weapon.category));
        if let Some(item) = self.loadout.weapon_item(registry) {
            flags.push(format!("weapon:{}", item.id));
            flags.extend(item.tags.iter().map(|tag| format!("weapon_tag:{tag}")));
        }
        if let Some(stance) = &stance {
            flags.push(format!("stance:{}", snake(&stance.name)));
        }

        let mut max_hit = MaxHitSource::Derived;
        let mut hit_count = 1;
        let mut always_hits = false;
        let mut special_cost = None;
        let mut on_hit = None;
        let mut special_speed = None;
        let mut hit_sound = None;
        let sound;
        let name;

        let style;
        let mut damage_type;
        let speed;
        let range;

        if let (Some(spell_id), true) = (self.selected_spell.as_deref(), weapon.can_cast) {
            let spell = registry.spell(spell_id).ok_or_else(|| format!("unknown spell '{spell_id}'"))?;
            if let Some(required) = spell.requires_target_attribute {
                if !target_attributes.contains(required) {
                    return Err(format!("{} can only be cast at {:?} targets", spell.name, required));
                }
            }
            name = spell.name.clone();
            style = AttackStyle::Magic;
            damage_type = DamageType::Magic;
            max_hit = match spell.level_scaling {
                Some(scaling) => MaxHitSource::powered_staff(scaling.divisor, scaling.offset),
                None => MaxHitSource::Spell { base: spell.base_max_hit },
            };
            speed = spell.cast_speed;
            range = (spell.range + stance_range).clamp(1, MAX_RANGED_ATTACK_RANGE);
            on_hit = spell.on_hit;
            sound = spell.cast_sound;
            hit_sound = spell.hit_sound;
            modifiers.extend(spell.modifiers.iter().cloned());
            flags.push(format!("spell:{}", spell.id));
            flags.push(format!("spellbook:{}", spell.spellbook));
        } else if let Some(powered) = weapon.powered {
            name = "Powered staff".into();
            style = AttackStyle::Magic;
            damage_type = DamageType::Magic;
            max_hit = powered;
            sound = weapon.attack_sound;
            speed = (weapon.attack_speed + stance_speed).max(1);
            range = (weapon.attack_range + stance_range).clamp(1, MAX_RANGED_ATTACK_RANGE);
        } else {
            let guessed_style = if weapon.ammo_family.is_some() || weapon.thrown { AttackStyle::Ranged } else { AttackStyle::Melee };
            damage_type = stance
                .as_ref()
                .and_then(|s| s.damage_type)
                .unwrap_or_else(|| DamageType::default_for(guessed_style));
            style = damage_type.style();
            name = "Weapon attack".into();
            sound = stance.as_ref().and_then(|s| s.sound).or(weapon.attack_sound);

            if let Some(family) = &weapon.ammo_family {
                let ammo = self.loadout.ammo_item(registry).and_then(|item| item.ammo.as_ref().map(|a| (item, a)));
                match ammo {
                    Some((item, ammo)) if &ammo.family == family => {
                        flags.push(format!("ammo:{family}"));
                        flags.push(format!("ammo_item:{}", item.id));
                        flags.push(format!("ammo_weight:{:?}", ammo.weight).to_lowercase());
                    }
                    _ => return Err(format!("this weapon needs {family} ammunition equipped")),
                }
            }

            let mut base_speed = weapon.attack_speed + stance_speed;
            if let (true, Some(special)) = (self.special_enabled, self.loadout.weapon(registry).and_then(|w| w.special.as_ref())) {
                if self.special_energy >= special.energy_cost {
                    special_cost = Some(special.energy_cost);
                    flags.push("special".into());
                    modifiers.extend(special.modifiers.iter().cloned());
                    hit_count = special.hit_count;
                    always_hits = special.always_hits;
                    if let Some(special_type) = special.damage_type {
                        damage_type = special_type;
                    }
                    if let Some(source) = special.max_hit {
                        max_hit = source;
                    }
                    special_speed = special.attack_speed;
                }
            }
            if let Some(override_speed) = special_speed {
                base_speed = override_speed;
            }
            speed = base_speed.max(1);

            let raw_range = weapon.attack_range + stance_range;
            range = if style == AttackStyle::Ranged { raw_range.clamp(1, MAX_RANGED_ATTACK_RANGE) } else { raw_range.max(1) };
        }

        let range_kind = if style == AttackStyle::Melee && range <= 1 { AttackRangeKind::Melee } else { AttackRangeKind::Ranged };
        let scaling = ScalingContext {
            attacker_hp: hp_now,
            attacker_max_hp: hp_max,
            target_magic: target.magic().max(target.bonuses().attack.magic),
            target_is_xerician: target_attributes.contains(Attribute::Xerician),
        };
        let modifiers = modifiers.iter().map(|m| m.resolved(&scaling)).collect();

        let mut definition = AttackDefinition::new(name, range_kind, range, 0, style)
            .with_damage_type(damage_type)
            .with_max_hit(max_hit)
            .with_modifiers(modifiers)
            .with_flags(flags)
            .with_hit_count(hit_count)
            .with_always_hits(always_hits)
            .with_sounds(sound, hit_sound);
        if let Some(status) = on_hit.and_then(|effect| effect.to_status().ok()) {
            definition = definition.with_on_hit_status_effect(status);
        }

        Ok(AttackPlan { definition, speed, range, style, special_cost })
    }

    // ---- damage per second ----

    /// The expected damage per second of the current setup against `target`, by the wiki's
    /// average-damage-per-swing expression. Special-attack procs that roll extra dice (Keris) and
    /// the monster's own protection are not folded in.
    pub fn expected_dps(&self, registry: &Registry, target: &CombatStats) -> Result<Dps, String> {
        let plan = self.plan_attack(registry, target)?;
        let analysis = crate::combat::analyze_attack(&plan.definition, &self.mover.actor.stats(), target, true);
        let per_swing = analysis.average_damage() * plan.definition.hit_count as f64;
        Ok(Dps { analysis, hits_per_swing: plan.definition.hit_count, swing_ticks: plan.speed, per_swing, per_second: per_swing / (plan.speed as f64 * 0.6) })
    }

    /// Lets a freshly built player use several consumables back to back (setup, not gameplay).
    pub fn clear_consumable_delays(&mut self) {
        self.consumable_delays.clear();
    }

    // ---- consumables ----

    /// Uses a food or potion: applies its effects and starts the cooldown for its delay group.
    pub fn use_consumable(&mut self, registry: &Registry, consumable_id: &str) -> Result<(), String> {
        let item = registry.consumable(consumable_id).ok_or_else(|| format!("unknown consumable '{consumable_id}'"))?;
        if self.consumable_delays.get(&item.delay_group).is_some_and(|ticks| *ticks > 0) {
            return Err(format!("you must wait before using another {}", item.delay_group));
        }

        let amount = |base: i32, flat: i32, percent: i32| flat + rules::potion_boost(base, 0, percent);
        for effect in &item.effects {
            match effect {
                ConsumableEffect::Heal { flat, percent } => {
                    let max = self.mover.actor.health().max();
                    self.mover.actor.health_mut().heal(amount(max, *flat, *percent));
                }
                ConsumableEffect::Boost { skill, flat, percent } => {
                    let base = self.skills.base(*skill);
                    self.skills.boost_to(*skill, base + amount(base, *flat, *percent));
                }
                ConsumableEffect::Drain { skill, flat, percent } => {
                    let base = self.skills.base(*skill);
                    self.skills.drain(*skill, amount(base, *flat, *percent));
                }
                ConsumableEffect::Restore { skill, flat, percent } => {
                    let base = self.skills.base(*skill);
                    let restored = amount(base, *flat, *percent);
                    if *skill == Skill::Prayer {
                        self.prayer.restore(restored);
                    } else {
                        self.skills.restore(*skill, restored);
                    }
                }
                ConsumableEffect::RestoreRunEnergy { percent } => {
                    self.run_energy = (self.run_energy + percent * 100).min(rules::RUN_ENERGY_MAX);
                }
                ConsumableEffect::RestoreSpecialEnergy { percent } => {
                    self.special_energy = (self.special_energy + percent).min(rules::SPEC_ENERGY_MAX);
                }
                ConsumableEffect::CurePoison => self.status_effects.cure_poison(),
                ConsumableEffect::CureVenom => self.status_effects.cure_venom(),
                ConsumableEffect::PoisonImmunity { ticks } => self.status_effects.grant_poison_immunity(*ticks),
                ConsumableEffect::StaminaBoost { ticks } => self.stamina_ticks = self.stamina_ticks.max(*ticks),
                ConsumableEffect::Die => {
                    let current = self.mover.actor.health().current();
                    self.mover.actor.health_mut().damage(current);
                }
            }
        }

        self.consumable_delays.insert(item.delay_group.clone(), item.delay_ticks);
        self.attack_cooldown += item.attack_delay_ticks;
        self.refresh_stats(registry);
        Ok(())
    }

    // ---- the Regen phase and post-damage reactions ----

    /// Everything that happens once per tick regardless of combat: consumable delays, natural HP
    /// regen, boost/drain drift, prayer drain, run energy and special attack energy.
    pub(crate) fn run_regen(&mut self, registry: &Registry) {
        if self.is_dead() {
            return;
        }
        let mut stats_changed = false;

        self.consumable_delays.retain(|_, ticks| {
            *ticks -= 1;
            *ticks > 0
        });

        // Natural hitpoint regeneration.
        let rapid_heal = self.prayer_has_tag(registry, "rapid_heal") || self.loadout.has_tag(registry, "hitpoints_cape");
        let bracelet = self.loadout.has_tag(registry, "regen_bracelet");
        self.hp_regen_counter += 1;
        if self.hp_regen_counter >= rules::hp_regen_interval_ticks(rapid_heal, bracelet) {
            self.hp_regen_counter = 0;
            self.mover.actor.health_mut().heal(1);
        }

        // Boosts decay and drains recover, one level per interval.
        let preserve = self.prayer_has_tag(registry, "preserve");
        for skill in [Skill::Attack, Skill::Strength, Skill::Defence, Skill::Ranged, Skill::Magic] {
            let counter = &mut self.drift_counters[skill.index()];
            if !self.skills.is_off_base(skill) {
                *counter = 0;
                continue;
            }
            *counter += 1;
            let interval = rules::stat_drift_interval_ticks(self.skills.current(skill), self.skills.base(skill), preserve);
            if *counter >= interval {
                *counter = 0;
                stats_changed |= self.skills.drift(skill);
            }
        }

        // Prayer drain; running out of points switches everything off.
        let drain_effect: i32 = self.active_prayers.iter().filter_map(|id| registry.prayer(id)).map(|p| p.drain_effect).sum();
        let prayer_bonus = self.loadout.totals(registry).prayer;
        let points = self.prayer_drain.tick(drain_effect, prayer_bonus);
        self.prayer.drain(points);
        if !self.active_prayers.is_empty() && self.prayer.current() <= 0 {
            self.active_prayers.clear();
            self.after_prayers_changed(registry);
        }

        // Run energy: spent on a tick spent running, regained on any other.
        if self.run_energy_enabled {
            if self.ran_this_tick {
                let weight = self.loadout.weight_kg(registry);
                let drain = rules::run_energy_drain_per_tick(
                    weight,
                    self.agility_level,
                    self.stamina_ticks > 0,
                    self.loadout.has_tag(registry, "ring_of_endurance"),
                );
                self.run_energy = (self.run_energy - drain).max(0);
            } else {
                let graceful = self.loadout.has_set(registry, "graceful_outfit");
                let regen = rules::run_energy_regen_per_tick(self.agility_level, graceful);
                self.run_energy = (self.run_energy + regen).min(rules::RUN_ENERGY_MAX);
            }
            self.stamina_ticks = (self.stamina_ticks - 1).max(0);
        }

        // Special attack energy.
        self.spec_regen_counter += 1;
        if self.spec_regen_counter >= rules::spec_regen_interval_ticks(self.loadout.has_tag(registry, "lightbearer")) {
            self.spec_regen_counter = 0;
            self.special_energy = (self.special_energy + rules::SPEC_ENERGY_REGEN_AMOUNT).min(rules::SPEC_ENERGY_MAX);
        }

        if stats_changed {
            self.refresh_stats(registry);
        }
    }

    /// Reactions that trigger after incoming damage lands: the Phoenix necklace and Redemption.
    pub(crate) fn after_damage(&mut self, registry: &Registry) {
        let health = self.mover.actor.health();
        let (hp, max) = (health.current(), health.max());
        if hp <= 0 {
            return;
        }

        // Phoenix necklace: at 20% or less, heal 30% of max HP, wipe the whole damage queue and
        // use the necklace up.
        if hp * 5 <= max && self.loadout.has_tag(registry, "phoenix_necklace") {
            self.mover.actor.health_mut().heal(max * 3 / 10);
            self.damage_buffer.clear();
            self.unequip(registry, EquipSlot::Neck);
            return;
        }

        // Redemption: below 10% HP, heal a quarter of the Prayer level and empty the prayer pool.
        if hp * 10 < max && self.prayer_has_tag(registry, "redemption") {
            self.mover.actor.health_mut().heal(rules::redemption_heal(self.skills.base(Skill::Prayer)));
            self.prayer.drain(self.prayer.max());
            self.active_prayers.clear();
            self.after_prayers_changed(registry);
        }
    }
}
