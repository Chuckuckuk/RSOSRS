use crate::model::Skill;
use crate::rules;

/// A player's base and current (boosted or drained) levels for the combat skills. Hitpoints and
/// Prayer keep their live value elsewhere (`Health`, `PrayerPool`); only their BASE level is
/// meaningful here (it gates prayers and feeds the combat level).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skills {
    base: [i32; 7],
    current: [i32; 7],
}

impl Skills {
    pub fn new(attack: i32, strength: i32, defence: i32, ranged: i32, magic: i32, prayer: i32, hitpoints: i32) -> Self {
        let base = [attack, strength, defence, ranged, magic, prayer, hitpoints];
        Self { base, current: base }
    }

    pub fn base(&self, skill: Skill) -> i32 {
        self.base[skill.index()]
    }

    pub fn current(&self, skill: Skill) -> i32 {
        self.current[skill.index()]
    }

    pub fn set_base(&mut self, skill: Skill, level: i32) {
        self.base[skill.index()] = level;
        self.current[skill.index()] = level;
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
    fn combat_level_uses_base_levels_not_boosted_ones() {
        let mut skills = maxed();
        skills.boost_to(Skill::Strength, 200);

        assert_eq!(skills.combat_level(), 126);
    }
}
