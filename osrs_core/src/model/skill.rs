use serde::{Deserialize, Serialize};

/// Every skill, in the game's own stat order: `index()` is the stat number the client's scripts and
/// enums use (0 Attack, 1 Defence, 2 Strength, ... 8 Woodcutting, ...). Combat rules use the first
/// seven (reference §3); the rest only gain experience and gate actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skill {
    Attack,
    Defence,
    Strength,
    Hitpoints,
    Ranged,
    Prayer,
    Magic,
    Cooking,
    Woodcutting,
    Fletching,
    Fishing,
    Firemaking,
    Crafting,
    Smithing,
    Mining,
    Herblore,
    Agility,
    Thieving,
    Slayer,
    Farming,
    Runecraft,
    Hunter,
    Construction,
    Sailing,
}

impl Skill {
    pub const COUNT: usize = 24;

    pub const ALL: [Skill; Skill::COUNT] = [
        Skill::Attack,
        Skill::Defence,
        Skill::Strength,
        Skill::Hitpoints,
        Skill::Ranged,
        Skill::Prayer,
        Skill::Magic,
        Skill::Cooking,
        Skill::Woodcutting,
        Skill::Fletching,
        Skill::Fishing,
        Skill::Firemaking,
        Skill::Crafting,
        Skill::Smithing,
        Skill::Mining,
        Skill::Herblore,
        Skill::Agility,
        Skill::Thieving,
        Skill::Slayer,
        Skill::Farming,
        Skill::Runecraft,
        Skill::Hunter,
        Skill::Construction,
        Skill::Sailing,
    ];

    /// The game's stat number for this skill.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The skill with the game's stat number `stat`.
    pub fn from_index(stat: usize) -> Option<Skill> {
        Skill::ALL.get(stat).copied()
    }

    /// The level a new account starts on: 10 Hitpoints, 1 in everything else.
    pub fn starting_level(self) -> i32 {
        if self == Skill::Hitpoints {
            10
        } else {
            1
        }
    }

    /// The skill's name as the game writes it in messages ("Woodcutting").
    pub fn name(self) -> &'static str {
        match self {
            Skill::Attack => "Attack",
            Skill::Defence => "Defence",
            Skill::Strength => "Strength",
            Skill::Hitpoints => "Hitpoints",
            Skill::Ranged => "Ranged",
            Skill::Prayer => "Prayer",
            Skill::Magic => "Magic",
            Skill::Cooking => "Cooking",
            Skill::Woodcutting => "Woodcutting",
            Skill::Fletching => "Fletching",
            Skill::Fishing => "Fishing",
            Skill::Firemaking => "Firemaking",
            Skill::Crafting => "Crafting",
            Skill::Smithing => "Smithing",
            Skill::Mining => "Mining",
            Skill::Herblore => "Herblore",
            Skill::Agility => "Agility",
            Skill::Thieving => "Thieving",
            Skill::Slayer => "Slayer",
            Skill::Farming => "Farming",
            Skill::Runecraft => "Runecraft",
            Skill::Hunter => "Hunter",
            Skill::Construction => "Construction",
            Skill::Sailing => "Sailing",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_are_dense_and_ordered() {
        for (i, skill) in Skill::ALL.into_iter().enumerate() {
            assert_eq!(skill.index(), i);
            assert_eq!(Skill::from_index(i), Some(skill));
        }
        assert_eq!(Skill::from_index(Skill::COUNT), None);
    }

    #[test]
    fn indices_are_the_games_stat_numbers() {
        assert_eq!(Skill::Attack.index(), 0);
        assert_eq!(Skill::Defence.index(), 1);
        assert_eq!(Skill::Strength.index(), 2);
        assert_eq!(Skill::Hitpoints.index(), 3);
        assert_eq!(Skill::Magic.index(), 6);
        assert_eq!(Skill::Woodcutting.index(), 8);
        assert_eq!(Skill::Mining.index(), 14);
        assert_eq!(Skill::Construction.index(), 22);
    }
}
