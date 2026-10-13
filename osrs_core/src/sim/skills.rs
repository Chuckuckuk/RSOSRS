use crate::model::Skill;
use crate::rules;

/// A player's base and current (boosted or drained) level and experience in every skill.
/// Hitpoints and Prayer keep their live value elsewhere (`Health`, `PrayerPool`); only their BASE
/// level is meaningful here (it gates prayers and feeds the combat level).
///
/// Experience is in tenths of a point, as the game keeps it. A level set directly (`set_base`, a
/// scenario's levels) brings the experience to that level's threshold, so the two always agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skills {
    base: [i32; Skill::COUNT],
    current: [i32; Skill::COUNT],
    xp_tenths: [i32; Skill::COUNT],
}

impl Default for Skills {
    /// A new account: 10 Hitpoints, 1 in everything else.
    fn default() -> Self {
        let base = Skill::ALL.map(Skill::starting_level);
        Self { base, current: base, xp_tenths: base.map(rules::xp_tenths_for_level) }
    }
}

impl Skills {
    /// The seven combat skills as given, every other skill at its starting level.
    pub fn new(attack: i32, strength: i32, defence: i32, ranged: i32, magic: i32, prayer: i32, hitpoints: i32) -> Self {
        let mut skills = Self::default();
        for (skill, level) in [
            (Skill::Attack, attack),
            (Skill::Strength, strength),
            (Skill::Defence, defence),
            (Skill::Ranged, ranged),
            (Skill::Magic, magic),
            (Skill::Prayer, prayer),
            (Skill::Hitpoints, hitpoints),
        ] {
            skills.set_base(skill, level);
        }
        skills
    }

    /// The same skills with `skill` set to `level` (a builder for setups and tests).
    pub fn with(mut self, skill: Skill, level: i32) -> Self {
        self.set_base(skill, level);
        self
    }

    pub fn base(&self, skill: Skill) -> i32 {
        self.base[skill.index()]
    }

    pub fn current(&self, skill: Skill) -> i32 {
        self.current[skill.index()]
    }

    /// Experience in `skill`, in tenths of a point.
    pub fn xp_tenths(&self, skill: Skill) -> i32 {
        self.xp_tenths[skill.index()]
    }

    /// Sets the base and current level, and the experience to that level's threshold.
    pub fn set_base(&mut self, skill: Skill, level: i32) {
        self.base[skill.index()] = level;
        self.current[skill.index()] = level;
        self.xp_tenths[skill.index()] = rules::xp_tenths_for_level(level);
    }

    /// Adds `tenths` of experience (capped at 200M) and returns the new base level if it went up.
    /// A level gained raises the current level by as much, so a boost or drain keeps its size.
    pub fn add_xp(&mut self, skill: Skill, tenths: i32) -> Option<i32> {
        let i = skill.index();
        self.xp_tenths[i] = self.xp_tenths[i].saturating_add(tenths.max(0)).min(rules::MAX_XP_TENTHS);
        let level = rules::level_for_xp_tenths(self.xp_tenths[i]);
        if level <= self.base[i] {
            return None;
        }
        self.current[i] += level - self.base[i];
        self.base[i] = level;
        Some(level)
    }

    /// Raises the current level to at least `level`; a boost never lowers a stat.
    pub fn boost_to(&mut self, skill: Skill, level: i32) {
        let slot = &mut self.current[skill.index()];
        *slot = (*slot).max(level);
    }

    /// Lowers the current level, never below 1 (a combat roll needs a positive level).
    pub fn drain(&mut self, skill: Skill, amount: i32) {
        let slot = &mut self.current[skill.index()];
        *slot = (*slot - amount.max(0)).max(1);
    }

    /// Raises the current level toward base by `amount`, stopping at base. A boosted level is untouched.
    pub fn restore(&mut self, skill: Skill, amount: i32) {
        let base = self.base[skill.index()];
        let slot = &mut self.current[skill.index()];
        if *slot < base {
            *slot = (*slot + amount.max(0)).min(base);
        }
    }

    /// Moves one level toward base (the 100-tick drift). Returns whether anything changed.
    pub fn drift(&mut self, skill: Skill) -> bool {
        let before = self.current[skill.index()];
        self.current[skill.index()] = rules::drift_toward_base(before, self.base[skill.index()]);
        self.current[skill.index()] != before
    }

    pub fn is_off_base(&self, skill: Skill) -> bool {
        self.current[skill.index()] != self.base[skill.index()]
    }

    /// The combat level from the BASE levels (reference §3).
    pub fn combat_level(&self) -> i32 {
        rules::combat_level(
            self.base(Skill::Attack),
            self.base(Skill::Strength),
            self.base(Skill::Defence),
            self.base(Skill::Hitpoints),
            self.base(Skill::Prayer),
            self.base(Skill::Ranged),
            self.base(Skill::Magic),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maxed() -> Skills {
        Skills::new(99, 99, 99, 99, 99, 99, 99)
    }

    #[test]
    fn boosts_raise_but_never_lower() {
        let mut skills = maxed();
        skills.boost_to(Skill::Strength, 118);
        skills.boost_to(Skill::Strength, 110);

        assert_eq!(skills.current(Skill::Strength), 118);
        assert_eq!(skills.base(Skill::Strength), 99);
    }

    #[test]
    fn drains_floor_at_one_and_restores_stop_at_base() {
        let mut skills = maxed();
        skills.drain(Skill::Attack, 500);
        assert_eq!(skills.current(Skill::Attack), 1);

        skills.restore(Skill::Attack, 30);
        assert_eq!(skills.current(Skill::Attack), 31);
        skills.restore(Skill::Attack, 500);
        assert_eq!(skills.current(Skill::Attack), 99);
    }

    #[test]
    fn restore_leaves_a_boosted_stat_alone() {
        let mut skills = maxed();
        skills.boost_to(Skill::Ranged, 112);

        skills.restore(Skill::Ranged, 20);

        assert_eq!(skills.current(Skill::Ranged), 112);
    }

    #[test]
    fn drift_walks_both_directions_back_to_base() {
        let mut skills = maxed();
        skills.boost_to(Skill::Magic, 101);
        skills.drain(Skill::Defence, 2);

        assert!(skills.drift(Skill::Magic));
        assert!(skills.drift(Skill::Defence));
        assert_eq!(skills.current(Skill::Magic), 100);
        assert_eq!(skills.current(Skill::Defence), 98);
        assert!(!skills.drift(Skill::Attack));
    }

    #[test]
    fn a_new_account_has_ten_hitpoints_and_one_in_everything_else() {
        let skills = Skills::default();
        assert_eq!(skills.base(Skill::Hitpoints), 10);
        assert_eq!(skills.xp_tenths(Skill::Hitpoints), 11_540);
        assert_eq!(skills.base(Skill::Woodcutting), 1);
        assert_eq!(skills.xp_tenths(Skill::Woodcutting), 0);
    }

    #[test]
    fn experience_raises_levels_including_several_at_once() {
        let mut skills = Skills::default();
        assert_eq!(skills.add_xp(Skill::Woodcutting, 825), None, "82.5 XP is still level 1");
        assert_eq!(skills.add_xp(Skill::Woodcutting, 5), Some(2), "83 XP is level 2");
        assert_eq!(skills.add_xp(Skill::Woodcutting, 11_540 - 830), Some(10), "1,154 XP jumps to 10");
        assert_eq!(skills.current(Skill::Woodcutting), 10);
    }

    #[test]
    fn experience_stops_at_two_hundred_million() {
        let mut skills = Skills::default().with(Skill::Woodcutting, 99);
        skills.add_xp(Skill::Woodcutting, i32::MAX);
        assert_eq!(skills.xp_tenths(Skill::Woodcutting), rules::MAX_XP_TENTHS);
        assert_eq!(skills.base(Skill::Woodcutting), 99);
    }

    #[test]
    fn a_level_gained_keeps_a_boost() {
        let mut skills = Skills::default();
        skills.boost_to(Skill::Woodcutting, 4);
        skills.add_xp(Skill::Woodcutting, 830);
        assert_eq!((skills.base(Skill::Woodcutting), skills.current(Skill::Woodcutting)), (2, 5));
    }

    #[test]
    fn combat_level_uses_base_levels_not_boosted_ones() {
        let mut skills = maxed();
        skills.boost_to(Skill::Strength, 200);

        assert_eq!(skills.combat_level(), 126);
    }
}
