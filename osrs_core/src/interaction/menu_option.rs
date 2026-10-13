use std::fmt;
use std::rc::Rc;

/// One entry of an interaction menu: a label and what choosing it does.
#[derive(Clone)]
pub struct MenuOption {
    pub label: String,
    pub invoke: Rc<dyn Fn()>,
}

impl MenuOption {
    pub fn new(label: impl Into<String>, invoke: impl Fn() + 'static) -> Self {
        Self { label: label.into(), invoke: Rc::new(invoke) }
    }
}

impl fmt::Debug for MenuOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MenuOption").field("label", &self.label).finish_non_exhaustive()
    }
}
