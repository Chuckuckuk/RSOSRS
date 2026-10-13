use serde::{Deserialize, Serialize};

use crate::combat::AttackStyle;
use crate::model::{Modifier, PrayerEffects};

/// A prayer (reference §3 prayer table). Boost percentages live in `effects`; `drain_effect` is
/// the wiki's "drain effect" number that feeds `rules::PrayerDrainCounter`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrayerDef {
    pub id: String,
    pub name: String,
    pub level: i32,
    pub effects: PrayerEffects,
    pub drain_effect: i32,
    /// Activating a prayer switches off every active prayer that shares one of its groups
    /// (`attack`, `strength`, `defence`, `ranged`, `magic`, `overhead`, ...).
    pub groups: Vec<String>,
    /// Set for the three protection prayers.
    pub protects: Option<AttackStyle>,
    /// Extra conditional effects while active.
    pub modifiers: Vec<Modifier>,
    /// `rapid_heal`, `smite`, `redemption`, `retribution`, ...
    pub tags: Vec<String>,
    /// The cache sound effect heard as it is switched on.
    pub sound: Option<u32>,
    /// Free-form provenance (wiki page, whether the numbers are verified).
    pub notes: String,
}

impl Default for PrayerDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            level: 1,
            effects: PrayerEffects::default(),
            drain_effect: 1,
            groups: Vec::new(),
            protects: None,
            modifiers: Vec::new(),
            sound: None,
            tags: Vec::new(),
            notes: String::new(),
        }
    }
}

impl PrayerDef {
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piety_parses() {
        let prayer: PrayerDef = serde_json::from_str(
            r#"{"id":"piety","name":"Piety","level":70,"drain_effect":24,
                "groups":["attack","strength","defence"],
                "effects":{"attack_percent":120,"strength_percent":123,"defence_percent":125}}"#,
        )
        .unwrap();

        assert_eq!(prayer.effects.strength_percent, 123);
        assert_eq!(prayer.effects.magic_percent, 100);
        assert_eq!(prayer.drain_effect, 24);
    }
}
