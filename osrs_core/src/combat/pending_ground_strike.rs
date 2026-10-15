use std::rc::Rc;

use super::attack_definition::AttackDefinition;
use super::combat_stats::CombatStats;
use crate::actors::EntityId;
use crate::tiles::TileCoord;

/// A ground-targeted attack that has been released and is waiting out its `impact_delay_ticks`.
/// Its tiles were fixed the moment it resolved; when the delay is up the world finds whoever stands
/// on them and rolls `attack` against each. Owned by the world, not by a defender (it has none) or
/// by the attacker, so it still lands if the attacker dies in the meantime.
#[derive(Debug, Clone)]
pub struct PendingGroundStrike {
    pub attack: Rc<AttackDefinition>,
    pub tiles: Vec<TileCoord>,
    /// Where the strike is centred, for the attack's positional reaction.
    pub origin: TileCoord,
    pub attacker: EntityId,
    pub attacker_name: String,
    /// The attacker's stats when it released the attack: it may be dead by the time this lands.
    pub attacker_stats: CombatStats,
    ticks_remaining: i32,
}

impl PendingGroundStrike {
    pub fn new(
        attack: Rc<AttackDefinition>,
        tiles: Vec<TileCoord>,
        origin: TileCoord,
        attacker: EntityId,
        attacker_name: impl Into<String>,
        attacker_stats: CombatStats,
    ) -> Self {
        let ticks_remaining = attack.impact_delay_ticks;
        Self { attack, tiles, origin, attacker, attacker_name: attacker_name.into(), attacker_stats, ticks_remaining }
    }

    pub fn ticks_remaining(&self) -> i32 {
        self.ticks_remaining
    }

    /// Must be called exactly once per tick; true exactly on the tick it lands.
    pub fn advance(&mut self) -> bool {
        self.ticks_remaining -= 1;
        self.ticks_remaining == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::AttackStyle;

    fn strike(delay: i32) -> PendingGroundStrike {
        let attack = AttackDefinition::new("Rock", 5, 0, AttackStyle::Magic)
            .with_impact_delay_ticks(delay)
            .shared();
        PendingGroundStrike::new(
            attack,
            vec![TileCoord::new(1, 1)],
            TileCoord::new(1, 1),
            EntityId(1),
            "Boss",
            CombatStats::new(50, 50, 50, 50, 50).unwrap(),
        )
    }

    #[test]
    fn it_lands_exactly_when_the_attacks_impact_delay_has_elapsed() {
        let mut strike = strike(3);

        assert!(!strike.advance());
        assert!(!strike.advance());
        assert!(strike.advance());
    }
}
