use serde::{Deserialize, Serialize};

/// The seven combat-relevant skills (reference §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skill {
    Attack,
    Strength,
    Defence,
    Ranged,
    Magic,
    Prayer,
    Hitpoints,
}

impl Skill {
    pub const ALL: [Skill; 7] =
        [Skill::Attack, Skill::Strength, Skill::Defence, Skill::Ranged, Skill::Magic, Skill::Prayer, Skill::Hitpoints];

    pub fn index(self) -> usize {
        self as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_are_dense_and_ordered() {
        for (i, skill) in Skill::ALL.into_iter().enumerate() {
            assert_eq!(skill.index(), i);
        }
    }
}
