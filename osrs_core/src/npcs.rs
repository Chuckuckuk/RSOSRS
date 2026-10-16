//! Drawing the server's NPCs: each entity the server reports with an appearance gets a model from
//! the game cache, which stands where the server says it is, walks the tiles it steps through
//! and turns the way it is going.

use super::route::{follow_route, lay_route, Route};
use super::{ServerLink, Textures, Turn};
use bevy::prelude::*;
use osrs_cache::anim::{Animations, Clip};
use osrs_cache::config::Configs;
use osrs_cache::npc::build_npc;
use osrs_cache::scene::LocMesh;
use osrs_cache::Cache;
use super::combat_fx::{heading_between, Fighter};
use osrs_core::actors::EntityId;
use osrs_core::data::CombatAnimations;
use osrs_core::sim::{Combatant, EntitySnapshot};
use osrs_core::tiles::TileCoord;
use osrs_render::pick::GroundSurface;
use osrs_render::streaming::StreamedWorld;
use osrs_render::{spawn_animated, AnimatedModel};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const CLIP_STAND: usize = 0;
const CLIP_WALK: usize = 1;
/// Fights: the clips after standing and walking (each falls back to standing when the NPC has none).
pub const CLIP_ATTACK: usize = 2;
pub const CLIP_DEFEND: usize = 3;
pub const CLIP_DEATH: usize = 4;

/// How long a dead NPC lies there, after its death animation, before it is taken away.
const LINGER_AFTER_DEATH: f32 = 2.0;
/// The longest a death animation is waited out for: some hold their last frame for a minute
/// (Jad's lasts 70 seconds), which is meant to be cut short by the body being taken away.
const LONGEST_DEATH: f32 = 6.0;

/// What the decoding thread is asked for: an NPC definition, its fight animations and the other
/// animations it plays (attacks with their own, healing).
type LookRequest = (u32, Option<CombatAnimations>, Vec<u32>);

/// An NPC model decoded from the cache, with its animations baked against it.
struct NpcLook {
    mesh: LocMesh,
    clips: Vec<Clip>,
    name: String,
    /// Menu options by slot, as the cache lists them ("Talk-to", "Attack", "Pickpocket"...).
    actions: Vec<String>,
    combat_level: Option<u16>,
    /// Lowest and highest corner of the model, in tiles from its feet.
    bounds: (Vec3, Vec3),
}

/// Where a look is: asked for, decoded, or impossible to decode.
enum LookState {
    Loading,
    Ready(NpcLook),
    Failed,
}

/// The looks asked for so far, by cache NPC definition id, the thread decoding them, and the
/// entity drawing each server entity.
pub struct NpcViews {
    looks: HashMap<u32, LookState>,
    spawned: HashMap<EntityId, Entity>,
    /// Entities that died and were taken away: they are still reported, and are not drawn again.
    finished: HashSet<EntityId>,
    requests: Sender<LookRequest>,
    results: Receiver<(u32, Result<NpcLook, String>)>,
}

impl NpcViews {
    /// Starts the thread that decodes looks. It opens the cache and reads the definitions once,
    /// then answers each request with a model and its baked stand and walk animations, so no
    /// frame ever waits on the cache.
    pub fn spawn() -> NpcViews {
        let (request_tx, request_rx) = mpsc::channel::<LookRequest>();
        let (result_tx, result_rx) = mpsc::channel();
        thread::spawn(move || {
            let prepared = prepare();
            for (id, animations, extra_animations) in request_rx {
                let result = match &prepared {
                    Ok(p) => p.look(id, animations, &extra_animations),
                    Err(why) => Err(why.clone()),
                };
                if result_tx.send((id, result)).is_err() {
                    return;
                }
            }
        });
        NpcViews {
            looks: HashMap::new(),
            spawned: HashMap::new(),
            finished: HashSet::new(),
            requests: request_tx,
            results: result_rx,
        }
    }

    /// The entity drawing server entity `id`, if it is in the scene.
    pub fn entity_of(&self, id: EntityId) -> Option<Entity> {
        self.spawned.get(&id).copied()
    }

    /// Takes in the looks that have been decoded since the last call.
    fn collect(&mut self) {
        for (id, result) in self.results.try_iter().collect::<Vec<_>>() {
            let state = match result {
                Ok(look) => LookState::Ready(look),
                Err(why) => {
                    eprintln!("[client] cannot draw NPC definition {id}: {why}");
                    LookState::Failed
                }
            };
            self.looks.insert(id, state);
        }
    }

    /// The look of cache NPC definition `id` if it is decoded; asks for it the first time.
    fn look(&mut self, id: u32, animations: Option<CombatAnimations>, extra_animations: &[u32]) -> Option<&NpcLook> {
        if !self.looks.contains_key(&id) {
            self.looks.insert(id, LookState::Loading);
            let _ = self.requests.send((id, animations, extra_animations.to_vec()));
        }
        match self.looks.get(&id) {
            Some(LookState::Ready(look)) => Some(look),
            _ => None,
        }
    }
}

/// One NPC being drawn.
#[derive(Component)]
pub struct NpcView {
    id: EntityId,
    /// Where the server last said it stands.
    tile: TileCoord,
    seen_tick: Option<i32>,
    route: Route,
    /// Tiles per second along `route`.
    speed: f32,
    facing: f32,
    heading: f32,
    turn: Turn,
    /// Seconds spent standing still.
    still_for: f32,
    name: String,
    actions: Vec<String>,
    combat_level: Option<u16>,
    bounds: (Vec3, Vec3),
    /// Dead: it lies there until it is taken away, and cannot be clicked.
    dead: bool,
    /// When (app time) it died, once it has.
    dead_since: Option<f32>,
}

/// How far up a model its mouth is, as a fraction of its height: where a monster's spit, breath
/// or fireball leaves from. One fraction for every monster, tuned by eye on Jad.
const MOUTH_HEIGHT: f32 = 0.8;
/// ...and how far out along its front, as a fraction of how far the model reaches forward.
const MOUTH_FORWARD: f32 = 0.9;

impl NpcView {
    /// How high the model stands above its feet, in tiles.
    pub(crate) fn top(&self) -> f32 {
        self.bounds.1.y.max(0.5)
    }

    /// Where its mouth is, relative to its feet, in its own frame (the front is +z), when the
    /// model currently stands `top` tiles high: the head rises as it rears and drops as it crouches.
    pub(crate) fn mouth_at(&self, top: f32) -> Vec3 {
        Vec3::new(0.0, top * MOUTH_HEIGHT, self.bounds.1.z.max(0.2) * MOUTH_FORWARD)
    }
}

/// An NPC under the cursor.
#[derive(Debug, Clone)]
pub struct NpcHit {
    /// How far along the ray (world units).
    pub distance: f32,
    pub id: EntityId,
    pub name: String,
    pub actions: Vec<String>,
    pub combat_level: Option<u16>,
}

impl NpcHit {
    /// The options a menu lists for it, in the cache's order: those that are real actions.
    pub fn options(&self) -> impl Iterator<Item = &str> {
        self.actions.iter().map(|a| a.trim()).filter(|a| {
            !a.is_empty() && !a.eq_ignore_ascii_case("hidden") && !a.eq_ignore_ascii_case("walk here")
        })
    }
}

/// The nearest NPC the ray passes through. Each is a box around its model (as wide as the model
/// at its widest, so turning does not change what can be clicked).
pub fn pick_npc<'a>(
    origin: Vec3,
    direction: Vec3,
    npcs: impl Iterator<Item = (&'a NpcView, &'a Transform)>,
) -> Option<NpcHit> {
    let mut best: Option<NpcHit> = None;
    for (view, transform) in npcs {
        if view.dead {
            continue;
        }
        let (lo, hi) = view.bounds;
        let r = lo.x.abs().max(hi.x.abs()).max(lo.z.abs()).max(hi.z.abs()).clamp(0.3, 0.7);
        let at = transform.translation;
        let min = Vec3::new(at.x - r, at.y + lo.y, at.z - r);
        let max = Vec3::new(at.x + r, at.y + hi.y + 0.1, at.z + r);
        let Some(distance) = osrs_render::pick::ray_aabb(origin, direction, min, max) else { continue };
        if best.as_ref().is_none_or(|b| distance < b.distance) {
            best = Some(NpcHit {
                distance,
                id: view.id,
                name: view.name.clone(),
                actions: view.actions.clone(),
                combat_level: view.combat_level,
            });
        }
    }
    best
}

/// How long it keeps its walking pose after it stops advancing, so a late tick does not flicker
/// the animation to standing for a frame.
const WALK_LINGER: f32 = 0.08;

/// What the decoding thread keeps: the cache and the definitions, read once.
struct Prepared {
    cache: Cache,
    defs: HashMap<u32, osrs_cache::npc::NpcDef>,
    texture_hsl: HashMap<u32, u16>,
}

fn prepare() -> Result<Prepared, String> {
    let cache = Cache::open_default().map_err(|e| e.to_string())?;
    let (defs, texture_hsl) = {
        let configs = Configs::new(&cache).map_err(|e| e.to_string())?;
        (
            configs.npcs().map_err(|e| e.to_string())?,
            configs.texture_average_hsl().map_err(|e| e.to_string())?,
        )
    };
    Ok(Prepared { cache, defs, texture_hsl })
}

impl Prepared {
    /// Decodes the model of NPC definition `id` and bakes its animations: standing, walking, the
    /// fight ones (`CLIP_ATTACK`..`CLIP_DEATH`), then each of `extra_animations` that fits its rig,
    /// found again by sequence (`AnimatedModel::clip_of_sequence`).
    fn look(&self, id: u32, animations: Option<CombatAnimations>, extra_animations: &[u32]) -> Result<NpcLook, String> {
        let def = self.defs.get(&id).ok_or("no such NPC definition")?;
        let mesh = build_npc(&self.cache, def, &self.texture_hsl).ok_or("its models are missing from the cache")?;
        let rig = mesh.rig.as_ref().ok_or("the model kept no rig")?;

        let mut animations_source = Animations::new(&self.cache).map_err(|e| e.to_string())?;
        let mut clips = Vec::new();
        let stand = def.stand.ok_or("the definition has no animations")?;
        let fight = animations.map(|a| [a.attack, a.defend, a.death]);
        let wanted = [Some(stand), def.walk, fight.map(|f| f[0]), fight.map(|f| f[1]), fight.map(|f| f[2])];
        for sequence in wanted {
            // a clip the NPC lacks (or that does not fit its rig) stands in as its standing one
            let baked = sequence.and_then(|s| Clip::bake(&mut animations_source, s, rig).ok());
            match baked {
                Some(clip) => clips.push(clip),
                None => {
                    let stand_clip = clips.first().cloned().ok_or("the standing animation did not bake")?;
                    clips.push(stand_clip);
                }
            }
        }
        for &sequence in extra_animations {
            match Clip::bake(&mut animations_source, sequence, rig) {
                Ok(clip) => clips.push(clip),
                Err(e) => eprintln!("[client] NPC {id} ({}): animation {sequence} does not bake: {e}", def.name),
            }
        }
        println!("[client] NPC {id} ({}): {} models, {} animations baked", def.name, def.models.len(), clips.len());

        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let parts = [&mesh.opaque, &mesh.translucent].into_iter().chain(mesh.textured.values());
        for part in parts {
            for p in &part.positions {
                lo = lo.min(Vec3::from(*p));
                hi = hi.max(Vec3::from(*p));
            }
        }
        if lo.x > hi.x {
            (lo, hi) = (Vec3::new(-0.4, 0.0, -0.4), Vec3::new(0.4, 1.6, 0.4));
        }
        Ok(NpcLook {
            mesh,
            clips,
            name: def.name.clone(),
            actions: def.actions.clone(),
            combat_level: def.combat_level.filter(|l| *l > 0),
            bounds: (lo, hi),
        })
    }
}

/// Where an NPC whose south-west tile is `tile` is drawn: the middle of its `size` x `size`
/// footprint, on the ground there (render space).
fn footprint_centre(streamed: &StreamedWorld, tile: TileCoord, size: i32) -> Option<Vec3> {
    let half = size.max(1) as f32 / 2.0;
    let (x, z) = (tile.x as f32 + half, tile.z as f32 + half);
    let y = streamed.surface_y(0, x, z)?;
    Some(Vec3::new(x, y, -z))
}

/// Spawns models for entities the server has started reporting, removes those it no longer does,
/// and moves the rest.
#[allow(clippy::too_many_arguments)]
pub fn sync_npcs(
    mut commands: Commands,
    mut views: NonSendMut<NpcViews>,
    server: NonSend<ServerLink>,
    streamed: Res<StreamedWorld>,
    textures: Res<Textures>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut npcs: Query<(&mut NpcView, &mut Transform, &mut AnimatedModel, &mut Fighter)>,
    players: Query<&Transform, (With<super::Player>, Without<NpcView>)>,
) {
    let Some(snapshot) = &server.latest else { return };
    views.collect();

    // entities that appeared
    let reported: Vec<&EntitySnapshot> = snapshot.entities.iter().filter(|e| e.appearance.is_some()).collect();
    for entity in &reported {
        if views.spawned.contains_key(&entity.id) {
            continue;
        }
        let Some(appearance) = entity.appearance else { continue };
        let Some(feet) = footprint_centre(&streamed, entity.tile, entity.size) else { continue };
        // not drawn until the thread has its look ready
        if views.finished.contains(&entity.id) || entity.is_dead {
            continue;
        }
        let Some(look) = views.look(appearance, entity.combat_animations, &entity.extra_animations) else { continue };
        let (mesh, clips) = (look.mesh.clone(), look.clips.clone());
        let (name, actions, combat_level, bounds) =
            (look.name.clone(), look.actions.clone(), look.combat_level, look.bounds);
        let model = spawn_animated(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &textures.0 .0,
            &mesh,
            clips,
            Transform::from_translation(feet),
        );
        commands.entity(model).insert(NpcView {
            id: entity.id,
            tile: entity.tile,
            seen_tick: None,
            route: Route::new(),
            speed: 0.0,
            facing: 0.0,
            heading: 0.0,
            turn: Turn::default(),
            still_for: 0.0,
            name,
            actions,
            combat_level,
            bounds,
            dead: false,
            dead_since: None,
        });
        commands.entity(model).insert(Fighter::default());
        views.spawned.insert(entity.id, model);
        println!("[client] {} (entity {}) appears at ({}, {})", entity.name, entity.id.0, entity.tile.x, entity.tile.z);
    }

    // entities that went away
    let gone: Vec<EntityId> =
        views.spawned.keys().copied().filter(|id| !reported.iter().any(|e| e.id == *id)).collect();
    for id in gone {
        if let Some(model) = views.spawned.remove(&id) {
            commands.entity(model).despawn();
        }
    }

    // where everyone stands, for turning to face whom they are fighting
    let player_at = players.single().ok().map(|t| t.translation);
    let npc_at: HashMap<EntityId, Vec3> = npcs.iter().map(|(view, transform, _, _)| (view.id, transform.translation)).collect();

    // everything that is there
    let now = time.elapsed_secs();
    let mut taken_away = Vec::new();
    for (mut view, mut transform, mut model, mut fighter) in &mut npcs {
        let Some(report) = reported.iter().find(|e| e.id == view.id) else { continue };
        let size = report.size;
        let ground = |tile: TileCoord| footprint_centre(&streamed, tile, size);
        fighter.health = Some((report.health, report.max_health));
        if report.is_dead {
            fighter.dead = true;
        }
        if fighter.dead {
            view.dead = true;
            let since = *view.dead_since.get_or_insert(now);
            if now - since > model.clip_seconds(CLIP_DEATH).min(LONGEST_DEATH) + LINGER_AFTER_DEATH {
                taken_away.push(view.id);
            }
            view.route.clear();
        }
        if let Some(heading) = fighter.face.take() {
            view.heading = heading;
        }

        if view.seen_tick != Some(snapshot.tick) {
            view.seen_tick = Some(snapshot.tick);
            if report.step_tiles.is_empty() {
                if report.tile != view.tile {
                    // appeared elsewhere without stepping (a teleport)
                    if let Some(p) = ground(report.tile) {
                        transform.translation = p;
                    }
                    view.route.clear();
                }
            } else {
                // drawn one tick behind the server, like the player: this tick's steps are
                // walked during the next one
                let (route, length) = lay_route(&report.step_tiles, view.tile, transform.translation, ground);
                view.route = route;
                view.speed = length / (snapshot.tick_seconds.max(0.05) * server.ticks_in_latest.max(1) as f32);
            }
            view.tile = report.tile;
        }

        let moving = !view.route.is_empty();
        let budget = view.speed * time.delta_secs();
        let surface = |x: f32, z: f32| streamed.surface_y(0, x, z);
        if let Some(heading) = follow_route(&mut view.route, &mut transform, budget, surface) {
            view.heading = heading;
        }
        // an NPC with a target faces it all the time, whichever way it is walking
        let target_at = match report.facing {
            Some(Combatant::Player) => player_at,
            Some(Combatant::Npc(id)) => npc_at.get(&id).copied(),
            None => None,
        };
        if let Some(target) = target_at.filter(|_| !view.dead) {
            let here = transform.translation;
            if (target - here).with_y(0.0).length_squared() > 1e-4 {
                view.heading = heading_between(here, target);
            }
        }
        let (heading, facing, dt) = (view.heading, view.facing, time.delta_secs());
        view.facing = view.turn.step(facing, heading, dt);
        transform.rotation = Quat::from_rotation_y(view.facing);

        view.still_for = if moving { 0.0 } else { view.still_for + dt };
        let walking = moving || view.still_for <= WALK_LINGER && !report.step_tiles.is_empty();
        match fighter.clip(now, CLIP_DEATH) {
            Some((clip, first, _)) => {
                if first {
                    model.play_once(clip);
                }
                model.set_hover(fighter.hover_at());
            }
            None => model.play(if walking { CLIP_WALK } else { CLIP_STAND }),
        }
    }

    // the dead that have lain long enough go
    for id in taken_away {
        if let Some(model) = views.spawned.remove(&id) {
            commands.entity(model).despawn();
        }
        views.finished.insert(id);
    }
}
