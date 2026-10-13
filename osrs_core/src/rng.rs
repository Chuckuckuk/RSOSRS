//! Random number sources. The C# code takes a `System.Random` everywhere it needs randomness
//! (`EncounterContext(seed)`, `IAttackPattern.SelectNext`, `DamageResolver`). The Rust port takes
//! an `&mut dyn Rng` instead, with two implementations:
//!
//! - `DotNetRandom`: a port of .NET's seeded `System.Random` (Knuth subtractive generator), so the
//!   same seed produces the same rolls as the Unity project - the golden-trace parity check
//!   depends on this.
//! - `ScriptedRng`: a fixed sequence of values for tests (the C# `ScriptedRandom`).

/// The two `System.Random` members the C# core actually calls.
pub trait Rng {
    /// A double in `[0, 1)` (`Random.NextDouble`).
    fn next_double(&mut self) -> f64;

    /// An int in `[min, max)` (`Random.Next(min, max)`).
    fn next_range(&mut self, min: i32, max: i32) -> i32;
}

const MBIG: i32 = i32::MAX;
const MSEED: i32 = 161_803_398;

/// .NET's seeded `System.Random` (the "net5 compat" Knuth subtractive generator that Mono, and
/// therefore Unity, also use for `new Random(seed)`).
#[derive(Debug, Clone)]
pub struct DotNetRandom {
    seed_array: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl DotNetRandom {
    pub fn new(seed: i32) -> Self {
        let mut seed_array = [0i32; 56];
        let subtraction = if seed == i32::MIN { i32::MAX } else { seed.abs() };
        let mut mj = MSEED - subtraction;
        seed_array[55] = mj;
        let mut mk = 1i32;

        for i in 1..55 {
            let ii = (21 * i) % 55;
            seed_array[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(MBIG);
            }
            mj = seed_array[ii];
        }

        for _ in 1..5 {
            for i in 1..56 {
                let n = 1 + (i + 30) % 55;
                seed_array[i] = seed_array[i].wrapping_sub(seed_array[n]);
                if seed_array[i] < 0 {
                    seed_array[i] = seed_array[i].wrapping_add(MBIG);
                }
            }
        }

        Self { seed_array, inext: 0, inextp: 21 }
    }

    fn internal_sample(&mut self) -> i32 {
        let mut loc_inext = self.inext + 1;
        if loc_inext >= 56 {
            loc_inext = 1;
        }
        let mut loc_inextp = self.inextp + 1;
        if loc_inextp >= 56 {
            loc_inextp = 1;
        }

        let mut ret = self.seed_array[loc_inext].wrapping_sub(self.seed_array[loc_inextp]);
        if ret == MBIG {
            ret -= 1;
        }
        if ret < 0 {
            ret = ret.wrapping_add(MBIG);
        }

        self.seed_array[loc_inext] = ret;
        self.inext = loc_inext;
        self.inextp = loc_inextp;
        ret
    }

    fn sample(&mut self) -> f64 {
        self.internal_sample() as f64 * (1.0 / MBIG as f64)
    }

    /// `Random.Next()` - a non-negative int. Not used by the core directly, but it is the value
    /// the well-known .NET seed vectors are published for, so tests check against it.
    pub fn next_int(&mut self) -> i32 {
        self.internal_sample()
    }

    fn sample_for_large_range(&mut self) -> f64 {
        let mut result = self.internal_sample();
        if self.internal_sample() % 2 == 0 {
            result = -result;
        }
        let mut d = result as f64;
        d += (MBIG - 1) as f64;
        d / (2.0 * MBIG as f64 - 1.0)
    }
}

impl Rng for DotNetRandom {
    fn next_double(&mut self) -> f64 {
        self.sample()
    }

    fn next_range(&mut self, min: i32, max: i32) -> i32 {
        assert!(min <= max, "min must not exceed max");
        let range = max as i64 - min as i64;
        if range <= MBIG as i64 {
            (self.sample() * range as f64) as i32 + min
        } else {
            ((self.sample_for_large_range() * range as f64) as i64 + min as i64) as i32
        }
    }
}

/// A "1 in `denominator`" roll: a draw of 0 from `0..denominator`.
pub fn one_in(rng: &mut dyn Rng, denominator: i32) -> bool {
    rng.next_range(0, denominator.max(1)) == 0
}

/// Test RNG that replays fixed values. Panics when a sequence runs out (the C# helper throws an
/// index-out-of-range), which is what a test wants: an unexpected extra roll should fail loudly.
#[derive(Debug, Clone, Default)]
pub struct ScriptedRng {
    doubles: Vec<f64>,
    ints: Vec<i32>,
    double_index: usize,
    int_index: usize,
}

impl ScriptedRng {
    pub fn new(doubles: Vec<f64>, ints: Vec<i32>) -> Self {
        Self { doubles, ints, double_index: 0, int_index: 0 }
    }

    pub fn from_doubles(doubles: Vec<f64>) -> Self {
        Self::new(doubles, Vec::new())
    }

    pub fn from_ints(ints: Vec<i32>) -> Self {
        Self::new(Vec::new(), ints)
    }
}

impl Rng for ScriptedRng {
    fn next_double(&mut self) -> f64 {
        let value = self.doubles[self.double_index];
        self.double_index += 1;
        value
    }

    fn next_range(&mut self, _min: i32, _max: i32) -> i32 {
        let value = self.ints[self.int_index];
        self.int_index += 1;
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_net_random_matches_published_seed_vectors() {
        // Well-known `new Random(seed).Next()` outputs.
        assert_eq!(DotNetRandom::new(0).next_int(), 1_559_595_546);
        assert_eq!(DotNetRandom::new(42).next_int(), 1_434_747_710);
    }

    #[test]
    fn dot_net_random_is_deterministic_per_seed() {
        let mut a = DotNetRandom::new(12345);
        let mut b = DotNetRandom::new(12345);
        for _ in 0..100 {
            assert_eq!(a.next_double().to_bits(), b.next_double().to_bits());
        }
    }

    #[test]
    fn next_double_is_in_unit_range_and_next_range_is_in_bounds() {
        let mut rng = DotNetRandom::new(7);
        for _ in 0..1000 {
            let d = rng.next_double();
            assert!((0.0..1.0).contains(&d));
            let n = rng.next_range(-3, 4);
            assert!((-3..4).contains(&n));
        }
    }

    #[test]
    fn scripted_rng_replays_values_in_order() {
        let mut rng = ScriptedRng::new(vec![0.25, 0.75], vec![3, 1]);
        assert_eq!(rng.next_double(), 0.25);
        assert_eq!(rng.next_double(), 0.75);
        assert_eq!(rng.next_range(0, 10), 3);
        assert_eq!(rng.next_range(0, 10), 1);
    }

    #[test]
    #[should_panic]
    fn scripted_rng_panics_when_exhausted() {
        ScriptedRng::default().next_double();
    }
}
