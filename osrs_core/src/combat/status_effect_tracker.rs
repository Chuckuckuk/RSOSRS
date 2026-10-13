use super::prayer_pool::PrayerPool;
use super::status_effect::{StatusEffectDefinition, StatusEffectKind};
use crate::actors::Health;
use crate::rules;

#[derive(Debug, Clone, Copy)]
struct ActiveEffect {
    kind: StatusEffectKind,
    magnitude: i32,
    ticks_remaining: i32,
}

/// The status effects currently active on one actor. Re-applying an effect of the same kind
/// refreshes its duration and magnitude instead of stacking.
#[derive(Debug, Clone, Default)]
pub struct StatusEffectTracker {
    active: Vec<ActiveEffect>,
    poison_immunity_ticks: i32,
}

impl StatusEffectTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_running_disabled(&self) -> bool {
        self.has_active(StatusEffectKind::DisableRunning)
    }

    pub fn is_stunned(&self) -> bool {
        self.has_active(StatusEffectKind::Stun)
    }

    fn has_active(&self, kind: StatusEffectKind) -> bool {
        self.active.iter().any(|effect| effect.kind == kind)
    }

    pub fn is_frozen(&self) -> bool {
        self.has_active(StatusEffectKind::Freeze)
    }

    pub fn is_poisoned(&self) -> bool {
        self.has_active(StatusEffectKind::Poison)
    }

    pub fn is_venomed(&self) -> bool {
        self.has_active(StatusEffectKind::Venom)
    }

    /// Remaining severity of the active poison (0 if none).
    pub fn poison_severity(&self) -> i32 {
        self.active.iter().find(|e| e.kind == StatusEffectKind::Poison).map_or(0, |e| e.magnitude)
    }

    pub fn is_poison_immune(&self) -> bool {
        self.poison_immunity_ticks > 0
    }

    pub fn cure_poison(&mut self) {
        self.active.retain(|e| e.kind != StatusEffectKind::Poison);
    }

    pub fn cure_venom(&mut self) {
        self.active.retain(|e| e.kind != StatusEffectKind::Venom);
    }

    pub fn grant_poison_immunity(&mut self, ticks: i32) {
        self.poison_immunity_ticks = self.poison_immunity_ticks.max(ticks);
    }

    pub fn apply(&mut self, definition: StatusEffectDefinition) {
        match definition.kind {
            // Poison and venom are mutually exclusive; since April 2024 a poison no longer
            // downgrades an existing venom, and immunity blocks both.
            StatusEffectKind::Poison => {
                if self.is_poison_immune() || self.is_venomed() {
                    return;
                }
                if let Some(existing) = self.active.iter_mut().find(|e| e.kind == StatusEffectKind::Poison) {
                    existing.magnitude = existing.magnitude.max(definition.magnitude);
                    return;
                }
                self.active.push(ActiveEffect {
                    kind: StatusEffectKind::Poison,
                    magnitude: definition.magnitude,
                    ticks_remaining: rules::DAMAGE_OVER_TIME_INTERVAL_TICKS,
                });
                return;
            }
            StatusEffectKind::Venom => {
                if self.is_poison_immune() || self.is_venomed() {
                    return;
                }
                self.cure_poison();
                self.active.push(ActiveEffect {
                    kind: StatusEffectKind::Venom,
                    magnitude: 0,
                    ticks_remaining: rules::DAMAGE_OVER_TIME_INTERVAL_TICKS,
                });
                return;
            }
            _ => {}
        }

        if let Some(existing) = self.active.iter_mut().find(|effect| effect.kind == definition.kind) {
            existing.magnitude = definition.magnitude;
            existing.ticks_remaining = definition.duration_ticks;
            return;
        }

        self.active.push(ActiveEffect {
            kind: definition.kind,
            magnitude: definition.magnitude,
            ticks_remaining: definition.duration_ticks,
        });
    }

    /// Pulses every active effect once, then counts its duration down (expired effects drop
    /// out). `prayer` is `None` for actors without a prayer pool, in which case Corruption does
    /// nothing.
    pub fn advance(&mut self, health: &mut Health, mut prayer: Option<&mut PrayerPool>) {
        self.poison_immunity_ticks = (self.poison_immunity_ticks - 1).max(0);
        self.active.retain_mut(|effect| {
            match effect.kind {
                StatusEffectKind::Poison | StatusEffectKind::Venom => {
                    effect.ticks_remaining -= 1;
                    if effect.ticks_remaining > 0 {
                        return true;
                    }
                    effect.ticks_remaining = rules::DAMAGE_OVER_TIME_INTERVAL_TICKS;
                    if effect.kind == StatusEffectKind::Poison {
                        health.damage(rules::poison_damage(effect.magnitude));
                        effect.magnitude -= 1;
                        return effect.magnitude > 0;
                    }
                    effect.magnitude = rules::next_venom_damage(effect.magnitude);
                    health.damage(effect.magnitude);
                    return true;
                }
                StatusEffectKind::Burn => health.damage(effect.magnitude),
                StatusEffectKind::Corruption => {
                    if let Some(prayer) = prayer.as_mut() {
                        prayer.drain(effect.magnitude);
                    }
                }
                StatusEffectKind::DisableRunning | StatusEffectKind::Stun | StatusEffectKind::Freeze => {}
            }

            effect.ticks_remaining -= 1;
            effect.ticks_remaining > 0
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burn_deals_damage_per_tick_until_duration_expires() {
        let mut health = Health::new(100);
        let mut tracker = StatusEffectTracker::new();
        tracker.apply(StatusEffectDefinition::burn(5, 3));

        tracker.advance(&mut health, None); // tick 1
        assert_eq!(health.current(), 95);
        tracker.advance(&mut health, None); // tick 2
        assert_eq!(health.current(), 90);
        tracker.advance(&mut health, None); // tick 3 - last tick
        assert_eq!(health.current(), 85);
        tracker.advance(&mut health, None); // expired - no further damage
        assert_eq!(health.current(), 85);
    }

    #[test]
    fn corruption_drains_prayer_per_tick_and_is_a_no_op_with_no_prayer_pool() {
        let mut health = Health::new(100);
        let mut prayer = PrayerPool::new(99);
        let mut tracker = StatusEffectTracker::new();
        tracker.apply(StatusEffectDefinition::corruption(10, 2));

        tracker.advance(&mut health, Some(&mut prayer));
        assert_eq!(prayer.current(), 89);
        assert_eq!(health.current(), 100, "Corruption drains prayer, never Health.");

        tracker.advance(&mut health, None); // no prayer pool given - must not panic, just do nothing
        assert_eq!(prayer.current(), 89);
    }

    #[test]
    fn reapplying_refreshes_duration_and_magnitude_rather_than_stacking() {
        let mut health = Health::new(100);
        let mut tracker = StatusEffectTracker::new();
        tracker.apply(StatusEffectDefinition::burn(5, 2));
        tracker.advance(&mut health, None); // 1 tick left
        assert_eq!(health.current(), 95);

        tracker.apply(StatusEffectDefinition::burn(9, 5)); // refreshed, not stacked
        tracker.advance(&mut health, None);

        assert_eq!(health.current(), 86, "Only one Burn instance should ever tick - 9 damage, not 5+9.");
    }

    #[test]
    fn is_running_disabled_and_is_stunned_reflect_active_effects_and_clear_on_expiry() {
        let mut health = Health::new(100);
        let mut tracker = StatusEffectTracker::new();
        assert!(!tracker.is_running_disabled());
        assert!(!tracker.is_stunned());

        tracker.apply(StatusEffectDefinition::disable_running(1));
        tracker.apply(StatusEffectDefinition::stun(1));
        assert!(tracker.is_running_disabled());
        assert!(tracker.is_stunned());

        tracker.advance(&mut health, None); // both expire after this tick
        assert!(!tracker.is_running_disabled());
        assert!(!tracker.is_stunned());
    }

    fn run(tracker: &mut StatusEffectTracker, health: &mut Health, ticks: i32) {
        for _ in 0..ticks {
            tracker.advance(health, None);
        }
    }

    #[test]
    fn poison_hits_every_thirty_ticks_and_steps_down_with_severity() {
        let mut health = Health::new(1000);
        let mut tracker = StatusEffectTracker::new();
        tracker.apply(StatusEffectDefinition::poison(30));

        run(&mut tracker, &mut health, 29);
        assert_eq!(health.current(), 1000, "no damage before the first 30-tick boundary");
        run(&mut tracker, &mut health, 1);
        assert_eq!(health.current(), 994); // severity 30 -> 6 damage
        assert_eq!(tracker.poison_severity(), 29);

        run(&mut tracker, &mut health, 30 * 4); // four more hits at severities 29..26: all 6
        assert_eq!(health.current(), 994 - 24);
        run(&mut tracker, &mut health, 30); // severity 25 -> 5
        assert_eq!(health.current(), 994 - 24 - 5);
    }

    #[test]
    fn poison_wears_off_when_severity_reaches_zero() {
        let mut health = Health::new(1000);
        let mut tracker = StatusEffectTracker::new();
        tracker.apply(StatusEffectDefinition::poison(2));

        run(&mut tracker, &mut health, 30 * 2);

        assert_eq!(health.current(), 998);
        assert!(!tracker.is_poisoned());
    }

    #[test]
    fn venom_climbs_and_never_expires_and_beats_poison() {
        let mut health = Health::new(1000);
        let mut tracker = StatusEffectTracker::new();
        tracker.apply(StatusEffectDefinition::poison(10));
        tracker.apply(StatusEffectDefinition::venom());
        assert!(tracker.is_venomed());
        assert!(!tracker.is_poisoned(), "venom replaces poison");

        tracker.apply(StatusEffectDefinition::poison(50));
        assert!(!tracker.is_poisoned(), "a poison cannot downgrade an existing venom");

        run(&mut tracker, &mut health, 30 * 3);
        assert_eq!(health.current(), 1000 - 6 - 8 - 10);
        run(&mut tracker, &mut health, 30 * 20);
        assert!(tracker.is_venomed());
    }

    #[test]
    fn immunity_blocks_new_poison_until_it_runs_out() {
        let mut health = Health::new(100);
        let mut tracker = StatusEffectTracker::new();
        tracker.grant_poison_immunity(2);
        tracker.apply(StatusEffectDefinition::poison(10));
        assert!(!tracker.is_poisoned());

        run(&mut tracker, &mut health, 2);
        tracker.apply(StatusEffectDefinition::poison(10));
        assert!(tracker.is_poisoned());
    }

    #[test]
    fn cures_remove_their_effect() {
        let mut tracker = StatusEffectTracker::new();
        tracker.apply(StatusEffectDefinition::poison(10));
        tracker.cure_poison();
        assert!(!tracker.is_poisoned());

        tracker.apply(StatusEffectDefinition::venom());
        tracker.cure_venom();
        assert!(!tracker.is_venomed());
    }

    #[test]
    fn freeze_lasts_its_duration() {
        let mut health = Health::new(100);
        let mut tracker = StatusEffectTracker::new();
        tracker.apply(StatusEffectDefinition::freeze(2));
        assert!(tracker.is_frozen());

        run(&mut tracker, &mut health, 2);
        assert!(!tracker.is_frozen());
    }
}
