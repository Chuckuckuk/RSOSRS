use std::fmt::Debug;
use std::rc::Rc;

use super::player_reaction_state::PlayerReactionState;
use super::position_resolver::PositionResolver;
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
    /// Where the player stood when the attack was selected: all a resolver may take from the
    /// target when deriving where to aim.
    pub target_tile: TileCoord,
    /// Where the player stands now.
    pub player_tile: TileCoord,
    pub player_alive: bool,
    /// The player's prayer and stance now.
    pub player_reaction: &'a PlayerReactionState,
}

/// What a special does, for the world to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialEffect {
    /// Damage to the player, already reduced by whatever applies.
    HurtPlayer { damage: i32 },
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
            vec![SpecialEffect::HurtPlayer { damage: self.0 }]
        }
    }

    fn context(reaction: &PlayerReactionState) -> SpecialContext<'_> {
        SpecialContext {
            self_tile: TileCoord::new(0, 0),
            target_tile: TileCoord::new(5, 5),
            player_tile: TileCoord::new(5, 5),
            player_alive: true,
            player_reaction: reaction,
        }
    }

    #[test]
    fn a_boss_can_register_a_resolver_of_its_own() {
        let attack = AttackDefinition::special("Smite", Some(10), 4, Smite(25));
        let special = attack.special_part().expect("special");

        let effects = special.resolver.resolve(&context(&PlayerReactionState::default()), &mut ScriptedRng::default());

        assert_eq!(effects, vec![SpecialEffect::HurtPlayer { damage: 25 }]);
        assert_eq!(attack.telegraph_ticks(), 4);
        assert!(attack.position_resolver().is_none(), "it is not a position special");
    }

    #[test]
    fn the_built_in_position_resolver_hurts_only_a_player_on_its_tiles() {
        let on_player = PositionResolver::new(GroundTarget::square(HazardAnchor::Target, 0), GroundDamage::Fixed(30));
        let on_boss = PositionResolver::new(GroundTarget::square(HazardAnchor::SelfTile, 0), GroundDamage::Fixed(30));
        let reaction = PlayerReactionState::default();

        let mut rng = ScriptedRng::default();
        assert_eq!(on_player.resolve(&context(&reaction), &mut rng), vec![SpecialEffect::HurtPlayer { damage: 30 }]);
        assert!(on_boss.resolve(&context(&reaction), &mut rng).is_empty());
    }
}
