use super::protection_prayer::ProtectionPrayer;

/// What the player must do to avoid (or reduce) an attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReactionKind {
    #[default]
    None,
    PrayerMatch,
    MoveToTile,
    Interrupt,
}

/// Which tile a `MoveToTile` hazard is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HazardAnchor {
    /// The attacker's own tile at the moment the attack was selected.
    #[default]
    SelfTile,
    /// The target's tile at the moment the attack was selected.
    Target,
}

/// The reaction an attack asks of the player. `Default` is `None()` - the same all-zero value the
/// C# `default(ReactionRequirement)` produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReactionRequirement {
    pub kind: ReactionKind,
    pub required_prayer: ProtectionPrayer,
    pub move_away_distance: i32,
    pub anchor: HazardAnchor,
}

impl ReactionRequirement {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn prayer_match(required_prayer: ProtectionPrayer) -> Self {
        Self { kind: ReactionKind::PrayerMatch, required_prayer, ..Self::default() }
    }

    pub fn move_to_tile(move_away_distance: i32, anchor: HazardAnchor) -> Self {
        Self { kind: ReactionKind::MoveToTile, move_away_distance, anchor, ..Self::default() }
    }

    pub fn interrupt() -> Self {
        Self { kind: ReactionKind::Interrupt, ..Self::default() }
    }
}
