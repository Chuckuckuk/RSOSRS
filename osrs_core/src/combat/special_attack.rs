use std::fmt::Debug;
use std::rc::Rc;

use super::combat_stats::CombatStats;
use super::damage_resolver;
use super::hazard::HazardDef;
use super::player_reaction_state::PlayerReactionState;
use super::position_resolver::PositionResolver;
use super::reaction_resolver::get_damage_multiplier;
use super::targeted_attack::TargetedAttack;
use crate::rng::Rng;
use crate::tiles::TileCoord;

/// An attack that is not one of the three standard timings (instant, telegraphed, on-contact) or
/// that needs to check anything besides the overhead protection prayer. A standard attack is
/// rolled against one defender and answered by its protection prayer, nothing more; a special
/// waits out its own wind-up and then hands the decision of who is hurt, and for how much, to its
/// `resolver`.
///
/// The resolver is a trait object so a boss can register a special of its own, while the logic
/// several bosses share (`PositionResolver`, who is standing on which tiles) is one reusable piece.
#[derive(Debug, Clone)]
pub struct SpecialAttack {
    /// Ticks of visible wind-up before the resolver runs; 0 resolves the tick it is chosen.
    pub telegraph_ticks: i32,
    pub resolver: Rc<dyn SpecialResolver>,
}

impl SpecialAttack {
    pub fn new(telegraph_ticks: i32, resolver: impl SpecialResolver + 'static) -> Self {
        assert!(telegraph_ticks >= 0, "telegraph_ticks must not be negative");
        Self { telegraph_ticks, resolver: Rc::new(resolver) }
    }
}

/// What a special is told about the fight when its wind-up ends. Positions are the ones captured
/// when the attack was selected (so moving during the wind-up is how to dodge), apart from the
/// defender's current ones.
#[derive(Debug, Clone, Copy)]
pub struct SpecialContext<'a> {
    /// Where the attacker stood when the attack was selected.
    pub self_tile: TileCoord,
    /// The middle of the attacker's footprint: where a shot leaves from.
    pub attacker_centre: TileCoord,
    pub attacker_stats: CombatStats,
    /// Where the player stood when the attack was selected: all a resolver may take from the
    /// target when deriving where to aim.
    pub target_tile: TileCoord,
    /// Where the player stands now.
    pub player_tile: TileCoord,
    pub player_alive: bool,
    pub player_stats: CombatStats,
    /// The player's prayer and stance now.
    pub player_reaction: &'a PlayerReactionState,
    /// The tiles that stand in the attacker's line of sight.
    pub sight_blockers: &'a [TileCoord],
    /// The tiles the special covers, when they were chosen ahead of its landing (so the fall could
    /// be shown); otherwise it chooses them as it resolves.
    pub chosen_tiles: Option<&'a [TileCoord]>,
}

impl SpecialContext<'_> {
    /// Rolls `shot` against the player as a standard attack would: the accuracy dice, then the
    /// damage, cut by the player's protection prayer.
    pub fn roll_shot(&self, shot: &TargetedAttack, rng: &mut dyn Rng) -> i32 {
        let multiplier = get_damage_multiplier(shot, Some(self.player_reaction));
        damage_resolver::roll(shot, &self.attacker_stats, &self.player_stats, false, rng, multiplier)
    }
}

/// What a special does, for the world to apply.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecialEffect {
    /// Damage to the player, already reduced by whatever applies, landing `delay_ticks` from now
    /// (the flight of a shot).
    HurtPlayer { damage: i32, delay_ticks: i32 },
    /// Holds the player in place for `ticks`.
    BindPlayer { ticks: i32 },
    /// Leaves a hazard on `tile` that takes effect after `armed_in` ticks.
    SpawnHazard { def: Rc<HazardDef>, tile: TileCoord, armed_in: i32 },
    /// Destroys the attacker's hazard standing on `tile`, if there is one, `delay_ticks` from now.
    DestroyHazard { tile: TileCoord, delay_ticks: i32 },
    /// The attack's projectile flies to `tile` instead of the player, arriving in `flight_ticks`,
    /// where its impact graphic plays (and `sound` is heard, if it has one). Any hurt that comes
    /// with it is the blast there, not a hit from the projectile.
    Strike { tile: TileCoord, flight_ticks: i32, sound: Option<u32> },
}

/// Decides what a special does when its wind-up ends. Like `AttackPattern`, an implementation is
/// a pure function of the context and the encounter's seeded RNG, so one instance is safe to share
/// between attacks and bosses and the outcome stays reproducible.
pub trait SpecialResolver: Debug {
    fn resolve(&self, context: &SpecialContext, rng: &mut dyn Rng) -> Vec<SpecialEffect>;

    /// This resolver as a `PositionResolver`, for the code that configures or inspects one.
    fn as_position(&self) -> Option<&PositionResolver> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{AttackDefinition, GroundDamage, GroundTarget, HazardAnchor, PositionResolver};
    use crate::rng::ScriptedRng;

    /// A boss-specific special: hurts the player for a flat amount wherever they are.
    #[derive(Debug)]
    struct Smite(i32);

    impl SpecialResolver for Smite {
        fn resolve(&self, _: &SpecialContext, _: &mut dyn Rng) -> Vec<SpecialEffect> {
            vec![SpecialEffect::HurtPlayer { damage: self.0, delay_ticks: 0 }]
        }
    }

    fn context(reaction: &PlayerReactionState) -> SpecialContext<'_> {
        SpecialContext {
            self_tile: TileCoord::new(0, 0),
            attacker_centre: TileCoord::new(0, 0),
            attacker_stats: CombatStats::new(50, 50, 50, 50, 50).unwrap(),
            target_tile: TileCoord::new(5, 5),
            player_tile: TileCoord::new(5, 5),
            player_alive: true,
            player_stats: CombatStats::new(50, 50, 50, 50, 50).unwrap(),
            player_reaction: reaction,
            sight_blockers: &[],
            chosen_tiles: None,
        }
    }

    #[test]
    fn a_boss_can_register_a_resolver_of_its_own() {
        let attack = AttackDefinition::special("Smite", Some(10), 4, Smite(25));
        let special = attack.special_part().expect("special");

        let effects = special.resolver.resolve(&context(&PlayerReactionState::default()), &mut ScriptedRng::default());

        assert_eq!(effects, vec![SpecialEffect::HurtPlayer { damage: 25, delay_ticks: 0 }]);
        assert_eq!(attack.telegraph_ticks(), 4);
        assert!(attack.position_resolver().is_none(), "it is not a position special");
    }

    #[test]
    fn the_built_in_position_resolver_hurts_only_a_player_on_its_tiles() {
        let on_player = PositionResolver::new(GroundTarget::square(HazardAnchor::Target, 0), GroundDamage::Fixed(30));
        let on_boss = PositionResolver::new(GroundTarget::square(HazardAnchor::SelfTile, 0), GroundDamage::Fixed(30));
        let reaction = PlayerReactionState::default();

        let mut rng = ScriptedRng::default();
        assert_eq!(on_player.resolve(&context(&reaction), &mut rng), vec![SpecialEffect::HurtPlayer { damage: 30, delay_ticks: 0 }]);
        assert!(on_boss.resolve(&context(&reaction), &mut rng).is_empty());
    }

    #[test]
    fn every_covered_tile_hurts_though_only_the_anchor_gets_a_hazard() {
        use crate::combat::{HazardDef, HazardPlacement};
        let boulder = Rc::new(HazardDef::new("Boulder"));
        let rocks = PositionResolver::new(GroundTarget::square(HazardAnchor::Target, 1), GroundDamage::Fixed(30))
            .leaving(boulder, HazardPlacement::AnchorTile);
        let reaction = PlayerReactionState::default();
        let mut rng = ScriptedRng::default();

        // the rock on the player's tile stays and hurts
        let effects = rocks.resolve(&context(&reaction), &mut rng);
        assert!(matches!(effects[0], SpecialEffect::HurtPlayer { damage: 30, .. }), "{effects:?}");

        // on a neighbouring covered tile no rock stays, but it still hurts
        let beside = SpecialContext { player_tile: TileCoord::new(6, 5), ..context(&reaction) };
        let effects = rocks.resolve(&beside, &mut rng);
        assert!(matches!(effects[0], SpecialEffect::HurtPlayer { damage: 30, .. }), "{effects:?}");
        assert!(effects.iter().any(|e| matches!(e, SpecialEffect::SpawnHazard { .. })), "the boulder still lands");

        // off the covered tiles altogether nothing hurts
        let away = SpecialContext { player_tile: TileCoord::new(9, 9), ..context(&reaction) };
        assert!(rocks.resolve(&away, &mut rng).iter().all(|e| !matches!(e, SpecialEffect::HurtPlayer { .. })));
    }

    #[test]
    fn tiles_chosen_ahead_of_landing_are_the_ones_it_covers() {
        let rocks = PositionResolver::new(GroundTarget::square(HazardAnchor::SelfTile, 0), GroundDamage::Fixed(30));
        let reaction = PlayerReactionState::default();
        let chosen = [TileCoord::new(5, 5)];
        let cx = SpecialContext { chosen_tiles: Some(&chosen), ..context(&reaction) };

        let effects = rocks.resolve(&cx, &mut ScriptedRng::default());

        assert_eq!(effects, vec![SpecialEffect::HurtPlayer { damage: 30, delay_ticks: 0 }]);
    }
}
