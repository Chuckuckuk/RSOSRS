use super::encounter_context::EncounterContext;
use super::phase_trigger::{is_satisfied, PhaseTrigger};
use super::trigger_action::TriggerAction;

/// Pairs a trigger with the action it fires, once.
#[derive(Debug, Clone)]
pub struct TriggerBinding {
    trigger: PhaseTrigger,
    action: TriggerAction,
    has_fired: bool,
}

impl TriggerBinding {
    pub fn new(trigger: PhaseTrigger, action: TriggerAction) -> Self {
        Self { trigger, action, has_fired: false }
    }

    pub fn trigger(&self) -> &PhaseTrigger {
        &self.trigger
    }

    pub fn action(&self) -> &TriggerAction {
        &self.action
    }

    pub fn has_fired(&self) -> bool {
        self.has_fired
    }

    /// Returns the action the first time the trigger is satisfied, `None` before that and on
    /// every call after it has fired.
    pub fn try_fire(
        &mut self,
        encounter: &EncounterContext,
        tracked_health_percent: f32,
        tracked_entity_died: bool,
    ) -> Option<TriggerAction> {
        if self.has_fired {
            return None;
        }
        if !is_satisfied(&self.trigger, encounter, tracked_health_percent, tracked_entity_died) {
            return None;
        }

        self.has_fired = true;
        Some(self.action.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_fire_hp_threshold_fires_once_health_drops_to_or_below_it() {
        let mut binding = TriggerBinding::new(PhaseTrigger::hp_threshold(0.5), TriggerAction::spawn_entities("healers"));
        let encounter = EncounterContext::new(1);

        assert!(binding.try_fire(&encounter, 0.6, false).is_none());
        assert!(!binding.has_fired());

        let fired = binding.try_fire(&encounter, 0.5, false);
        assert_eq!(fired, Some(TriggerAction::spawn_entities("healers")));
        assert!(binding.has_fired());
    }

    #[test]
    fn try_fire_never_fires_twice_even_if_condition_stays_true() {
        let mut binding = TriggerBinding::new(PhaseTrigger::hp_threshold(0.5), TriggerAction::spawn_entities("healers"));
        let encounter = EncounterContext::new(1);

        assert!(binding.try_fire(&encounter, 0.1, false).is_some());
        assert!(binding.try_fire(&encounter, 0.1, false).is_none());
        assert!(binding.try_fire(&encounter, 0.01, false).is_none());
    }

    #[test]
    fn try_fire_elapsed_ticks_fires_once_encounter_reaches_the_threshold() {
        let mut binding = TriggerBinding::new(PhaseTrigger::at_elapsed_ticks(3), TriggerAction::set_flag("enraged", true));
        let mut encounter = EncounterContext::new(1);

        assert!(binding.try_fire(&encounter, 1.0, false).is_none()); // tick 0
        encounter.advance_tick(); // 1
        encounter.advance_tick(); // 2
        assert!(binding.try_fire(&encounter, 1.0, false).is_none());
        encounter.advance_tick(); // 3
        assert!(binding.try_fire(&encounter, 1.0, false).is_some());
    }

    #[test]
    fn try_fire_encounter_flag_fires_when_the_flag_matches_expected() {
        let mut binding = TriggerBinding::new(PhaseTrigger::when_flag("doorOpen", true), TriggerAction::swap_phase("Enraged"));
        let mut encounter = EncounterContext::new(1);

        assert!(binding.try_fire(&encounter, 1.0, false).is_none());
        encounter.set_flag("doorOpen", true);
        assert!(binding.try_fire(&encounter, 1.0, false).is_some());
    }

    #[test]
    fn try_fire_entity_died_fires_only_when_told_the_tracked_entity_died() {
        let mut binding = TriggerBinding::new(PhaseTrigger::when_entity_died(), TriggerAction::set_flag("healerDead", true));
        let encounter = EncounterContext::new(1);

        assert!(binding.try_fire(&encounter, 1.0, false).is_none());
        assert!(binding.try_fire(&encounter, 1.0, true).is_some());
    }
}
