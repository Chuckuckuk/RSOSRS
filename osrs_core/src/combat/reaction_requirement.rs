use super::protection_prayer::ProtectionPrayer;

/// What the player must do to avoid (or reduce) an attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReactionKind {
    #[default]
    None,
    PrayerMatch(ProtectionPrayer),
    DisablePrayer,
    MoveOffTile(HazardAnchor), // which tile the hazard is anchored to
    Interrupt,
}

/// Which tile a `MoveToTile` or `MoveOffTile` hazard is anchored to.
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
}

impl ReactionRequirement {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn prayer_match(required_prayer: ProtectionPrayer) -> Self {
        Self { kind: ReactionKind::PrayerMatch(required_prayer), ..Self::default() }
    }

    pub fn disable_prayer() -> Self {
        Self { kind: ReactionKind::DisablePrayer, ..Self::default() }
    }

    pub fn move_off_tile(anchor: HazardAnchor) -> Self {
        Self { kind: ReactionKind::MoveOffTile(anchor), ..Self::default() }
    }

    pub fn interrupt() -> Self {
        Self { kind: ReactionKind::Interrupt, ..Self::default() }
    }
}
