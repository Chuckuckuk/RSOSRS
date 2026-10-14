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
    /// Must be standing ON the anchor tile: Mad Angel's blast bounce, a Moons sigil, a Yama glyph.
    StandOnTile(HazardAnchor),
    /// Hit unless more than `radius` tiles (Chebyshev) from the anchor, so a 3x3 AoE is radius 1:
    /// Echo Amoxliatl's black icicle, Araxxor's acid splash, a Duke Sucellus gas vent.
    MoveOutsideArea { anchor: HazardAnchor, radius: i32 },
    /// Damage fades with distance from the anchor. `rings_percent[d]` is the percentage of full
    /// damage taken at Chebyshev distance `d`; anything past the last ring uses the last value.
    /// Yama's meteor (80 / 40 / 0), Araxxor's ruptura, Yama's glyph of fire.
    DistanceFalloff { anchor: HazardAnchor, rings_percent: [u8; 4] },
    /// Avoided only from behind a blocker: Duke Sucellus's Freezing Gaze, a Whisperer screech
    /// pillar. Needs a line-of-sight query, so it is recognised but not resolved yet.
    BehindCover,
    /// Avoided only by not standing on the given side of the attacker (relative to its facing):
    /// Mad Angel's sweep. Needs the attacker's facing, so recognised but not resolved yet.
    AvoidSide(RelativeSide),
    /// Avoided by finishing a task before a deadline: clicking Sol's called equipment slot,
    /// destroying Vardorvis's strangle spores, killing Phosani's husks. Recognised but not
    /// resolved yet; it needs the deadline-and-consequence primitive.
    Objective { kind: ObjectiveKind, window_ticks: i32 },
}

/// A side of an attacker, relative to the way it is facing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativeSide {
    Front,
    Back,
    Left,
    Right,
}

/// What an `Objective` reaction asks the player to complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectiveKind {
    /// Click the equipment slot the boss called out (Sol Heredit's grapple).
    ClickEquipmentSlot,
    /// Destroy `count` spawned objects (Vardorvis's spores, Phosani's husks).
    DestroyTargets { count: i32 },
    /// Step on every marked tile (Whisperer's light-green seeds).
    StepOnTargets,
}

impl ReactionKind {
    /// The tile this reaction's hazard is anchored to, for the kinds that have one.
    pub fn anchor(&self) -> Option<HazardAnchor> {
        match *self {
            ReactionKind::MoveOffTile(anchor)
            | ReactionKind::StandOnTile(anchor)
            | ReactionKind::MoveOutsideArea { anchor, .. }
            | ReactionKind::DistanceFalloff { anchor, .. } => Some(anchor),
            _ => None,
        }
    }
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

    pub fn stand_on_tile(anchor: HazardAnchor) -> Self {
        Self { kind: ReactionKind::StandOnTile(anchor), ..Self::default() }
    }

    pub fn move_outside_area(anchor: HazardAnchor, radius: i32) -> Self {
        assert!(radius >= 0, "radius must not be negative");
        Self { kind: ReactionKind::MoveOutsideArea { anchor, radius }, ..Self::default() }
    }

    pub fn distance_falloff(anchor: HazardAnchor, rings_percent: [u8; 4]) -> Self {
        assert!(rings_percent.iter().all(|&p| p <= 100), "ring percentages must be within [0, 100]");
        Self { kind: ReactionKind::DistanceFalloff { anchor, rings_percent }, ..Self::default() }
    }

    pub fn behind_cover() -> Self {
        Self { kind: ReactionKind::BehindCover, ..Self::default() }
    }

    pub fn avoid_side(side: RelativeSide) -> Self {
        Self { kind: ReactionKind::AvoidSide(side), ..Self::default() }
    }

    pub fn objective(kind: ObjectiveKind, window_ticks: i32) -> Self {
        assert!(window_ticks > 0, "window_ticks must be positive");
        Self { kind: ReactionKind::Objective { kind, window_ticks }, ..Self::default() }
    }
}
