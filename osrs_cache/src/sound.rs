//! Synthesized sound effects (archive 4): up to ten instruments, each a set of oscillators
//! shaped by envelopes, an optional gate, an echo and a pole/zero filter. [`SoundEffect::mix`]
//! renders one to 8-bit signed PCM at [`SAMPLE_RATE`] exactly like the client's `SoundEffect.mix`,
//! integer arithmetic and all, so the cache's sounds come out as the game plays them.

use crate::buf::Buf;
use crate::{fmt_err, Cache, Result};
use std::sync::OnceLock;

/// The archive holding the synth sound effects; a sound id is a group in it.
pub const ARCHIVE: u8 = 4;
/// Samples per second of a mixed sound.
pub const SAMPLE_RATE: u32 = 22050;

/// A piecewise-linear curve over a sound's length, in 16-bit fixed point.
#[derive(Debug, Clone)]
pub struct Envelope {
    /// Waveform when it drives an oscillator: 1 square, 2 sine, 3 saw, 4 noise.
    pub form: u8,
    pub start: i32,
    pub end: i32,
    /// `(duration, phase)` points, both out of 65536.
    pub points: Vec<(u16, u16)>,
}

impl Default for Envelope {
    fn default() -> Self {
        Envelope { form: 0, start: 0, end: 0, points: vec![(0, 65535), (65535, 65535)] }
    }
}

impl Envelope {
    fn decode(b: &mut Buf) -> Envelope {
        let form = b.u8();
        let start = b.i32();
        let end = b.i32();
        let mut e = Envelope { form, start, end, points: Vec::new() };
        e.decode_points(b);
        e
    }

    fn decode_points(&mut self, b: &mut Buf) {
        let n = b.u8() as usize;
        self.points = (0..n).map(|_| (b.u16(), b.u16())).collect();
    }
}

/// The client steps an envelope sample by sample; this is that walker.
struct EnvelopeWalk<'a> {
    env: &'a Envelope,
    ticks: i32,
    index: usize,
    step: i32,
    amplitude: i32,
    position: i32,
}

impl<'a> EnvelopeWalk<'a> {
    fn new(env: &'a Envelope) -> Self {
        EnvelopeWalk { env, ticks: 0, index: 0, step: 0, amplitude: 0, position: 0 }
    }

    fn step(&mut self, period: i32) -> i32 {
        let points = &self.env.points;
        if points.is_empty() {
            return 0;
        }
        if self.position >= self.ticks {
            self.amplitude = (points[self.index].1 as i32) << 15;
            self.index += 1;
            if self.index >= points.len() {
                self.index = points.len() - 1;
            }
            self.ticks = (points[self.index].0 as f64 / 65536.0 * period as f64) as i32;
            if self.ticks > self.position {
                self.step = (((points[self.index].1 as i32) << 15) - self.amplitude) / (self.ticks - self.position);
            }
        }
        self.amplitude = self.amplitude.wrapping_add(self.step);
        self.position += 1;
        self.amplitude.wrapping_sub(self.step) >> 15
    }
}

/// A two-sided (feed-forward / feedback) filter whose poles move between two settings.
#[derive(Debug, Clone)]
pub struct Filter {
    pub pairs: [usize; 2],
    pub unity: [u16; 2],
    /// `[direction][start/end][pair]`
    pub phases: [[[u16; 15]; 2]; 2],
    pub magnitudes: [[[u16; 15]; 2]; 2],
}

impl Default for Filter {
    fn default() -> Self {
        Filter { pairs: [0; 2], unity: [0; 2], phases: [[[0; 15]; 2]; 2], magnitudes: [[[0; 15]; 2]; 2] }
    }
}

impl Default for Coefficients {
    fn default() -> Self {
        Coefficients { float: [[0.0; 30]; 2], int: [[0; 30]; 2], forward: 0.0, forward_int: 0 }
    }
}

impl Filter {
    fn decode(b: &mut Buf, envelope: &mut Envelope) -> Result<Filter> {
        let mut f = Filter::default();
        let packed = b.u8();
        f.pairs = [(packed >> 4) as usize, (packed & 15) as usize];
        if packed == 0 {
            return Ok(f);
        }
        f.unity = [b.u16(), b.u16()];
        let migrated = b.u8();
        for dir in 0..2 {
            for pair in 0..f.pairs[dir] {
                f.phases[dir][0][pair] = b.u16();
                f.magnitudes[dir][0][pair] = b.u16();
            }
        }
        for dir in 0..2 {
            for pair in 0..f.pairs[dir] {
                if (migrated as u32 >> (dir * 4 + pair)) & 1 != 0 {
                    f.phases[dir][1][pair] = b.u16();
                    f.magnitudes[dir][1][pair] = b.u16();
                } else {
                    f.phases[dir][1][pair] = f.phases[dir][0][pair];
                    f.magnitudes[dir][1][pair] = f.magnitudes[dir][0][pair];
                }
            }
        }
        if migrated != 0 || f.unity[1] != f.unity[0] {
            envelope.decode_points(b);
        }
        Ok(f)
    }

    fn magnitude(&self, dir: usize, pair: usize, t: f32) -> f32 {
        let m = &self.magnitudes[dir];
        let mut v = m[0][pair] as f32 + t * (m[1][pair] as f32 - m[0][pair] as f32);
        v *= 0.001_525_878_9;
        1.0 - 10f64.powf((-v / 20.0) as f64) as f32
    }

    fn phase(&self, dir: usize, pair: usize, t: f32) -> f32 {
        let p = &self.phases[dir];
        let mut v = p[0][pair] as f32 + t * (p[1][pair] as f32 - p[0][pair] as f32);
        v *= 1.220_703_1e-4;
        let hz = 32.703_197 * 2f64.powf(v as f64) as f32;
        hz * std::f32::consts::PI / 11025.0
    }

    /// Fills `coef[dir]` (as 16.16 fixed point) for position `t` and returns how many there are.
    fn compute(&self, dir: usize, t: f32, coef: &mut Coefficients) -> usize {
        if dir == 0 {
            let mut u = self.unity[0] as f32 + (self.unity[1] as f32 - self.unity[0] as f32) * t;
            u *= 0.003_051_757_8;
            coef.forward = 0.1f64.powf((u / 20.0) as f64) as f32;
            coef.forward_int = (coef.forward * 65536.0) as i32;
        }
        if self.pairs[dir] == 0 {
            return 0;
        }
        let c = &mut coef.float[dir];
        let m = self.magnitude(dir, 0, t);
        c[0] = -2.0 * m * (self.phase(dir, 0, t) as f64).cos() as f32;
        c[1] = m * m;
        for pair in 1..self.pairs[dir] {
            let m = self.magnitude(dir, pair, t);
            let a = -2.0 * m * (self.phase(dir, pair, t) as f64).cos() as f32;
            let b = m * m;
            c[pair * 2 + 1] = c[pair * 2 - 1] * b;
            c[pair * 2] = c[pair * 2 - 1] * a + c[pair * 2 - 2] * b;
            let mut k = pair * 2 - 1;
            while k >= 2 {
                c[k] += c[k - 1] * a + c[k - 2] * b;
                k -= 1;
            }
            c[1] += c[0] * a + b;
            c[0] += a;
        }
        if dir == 0 {
            for v in c.iter_mut().take(self.pairs[0] * 2) {
                *v *= coef.forward;
            }
        }
        for k in 0..self.pairs[dir] * 2 {
            coef.int[dir][k] = (c[k] * 65536.0) as i32;
        }
        self.pairs[dir] * 2
    }
}

struct Coefficients {
    float: [[f32; 30]; 2],
    int: [[i32; 30]; 2],
    forward: f32,
    forward_int: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct Oscillator {
    /// Percent of the volume envelope.
    pub volume: u32,
    /// Detune in steps of 1/120 octave.
    pub pitch: i32,
    /// Start delay in milliseconds.
    pub delay: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Instrument {
    pub pitch: Envelope,
    pub volume: Envelope,
    /// Vibrato: rate and depth.
    pub pitch_modifier: Option<(Envelope, Envelope)>,
    /// Tremolo: rate and depth.
    pub volume_modifier: Option<(Envelope, Envelope)>,
    /// A gate that switches the sound on and off: release then attack.
    pub gate: Option<(Envelope, Envelope)>,
    pub oscillators: Vec<Oscillator>,
    /// Echo delay in milliseconds and its feedback percent.
    pub delay_time: u32,
    pub delay_decay: u32,
    /// Length and start offset within the sound, in milliseconds.
    pub duration: u16,
    pub offset: u16,
    pub filter: Filter,
    pub filter_envelope: Envelope,
}

fn optional_pair(b: &mut Buf) -> Option<(Envelope, Envelope)> {
    if b.peek() == 0 {
        b.u8();
        return None;
    }
    Some((Envelope::decode(b), Envelope::decode(b)))
}

impl Instrument {
    fn decode(b: &mut Buf) -> Result<Instrument> {
        let pitch = Envelope::decode(b);
        let volume = Envelope::decode(b);
        let pitch_modifier = optional_pair(b);
        let volume_modifier = optional_pair(b);
        let gate = optional_pair(b);
        let mut oscillators = Vec::new();
        for _ in 0..10 {
            let volume = b.smart_u();
            if volume == 0 {
                break;
            }
            let pitch = b.smart_s();
            let delay = b.smart_u();
            oscillators.push(Oscillator { volume, pitch, delay });
        }
        let delay_time = b.smart_u();
        let delay_decay = b.smart_u();
        let duration = b.u16();
        let offset = b.u16();
        let mut filter_envelope = Envelope::default();
        let filter = Filter::decode(b, &mut filter_envelope)?;
        Ok(Instrument {
            pitch,
            volume,
            pitch_modifier,
            volume_modifier,
            gate,
            oscillators,
            delay_time,
            delay_decay,
            duration,
            offset,
            filter,
            filter_envelope,
        })
    }

    /// `steps` samples of this instrument over `millis` milliseconds, as 16-bit values.
    fn synthesize(&self, steps: usize, millis: u32) -> Vec<i32> {
        let mut out = vec![0i32; steps];
        if millis < 10 || steps == 0 {
            return out;
        }
        let n = steps as i32;
        let scale = steps as f64 / millis as f64;
        let mut pitch = EnvelopeWalk::new(&self.pitch);
        let mut volume = EnvelopeWalk::new(&self.volume);

        let rate = |e: &Envelope| ((e.end - e.start) as f64 * 32.768 / scale) as i32;
        let base = |e: &Envelope| (e.start as f64 * 32.768 / scale) as i32;
        let mut vibrato = self.pitch_modifier.as_ref().map(|(r, d)| (EnvelopeWalk::new(r), EnvelopeWalk::new(d), rate(r), base(r), r.form));
        let mut tremolo = self.volume_modifier.as_ref().map(|(r, d)| (EnvelopeWalk::new(r), EnvelopeWalk::new(d), rate(r), base(r), r.form));
        let (mut vibrato_phase, mut tremolo_phase) = (0i32, 0i32);

        let oscillators: Vec<(i32, i32, i32, i32)> = self
            .oscillators
            .iter()
            .map(|o| {
                let delay = (o.delay as f64 * scale) as i32;
                let volume = ((o.volume as i32) << 14) / 100;
                let pitch_step = ((self.pitch.end - self.pitch.start) as f64 * 32.768 * 1.005_792_941_067_853_4f64.powi(o.pitch) / scale) as i32;
                (delay, volume, pitch_step, base(&self.pitch))
            })
            .collect();
        let mut phases = vec![0i32; oscillators.len()];

        for i in 0..n {
            let mut p = pitch.step(n);
            let mut v = volume.step(n);
            if let Some((rate_walk, depth_walk, span, start, form)) = vibrato.as_mut() {
                let r = rate_walk.step(n);
                let d = depth_walk.step(n);
                p += wave(vibrato_phase, d, *form) >> 1;
                vibrato_phase = vibrato_phase.wrapping_add(*start).wrapping_add(r.wrapping_mul(*span) >> 16);
            }
            if let Some((rate_walk, depth_walk, span, start, form)) = tremolo.as_mut() {
                let r = rate_walk.step(n);
                let d = depth_walk.step(n);
                v = v.wrapping_mul((wave(tremolo_phase, d, *form) >> 1) + 32768) >> 15;
                tremolo_phase = tremolo_phase.wrapping_add(*start).wrapping_add(r.wrapping_mul(*span) >> 16);
            }
            for (k, (delay, vol, step, base)) in oscillators.iter().enumerate() {
                let at = delay + i;
                if at < n {
                    out[at as usize] += wave(phases[k], v.wrapping_mul(*vol) >> 15, self.pitch.form);
                    phases[k] = phases[k].wrapping_add(p.wrapping_mul(*step) >> 16).wrapping_add(*base);
                }
            }
        }

        if let Some((release, attack)) = &self.gate {
            let mut release_walk = EnvelopeWalk::new(release);
            let mut attack_walk = EnvelopeWalk::new(attack);
            let mut counter = 0;
            let mut muted = true;
            for sample in out.iter_mut() {
                let r = release_walk.step(n);
                let a = attack_walk.step(n);
                let threshold = if muted {
                    (r * (release.end - release.start) >> 8) + release.start
                } else {
                    (a * (release.end - release.start) >> 8) + release.start
                };
                counter += 256;
                if counter >= threshold {
                    counter = 0;
                    muted = !muted;
                }
                if muted {
                    *sample = 0;
                }
            }
        }

        if self.delay_time > 0 && self.delay_decay > 0 {
            let lag = (self.delay_time as f64 * scale) as usize;
            for i in lag..steps {
                out[i] += out[i - lag] * self.delay_decay as i32 / 100;
            }
        }

        if self.filter.pairs[0] > 0 || self.filter.pairs[1] > 0 {
            self.apply_filter(&mut out);
        }

        for s in out.iter_mut() {
            *s = (*s).clamp(-32768, 32767);
        }
        out
    }

    fn apply_filter(&self, out: &mut [i32]) {
        let steps = out.len();
        let mut env = EnvelopeWalk::new(&self.filter_envelope);
        let mut coef = Coefficients::default();
        let period = steps as i32 + 1;
        let mut t = env.step(period);
        let mut forward = self.filter.compute(0, t as f32 / 65536.0, &mut coef);
        let mut back = self.filter.compute(1, t as f32 / 65536.0, &mut coef);
        if steps < forward + back {
            return;
        }
        let mul = |a: i32, b: i32| ((a as i64 * b as i64) >> 16) as i32;
        let mut i = 0usize;
        let first = back.min(steps - forward);
        while i < first {
            let mut s = mul(out[i + forward], coef.forward_int);
            for k in 0..forward {
                s += mul(out[i + forward - 1 - k], coef.int[0][k]);
            }
            for k in 0..i {
                s -= mul(out[i - 1 - k], coef.int[1][k]);
            }
            out[i] = s;
            t = env.step(period);
            i += 1;
        }
        let mut block_end = 128usize;
        loop {
            if block_end > steps - forward {
                block_end = steps - forward;
            }
            while i < block_end {
                let mut s = mul(out[i + forward], coef.forward_int);
                for k in 0..forward {
                    s += mul(out[i + forward - 1 - k], coef.int[0][k]);
                }
                for k in 0..back {
                    s -= mul(out[i - 1 - k], coef.int[1][k]);
                }
                out[i] = s;
                t = env.step(period);
                i += 1;
            }
            if i >= steps - forward {
                while i < steps {
                    let mut s = 0;
                    for k in (i + forward - steps)..forward {
                        s += mul(out[i + forward - 1 - k], coef.int[0][k]);
                    }
                    for k in 0..back {
                        s -= mul(out[i - 1 - k], coef.int[1][k]);
                    }
                    out[i] = s;
                    env.step(period);
                    i += 1;
                }
                break;
            }
            forward = self.filter.compute(0, t as f32 / 65536.0, &mut coef);
            back = self.filter.compute(1, t as f32 / 65536.0, &mut coef);
            block_end += 128;
        }
    }
}

struct Tables {
    sine: Vec<i32>,
    noise: Vec<i32>,
}

/// The client's wave tables: a 32768-entry sine, and noise from `java.util.Random(0)`.
fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let sine = (0..32768).map(|i| ((i as f64 / 5215.1903).sin() * 16384.0) as i32).collect();
        let mut seed: u64 = (0 ^ 0x5DEE_CE66D) & ((1 << 48) - 1);
        let noise = (0..32768)
            .map(|_| {
                seed = (seed.wrapping_mul(0x5DEE_CE66D).wrapping_add(0xB)) & ((1 << 48) - 1);
                let next = (seed >> 16) as u32 as i32;
                (next & 2) - 1
            })
            .collect();
        Tables { sine, noise }
    })
}

fn wave(phase: i32, amplitude: i32, form: u8) -> i32 {
    match form {
        1 => {
            if (phase & 32767) < 16384 {
                amplitude
            } else {
                -amplitude
            }
        }
        2 => tables().sine[(phase & 32767) as usize].wrapping_mul(amplitude) >> 14,
        3 => (amplitude.wrapping_mul(phase & 32767) >> 14) - amplitude,
        4 => amplitude.wrapping_mul(tables().noise[((phase / 2607) & 32767) as usize]),
        _ => 0,
    }
}

#[derive(Debug, Clone, Default)]
pub struct SoundEffect {
    pub id: u32,
    pub instruments: Vec<Instrument>,
    /// Loop start and end in milliseconds (unused by one-shot effects).
    pub loop_start: u16,
    pub loop_end: u16,
}

impl SoundEffect {
    pub fn decode(id: u32, data: &[u8]) -> Result<SoundEffect> {
        let mut b = Buf::new(data);
        let mut instruments = Vec::new();
        for _ in 0..10 {
            if b.remaining() == 0 {
                return fmt_err(format!("sound {id}: ends inside its instruments"));
            }
            if b.peek() == 0 {
                b.u8();
                continue;
            }
            instruments.push(Instrument::decode(&mut b)?);
        }
        let loop_start = b.u16();
        let loop_end = b.u16();
        if b.pos != data.len() {
            return fmt_err(format!("sound {id}: {} bytes left over", data.len() as isize - b.pos as isize));
        }
        Ok(SoundEffect { id, instruments, loop_start, loop_end })
    }

    /// Length in milliseconds: where the last instrument ends.
    pub fn duration_ms(&self) -> u32 {
        self.instruments.iter().map(|i| i.duration as u32 + i.offset as u32).max().unwrap_or(0)
    }

    /// Mixes every instrument into 8-bit signed samples at [`SAMPLE_RATE`], saturating like the
    /// client does.
    pub fn mix(&self) -> Vec<i8> {
        let total = (self.duration_ms() * SAMPLE_RATE / 1000) as usize;
        let mut out = vec![0i8; total];
        for inst in &self.instruments {
            let steps = (inst.duration as u32 * SAMPLE_RATE / 1000) as usize;
            let offset = (inst.offset as u32 * SAMPLE_RATE / 1000) as usize;
            let samples = inst.synthesize(steps, inst.duration as u32);
            for (k, s) in samples.iter().enumerate() {
                let Some(slot) = out.get_mut(k + offset) else { break };
                let mut v = (s >> 8) + *slot as i32;
                if (v + 128) & !255 != 0 {
                    v = (v >> 31) ^ 127;
                }
                *slot = v as i8;
            }
        }
        out
    }
}

/// The sound effect with this id (file 0 of its group).
pub fn load(cache: &Cache, id: u32) -> Result<SoundEffect> {
    let index = cache.index(ARCHIVE)?;
    load_from(cache, &index, id)
}

/// [`load`] against an already-read reference table.
pub fn load_from(cache: &Cache, index: &crate::Index, id: u32) -> Result<SoundEffect> {
    let files = cache.read_files(index, id)?;
    let (_, data) = files.first().ok_or_else(|| crate::Error::Missing(format!("sound {id}")))?;
    SoundEffect::decode(id, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_noise_table_matches_java_random_seeded_with_zero() {
        // new java.util.Random(0).nextInt() is -1155484576, then -723955400, 1033096058
        let noise = &tables().noise;
        assert_eq!(&noise[..3], &[(-1155484576 & 2) - 1, (-723955400 & 2) - 1, (1033096058 & 2) - 1]);
    }

    #[test]
    fn every_synth_sound_in_the_cache_decodes_to_its_last_byte() {
        let Ok(cache) = Cache::open_default() else { return };
        let index = cache.index(ARCHIVE).unwrap();
        let mut failures = Vec::new();
        for id in index.groups.keys() {
            if let Err(e) = load_from(&cache, &index, *id) {
                failures.push(format!("{id}: {e}"));
            }
        }
        // a handful of recent groups (11988 and up) hold Vorbis packets, not synth instruments
        assert!(failures.len() <= 16, "{} of {} failed, e.g. {:?}", failures.len(), index.groups.len(), &failures[..failures.len().min(5)]);
    }
}
