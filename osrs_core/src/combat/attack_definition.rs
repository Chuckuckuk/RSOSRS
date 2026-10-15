use std::rc::Rc;

use super::attack_style::AttackStyle;
use super::ground_attack::{GroundAttack, GroundDamage, PrayerReduction};
use super::ground_target::GroundTarget;
use super::max_hit_source::MaxHitSource;
use super::projectile_speed::ProjectileSpeed;
use super::protection_prayer::ProtectionPrayer;
use super::reaction_requirement::ReactionRequirement;
use super::status_effect::StatusEffectDefinition;
use super::targeted_attack::TargetedAttack;
use crate::model::{DamageType, Modifier};
use crate::pathfinding::TargetSpec;
use crate::tiles::TileCoord;

/// One attack an NPC can select. Shared (`Rc<AttackDefinition>`) between patterns, the combat
/// state that is currently telegraphing it and the pending-attack queue that later rolls it, so
/// "the same attack" stays pointer-identical like the C# reference type.
///
/// This holds what every attack has: how far it reaches, how it looks and sounds, and an optional
/// override of the attacker's own attack rate. When it happens, and what it does, is its `kind`:
/// an attack that is rolled against a defender is `Instant`, `Telegraphed` or `OnContact` (the
/// three timing archetypes of the OSRS bosses), and one aimed at tiles is `Ground`. Each carries
/// only the timing fields that kind has.
///
/// The optional fields are set with the `with_*` builders (each validates like the C# constructor
/// and panics on bad data, including a builder for a field the attack's kind does not have).
#[derive(Debug, Clone)]
pub struct AttackDefinition {
    pub name: String,
    pub kind: AttackKind,
    /// Overrides the attacker's own attack range for this attack: how many tiles away it can be
    /// used from. `None` uses the attacker's.
    pub range: Option<i32>,
    /// Overrides the attacker's own attack speed for this attack: the ticks between this attack
    /// resolving and the next one (OSRS attack speed). `None` uses the attacker's.
    pub attack_speed: Option<i32>,
    pub on_hit_status_effect: Option<StatusEffectDefinition>,
    /// The cache sequence the attacker plays for this attack, if it has its own (presentation
    /// only: the rules never read it).
    pub animation: Option<u32>,
    /// Graphics (cache spot animations) for the client: one flying from attacker to target, one
    /// on the target as the attack is released, one on it as the hit lands. Presentation only.
    pub projectile: Option<u32>,
    pub target_graphic: Option<u32>,
    pub impact_graphic: Option<u32>,
    /// Cache sound effects (archive 4): one heard as the attack is thrown, one as it lands (a
    /// spell's impact). Presentation only.
    pub sound: Option<u32>,
    pub hit_sound: Option<u32>,
}

/// When an attack resolves, and the data only that kind of attack has.
#[derive(Debug, Clone)]
pub enum AttackKind {
    /// Rolls the tick it is chosen, with no wind-up to read (the "traditional" archetype: most
    /// melee, and any attack the player must already be praying against when it starts).
    Instant(InstantAttack),
    /// A visible wind-up, then it rolls the tick the wind-up ends (Jad).
    Telegraphed(TelegraphedAttack),
    /// Rolls only once it makes contact, `impact_delay_ticks` after it is released, so the
    /// defender's state at that moment is what counts (Doom of Mokhaiotl's "Doom-style").
    OnContact(OnContactAttack),
    /// Aimed at tiles; never rolls accuracy.
    Ground(GroundAttack),
    // Catch-all for attacks that do not neatly match either of the above. The OSRS combat system has a few of these, but they are not used in this project.
    Special,
}

/// A targeted attack that rolls as soon as it is chosen. If it flies, the damage lands after the
/// flight.
#[derive(Debug, Clone)]
pub struct InstantAttack {
    pub payload: TargetedAttack,
    /// Overrides the engine default flight speed of an attack that flies after it rolls.
    pub projectile_speed: Option<ProjectileSpeed>,
}

/// A targeted attack with a wind-up before it rolls.
#[derive(Debug, Clone)]
pub struct TelegraphedAttack {
    /// Ticks of visible wind-up before the roll.
    pub telegraph_ticks: i32,
    pub payload: TargetedAttack,
    /// Overrides the engine default flight speed of an attack that flies after it rolls.
    pub projectile_speed: Option<ProjectileSpeed>,
}

/// A targeted attack whose roll waits until it lands.
#[derive(Debug, Clone)]
pub struct OnContactAttack {
    /// Ticks between the attack being released and the roll.
    pub impact_delay_ticks: i32,
    pub payload: TargetedAttack,
}

impl AttackDefinition {
    /// An attack rolled against a defender the tick it is chosen.
    pub fn instant(name: impl Into<String>, range: Option<i32>, style: AttackStyle) -> Self {
        Self::with_kind(
            name.into(),
            range,
            AttackKind::Instant(InstantAttack { payload: TargetedAttack::new(style), projectile_speed: None }),
        )
    }

    /// An attack rolled against a defender once `telegraph_ticks` of wind-up have passed.
    pub fn telegraphed(name: impl Into<String>, range: Option<i32>, telegraph_ticks: i32, style: AttackStyle) -> Self {
        assert!(telegraph_ticks >= 1, "a telegraphed attack needs at least one tick of wind-up");
        Self::with_kind(
            name.into(),
            range,
            AttackKind::Telegraphed(TelegraphedAttack {
                telegraph_ticks,
                payload: TargetedAttack::new(style),
                projectile_speed: None,
            }),
        )
    }

    /// An attack rolled against a defender only when it lands, `impact_delay_ticks` after release.
    pub fn on_contact(name: impl Into<String>, range: Option<i32>, impact_delay_ticks: i32, style: AttackStyle) -> Self {
        assert!(impact_delay_ticks >= 1, "an on-contact attack needs at least one tick before it lands");
        Self::with_kind(
            name.into(),
            range,
            AttackKind::OnContact(OnContactAttack { impact_delay_ticks, payload: TargetedAttack::new(style) }),
        )
    }

    /// A targeted attack from a wind-up length: no wind-up is an `Instant` attack, any is
    /// `Telegraphed`. For data, where the length is a number.
    pub fn targeted(name: impl Into<String>, range: Option<i32>, telegraph_ticks: i32, style: AttackStyle) -> Self {
        assert!(telegraph_ticks >= 0, "telegraph_ticks must not be negative");
        if telegraph_ticks == 0 {
            Self::instant(name, range, style)
        } else {
            Self::telegraphed(name, range, telegraph_ticks, style)
        }
    }

    /// An attack aimed at the ground: when its `telegraph_ticks` end it deals `damage` to whoever
    /// stands on the tiles `target` picks, and is gone.
    pub fn ground(
        name: impl Into<String>,
        range: Option<i32>,
        telegraph_ticks: i32,
        target: GroundTarget,
        damage: GroundDamage,
    ) -> Self {
        Self::with_kind(name.into(), range, AttackKind::Ground(GroundAttack::new(telegraph_ticks, target, damage)))
    }

    fn with_kind(name: String, range: Option<i32>, kind: AttackKind) -> Self {
        Self {
            name,
            range,
            attack_speed: None,
            on_hit_status_effect: None,
            animation: None,
            projectile: None,
            target_graphic: None,
            impact_graphic: None,
            sound: None,
            hit_sound: None,
            kind,
        }
    }

    // ---- what kind of attack this is ----

    /// The part of an attack that is rolled against a defender, whichever of the three timings it
    /// has; `None` for a ground attack.
    pub fn targeted_part(&self) -> Option<&TargetedAttack> {
        match &self.kind {
            AttackKind::Instant(attack) => Some(&attack.payload),
            AttackKind::Telegraphed(attack) => Some(&attack.payload),
            AttackKind::OnContact(attack) => Some(&attack.payload),
            AttackKind::Ground(_) | AttackKind::Special => None,
        }
    }

    fn targeted_part_mut(&mut self) -> Option<&mut TargetedAttack> {
        match &mut self.kind {
            AttackKind::Instant(attack) => Some(&mut attack.payload),
            AttackKind::Telegraphed(attack) => Some(&mut attack.payload),
            AttackKind::OnContact(attack) => Some(&mut attack.payload),
            AttackKind::Ground(_) | AttackKind::Special => None,
        }
    }

    pub fn ground_part(&self) -> Option<&GroundAttack> {
        match &self.kind {
            AttackKind::Ground(ground) => Some(ground),
            _ => None,
        }
    }

    /// The targeted part of an attack that is known to be one: a player's swing, or one waiting in
    /// a defender's queue. Panics for any other kind, which can be neither.
    pub fn expect_targeted(&self) -> &TargetedAttack {
        self.targeted_part().unwrap_or_else(|| panic!("'{}' is not rolled against a defender", self.name))
    }

    /// What to call an attack that is not a targeted one, for the panic of a builder that does
    /// not apply to it.
    fn untargeted_label(&self) -> &'static str {
        match self.kind {
            AttackKind::Ground(_) => "a ground attack",
            AttackKind::Special => "a special attack",
            _ => "a targeted attack",
        }
    }

    // ---- timing ----

    /// Ticks of visible wind-up before this attack resolves; 0 for one that resolves the tick it
    /// is chosen.
    pub fn telegraph_ticks(&self) -> i32 {
        match &self.kind {
            AttackKind::Telegraphed(attack) => attack.telegraph_ticks,
            AttackKind::Ground(ground) => ground.telegraph_ticks,
            AttackKind::Instant(_) | AttackKind::OnContact(_) | AttackKind::Special => 0,
        }
    }

    /// Ticks between a targeted attack being released and its roll; 0 for every kind but
    /// `OnContact`, which rolls when it lands.
    pub fn impact_delay_ticks(&self) -> i32 {
        match &self.kind {
            AttackKind::OnContact(attack) => attack.impact_delay_ticks,
            _ => 0,
        }
    }

    /// Ticks the damage spends in the air after the roll, for an attack released `distance` tiles
    /// from its target: none for melee, an `OnContact` attack (its delay is the flight) or a
    /// ground attack, otherwise the attack's own `ProjectileSpeed` or the engine default.
    pub fn flight_ticks(&self, distance: i32) -> i32 {
        let (payload, speed) = match &self.kind {
            AttackKind::Instant(attack) => (&attack.payload, attack.projectile_speed),
            AttackKind::Telegraphed(attack) => (&attack.payload, attack.projectile_speed),
            AttackKind::OnContact(_) | AttackKind::Ground(_) | AttackKind::Special => return 0,
        };
        if payload.style == AttackStyle::Melee {
            return 0;
        }
        speed.unwrap_or(ProjectileSpeed::DEFAULT).flight_ticks(distance)
    }

    /// Applies `change` to the targeted part. Panics for an attack that has none: setting an
    /// accuracy modifier on a boulder is a data bug, not something to ignore.
    fn change_targeted(mut self, builder: &str, change: impl FnOnce(TargetedAttack) -> TargetedAttack) -> Self {
        let (name, label) = (self.name.clone(), self.untargeted_label());
        let targeted = self
            .targeted_part_mut()
            .unwrap_or_else(|| panic!("`{builder}` only applies to a targeted attack, and '{name}' is {label}"));
        *targeted = change(targeted.clone());
        self
    }

    // ---- common builders ----

    /// Makes this attack's own attack speed `speed_ticks` instead of the attacker's: consecutive
    /// uses of it resolve exactly that many ticks apart. An attack without a telegraph resolves on
    /// the tick it is chosen; one with a telegraph spends its wind-up first, so the fastest speed
    /// this can express is `telegraph_ticks + 1`.
    pub fn with_attack_speed(mut self, speed_ticks: i32) -> Self {
        assert!(speed_ticks >= 1, "attack speed must be at least 1 tick");
        self.attack_speed = Some(speed_ticks);
        self
    }

    pub fn with_on_hit_status_effect(mut self, effect: StatusEffectDefinition) -> Self {
        self.on_hit_status_effect = Some(effect);
        self
    }

    pub fn with_animation(mut self, animation: Option<u32>) -> Self {
        self.animation = animation;
        self
    }

    pub fn with_graphics(mut self, projectile: Option<u32>, target_graphic: Option<u32>, impact_graphic: Option<u32>) -> Self {
        self.projectile = projectile;
        self.target_graphic = target_graphic;
        self.impact_graphic = impact_graphic;
        self
    }

    pub fn with_sounds(mut self, sound: Option<u32>, hit_sound: Option<u32>) -> Self {
        self.sound = sound;
        self.hit_sound = hit_sound;
        self
    }

    // ---- targeted-only builders ----

    pub fn with_damage_type(self, damage_type: DamageType) -> Self {
        self.change_targeted("with_damage_type", |t| t.with_damage_type(damage_type))
    }

    pub fn with_max_hit(self, max_hit: MaxHitSource) -> Self {
        self.change_targeted("with_max_hit", |t| t.with_max_hit(max_hit))
    }

    pub fn with_modifiers(self, modifiers: Vec<Modifier>) -> Self {
        self.change_targeted("with_modifiers", |t| t.with_modifiers(modifiers))
    }

    pub fn with_flags(self, flags: Vec<String>) -> Self {
        self.change_targeted("with_flags", |t| t.with_flags(flags))
    }

    pub fn with_hit_count(self, hit_count: i32) -> Self {
        self.change_targeted("with_hit_count", |t| t.with_hit_count(hit_count))
    }

    pub fn with_always_hits(self, always_hits: bool) -> Self {
        self.change_targeted("with_always_hits", |t| t.with_always_hits(always_hits))
    }

    pub fn with_prayer_penetration(self, prayer_penetration: f32) -> Self {
        self.change_targeted("with_prayer_penetration", |t| t.with_prayer_penetration(prayer_penetration))
    }

    pub fn with_reaction(self, reaction: ReactionRequirement) -> Self {
        self.change_targeted("with_reaction", |t| t.with_reaction(reaction))
    }

    // ---- flight builder ----

    /// How fast this attack's projectile flies, for an attack that rolls and then lands after the
    /// flight (`Instant` and `Telegraphed`): an override of the engine default.
    pub fn with_projectile_speed(mut self, speed: ProjectileSpeed) -> Self {
        match &mut self.kind {
            AttackKind::Instant(attack) => attack.projectile_speed = Some(speed),
            AttackKind::Telegraphed(attack) => attack.projectile_speed = Some(speed),
            AttackKind::OnContact(_) | AttackKind::Ground(_) | AttackKind::Special => {
                panic!("`with_projectile_speed` only applies to an attack that rolls before it lands, and '{}' does not", self.name)
            }
        }
        self
    }

    // ---- ground-only builders ----

    /// The one protection prayer that cuts this ground attack's damage without negating it:
    /// `penetration` of the damage still gets through when it is active.
    pub fn with_prayer_reduction(mut self, prayer: ProtectionPrayer, penetration: f32) -> Self {
        assert!(prayer != ProtectionPrayer::None, "a prayer reduction needs a prayer");
        assert!((0.0..=1.0).contains(&penetration), "penetration must be within [0, 1]");
        match &mut self.kind {
            AttackKind::Ground(ground) => ground.prayer_reduction = Some(PrayerReduction { prayer, penetration }),
            _ => panic!("`with_prayer_reduction` only applies to a ground attack, and '{}' is not one", self.name),
        }
        self
    }

    /// Wraps the finished definition for sharing.
    pub fn shared(self) -> Rc<Self> {
        Rc::new(self)
    }

    /// How far this attack reaches: its own `range` if it names one, else `attacker_range`, the
    /// attacker's own range stat.
    pub fn reach(&self, attacker_range: i32) -> i32 {
        self.range.unwrap_or(attacker_range)
    }

    /// Whether this is a melee attack at base reach (range 1): it must be orthogonally adjacent
    /// to the attacker's footprint. A melee attack with a longer reach (a halberd's 2), and every
    /// ground attack, is measured like any other: within its range in tiles.
    pub fn is_melee_reach(&self, attacker_range: i32) -> bool {
        self.targeted_part().is_some_and(|targeted| targeted.style == AttackStyle::Melee) && self.reach(attacker_range) <= 1
    }

    /// The target spec an attacker standing at `attacker_tile` with the given footprint and attack
    /// range stat must satisfy for this attack to be in range of a tile.
    pub fn build_range_check(&self, attacker_tile: TileCoord, attacker_footprint: i32, attacker_range: i32) -> TargetSpec {
        if self.is_melee_reach(attacker_range) {
            TargetSpec::adjacent(attacker_tile, attacker_footprint, attacker_footprint)
        } else {
            TargetSpec::within_range_of(attacker_tile, self.reach(attacker_range))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::HazardAnchor;

    fn boulder() -> AttackDefinition {
        AttackDefinition::ground("Boulder", Some(10), 2, GroundTarget::square(HazardAnchor::Target, 1), GroundDamage::Fixed(30))
    }

    #[test]
    fn an_instant_attack_has_no_telegraph_and_no_delay() {
        let attack = AttackDefinition::instant("Test", None, AttackStyle::Melee);

        assert!(matches!(attack.kind, AttackKind::Instant(_)));
        assert_eq!(attack.telegraph_ticks(), 0);
        assert_eq!(attack.impact_delay_ticks(), 0);
        assert!(attack.on_hit_status_effect.is_none());
        assert!(attack.attack_speed.is_none());
    }

    #[test]
    fn each_timing_kind_carries_only_its_own_number() {
        let telegraphed = AttackDefinition::telegraphed("Jad", Some(15), 3, AttackStyle::Magic);
        let on_contact = AttackDefinition::on_contact("Slam", Some(15), 4, AttackStyle::Magic);

        assert_eq!((telegraphed.telegraph_ticks(), telegraphed.impact_delay_ticks()), (3, 0));
        assert_eq!((on_contact.telegraph_ticks(), on_contact.impact_delay_ticks()), (0, 4));
    }

    #[test]
    fn the_data_constructor_picks_instant_or_telegraphed_from_the_wind_up() {
        assert!(matches!(AttackDefinition::targeted("A", None, 0, AttackStyle::Melee).kind, AttackKind::Instant(_)));
        assert!(matches!(AttackDefinition::targeted("A", None, 2, AttackStyle::Melee).kind, AttackKind::Telegraphed(_)));
    }

    #[test]
    fn a_ground_attack_has_no_targeted_part_and_its_telegraph_is_its_timing() {
        let attack = boulder();

        assert!(attack.targeted_part().is_none());
        assert_eq!(attack.ground_part().expect("ground").damage, GroundDamage::Fixed(30));
        assert_eq!(attack.telegraph_ticks(), 2);
        assert_eq!(attack.impact_delay_ticks(), 0);
    }

    #[test]
    #[should_panic]
    fn prayer_penetration_above_one_panics() {
        AttackDefinition::instant("Test", None, AttackStyle::Melee).with_prayer_penetration(1.5);
    }

    #[test]
    #[should_panic]
    fn a_telegraph_of_zero_ticks_is_not_a_telegraphed_attack() {
        AttackDefinition::telegraphed("Test", None, 0, AttackStyle::Melee);
    }

    #[test]
    #[should_panic(expected = "ground attack")]
    fn an_accuracy_builder_on_a_ground_attack_panics() {
        boulder().with_always_hits(true);
    }

    #[test]
    #[should_panic(expected = "is not one")]
    fn a_prayer_reduction_on_a_targeted_attack_panics() {
        AttackDefinition::instant("Test", None, AttackStyle::Melee).with_prayer_reduction(ProtectionPrayer::Melee, 0.5);
    }

    #[test]
    fn a_ground_attack_takes_a_prayer_reduction() {
        let attack = boulder().with_prayer_reduction(ProtectionPrayer::Melee, 0.5);

        let reduction = attack.ground_part().unwrap().prayer_reduction.expect("reduction");
        assert_eq!(reduction.prayer, ProtectionPrayer::Melee);
        assert_eq!(reduction.penetration, 0.5);
    }

    #[test]
    fn a_projectile_speed_only_applies_to_an_attack_that_rolls_before_it_lands() {
        let flies = AttackDefinition::instant("Arrow", Some(7), AttackStyle::Ranged)
            .with_projectile_speed(ProjectileSpeed::tiles_per_tick(1.0));

        assert_eq!(flies.flight_ticks(5), 5);
    }

    #[test]
    #[should_panic(expected = "rolls before it lands")]
    fn an_on_contact_attack_has_no_projectile_speed() {
        AttackDefinition::on_contact("Slam", Some(10), 3, AttackStyle::Magic).with_projectile_speed(ProjectileSpeed::DEFAULT);
    }

    #[test]
    fn flight_uses_the_engine_default_speed_unless_the_attack_overrides_it() {
        let arrow = AttackDefinition::instant("Arrow", Some(7), AttackStyle::Ranged);

        assert_eq!(arrow.flight_ticks(3), 1);
        assert_eq!(arrow.flight_ticks(7), 3);
    }

    #[test]
    fn melee_ground_and_on_contact_attacks_have_no_flight() {
        assert_eq!(AttackDefinition::instant("Punch", Some(1), AttackStyle::Melee).flight_ticks(1), 0);
        assert_eq!(AttackDefinition::on_contact("Slam", Some(10), 3, AttackStyle::Magic).flight_ticks(8), 0);
        assert_eq!(boulder().flight_ticks(8), 0);
    }

    #[test]
    fn melee_range_check_is_adjacency_to_the_attackers_footprint() {
        let attack = AttackDefinition::instant("Test", None, AttackStyle::Melee);

        let check = attack.build_range_check(TileCoord::new(5, 5), 2, 1);

        assert!(check.is_satisfied_by(TileCoord::new(4, 5)));
        assert!(check.is_satisfied_by(TileCoord::new(7, 6)));
        assert!(!check.is_satisfied_by(TileCoord::new(5, 5)));
    }

    #[test]
    fn ranged_range_check_is_chebyshev_distance_from_the_attacker_tile() {
        let attack = AttackDefinition::instant("Test", Some(3), AttackStyle::Magic);

        let check = attack.build_range_check(TileCoord::new(5, 5), 1, 1);

        assert!(check.is_satisfied_by(TileCoord::new(8, 8)));
        assert!(!check.is_satisfied_by(TileCoord::new(9, 5)));
    }

    #[test]
    fn a_ground_attack_is_measured_by_range_never_by_adjacency() {
        let attack = AttackDefinition::ground("Slam", None, 1, GroundTarget::square(HazardAnchor::SelfTile, 1), GroundDamage::Fixed(5));

        let check = attack.build_range_check(TileCoord::new(5, 5), 1, 1);

        assert!(check.is_satisfied_by(TileCoord::new(6, 6)), "range 1 reaches the diagonal: it is not melee adjacency");
    }

    #[test]
    fn an_attack_without_a_range_of_its_own_uses_the_attackers() {
        let inherits = AttackDefinition::instant("Arrow", None, AttackStyle::Ranged);
        let overrides = AttackDefinition::instant("Poke", Some(1), AttackStyle::Melee);

        // an attacker with a range stat of 7 reaches 7 tiles; the poke still needs adjacency
        let check = inherits.build_range_check(TileCoord::new(0, 0), 1, 7);
        assert!(check.is_satisfied_by(TileCoord::new(7, 0)));
        assert!(!check.is_satisfied_by(TileCoord::new(8, 0)));
        assert_eq!(inherits.reach(7), 7);
        assert!(overrides.is_melee_reach(7));
        assert_eq!(overrides.reach(7), 1);
    }

    #[test]
    fn a_melee_attack_inheriting_a_long_range_is_measured_by_that_range() {
        // a halberd-length reach: melee style, but past base reach, so within-range not adjacency
        let attack = AttackDefinition::instant("Swipe", None, AttackStyle::Melee);

        assert!(!attack.is_melee_reach(2));
        assert!(attack.build_range_check(TileCoord::new(0, 0), 1, 2).is_satisfied_by(TileCoord::new(2, 2)));
    }
}
