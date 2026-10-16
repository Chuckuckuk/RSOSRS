use super::attack_style::AttackStyle;

/// Which protection prayer is active (or `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProtectionPrayer {
    #[default]
    None,
    Melee,
    Ranged,
    Magic,
}

impl ProtectionPrayer {
    /// The protection prayer that answers attacks of `style`.
    pub fn against(style: AttackStyle) -> Self {
        match style {
            AttackStyle::Melee => ProtectionPrayer::Melee,
            AttackStyle::Ranged => ProtectionPrayer::Ranged,
            AttackStyle::Magic => ProtectionPrayer::Magic,
        }
    }
}
