use super::actor_kind::ActorKind;
use super::combat_target_view::CombatTargetView;
use super::health::Health;
use crate::collision::{CollisionMap, OccupancyMask};
use crate::combat::CombatStats;
use crate::tiles::TileCoord;

/// A single tile-grid mover: player or NPC, both represented the same way. Carries its own
/// previous/current tile (not just current) so a view can interpolate between them via
/// tick progress without the view ever needing to know how many tiles were actually
/// consumed this tick (1 for a walk, 2 for a run, 0 if idle).
///
/// General enough to eventually back non-combat entities too (a finite-capacity shield/
/// barrier object, say) - "Actor" means "something that occupies a tile," not "something
/// that fights."
#[derive(Debug, Clone)]
pub struct Actor {
    kind: ActorKind,
    footprint_size: i32,
    health: Health,
    stats: CombatStats,
    previous_tile: TileCoord,
    current_tile: TileCoord,
    /// For an NPC: whether it also stands in players' way. Every NPC blocks other NPCs.
    blocks_players: bool,
}

impl Actor {
    pub fn new(kind: ActorKind, start_tile: TileCoord, footprint_size: i32, max_health: i32, stats: CombatStats) -> Self {
        Self {
            kind,
            footprint_size,
            health: Health::new(max_health),
            stats,
            previous_tile: start_tile,
            current_tile: start_tile,
            blocks_players: true,
        }
    }

    pub fn blocks_players(&self) -> bool {
        self.blocks_players
    }

    pub fn kind(&self) -> ActorKind {
        self.kind
    }

    pub fn footprint_size(&self) -> i32 {
        self.footprint_size
    }

    pub fn health(&self) -> &Health {
        &self.health
    }

    pub fn health_mut(&mut self) -> &mut Health {
        &mut self.health
    }

    pub fn stats(&self) -> CombatStats {
        self.stats
    }

    /// Replaces the combat stats (a player's gear, prayers, stance or boosts changed).
    pub fn set_stats(&mut self, stats: CombatStats) {
        self.stats = stats;
    }

    pub fn previous_tile(&self) -> TileCoord {
        self.previous_tile
    }

    pub fn current_tile(&self) -> TileCoord {
        self.current_tile
    }

    /// What this actor checks against when moving. Players respect NPCs only;
    /// NPCs respect everyone - matching `CollisionMap`'s documented OSRS semantics exactly.
    pub fn respects(&self) -> OccupancyMask {
        match self.kind {
            ActorKind::Player => CollisionMap::PLAYER_DEFAULT_RESPECTS,
            ActorKind::Npc => CollisionMap::NPC_DEFAULT_RESPECTS,
        }
    }

    /// What this actor writes into the occupancy overlay (over its whole footprint).
    pub fn occupies_as(&self) -> OccupancyMask {
        match self.kind {
            ActorKind::Player => OccupancyMask::PLAYER,
            ActorKind::Npc if self.blocks_players => OccupancyMask::NPC | OccupancyMask::BLOCKS_PLAYERS,
            ActorKind::Npc => OccupancyMask::NPC,
        }
    }

    pub(crate) fn set_blocks_players_flag(&mut self, blocks_players: bool) {
        self.blocks_players = blocks_players;
    }

    /// Shifts current -> previous and sets a new current tile. Passing the current tile again
    /// collapses previous == current (no interpolation for the view this tick).
    pub fn advance_tile(&mut self, tile: TileCoord) {
        self.previous_tile = self.current_tile;
        self.current_tile = tile;
    }

    /// The read-only snapshot other systems use to target this actor.
    pub fn view(&self) -> CombatTargetView {
        CombatTargetView {
            tile: self.current_tile,
            footprint_size: self.footprint_size,
            stats: self.stats,
            kind: self.kind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> CombatStats {
        CombatStats::new(1, 1, 1, 1, 1).unwrap()
    }

    #[test]
    fn new_actor_starts_with_previous_equal_to_current() {
        let start = TileCoord::new(3, 4);
        let actor = Actor::new(ActorKind::Npc, start, 1, 10, stats());

        assert_eq!(actor.previous_tile(), start);
        assert_eq!(actor.current_tile(), start);
        assert_eq!(actor.health().max(), 10);
    }

    #[test]
    fn advance_tile_shifts_current_into_previous() {
        let mut actor = Actor::new(ActorKind::Npc, TileCoord::new(0, 0), 1, 10, stats());

        actor.advance_tile(TileCoord::new(1, 0));
        assert_eq!(actor.previous_tile(), TileCoord::new(0, 0));
        assert_eq!(actor.current_tile(), TileCoord::new(1, 0));

        actor.advance_tile(TileCoord::new(1, 0)); // stalled: interpolation collapses
        assert_eq!(actor.previous_tile(), TileCoord::new(1, 0));
    }

    #[test]
    fn collision_semantics_follow_the_actor_kind() {
        let player = Actor::new(ActorKind::Player, TileCoord::new(0, 0), 1, 10, stats());
        let npc = Actor::new(ActorKind::Npc, TileCoord::new(0, 0), 1, 10, stats());

        assert_eq!(player.respects(), OccupancyMask::BLOCKS_PLAYERS);
        assert_eq!(player.occupies_as(), OccupancyMask::PLAYER);
        assert_eq!(npc.respects(), OccupancyMask::ALL);
        // a bare actor stands in players' way; data-defined monsters only with `blocks_movement`
        assert_eq!(npc.occupies_as(), OccupancyMask::NPC | OccupancyMask::BLOCKS_PLAYERS);
    }

    #[test]
    fn view_snapshots_the_current_tile() {
        let mut actor = Actor::new(ActorKind::Npc, TileCoord::new(0, 0), 3, 10, stats());
        actor.advance_tile(TileCoord::new(2, 2));

        let view = actor.view();

        assert_eq!(view.tile, TileCoord::new(2, 2));
        assert_eq!(view.footprint_size, 3);
        assert_eq!(view.kind, ActorKind::Npc);
    }
}
