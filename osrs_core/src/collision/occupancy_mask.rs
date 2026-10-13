use std::ops::{BitAnd, BitOr};

/// Which kinds of mover occupy / respect a tile (the C# `[Flags] enum OccupancyMask`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OccupancyMask(u8);

impl OccupancyMask {
    pub const NONE: OccupancyMask = OccupancyMask(0);
    /// Every NPC: NPCs never walk through one another.
    pub const NPC: OccupancyMask = OccupancyMask(1 << 0);
    pub const PLAYER: OccupancyMask = OccupancyMask(1 << 1);
    /// An NPC that also stands in a player's way (`blocks_movement`); players walk through the rest.
    pub const BLOCKS_PLAYERS: OccupancyMask = OccupancyMask(1 << 2);
    /// What an NPC respects: other NPCs and players.
    pub const ALL: OccupancyMask = OccupancyMask(1 << 0 | 1 << 1);

    /// True if any bit of `other` is set in `self` (the C# `(mask & other) != 0`).
    pub fn intersects(self, other: OccupancyMask) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for OccupancyMask {
    type Output = OccupancyMask;

    fn bitor(self, rhs: OccupancyMask) -> OccupancyMask {
        OccupancyMask(self.0 | rhs.0)
    }
}

impl BitAnd for OccupancyMask {
    type Output = OccupancyMask;

    fn bitand(self, rhs: OccupancyMask) -> OccupancyMask {
        OccupancyMask(self.0 & rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_is_npc_or_player() {
        assert_eq!(OccupancyMask::ALL, OccupancyMask::NPC | OccupancyMask::PLAYER);
    }

    #[test]
    fn intersects_checks_for_any_shared_bit() {
        assert!(OccupancyMask::ALL.intersects(OccupancyMask::NPC));
        assert!(OccupancyMask::ALL.intersects(OccupancyMask::PLAYER));
        assert!(!OccupancyMask::NPC.intersects(OccupancyMask::PLAYER));
        assert!(!OccupancyMask::NONE.intersects(OccupancyMask::ALL));
    }
}
