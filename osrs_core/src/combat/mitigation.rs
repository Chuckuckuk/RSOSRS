use super::player_reaction_state::PlayerReactionState;
use super::protection_prayer::ProtectionPrayer;

/// A state of the defender that an attack can be conditional on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenderCondition {
    /// This protection prayer is active.
    ProtectionPrayer(ProtectionPrayer),
    /// No protection prayer is active (Maggot King's screech, Echo Hunllef's black orb).
    NoProtectionPrayer,
}

impl DefenderCondition {
    pub fn holds(self, state: &PlayerReactionState) -> bool {
        match self {
            DefenderCondition::ProtectionPrayer(prayer) => prayer != ProtectionPrayer::None && state.active_prayer == prayer,
            DefenderCondition::NoProtectionPrayer => state.active_prayer == ProtectionPrayer::None,
        }
    }
}

/// What lets a defender cut a targeted attack down, to `TargetedAttack::prayer_penetration` of its
/// damage (0 blocks it entirely, in between pierces, 1 does nothing).
///
/// Area attacks have no mitigation of this kind: they hit whoever stands on their tiles
/// (`GroundAttack`), so what avoids them is where the defender is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mitigation {
    /// The protection prayer that matches the attack's style, which is how every melee, ranged and
    /// magic attack is answered unless it says otherwise.
    #[default]
    ProtectionPrayer,
    /// A condition of the defender other than the matching prayer.
    When(DefenderCondition),
    /// Nothing the defender does changes the damage (Fragment of Seren's nuke): a typeless attack.
    Unmitigable,
}

impl Mitigation {
    /// Whether the defender is doing what cuts an attack of `style` down.
    pub fn is_met(self, style: super::attack_style::AttackStyle, state: Option<&PlayerReactionState>) -> bool {
        let Some(state) = state else { return false };
        match self {
            Mitigation::ProtectionPrayer => DefenderCondition::ProtectionPrayer(ProtectionPrayer::against(style)).holds(state),
            Mitigation::When(condition) => condition.holds(state),
            Mitigation::Unmitigable => false,
        }
    }
}
