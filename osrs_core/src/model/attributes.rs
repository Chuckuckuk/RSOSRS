use serde::{Deserialize, Serialize};

/// Monster attribute tags (reference §4): categorical, not stats - specific gear is written to
/// exploit them via `Condition::TargetHas`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribute {
    Undead,
    Demon,
    Draconic,
    Kalphite,
    Leafy,
    Golem,
    Shade,
    Spectral,
    VampyreTier1,
    VampyreTier2,
    VampyreTier3,
    Fiery,
    Flying,
    Xerician,
}

const ALL: [Attribute; 14] = [
    Attribute::Undead,
    Attribute::Demon,
    Attribute::Draconic,
    Attribute::Kalphite,
    Attribute::Leafy,
    Attribute::Golem,
    Attribute::Shade,
    Attribute::Spectral,
    Attribute::VampyreTier1,
    Attribute::VampyreTier2,
    Attribute::VampyreTier3,
    Attribute::Fiery,
    Attribute::Flying,
    Attribute::Xerician,
];

impl Attribute {
    fn bit(self) -> u32 {
        1 << (self as u32)
    }
}

/// A small `Copy` set of attributes (so it can ride inside `CombatStats`). Serialises as a list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<Attribute>", into = "Vec<Attribute>")]
pub struct AttributeSet(u32);

impl AttributeSet {
    pub fn with(mut self, attribute: Attribute) -> Self {
        self.0 |= attribute.bit();
        self
    }

    pub fn contains(self, attribute: Attribute) -> bool {
        self.0 & attribute.bit() != 0
    }

    pub fn iter(self) -> impl Iterator<Item = Attribute> {
        ALL.into_iter().filter(move |attribute| self.contains(*attribute))
    }
}

impl From<Vec<Attribute>> for AttributeSet {
    fn from(list: Vec<Attribute>) -> Self {
        list.into_iter().fold(Self::default(), Self::with)
    }
}

impl From<AttributeSet> for Vec<Attribute> {
    fn from(set: AttributeSet) -> Self {
        set.iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_membership() {
        let set = AttributeSet::default().with(Attribute::Undead).with(Attribute::Fiery);
        assert!(set.contains(Attribute::Undead));
        assert!(set.contains(Attribute::Fiery));
        assert!(!set.contains(Attribute::Demon));
    }

    #[test]
    fn json_round_trip_is_a_plain_list() {
        let set: AttributeSet = serde_json::from_str(r#"["undead","vampyre_tier2"]"#).unwrap();
        assert!(set.contains(Attribute::VampyreTier2));
        assert_eq!(serde_json::to_string(&set).unwrap(), r#"["undead","vampyre_tier2"]"#);
    }
}
