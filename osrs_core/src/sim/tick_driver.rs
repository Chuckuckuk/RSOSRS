use crate::tick::TickAccumulator;

/// The pacing logic of the C# `TickRunner`: turns real elapsed time into whole ticks (with a
/// catch-up clamp), plus pause and single-step controls. Headless - the host calls `update`
/// every frame with its own delta time (the C# `useUnscaledTime` choice is the host's) and runs
/// whatever a tick means for it (usually `World::run_tick`) in the callback.
#[derive(Debug, Clone)]
pub struct TickDriver {
    pub base_tick_seconds: f32,
    pub tick_rate_multiplier: f32,
    pub max_catch_up_ticks: i32,
    paused: bool,
    single_step_requested: bool,
    accumulator: TickAccumulator,
}

impl Default for TickDriver {
    fn default() -> Self {
        Self::new(0.6, 1.0, 5)
    }
}

impl TickDriver {
    pub fn new(base_tick_seconds: f32, tick_rate_multiplier: f32, max_catch_up_ticks: i32) -> Self {
        let mut driver = Self {
            base_tick_seconds,
            tick_rate_multiplier,
            max_catch_up_ticks,
            paused: false,
            single_step_requested: false,
            accumulator: TickAccumulator::new(0.0),
        };
        // Start the accumulator primed so the very first tick runs on frame 1 rather than
        // waiting a full 600ms of silence after the scene loads.
        driver.accumulator = TickAccumulator::new(driver.effective_tick_seconds());
        driver
    }

    pub fn effective_tick_seconds(&self) -> f32 {
        self.base_tick_seconds / self.tick_rate_multiplier.max(0.0001)
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn dropped_ticks(&self) -> i32 {
        self.accumulator.dropped_ticks()
    }

    /// How far through the current tick we are, for view interpolation (1 while paused).
    pub fn tick_progress01(&self) -> f32 {
        if self.paused {
            1.0
        } else {
            self.accumulator.progress01(self.effective_tick_seconds())
        }
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn set_paused(&mut self, value: bool) {
        self.paused = value;
    }

    pub fn request_single_step(&mut self) {
        self.single_step_requested = true;
    }

    /// One frame: runs `run_tick` for every whole tick `dt` seconds add up to (or exactly once
    /// for a requested single step while paused). Returns how many ticks ran.
    pub fn update(&mut self, dt: f32, mut run_tick: impl FnMut()) -> i32 {
        if self.paused {
            if self.single_step_requested {
                run_tick();
                self.single_step_requested = false;
                return 1;
            }
            return 0;
        }

        let ticks_to_run = self.accumulator.advance(dt, self.effective_tick_seconds(), self.max_catch_up_ticks);
        for _ in 0..ticks_to_run {
            run_tick();
        }
        ticks_to_run
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_very_first_update_runs_a_tick_even_with_zero_elapsed_time() {
        let mut driver = TickDriver::default();
        let mut ran = 0;

        driver.update(0.0, || ran += 1);

        assert_eq!(ran, 1);
    }

    #[test]
    fn updates_accumulate_real_time_into_whole_ticks() {
        let mut driver = TickDriver::default();
        driver.update(0.0, || {}); // consume the primed tick
        let mut ran = 0;

        driver.update(0.3, || ran += 1);
        assert_eq!(ran, 0);
        driver.update(0.3, || ran += 1);
        assert_eq!(ran, 1);
    }

    #[test]
    fn a_huge_frame_is_clamped_to_the_catch_up_limit_and_the_rest_dropped() {
        let mut driver = TickDriver::default();
        driver.update(0.0, || {});
        let mut ran = 0;

        driver.update(60.0, || ran += 1);

        assert_eq!(ran, 5);
        assert!(driver.dropped_ticks() > 0);
    }

    #[test]
    fn paused_runs_nothing_until_a_single_step_is_requested() {
        let mut driver = TickDriver::default();
        driver.set_paused(true);
        let mut ran = 0;

        driver.update(10.0, || ran += 1);
        assert_eq!(ran, 0);

        driver.request_single_step();
        driver.update(0.0, || ran += 1);
        assert_eq!(ran, 1);

        driver.update(0.0, || ran += 1); // the request was consumed
        assert_eq!(ran, 1);
        assert_eq!(driver.tick_progress01(), 1.0);
    }

    #[test]
    fn tick_rate_multiplier_scales_the_effective_tick_length() {
        let driver = TickDriver::new(0.6, 2.0, 5);

        assert!((driver.effective_tick_seconds() - 0.3).abs() < 1e-6);
    }
}
