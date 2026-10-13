use serde::{Deserialize, Serialize};

use super::{Attribute, AttributeSet, DamageType};
use crate::combat::AttackStyle;
use crate::rules::{self, Ratio};

/// Which step of the roll pipeline a `Modifier` changes (reference §3/§5). The order here is the
/// order the resolver applies them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModifierTarget {
    /// Multiplies the effective Attack/Ranged/Magic level (Void Knight sets).
    AccuracyLevel,
    /// Multiplies the effective Strength/Ranged level used for the max hit (Void Knight sets).
    DamageLevel,
    /// Multiplies the final attack roll (Slayer helm, Salve amulet, Dragon hunter gear, spec accuracy).
    AttackRoll,
    /// Multiplies the defender's defence roll (defence-reducing specials: `ratio` < 1).
    TargetDefenceRoll,
    /// Adds `add` permille to the Magic Damage % sum (Augury, Salve on magic, Virtus robes).
    MagicDamagePermille,
    /// Multiplies the final max hit, flooring (Slayer helm, Salve, spec damage, Dharok's).
    MaxHit,
    /// Adds `add` to the final max hit after multipliers.
    MaxHitAdd,
    /// Multiplies each rolled hit AFTER the roll, optionally only `chance_one_in` of the time
    /// (Keris x3 on Kalphite 1/51, Gadderhammer x2 on Shades 1/20).
    DamageRoll,
}

/// When a modifier applies. Evaluated against the attack being rolled and its target.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    #[default]
    Always,
    /// The defender carries this monster attribute tag.
    TargetHas(Attribute),
    Style(AttackStyle),
    DamageType(DamageType),
    /// A free-form flag set on the attack itself: `on_task`, `special`, `inside_toa`, ...
    Flag(String),
    Not(Box<Condition>),
    All(Vec<Condition>),
    Any(Vec<Condition>),
}

/// What a `Condition` is checked against.
#[derive(Debug, Clone, Copy)]
pub struct ConditionContext<'a> {
    pub style: AttackStyle,
    pub damage_type: DamageType,
    pub target_attributes: AttributeSet,
    pub flags: &'a [String],
}

impl Condition {
    pub fn holds(&self, ctx: &ConditionContext) -> bool {
        match self {
            Condition::Always => true,
            Condition::TargetHas(attribute) => ctx.target_attributes.contains(*attribute),
            Condition::Style(style) => ctx.style == *style,
            Condition::DamageType(damage_type) => ctx.damage_type == *damage_type,
            Condition::Flag(flag) => ctx.flags.iter().any(|f| f == flag),
            Condition::Not(inner) => !inner.holds(ctx),
            Condition::All(all) => all.iter().all(|c| c.holds(ctx)),
            Condition::Any(any) => any.iter().any(|c| c.holds(ctx)),
        }
    }
}

fn default_ratio() -> Ratio {
    Ratio::ONE
}

/// A modifier whose `ratio` is not fixed but computed from the attacker's live state when the
/// attack is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModifierScaling {
    /// Dharok's set: `1 + (max_hp - current_hp) / 100 * max_hp / 100` on the max hit.
    MissingHitpoints,
    /// Twisted bow accuracy against the target's Magic.
    TwistedBowAccuracy,
    /// Twisted bow damage against the target's Magic.
    TwistedBowDamage,
}

/// What a scaling modifier needs to know to work out its ratio when an attack is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScalingContext {
    pub attacker_hp: i32,
    pub attacker_max_hp: i32,
    /// The higher of the target's Magic level and Magic attack bonus.
    pub target_magic: i32,
    /// Xerician targets (Chambers of Xeric) raise the twisted bow's Magic cap from 250 to 350.
    pub target_is_xerician: bool,
}

/// One conditional effect on a roll: the single primitive behind Void, Slayer helm, Salve,
/// Dragon hunter, Dharok's, spec bonuses, Keris procs and so on. Gear and prayers are just lists
/// of these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Modifier {
    pub target: ModifierTarget,
    #[serde(default = "default_ratio")]
    pub ratio: Ratio,
    pub add: i32,
    pub condition: Condition,
    /// Modifiers sharing a group do not stack: only the highest-`priority` applicable one counts
    /// (Salve amulet beats Slayer helm; the first in list order wins a tie).
    pub group: Option<String>,
    pub priority: i32,
    /// For `DamageRoll`: apply on a 1-in-N chance. 0 or 1 means always.
    pub chance_one_in: i32,
    pub scaling: Option<ModifierScaling>,
}

impl Default for Modifier {
    fn default() -> Self {
        Self {
            target: ModifierTarget::MaxHit,
            ratio: Ratio::ONE,
            add: 0,
            condition: Condition::Always,
            group: None,
            priority: 0,
            chance_one_in: 0,
            scaling: None,
        }
    }
}

impl Modifier {
    pub fn multiplier(target: ModifierTarget, num: i64, den: i64) -> Self {
        Self { target, ratio: Ratio::new(num, den), ..Self::default() }
    }

    pub fn additive(target: ModifierTarget, add: i32) -> Self {
        Self { target, add, ..Self::default() }
    }

    pub fn when(mut self, condition: Condition) -> Self {
        self.condition = condition;
        self
    }

    pub fn in_group(mut self, group: &str, priority: i32) -> Self {
        self.group = Some(group.to_owned());
        self.priority = priority;
        self
    }

    pub fn with_chance_one_in(mut self, n: i32) -> Self {
        self.chance_one_in = n;
        self
    }

    pub fn scaled(mut self, scaling: ModifierScaling) -> Self {
        self.scaling = Some(scaling);
        self
    }

    /// This modifier with any `scaling` replaced by the concrete ratio it works out to in `ctx`.
    pub fn resolved(&self, ctx: &ScalingContext) -> Modifier {
        let mut resolved = self.clone();
        let cap = if ctx.target_is_xerician { 350 } else { 250 };
        match self.scaling {
            None => return resolved,
            Some(ModifierScaling::MissingHitpoints) => {
                let missing = (ctx.attacker_max_hp - ctx.attacker_hp).max(0) as i64;
                resolved.ratio = Ratio::new(10_000 + missing * ctx.attacker_max_hp as i64, 10_000);
            }
            Some(ModifierScaling::TwistedBowAccuracy) => {
                resolved.ratio = Ratio::new(rules::twisted_bow_accuracy_percent(ctx.target_magic, cap) as i64, 100);
            }
            Some(ModifierScaling::TwistedBowDamage) => {
                resolved.ratio = Ratio::new(rules::twisted_bow_damage_percent(ctx.target_magic, cap) as i64, 100);
            }
        }
        resolved.scaling = None;
        resolved
    }

    /// The modifiers in `modifiers` that apply to `target` right now, in list order, with
    /// non-stacking groups reduced to their single winner.
    pub fn applicable<'a>(modifiers: &'a [Modifier], target: ModifierTarget, ctx: &ConditionContext) -> Vec<&'a Modifier> {
        let candidates: Vec<&Modifier> = modifiers.iter().filter(|m| m.target == target && m.condition.holds(ctx)).collect();

        candidates
            .iter()
            .enumerate()
            .filter(|(index, modifier)| match &modifier.group {
                None => true,
                Some(group) => {
                    let winner = candidates
                        .iter()
                        .enumerate()
                        .filter(|(_, other)| other.group.as_ref() == Some(group))
                        .fold(None, |best: Option<(usize, i32)>, (i, other)| match best {
                            Some((_, priority)) if priority >= other.priority => best,
                            _ => Some((i, other.priority)),
                        });
                    winner.is_some_and(|(winner_index, _)| winner_index == *index)
                }
            })
            .map(|(_, modifier)| *modifier)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(attributes: AttributeSet, flags: &'a [String]) -> ConditionContext<'a> {
        ConditionContext {
            style: AttackStyle::Melee,
            damage_type: DamageType::Slash,
            target_attributes: attributes,
            flags,
        }
    }

    #[test]
    fn conditions_compose() {
        let undead = AttributeSet::default().with(Attribute::Undead);
        let flags = vec!["on_task".to_owned()];
        let c = ctx(undead, &flags);

        assert!(Condition::TargetHas(Attribute::Undead).holds(&c));
        assert!(!Condition::TargetHas(Attribute::Demon).holds(&c));
        assert!(Condition::Flag("on_task".into()).holds(&c));
        assert!(Condition::Not(Box::new(Condition::Flag("special".into()))).holds(&c));
        assert!(Condition::All(vec![Condition::Style(AttackStyle::Melee), Condition::DamageType(DamageType::Slash)]).holds(&c));
        assert!(Condition::Any(vec![Condition::Style(AttackStyle::Magic), Condition::Always]).holds(&c));
        assert!(!Condition::All(vec![Condition::Style(AttackStyle::Magic)]).holds(&c));
    }

    #[test]
    fn grouped_modifiers_do_not_stack_and_the_highest_priority_wins() {
        let undead = AttributeSet::default().with(Attribute::Undead);
        let flags = vec!["on_task".to_owned()];
        let c = ctx(undead, &flags);
        let mods = vec![
            Modifier::multiplier(ModifierTarget::MaxHit, 7, 6).when(Condition::Flag("on_task".into())).in_group("slayer_or_salve", 1),
            Modifier::multiplier(ModifierTarget::MaxHit, 6, 5).when(Condition::TargetHas(Attribute::Undead)).in_group("slayer_or_salve", 2),
            Modifier::multiplier(ModifierTarget::MaxHit, 11, 10),
        ];

        let applied = Modifier::applicable(&mods, ModifierTarget::MaxHit, &c);

        assert_eq!(applied.len(), 2);
        assert_eq!(applied[0].ratio, Ratio::new(6, 5)); // the salve, not the slayer helm
        assert_eq!(applied[1].ratio, Ratio::new(11, 10)); // ungrouped still applies
    }

    #[test]
    fn an_inapplicable_group_member_contributes_nothing() {
        let c = ctx(AttributeSet::default(), &[]);
        let mods = vec![Modifier::multiplier(ModifierTarget::MaxHit, 6, 5).when(Condition::TargetHas(Attribute::Undead)).in_group("g", 2)];

        assert!(Modifier::applicable(&mods, ModifierTarget::MaxHit, &c).is_empty());
    }

    #[test]
    fn dharok_scaling_grows_with_missing_hitpoints() {
        let dharok = Modifier::default().scaled(ModifierScaling::MissingHitpoints);

        let at = |hp| {
            dharok.resolved(&ScalingContext { attacker_hp: hp, attacker_max_hp: 99, target_magic: 1, target_is_xerician: false })
        };
        assert_eq!(at(99).ratio.apply(100), 100);
        // 99 max, 1 left: 1 + 98/100 * 99/100 = 1.9702
        assert_eq!(at(1).ratio.apply(100), 197);
        assert!(at(50).ratio.num > at(80).ratio.num);
    }

    #[test]
    fn twisted_bow_scaling_reads_the_targets_magic_and_the_xerician_cap() {
        let accuracy = Modifier::default().scaled(ModifierScaling::TwistedBowDamage);
        let ctx = |magic, xeric| ScalingContext { attacker_hp: 1, attacker_max_hp: 1, target_magic: magic, target_is_xerician: xeric };

        assert_eq!(accuracy.resolved(&ctx(250, false)).ratio, Ratio::new(215, 100));
        assert_eq!(accuracy.resolved(&ctx(350, true)).ratio, Ratio::new(248, 100));
        assert_eq!(accuracy.resolved(&ctx(350, false)).ratio, Ratio::new(215, 100), "cap is 250 outside the Chambers");
    }

    #[test]
    fn json_modifier_with_defaults() {
        let m: Modifier = serde_json::from_str(
            r#"{"target":"max_hit","ratio":{"num":6,"den":5},"condition":{"target_has":"undead"},"group":"salve","priority":2}"#,
        )
        .unwrap();
        assert_eq!(m.ratio, Ratio::new(6, 5));
        assert_eq!(m.condition, Condition::TargetHas(Attribute::Undead));
        assert_eq!(m.add, 0);

        let plain: Modifier = serde_json::from_str(r#"{"target":"magic_damage_permille","add":40}"#).unwrap();
        assert_eq!(plain.ratio, Ratio::ONE);
        assert_eq!(plain.condition, Condition::Always);
    }
}
