//! The player's inventory: 28 slots, each empty or a stack of one kind of item.

pub const INVENTORY_SIZE: usize = 28;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemStack {
    pub item: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    slots: Vec<Option<ItemStack>>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

impl Inventory {
    pub fn new() -> Self {
        Self { slots: vec![None; INVENTORY_SIZE] }
    }

    pub fn slots(&self) -> &[Option<ItemStack>] {
        &self.slots
    }

    pub fn get(&self, slot: usize) -> Option<&ItemStack> {
        self.slots.get(slot).and_then(Option::as_ref)
    }

    pub fn free_slots(&self) -> usize {
        self.slots.iter().filter(|s| s.is_none()).count()
    }

    /// How many of `item` are held, over every slot.
    pub fn count(&self, item: &str) -> u32 {
        self.slots.iter().flatten().filter(|s| s.item == item).map(|s| s.quantity).sum()
    }

    /// Adds `quantity` of `item`: onto an existing stack when it `stackable`, else into free slots
    /// (one per item). Returns how many did not fit.
    pub fn add(&mut self, item: &str, quantity: u32, stackable: bool) -> u32 {
        if quantity == 0 {
            return 0;
        }
        if stackable {
            if let Some(stack) = self.slots.iter_mut().flatten().find(|s| s.item == item) {
                let room = u32::MAX - stack.quantity;
                let added = quantity.min(room);
                stack.quantity += added;
                return quantity - added;
            }
            return match self.slots.iter_mut().find(|s| s.is_none()) {
                Some(slot) => {
                    *slot = Some(ItemStack { item: item.to_owned(), quantity });
                    0
                }
                None => quantity,
            };
        }
        let mut left = quantity;
        for slot in self.slots.iter_mut().filter(|s| s.is_none()) {
            if left == 0 {
                break;
            }
            *slot = Some(ItemStack { item: item.to_owned(), quantity: 1 });
            left -= 1;
        }
        left
    }

    /// Puts `item` into exactly `slot` if it is empty.
    pub fn put(&mut self, slot: usize, stack: ItemStack) -> Result<(), ItemStack> {
        match self.slots.get_mut(slot) {
            Some(place @ None) => {
                *place = Some(stack);
                Ok(())
            }
            _ => Err(stack),
        }
    }

    /// Empties `slot`, returning what was in it.
    pub fn take(&mut self, slot: usize) -> Option<ItemStack> {
        self.slots.get_mut(slot).and_then(Option::take)
    }

    /// Takes one item out of the stack in `slot` (the whole stack when it is the last one).
    pub fn take_one(&mut self, slot: usize) -> Option<String> {
        let stack = self.slots.get_mut(slot)?.as_mut()?;
        let item = stack.item.clone();
        if stack.quantity > 1 {
            stack.quantity -= 1;
        } else {
            self.slots[slot] = None;
        }
        Some(item)
    }

    /// Swaps two slots (a drag in the interface).
    pub fn swap(&mut self, a: usize, b: usize) {
        if a < self.slots.len() && b < self.slots.len() {
            self.slots.swap(a, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_items_fill_slots_and_report_what_did_not_fit() {
        let mut inventory = Inventory::new();
        assert_eq!(inventory.add("shark", 30, false), 2);
        assert_eq!(inventory.free_slots(), 0);
        assert_eq!(inventory.count("shark"), 28);
    }

    #[test]
    fn a_stackable_joins_its_stack() {
        let mut inventory = Inventory::new();
        inventory.add("bronze_arrow", 10, true);
        inventory.add("bronze_arrow", 15, true);
        assert_eq!(inventory.free_slots(), 27);
        assert_eq!(inventory.get(0), Some(&ItemStack { item: "bronze_arrow".into(), quantity: 25 }));
    }

    #[test]
    fn taking_one_empties_the_slot_on_the_last() {
        let mut inventory = Inventory::new();
        inventory.add("shark", 1, false);
        assert_eq!(inventory.take_one(0).as_deref(), Some("shark"));
        assert!(inventory.get(0).is_none());
        assert_eq!(inventory.take_one(0), None);
    }

    #[test]
    fn swapping_exchanges_slots() {
        let mut inventory = Inventory::new();
        inventory.add("a", 1, false);
        inventory.add("b", 1, false);
        inventory.swap(0, 5);
        assert!(inventory.get(0).is_none());
        assert_eq!(inventory.get(5).map(|s| s.item.as_str()), Some("a"));
    }
}
