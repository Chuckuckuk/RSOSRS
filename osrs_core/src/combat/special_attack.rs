use super::ground_attack::GroundAttack;

/// An attack that is not one of the three standard timings (instant, telegraphed, on-contact) or
/// that needs to check anything besides the overhead protection prayer. A standard attack is
/// rolled against one defender and answered by its protection prayer, nothing more; everything
/// else is a special, and each kind of special carries the data and the resolution of its own.
#[derive(Debug, Clone)]
pub enum SpecialAttack {
    /// Aimed at tiles, resolved against whoever stands on them when it lands; never rolls accuracy.
    Ground(GroundAttack),
}

impl SpecialAttack {
    /// Ticks of visible wind-up before this special resolves.
    pub fn telegraph_ticks(&self) -> i32 {
        match self {
            SpecialAttack::Ground(ground) => ground.telegraph_ticks,
        }
    }
}
