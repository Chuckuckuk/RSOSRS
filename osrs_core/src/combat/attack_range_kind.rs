/// How an attack decides whether its attacker is "in range" of the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackRangeKind {
    /// Orthogonally adjacent to the attacker's footprint.
    Melee,
    /// Within a Chebyshev distance of the attacker's tile.
    Ranged,
}
