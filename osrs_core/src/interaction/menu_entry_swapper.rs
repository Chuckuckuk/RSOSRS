use std::borrow::Cow;
use std::collections::HashMap;

use super::menu_option::MenuOption;

type DefaultChangedHandler = Box<dyn FnMut(&str, &str)>;

/// Remembers, per swap key, which menu label the player promoted to the top ("entry swap"), and
/// reorders canonical menus to match. Persisting the choice is the host's job: it subscribes via
/// `on_default_changed` and feeds saved values back in with `load`.
#[derive(Default)]
pub struct MenuEntrySwapper {
    default_label_by_key: HashMap<String, String>,
    default_changed: Vec<DefaultChangedHandler>,
}

impl MenuEntrySwapper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribes to changes (the C# `DefaultChanged` event). Called with `(swap_key, label)`.
    pub fn on_default_changed(&mut self, handler: impl FnMut(&str, &str) + 'static) {
        self.default_changed.push(Box::new(handler));
    }

    /// Seeds an override from a previously persisted value, WITHOUT raising the changed event
    /// (so loading never writes the value straight back out). Empty labels are ignored.
    pub fn load(&mut self, swap_key: &str, label: &str) {
        if label.is_empty() {
            return;
        }
        self.default_label_by_key.insert(swap_key.to_owned(), label.to_owned());
    }

    /// Sets the preferred default label for a key, raising the changed event only if it differs
    /// from what is already stored.
    pub fn set_default(&mut self, swap_key: &str, label: &str) {
        if self.default_label_by_key.get(swap_key).is_some_and(|existing| existing == label) {
            return;
        }

        self.default_label_by_key.insert(swap_key.to_owned(), label.to_owned());
        for handler in &mut self.default_changed {
            handler(swap_key, label);
        }
    }

    /// Returns `canonical` untouched (borrowed, no allocation) when there is nothing to promote;
    /// otherwise a new list with the preferred option moved to index 0 and the rest in order.
    pub fn reorder<'a>(&self, swap_key: &str, canonical: &'a [MenuOption]) -> Cow<'a, [MenuOption]> {
        let Some(preferred_label) = self.default_label_by_key.get(swap_key) else {
            return Cow::Borrowed(canonical);
        };

        let index = canonical.iter().position(|option| option.label == *preferred_label);
        match index {
            // not found, or already the default - nothing to promote
            None | Some(0) => Cow::Borrowed(canonical),
            Some(index) => {
                let mut result = Vec::with_capacity(canonical.len());
                result.push(canonical[index].clone());
                for (i, option) in canonical.iter().enumerate() {
                    if i != index {
                        result.push(option.clone());
                    }
                }
                Cow::Owned(result)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    fn option(label: &str) -> MenuOption {
        MenuOption::new(label, || {})
    }

    fn labels(options: &[MenuOption]) -> Vec<&str> {
        options.iter().map(|o| o.label.as_str()).collect()
    }

    #[test]
    fn reorder_with_no_stored_override_returns_the_same_canonical_list() {
        let swapper = MenuEntrySwapper::new();
        let canonical = [option("Attack"), option("Examine")];

        let result = swapper.reorder("Goblin", &canonical);

        assert!(matches!(result, Cow::Borrowed(_)), "No override means no reason to allocate a new list at all.");
    }

    #[test]
    fn set_default_then_reorder_promotes_the_matching_label_to_index_zero() {
        let mut swapper = MenuEntrySwapper::new();
        let canonical = [option("Attack"), option("Examine"), option("Walk here")];
        swapper.set_default("Goblin", "Examine");

        let result = swapper.reorder("Goblin", &canonical);

        assert_eq!(labels(&result), vec!["Examine", "Attack", "Walk here"]);
    }

    #[test]
    fn set_default_with_a_label_already_at_index_zero_returns_the_same_canonical_list() {
        let mut swapper = MenuEntrySwapper::new();
        let canonical = [option("Attack"), option("Examine")];
        swapper.set_default("Goblin", "Attack");

        assert!(matches!(swapper.reorder("Goblin", &canonical), Cow::Borrowed(_)));
    }

    #[test]
    fn reorder_with_a_stale_override_naming_an_option_no_longer_present_returns_the_same_canonical_list() {
        let mut swapper = MenuEntrySwapper::new();
        swapper.set_default("Goblin", "Pickpocket");
        let canonical = [option("Attack"), option("Examine")];

        assert!(matches!(swapper.reorder("Goblin", &canonical), Cow::Borrowed(_)));
    }

    #[test]
    fn swap_keys_are_independent_setting_one_never_affects_another() {
        let mut swapper = MenuEntrySwapper::new();
        let canonical = [option("Attack"), option("Examine")];
        swapper.set_default("Goblin", "Examine");

        let cow_result = swapper.reorder("Cow", &canonical);

        assert!(matches!(cow_result, Cow::Borrowed(_)), "Cow never had a swap set, so it must be untouched by Goblin's.");
    }

    #[test]
    fn set_default_called_twice_with_the_same_label_does_not_raise_default_changed_the_second_time() {
        let mut swapper = MenuEntrySwapper::new();
        let raise_count = Rc::new(Cell::new(0));
        let counter = Rc::clone(&raise_count);
        swapper.on_default_changed(move |_, _| counter.set(counter.get() + 1));

        swapper.set_default("Goblin", "Examine");
        swapper.set_default("Goblin", "Examine");

        assert_eq!(raise_count.get(), 1, "A no-op set_default must not re-notify the host to re-persist an unchanged value.");
    }

    #[test]
    fn load_seeds_an_override_same_as_set_default_but_without_raising_default_changed() {
        let mut swapper = MenuEntrySwapper::new();
        let raise_count = Rc::new(Cell::new(0));
        let counter = Rc::clone(&raise_count);
        swapper.on_default_changed(move |_, _| counter.set(counter.get() + 1));

        swapper.load("Goblin", "Examine");
        let canonical = [option("Attack"), option("Examine")];
        let result = swapper.reorder("Goblin", &canonical);

        assert_eq!(result[0].label, "Examine");
        assert_eq!(raise_count.get(), 0, "Loading a previously-persisted value must not write it straight back out again.");
    }

    #[test]
    fn load_with_an_empty_label_is_a_no_op() {
        let mut swapper = MenuEntrySwapper::new();
        let canonical = [option("Attack"), option("Examine")];
        swapper.load("Goblin", "");

        assert!(matches!(swapper.reorder("Goblin", &canonical), Cow::Borrowed(_)));
    }

    #[test]
    fn choosing_an_option_invokes_its_action() {
        let fired = Rc::new(Cell::new(false));
        let flag = Rc::clone(&fired);
        let option = MenuOption::new("Attack", move || flag.set(true));

        (option.invoke)();

        assert!(fired.get());
    }
}
