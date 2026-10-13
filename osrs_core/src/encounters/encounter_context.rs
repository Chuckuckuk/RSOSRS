use std::collections::HashMap;

use crate::actors::EntityId;
use crate::rng::DotNetRandom;

/// Per-encounter shared state: the seeded RNG every roll in the fight draws from, the elapsed
/// tick count, named counters/flags that triggers read, and who is taking part. Participants are
/// `EntityId`s rather than the C# `Actor` references - the world resolves them.
#[derive(Debug, Clone)]
pub struct EncounterContext {
    participants: Vec<EntityId>,
    counters: HashMap<String, i32>,
    flags: HashMap<String, bool>,
    rng: DotNetRandom,
    elapsed_ticks: i32,
}

impl EncounterContext {
    pub fn new(seed: i32) -> Self {
        Self {
            participants: Vec::new(),
            counters: HashMap::new(),
            flags: HashMap::new(),
            rng: DotNetRandom::new(seed),
            elapsed_ticks: 0,
        }
    }

    /// The encounter's RNG. Every seeded roll in the fight goes through this one stream, in
    /// call order, so the same seed replays the same fight.
    pub fn rng(&mut self) -> &mut DotNetRandom {
        &mut self.rng
    }

    pub fn elapsed_ticks(&self) -> i32 {
        self.elapsed_ticks
    }

    pub fn participants(&self) -> &[EntityId] {
        &self.participants
    }

    pub fn add_participant(&mut self, id: EntityId) {
        self.participants.push(id);
    }

    pub fn remove_participant(&mut self, id: EntityId) {
        if let Some(pos) = self.participants.iter().position(|existing| *existing == id) {
            self.participants.remove(pos);
        }
    }

    pub fn advance_tick(&mut self) {
        self.elapsed_ticks += 1;
    }

    pub fn counter(&self, key: &str) -> i32 {
        self.counters.get(key).copied().unwrap_or(0)
    }

    pub fn set_counter(&mut self, key: &str, value: i32) {
        self.counters.insert(key.to_owned(), value);
    }

    pub fn flag(&self, key: &str) -> bool {
        self.flags.get(key).copied().unwrap_or(false)
    }

    pub fn set_flag(&mut self, key: &str, value: bool) {
        self.flags.insert(key.to_owned(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn counters_and_flags_default_to_zero_and_false_and_round_trip() {
        let mut context = EncounterContext::new(1);
        assert_eq!(context.counter("phase"), 0);
        assert!(!context.flag("enraged"));

        context.set_counter("phase", 3);
        context.set_flag("enraged", true);

        assert_eq!(context.counter("phase"), 3);
        assert!(context.flag("enraged"));
    }

    #[test]
    fn advance_tick_counts_elapsed_ticks() {
        let mut context = EncounterContext::new(1);

        context.advance_tick();
        context.advance_tick();

        assert_eq!(context.elapsed_ticks(), 2);
    }

    #[test]
    fn participants_can_be_added_and_removed() {
        let mut context = EncounterContext::new(1);
        context.add_participant(EntityId(1));
        context.add_participant(EntityId(2));

        context.remove_participant(EntityId(1));

        assert_eq!(context.participants(), &[EntityId(2)]);
    }

    #[test]
    fn the_same_seed_replays_the_same_rng_stream() {
        let mut a = EncounterContext::new(12345);
        let mut b = EncounterContext::new(12345);

        for _ in 0..10 {
            assert_eq!(a.rng().next_double().to_bits(), b.rng().next_double().to_bits());
        }
    }
}
