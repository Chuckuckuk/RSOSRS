//! The shared vocabulary of the simulator: damage types, equipment bonuses, monster attributes,
//! prayer/stance effects and the `Modifier` primitive that expresses every conditional gear,
//! prayer, set or special-attack effect. Plain `Copy`/`Clone` data with serde support, so the
//! `data` layer can load it and the `sim` layer can consume it.

mod attributes;
mod bonuses;
mod effects;
mod modifier;
mod skill;

pub use attributes::{Attribute, AttributeSet};
pub use bonuses::{DamageType, EquipmentBonuses, StyleBonuses};
pub use effects::{PrayerEffects, StanceBonuses};
pub use modifier::{Condition, ConditionContext, Modifier, ModifierScaling, ModifierTarget, ScalingContext};
pub use skill::Skill;
