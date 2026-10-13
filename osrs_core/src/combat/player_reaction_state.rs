use super::attack_style::AttackStyle;
use super::protection_prayer::ProtectionPrayer;

/// What the player is currently doing that an incoming attack's reaction requirement is checked
/// against. Read live at the tick an attack rolls, never earlier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerReactionState {
    pub active_prayer: ProtectionPrayer,
    pub combat_style: AttackStyle,
}

impl Default for PlayerReactionState {
    fn default() -> Self {
        Self { active_prayer: ProtectionPrayer::None, combat_style: AttackStyle::Melee }
    }
}
