/// What an attack's damage type is, for reaction-matching purposes (Stage 2's
/// `Mitigation` checks this against the target's active protection).
///
/// Deliberately only three values - "typeless" is NOT a real 4th style. Per OSRS wiki
/// research, it's old terminology for an attack with 100% prayer penetration: it still rolls
/// accuracy against the normal defensive stat of whichever real style it is, it just ignores
/// protection prayers' blocking. See `AttackDefinition::prayer_penetration` for that property -
/// modelling it as a style value here would conflate "what style is this" with "does it
/// bypass prayer," which are independent axes (a melee OR magic attack can pierce prayer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttackStyle {
    Melee,
    Ranged,
    Magic,
}
