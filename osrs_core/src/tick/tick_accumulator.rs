/// Pure accumulator math behind the tick runner's catch-up clamp: turns real elapsed time into
/// a whole number of ticks to run this frame, dropping time beyond the catch-up cap.
#[derive(Debug, Clone)]
pub struct TickAccumulator {
    // Accumulated in f64 even though the public API is f32: f64's error per operation is small
    // enough that even thousands of repeated subtractions stay far inside EPSILON, where f32
    // would not.
    accumulator: f64,
    dropped_ticks: i32,
}

// Binary floating point cannot represent 0.6 exactly, so repeated subtraction of a tick
// length that lands on an exact multiple (e.g. 12.0s / 0.6s = 20 ticks) can drift just
// under a tick boundary and silently lose one. This epsilon absorbs that representation
// error in every ">= tick_seconds" comparison below.
const EPSILON: f64 = 1e-6;

impl TickAccumulator {
    pub fn new(initial_accumulator: f32) -> Self {
        Self { accumulator: initial_accumulator as f64, dropped_ticks: 0 }
    }

    pub fn accumulator(&self) -> f32 {
        self.accumulator as f32
    }

    pub fn dropped_ticks(&self) -> i32 {
        self.dropped_ticks
    }

    /// Adds `dt` seconds, clamps the backlog to `max_catch_up_ticks` ticks (counting what was
    /// discarded in `dropped_ticks`) and returns how many whole ticks to run now.
    pub fn advance(&mut self, dt: f32, tick_seconds: f32, max_catch_up_ticks: i32) -> i32 {
        let tick = tick_seconds as f64;
        let max_accumulator = max_catch_up_ticks as f64 * tick;

        let raw = self.accumulator + dt as f64;
        let clamped = if raw < max_accumulator { raw } else { max_accumulator };

        let discarded = raw - clamped;
        if discarded > 0.0 {
            self.dropped_ticks += count_whole_ticks(discarded, tick);
        }

        self.accumulator = clamped;
        let ticks_to_run = count_whole_ticks(self.accumulator, tick);
        self.accumulator -= ticks_to_run as f64 * tick;
        if self.accumulator < 0.0 {
            self.accumulator = 0.0; // guard against epsilon pushing it slightly negative
        }

        ticks_to_run
    }

    /// How far through the current tick we are, clamped to `[0, 1]`.
    pub fn progress01(&self, tick_seconds: f32) -> f32 {
        let p = self.accumulator / tick_seconds as f64;
        if p < 0.0 {
            return 0.0;
        }
        if p > 1.0 {
            return 1.0;
        }
        p as f32
    }
}

impl Default for TickAccumulator {
    fn default() -> Self {
        Self::new(0.0)
    }
}

fn count_whole_ticks(mut amount: f64, tick: f64) -> i32 {
    let mut count = 0;
    while amount >= tick - EPSILON {
        amount -= tick;
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_exactly_one_tick_when_dt_equals_tick_length() {
        let mut acc = TickAccumulator::new(0.0);

        let ticks = acc.advance(0.6, 0.6, 5);

        assert_eq!(ticks, 1);
        assert!((acc.accumulator() - 0.0).abs() < 1e-6);
        assert_eq!(acc.dropped_ticks(), 0);
    }

    #[test]
    fn never_yields_progress_at_or_above_one_after_advancing() {
        let mut acc = TickAccumulator::new(0.0);

        acc.advance(2.05, 0.6, 10); // 3 full ticks + partial remainder

        let progress = acc.progress01(0.6);
        assert!(progress < 1.0);
        assert!(progress >= 0.0);
    }

    #[test]
    fn clamps_catch_up_and_counts_dropped_ticks() {
        // 20 ticks' worth of elapsed time with a catch-up cap of 5 ticks should run only 5
        // and count the other 15 as dropped, never returning a progress value >= 1.
        let mut acc = TickAccumulator::new(0.0);

        let ticks = acc.advance(12.0, 0.6, 5);

        assert_eq!(ticks, 5);
        assert_eq!(acc.dropped_ticks(), 15);
        assert!(acc.progress01(0.6) < 1.0);
    }

    #[test]
    fn partial_progress_reflects_fraction_of_tick() {
        let mut acc = TickAccumulator::new(0.0);

        let ticks = acc.advance(0.3, 0.6, 5);

        assert_eq!(ticks, 0);
        assert!((acc.progress01(0.6) - 0.5).abs() < 1e-5);
    }
}
