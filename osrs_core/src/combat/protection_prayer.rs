/// Which protection prayer is active (or `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProtectionPrayer {
    #[default]
    None,
    Melee,
    Ranged,
    Magic,
}
