//! What happened in the world, as a client would want to draw it: attacks being thrown, hits
//! landing (hit splats), things dying, experience gained. The sim records these as it resolves a tick; a
//! `Server` hands them to its client in the snapshot.

use crate::actors::EntityId;
use crate::model::Skill;

/// A fighter: the player or one of the world's NPCs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Combatant {
    Player,
    Npc(EntityId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldEvent {
    /// `attacker` threw an attack at `target`: its attack animation plays now. `animation` is the
    /// cache sequence of this particular attack when it has its own (else the attacker's usual
    /// attack animation). An attack with a telegraph is reported as its telegraph begins, so the
    /// animation is the warning; one without is reported as it resolves.
    Attack { attacker: Combatant, target: Combatant, animation: Option<u32> },
    /// `from` loosed a projectile (the cache graphic `graphic`) at `to`; it arrives in `ticks`.
    /// `draw` is the graphic shown on the shooter as it nocks and draws (an arrow in the bow).
    /// `released` says the shooter's wind-up has already played (a telegraphed attack, announced
    /// ticks earlier), so the projectile leaves now rather than partway through an attack animation.
    Projectile { from: Combatant, to: Combatant, graphic: u32, draw: Option<u32>, ticks: i32, released: bool },
    /// The cache graphic `graphic` plays on `on`, starting in `delay_ticks` (0 = now): the rocks of
    /// Jad's stomp, a spell's splash as it lands.
    Graphic { on: Combatant, graphic: u32, delay_ticks: i32 },
    /// `healer` restored `amount` hitpoints to `target`, playing the cache sequence `animation`.
    Heal { healer: Combatant, target: Combatant, amount: i32, animation: Option<u32> },
    /// A hit landed on `target`; 0 is a block.
    Hit { target: Combatant, damage: i32 },
    /// `who` has just died.
    Died { who: Combatant },
    /// The player has been put back in the world at its respawn point, alive again.
    Respawned,
    /// The cache sound effect `sound` (archive 4) is heard from `from`, starting in `delay_ticks`
    /// (0 = now). The server decides every sound, as the game's does: the client only plays them,
    /// quieter the further `from` is from the player.
    Sound { sound: u32, from: Combatant, delay_ticks: i32 },
    /// The player gained `gained_tenths` tenths of experience in `skill` (an XP drop).
    Experience { skill: Skill, gained_tenths: i32 },
    /// The player's base level in `skill` went up to `level`.
    LevelUp { skill: Skill, level: i32 },
}
