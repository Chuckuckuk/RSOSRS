use std::rc::Rc;

use super::ground_target::{GroundTarget, HazardAnchor};
use super::hazard::HazardDef;
use super::position_resolver::PositionResolver;
use super::projectile_speed::ProjectileSpeed;
use super::special_attack::{SpecialContext, SpecialEffect, SpecialResolver};
use super::targeted_attack::TargetedAttack;
use crate::pathfinding::{line_of_sight, LineOfSight};
use crate::rng::Rng;

/// What a shot leaves where it lands: a scatter of hazards around the player.
#[derive(Debug, Clone, PartialEq)]
pub struct LandingHazards {
    pub def: Rc<HazardDef>,
    /// How far from the player the hazards may fall.
    pub radius: i32,
    pub min: usize,
    pub max: usize,
}

/// A shot that needs a clear line to the player. When the line is clear it is a normal rolled hit,
/// flying at `projectile_speed`, that may bind the player until it lands and leave hazards around
/// them. When something stands in the way it hits that instead: the blocker is destroyed and
/// `on_blocked` goes off on its tile.
///
/// It is a special because it checks more than the overhead prayer: whether it can see the player.
#[derive(Debug, Clone)]
pub struct ObstructedShot {
    /// The hit it makes when it gets through.
    pub shot: TargetedAttack,
    pub projectile_speed: ProjectileSpeed,
    /// Holds the player in place until the shot lands.
    pub binds_until_landing: bool,
    pub landing_hazards: Option<LandingHazards>,
    /// What goes off on the blocker's tile when the shot is stopped: aimed at the blocker, so it
    /// is anchored on the target tile of the context it is given.
    pub on_blocked: PositionResolver,
}

impl ObstructedShot {
    pub fn new(shot: TargetedAttack, projectile_speed: ProjectileSpeed, on_blocked: PositionResolver) -> Self {
        Self { shot, projectile_speed, binds_until_landing: false, landing_hazards: None, on_blocked }
    }

    pub fn binding_until_landing(mut self) -> Self {
        self.binds_until_landing = true;
        self
    }

    pub fn leaving_hazards(mut self, def: Rc<HazardDef>, radius: i32, min: usize, max: usize) -> Self {
        assert!(radius >= 0 && min >= 1 && max >= min, "a hazard scatter needs a radius and a count range");
        self.landing_hazards = Some(LandingHazards { def, radius, min, max });
        self
    }
}

impl SpecialResolver for ObstructedShot {
    fn resolve(&self, context: &SpecialContext, rng: &mut dyn Rng) -> Vec<SpecialEffect> {
        if !context.player_alive {
            return Vec::new();
        }
        match line_of_sight(context.attacker_centre, context.player_tile, context.sight_blockers) {
            LineOfSight::Blocked { at } => {
                // it hits what is in the way: the blocker goes, and its tile is where it goes off
                let aimed_at_blocker = SpecialContext { self_tile: at, target_tile: at, ..*context };
                let mut effects = self.on_blocked.resolve(&aimed_at_blocker, rng);
                effects.push(SpecialEffect::DestroyHazard { tile: at });
                effects
            }
            LineOfSight::Clear => {
                let distance = context.attacker_centre.chebyshev_distance_to(context.player_tile);
                let flight = self.projectile_speed.flight_ticks(distance);
                let damage = context.roll_shot(&self.shot, rng);
                let mut effects = vec![SpecialEffect::HurtPlayer { damage, delay_ticks: flight }];
                if self.binds_until_landing {
                    effects.push(SpecialEffect::BindPlayer { ticks: flight });
                }
                if let Some(landing) = &self.landing_hazards {
                    let area = GroundTarget::scatter_between(HazardAnchor::Target, landing.radius, landing.min, landing.max);
                    for tile in area.tiles(context.self_tile, context.player_tile, rng) {
                        effects.push(SpecialEffect::SpawnHazard { def: Rc::clone(&landing.def), tile, armed_in: flight });
                    }
                }
                effects
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{AttackStyle, CombatStats, GroundDamage, PlayerReactionState};
    use crate::rng::DotNetRandom;
    use crate::tiles::TileCoord;

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    fn stats(level: i32) -> CombatStats {
        CombatStats::new(level, level, level, level, level).unwrap()
    }

    fn lava() -> Rc<HazardDef> {
        Rc::new(HazardDef::new("Lava").hurting(5))
    }

    fn blast() -> PositionResolver {
        PositionResolver::new(GroundTarget::square(HazardAnchor::SelfTile, 1), GroundDamage::Fixed(120))
    }

    fn shot() -> ObstructedShot {
        let hit = TargetedAttack::new(AttackStyle::Magic).with_always_hits(true).with_max_hit(crate::combat::MaxHitSource::Fixed(40));
        ObstructedShot::new(hit, ProjectileSpeed::tiles_per_tick(3.0), blast())
            .binding_until_landing()
            .leaving_hazards(lava(), 2, 2, 4)
    }

    fn context<'a>(reaction: &'a PlayerReactionState, blockers: &'a [TileCoord]) -> SpecialContext<'a> {
        SpecialContext {
            self_tile: tc(0, 0),
            attacker_centre: tc(0, 0),
            attacker_stats: stats(99),
            target_tile: tc(9, 0),
            player_tile: tc(9, 0),
            player_alive: true,
            player_stats: stats(1),
            player_reaction: reaction,
            sight_blockers: blockers,
        }
    }

    #[test]
    fn with_a_clear_line_it_hurts_binds_and_leaves_hazards_after_the_flight() {
        let reaction = PlayerReactionState::default();
        let effects = shot().resolve(&context(&reaction, &[]), &mut DotNetRandom::new(3));

        // 9 tiles at 3 per tick is a 3-tick flight
        assert!(matches!(effects[0], SpecialEffect::HurtPlayer { delay_ticks: 3, .. }), "{effects:?}");
        assert_eq!(effects[1], SpecialEffect::BindPlayer { ticks: 3 });
        let lava_tiles: Vec<_> = effects
            .iter()
            .filter_map(|effect| match effect {
                SpecialEffect::SpawnHazard { tile, armed_in, .. } => Some((*tile, *armed_in)),
                _ => None,
            })
            .collect();
        assert!((2..=4).contains(&lava_tiles.len()), "{lava_tiles:?}");
        assert!(lava_tiles.iter().all(|(tile, armed)| *armed == 3 && tile.chebyshev_distance_to(tc(9, 0)) <= 2));
    }

    #[test]
    fn a_blocker_in_the_way_takes_the_hit_instead_and_is_destroyed() {
        let reaction = PlayerReactionState::default();
        // a boulder two tiles in front of the shooter; the player is 9 away and out of the blast
        let effects = shot().resolve(&context(&reaction, &[tc(2, 0)]), &mut DotNetRandom::new(3));

        assert_eq!(effects, vec![SpecialEffect::DestroyHazard { tile: tc(2, 0) }]);
    }

    #[test]
    fn a_player_standing_next_to_the_blocker_is_caught_in_the_blast() {
        let reaction = PlayerReactionState::default();
        let blockers = [tc(8, 0)];
        let cx = context(&reaction, &blockers);

        let effects = shot().resolve(&cx, &mut DotNetRandom::new(3));

        assert_eq!(
            effects,
            vec![SpecialEffect::HurtPlayer { damage: 120, delay_ticks: 0 }, SpecialEffect::DestroyHazard { tile: tc(8, 0) }]
        );
    }

    #[test]
    fn a_blocked_shot_is_not_a_hit_on_the_player_and_does_not_bind_or_leave_lava() {
        let reaction = PlayerReactionState::default();

        let effects = shot().resolve(&context(&reaction, &[tc(4, 0)]), &mut DotNetRandom::new(3));

        assert!(effects.iter().all(|e| !matches!(e, SpecialEffect::BindPlayer { .. } | SpecialEffect::SpawnHazard { .. })));
    }
}
