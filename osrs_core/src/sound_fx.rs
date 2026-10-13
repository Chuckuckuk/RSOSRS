//! Plays the server's sound effects. The server says which cache sound is heard, from whom and
//! when (`WorldEvent::Sound`); this mixes the synth sound out of the cache the first time it is
//! needed (`osrs_cache::sound`) and plays it, quieter the further its source is from the player.

use bevy::audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source, Volume};
use bevy::prelude::*;
use osrs_cache::{sound, Cache, Index};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Loudness of a sound at its source, before distance: the cache's synth effects are mixed at
/// full scale.
const VOLUME: f32 = 0.5;
/// A sound fades out linearly over this many tiles from the player, as the game's area sounds do.
const HEARING_TILES: f32 = 15.0;

/// One mixed sound effect, as Bevy plays it.
#[derive(Asset, TypePath)]
pub struct Synth {
    samples: Arc<[i8]>,
}

pub struct SynthDecoder {
    samples: Arc<[i8]>,
    next: usize,
}

impl Iterator for SynthDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let s = *self.samples.get(self.next)?;
        self.next += 1;
        Some(s as f32 / 128.0)
    }
}

impl Source for SynthDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(1).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(sound::SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(self.samples.len() as f64 / sound::SAMPLE_RATE as f64))
    }
}

impl Decodable for Synth {
    type Decoder = SynthDecoder;

    fn decoder(&self) -> SynthDecoder {
        SynthDecoder { samples: Arc::clone(&self.samples), next: 0 }
    }
}

/// A sound waiting to be played: the cache id, the entity it comes from, and when.
pub struct PendingSound {
    pub sound: u32,
    pub from: Entity,
    pub at: f32,
}

/// Sounds handed over by `combat_fx`, played by [`play_sounds`] once they are due.
#[derive(Resource, Default)]
pub struct SoundQueue(pub Vec<PendingSound>);

/// The cache, opened on the first sound, and every sound mixed so far (`None`: it would not decode).
#[derive(Resource, Default)]
struct SoundBank {
    source: Option<Option<(Cache, Index)>>,
    mixed: HashMap<u32, Option<Handle<Synth>>>,
}

impl SoundBank {
    fn get(&mut self, id: u32, assets: &mut Assets<Synth>) -> Option<Handle<Synth>> {
        if let Some(known) = self.mixed.get(&id) {
            return known.clone();
        }
        let source = self.source.get_or_insert_with(|| {
            let cache = Cache::open_default().ok()?;
            let index = cache.index(sound::ARCHIVE).ok()?;
            Some((cache, index))
        });
        let handle = source.as_ref().and_then(|(cache, index)| match sound::load_from(cache, index, id) {
            Ok(effect) => Some(assets.add(Synth { samples: effect.mix().into() })),
            Err(e) => {
                warn!("sound {id}: {e}");
                None
            }
        });
        self.mixed.insert(id, handle.clone());
        handle
    }
}

/// How loud a sound is `tiles` away from the player (render space is one unit per tile).
pub fn volume_at(tiles: f32) -> f32 {
    VOLUME * (1.0 - (tiles - 1.0).max(0.0) / HEARING_TILES).clamp(0.0, 1.0)
}

fn play_sounds(
    mut commands: Commands,
    time: Res<Time>,
    mut queue: ResMut<SoundQueue>,
    mut bank: ResMut<SoundBank>,
    mut assets: ResMut<Assets<Synth>>,
    players: Query<&Transform, With<super::Player>>,
    transforms: Query<&Transform>,
) {
    let now = time.elapsed_secs();
    if queue.0.iter().all(|pending| pending.at > now) {
        return;
    }
    let listener = players.single().map(|t| t.translation).ok();
    let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut queue.0).into_iter().partition(|pending| pending.at <= now);
    queue.0 = waiting;
    for pending in due {
        let tiles = match (listener, transforms.get(pending.from)) {
            (Some(ear), Ok(source)) => (source.translation.xz() - ear.xz()).abs().max_element(),
            _ => 0.0,
        };
        let volume = volume_at(tiles);
        if volume <= 0.0 {
            continue;
        }
        if let Some(handle) = bank.get(pending.sound, &mut assets) {
            commands.spawn((AudioPlayer(handle), PlaybackSettings { volume: Volume::Linear(volume), ..PlaybackSettings::DESPAWN }));
        }
    }
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<Synth>()
            .init_resource::<SoundQueue>()
            .init_resource::<SoundBank>()
            .add_systems(Update, play_sounds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_fades_with_distance_and_is_gone_past_hearing() {
        assert_eq!(volume_at(0.0), VOLUME);
        assert_eq!(volume_at(1.0), VOLUME);
        assert!(volume_at(8.0) < VOLUME && volume_at(8.0) > 0.0);
        assert_eq!(volume_at(16.0), 0.0);
        assert_eq!(volume_at(30.0), 0.0);
    }

    #[test]
    fn a_synth_plays_its_samples_once_at_the_cache_rate() {
        let synth = Synth { samples: vec![0i8, 64, -128].into() };
        let decoder = synth.decoder();
        assert_eq!(decoder.sample_rate().get(), sound::SAMPLE_RATE);
        assert_eq!(decoder.collect::<Vec<_>>(), vec![0.0, 0.5, -1.0]);
    }
}
