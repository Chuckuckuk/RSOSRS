//! Gathering skills in data form: the tools (axes, later pickaxes and harpoons), the
//! requirements an action checks, and the resource nodes (trees, later rocks and fishing spots)
//! the player gathers from. `sim::gathering` runs them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::Skill;

/// A tool a gathering skill uses, keyed by its item. The player may carry it or wield it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDef {
    /// The item id (`"bronze_axe"`).
    pub item: String,
    /// What kind of tool it is (`"axe"`); a node names the kind it needs.
    pub category: String,
    pub skill: Skill,
    /// The level in `skill` needed to use it.
    pub level: i32,
    /// The cache sequence the player loops while using it.
    #[serde(default)]
    pub animation: Option<u32>,
}

/// Something a player must have to do an action. `message` replaces the default refusal; in it,
/// `{skill}` and `{level}` stand for the skill's name and the level needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Requirement {
    /// At least `level` in `skill` (boosts count).
    Level {
        skill: Skill,
        level: i32,
        #[serde(default)]
        message: Option<String>,
    },
    /// A tool of `category`, carried or worn, that the player has the level to use.
    Tool {
        category: String,
        #[serde(default)]
        message: Option<String>,
    },
    /// At least `slots` empty inventory slots.
    FreeSlots {
        slots: usize,
        #[serde(default)]
        message: Option<String>,
    },
}

impl Requirement {
    /// What the player is told when this is not met.
    pub fn refusal(&self) -> String {
        match self {
            Requirement::Level { skill, level, message } => message
                .as_deref()
                .unwrap_or("You need a {skill} level of {level} to do that.")
                .replace("{skill}", skill.name())
                .replace("{level}", &level.to_string()),
            Requirement::Tool { category, message } => message
                .clone()
                .unwrap_or_else(|| format!("You need {} {category} to do that.", crate::model::article(category))),
            Requirement::FreeSlots { message, .. } => {
                message.clone().unwrap_or_else(|| "You don't have enough inventory space.".to_owned())
            }
        }
    }
}

/// One of the scenery objects a node is drawn as, and what it turns into while depleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeLocDef {
    /// The cache object id of the resource (a tree).
    pub loc: u32,
    /// The cache object id shown while it is depleted (its stump).
    pub depleted: u32,
}

/// How a node runs out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DepletionDef {
    /// Every success depletes it (a regular tree is felled by its one log).
    EverySuccess,
    /// A despawn timer of `ticks` starts on the first tick anyone gathers from it, runs down
    /// while it is gathered from and back up (at the same rate) while it is not; once it has run
    /// out, the next success depletes it (oak, willow, yew...).
    Timer { ticks: i32 },
}

/// A pet a node can give, rolled once per product gathered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PetDropDef {
    /// The pet's item id.
    pub item: String,
    /// The base "1 in N" chance; each level in the node's skill takes 25 off it.
    pub base_chance: i32,
}

fn four() -> i32 {
    4
}

/// A resource the player gathers from: a kind of tree, rock or fishing spot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatheringNodeDef {
    pub id: String,
    pub skill: Skill,
    /// The object option that starts gathering (`"Chop down"`).
    pub action: String,
    pub locs: Vec<NodeLocDef>,
    /// Checked when the player clicks, again on arrival and before every attempt (a `Tool`
    /// requirement names the kind of tool and the message for lacking one).
    #[serde(default)]
    pub requirements: Vec<Requirement>,
    /// Ticks between attempts.
    #[serde(default = "four")]
    pub roll_interval_ticks: i32,
    /// Experience per success, in tenths of a point.
    pub xp_tenths: i32,
    /// The item each success gives.
    pub product: String,
    #[serde(default = "super::one_u32")]
    pub product_quantity: u32,
    /// The tools it is gathered with, by item, each with the `[low, high]` of its success chance
    /// (`rules::success_numerator`); the best one the player can use is chosen from these.
    pub chances: BTreeMap<String, [i32; 2]>,
    pub depletion: DepletionDef,
    /// How long it stays depleted, `[min, max]` ticks inclusive, chosen uniformly.
    pub respawn_ticks: [i32; 2],
    #[serde(default)]
    pub pet: Option<PetDropDef>,
    /// Said as the player starts (`"You swing your axe at the tree."`).
    #[serde(default)]
    pub start_message: Option<String>,
    /// Said on each success (`"You get some logs."`).
    #[serde(default)]
    pub success_message: Option<String>,
}

impl GatheringNodeDef {
    /// What `loc` turns into while depleted, if `loc` is one of this node's objects.
    pub fn depleted_form(&self, loc: u32) -> Option<u32> {
        self.locs.iter().find(|l| l.loc == loc).map(|l| l.depleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_fill_in_the_skill_and_level() {
        let level = Requirement::Level {
            skill: Skill::Woodcutting,
            level: 15,
            message: Some("You need a {skill} level of {level} to chop down this tree.".into()),
        };
        assert_eq!(level.refusal(), "You need a Woodcutting level of 15 to chop down this tree.");

        let tool = Requirement::Tool { category: "axe".into(), message: None };
        assert_eq!(tool.refusal(), "You need an axe to do that.");
    }

    #[test]
    fn requirements_read_from_json() {
        let parsed: Vec<Requirement> = serde_json::from_str(
            r#"[{"level":{"skill":"woodcutting","level":1}},{"tool":{"category":"axe"}},{"free_slots":{"slots":1}}]"#,
        )
        .unwrap();
        assert_eq!(parsed.len(), 3);
        assert!(serde_json::from_str::<Requirement>(r#"{"level":{"skill":"woodcutting","levle":1}}"#).is_err());
    }
}
