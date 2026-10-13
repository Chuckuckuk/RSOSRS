//! What the player character looks like: its kit (hair, torso...) plus whatever is worn. When the
//! server's worn equipment changes, a worker thread builds the character again (with its animations
//! baked against the new model) and the client swaps it in, so nothing waits on the cache.

use super::{Player, ServerLink, Textures, PLAYER_CLIPS};
use bevy::prelude::*;
use osrs_cache::anim::{Animations, Clip};
use osrs_cache::config::Configs;
use osrs_cache::item::{ids_by_name, ItemDef};
use osrs_cache::kit::{build_player, Appearance, WornPart};
use osrs_cache::scene::LocMesh;
use osrs_cache::Cache;
use osrs_render::AnimatedModel;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

type Built = Result<(LocMesh, Vec<Clip>), String>;

/// What the character is asked to look like and be able to do: the worn items (by name, in the
/// snapshot's slot order) and the sequences its own attack and block play.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LookKey {
    worn: Vec<String>,
    attack: u32,
    defend: u32,
}

/// The thread that builds characters, and what has been asked of it.
pub struct PlayerLooks {
    requests: Sender<LookKey>,
    results: Receiver<Built>,
    /// What the character was last asked to look like.
    wanted: Option<LookKey>,
}

impl PlayerLooks {
    pub fn spawn() -> PlayerLooks {
        let (request_tx, request_rx) = mpsc::channel::<LookKey>();
        let (result_tx, result_rx) = mpsc::channel();
        thread::spawn(move || {
            let prepared = Prepared::open();
            while let Ok(mut key) = request_rx.recv() {
                // only the newest request matters: skip what a later one has replaced
                while let Ok(newer) = request_rx.try_recv() {
                    key = newer;
                }
                let built = match &prepared {
                    Ok(p) => p.build(&key),
                    Err(why) => Err(why.clone()),
                };
                if result_tx.send(built).is_err() {
                    return;
                }
            }
        });
        PlayerLooks { requests: request_tx, results: result_rx, wanted: None }
    }
}

struct Prepared {
    cache: Cache,
    items: HashMap<u32, ItemDef>,
    by_name: HashMap<String, u32>,
    texture_hsl: HashMap<u32, u16>,
}

impl Prepared {
    fn open() -> Result<Prepared, String> {
        let cache = Cache::open_default().map_err(|e| e.to_string())?;
        let (items, texture_hsl) = {
            let configs = Configs::new(&cache).map_err(|e| e.to_string())?;
            (configs.items().map_err(|e| e.to_string())?, configs.texture_average_hsl().map_err(|e| e.to_string())?)
        };
        let by_name = ids_by_name(&items);
        Ok(Prepared { cache, items, by_name, texture_hsl })
    }

    fn build(&self, key: &LookKey) -> Built {
        let worn: Vec<WornPart> = key
            .worn
            .iter()
            .filter_map(|name| {
                let lower = name.to_lowercase();
                let id = self.by_name.get(&lower).or_else(|| self.by_name.get(&format!("{lower}(4)")))?;
                WornPart::of(self.items.get(id)?, false)
            })
            .collect();
        let mesh = build_player(&self.cache, &Appearance::default_male(), &worn, &self.texture_hsl)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "the character has no models".to_owned())?;
        let rig = mesh.rig.as_ref().ok_or_else(|| "the character has no rig".to_owned())?;
        let mut animations = Animations::new(&self.cache).map_err(|e| e.to_string())?;
        let mut clips = Vec::new();
        for (index, (_, sequence)) in PLAYER_CLIPS.into_iter().enumerate() {
            // the attack and block are the wielded weapon's; if one cannot be baked the fists' stay
            let wanted = match index {
                super::CLIP_ATTACK => key.attack,
                super::CLIP_BLOCK => key.defend,
                _ => sequence,
            };
            let clip = Clip::bake(&mut animations, wanted, rig)
                .or_else(|e| if wanted != sequence { Clip::bake(&mut animations, sequence, rig) } else { Err(e) })
                .map_err(|e| e.to_string())?;
            clips.push(clip);
        }
        Ok((mesh, clips))
    }
}

pub struct PlayerLookPlugin;

impl Plugin for PlayerLookPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send(PlayerLooks::spawn()).add_systems(Update, refresh_player_look);
    }
}

#[allow(clippy::too_many_arguments)]
fn refresh_player_look(
    mut commands: Commands,
    server: NonSend<ServerLink>,
    mut looks: NonSendMut<PlayerLooks>,
    textures: Res<Textures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut players: Query<(Entity, &mut AnimatedModel), With<Player>>,
) {
    if let Some(snapshot) = &server.latest {
        let key = LookKey {
            worn: snapshot.player.equipped.iter().map(|(_, item)| item.name.clone()).collect(),
            attack: snapshot.player.animations.attack,
            defend: snapshot.player.animations.defend,
        };
        // nothing to do while it is how the scene was first built: bare hands and fists
        let initial = LookKey { worn: Vec::new(), attack: PLAYER_CLIPS[super::CLIP_ATTACK].1, defend: PLAYER_CLIPS[super::CLIP_BLOCK].1 };
        let current = looks.wanted.clone().unwrap_or(initial);
        if key != current {
            looks.wanted = Some(key.clone());
            let _ = looks.requests.send(key);
        }
    }
    // the newest finished look is the one to wear
    let latest = looks.results.try_iter().last();
    let Some(result) = latest else { return };
    match result {
        Ok((mesh, clips)) => {
            for (entity, mut model) in &mut players {
                osrs_render::replace_animated(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut images,
                    &textures.0 .0,
                    entity,
                    &mut model,
                    &mesh,
                    clips.clone(),
                );
            }
        }
        Err(why) => eprintln!("[client] cannot dress the character: {why}"),
    }
}
