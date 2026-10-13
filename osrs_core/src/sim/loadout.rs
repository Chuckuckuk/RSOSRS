use std::collections::BTreeMap;

use crate::data::{EquipSlot, ItemDef, Registry, SetDef, WeaponDef};
use crate::model::{EquipmentBonuses, Modifier};

/// What a player is wearing: item ids by slot, resolved against a `Registry` whenever a total is
/// needed so the loadout itself stays tiny and cannot go stale.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loadout {
    equipped: BTreeMap<EquipSlot, String>,
}

impl Loadout {
    pub fn new() -> Self {
        Self::default()
    }

    /// Equips `item_id` into its slot, replacing whatever was there.
    pub fn equip(&mut self, registry: &Registry, item_id: &str) -> Result<(), String> {
        let item = registry.item(item_id).ok_or_else(|| format!("unknown item '{item_id}'"))?;
        let slot = item.slot.ok_or_else(|| format!("{} cannot be worn", item.name))?;
        self.equipped.insert(slot, item.id.clone());
        Ok(())
    }

    /// Removes and returns the item id in `slot`.
    pub fn unequip(&mut self, slot: EquipSlot) -> Option<String> {
        self.equipped.remove(&slot)
    }

    /// Every worn item with its slot, in slot order.
    pub fn slots(&self) -> impl Iterator<Item = (EquipSlot, &str)> {
        self.equipped.iter().map(|(slot, id)| (*slot, id.as_str()))
    }

    pub fn item_id_in(&self, slot: EquipSlot) -> Option<&str> {
        self.equipped.get(&slot).map(String::as_str)
    }

    pub fn items<'a>(&'a self, registry: &'a Registry) -> impl Iterator<Item = &'a ItemDef> {
        self.equipped.values().filter_map(|id| registry.item(id))
    }

    pub fn weapon_item<'a>(&self, registry: &'a Registry) -> Option<&'a ItemDef> {
        self.equipped.get(&EquipSlot::Weapon).and_then(|id| registry.item(id)).filter(|item| item.weapon.is_some())
    }

    pub fn weapon<'a>(&self, registry: &'a Registry) -> Option<&'a WeaponDef> {
        self.weapon_item(registry).and_then(|item| item.weapon.as_ref())
    }

    pub fn ammo_item<'a>(&self, registry: &'a Registry) -> Option<&'a ItemDef> {
        self.equipped.get(&EquipSlot::Ammo).and_then(|id| registry.item(id))
    }

    pub fn has_tag(&self, registry: &Registry, tag: &str) -> bool {
        self.items(registry).any(|item| item.has_tag(tag))
    }

    /// Sets whose every piece is currently worn.
    pub fn active_sets<'a>(&'a self, registry: &'a Registry) -> Vec<&'a SetDef> {
        registry
            .sets()
            .filter(|set| {
                !set.pieces.is_empty()
                    && set.pieces.iter().all(|alternatives| alternatives.iter().any(|id| self.equipped.values().any(|worn| worn == id)))
            })
            .collect()
    }

    pub fn has_set(&self, registry: &Registry, set_id: &str) -> bool {
        self.active_sets(registry).iter().any(|set| set.id == set_id)
    }

    /// Item bonuses plus the bonuses of every complete set.
    pub fn totals(&self, registry: &Registry) -> EquipmentBonuses {
        let mut total = EquipmentBonuses::sum(self.items(registry).map(|item| &item.bonuses));
        for set in self.active_sets(registry) {
            total.add(&set.bonuses);
        }
        total
    }

    /// Every always-on modifier from worn items and complete sets, in slot order.
    pub fn modifiers(&self, registry: &Registry) -> Vec<Modifier> {
        let mut modifiers: Vec<Modifier> = self.items(registry).flat_map(|item| item.modifiers.iter().cloned()).collect();
        for set in self.active_sets(registry) {
            modifiers.extend(set.modifiers.iter().cloned());
        }
        modifiers
    }

    /// Total carried weight in kg, never below zero (negative-weight gear behaves as 0 kg overall).
    pub fn weight_kg(&self, registry: &Registry) -> f64 {
        self.items(registry).map(|item| item.weight_kg).sum::<f64>().max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Registry {
        Registry::builtin()
    }

    #[test]
    fn equipping_sums_bonuses_across_slots() {
        let registry = registry();
        let mut loadout = Loadout::new();
        loadout.equip(&registry, "abyssal_whip").unwrap();
        loadout.equip(&registry, "amulet_of_torture").unwrap();
        loadout.equip(&registry, "berserker_ring_i").unwrap();

        let totals = loadout.totals(&registry);

        assert_eq!(totals.melee_strength, 82 + 10 + 8);
        assert_eq!(totals.attack.slash, 82 + 15);
    }

    #[test]
    fn a_new_item_replaces_the_old_one_in_its_slot() {
        let registry = registry();
        let mut loadout = Loadout::new();
        loadout.equip(&registry, "abyssal_whip").unwrap();
        loadout.equip(&registry, "dragon_dagger").unwrap();

        assert_eq!(loadout.item_id_in(EquipSlot::Weapon), Some("dragon_dagger"));
        assert_eq!(loadout.totals(&registry).melee_strength, 40);
    }

    #[test]
    fn unknown_items_are_rejected() {
        assert!(Loadout::new().equip(&registry(), "bogus").is_err());
    }

    #[test]
    fn a_set_only_activates_when_every_piece_is_worn_and_accepts_alternatives() {
        let registry = registry();
        let mut loadout = Loadout::new();
        for id in ["void_knight_top", "void_knight_robe", "void_knight_gloves"] {
            loadout.equip(&registry, id).unwrap();
        }
        assert!(!loadout.has_set(&registry, "void_melee"));

        loadout.equip(&registry, "void_melee_helm").unwrap();
        assert!(loadout.has_set(&registry, "void_melee"));
        assert!(!loadout.has_set(&registry, "void_ranged"));

        loadout.equip(&registry, "elite_void_top").unwrap(); // an alternative for the top piece
        assert!(loadout.has_set(&registry, "void_melee"));
    }

    #[test]
    fn set_modifiers_join_the_item_modifiers() {
        let registry = registry();
        let mut loadout = Loadout::new();
        for id in ["void_knight_top", "void_knight_robe", "void_knight_gloves", "void_melee_helm", "salve_amulet_ei"] {
            loadout.equip(&registry, id).unwrap();
        }

        let modifiers = loadout.modifiers(&registry);

        assert!(modifiers.iter().any(|m| m.target == crate::model::ModifierTarget::AccuracyLevel));
        assert!(modifiers.iter().any(|m| m.group.as_deref() == Some("slayer_or_salve")));
    }

    #[test]
    fn tags_and_weapon_lookup_work() {
        let registry = registry();
        let mut loadout = Loadout::new();
        loadout.equip(&registry, "regen_bracelet").unwrap();
        loadout.equip(&registry, "toxic_blowpipe").unwrap();

        assert!(loadout.has_tag(&registry, "regen_bracelet"));
        assert!(!loadout.has_tag(&registry, "lightbearer"));
        assert_eq!(loadout.weapon(&registry).unwrap().category, "blowpipe");
    }
}
