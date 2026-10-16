/// What lets a defender cut a standard (targeted) attack down, to
/// `TargetedAttack::prayer_penetration` of its damage (0 blocks it entirely, in between pierces,
/// 1 does nothing).
///
/// A standard attack is only ever answered by an overhead protection prayer. An attack that must
/// check anything else (a different state of the defender, where it stands) is a `SpecialAttack`,
/// which resolves itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mitigation {
    /// The protection prayer that matches the attack's style, which is how every melee, ranged and
    /// magic attack is answered unless it says otherwise.
    #[default]
    ProtectionPrayer,
    /// No prayer changes the damage (Fragment of Seren's nuke): a typeless attack.
    Unmitigable,
}

impl Mitigation {
    /// Whether the defender is praying against an attack of `style`.
    pub fn is_met(
        self,
        style: super::attack_style::AttackStyle,
        state: Option<&super::player_reaction_state::PlayerReactionState>,
    ) -> bool {
        use super::protection_prayer::ProtectionPrayer;
        match (self, state) {
            (Mitigation::ProtectionPrayer, Some(state)) => state.active_prayer == ProtectionPrayer::against(style),
            _ => false,
        }
    }
}
