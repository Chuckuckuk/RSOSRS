use super::menu_option::MenuOption;

/// Something the player can right-click for a menu of options (the C# `IInteractable`).
pub trait Interactable {
    /// Key under which a per-target default-option swap is remembered (e.g. an NPC's name).
    fn swap_key(&self) -> &str;

    /// The canonical (un-swapped) menu, default option first.
    fn options(&self) -> Vec<MenuOption>;
}
