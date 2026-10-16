//! Projectiles in flight: the arrow a bow looses travels from the shooter to the target as the
//! cache's own graphic (a model, recoloured by tier), arriving as the hit lands. Also graphics
//! played on a character (`Spot`): the rocks Jad's stomp drops, a spell's splash.

use super::Textures;
use bevy::prelude::*;
use osrs_cache::config::Configs;
use osrs_cache::scene::LocMesh;
use osrs_cache::anim::{Animations, Clip};
use osrs_cache::spotanim::{build_spotanim, build_spotanim_with_rig, SpotAnim};
use osrs_cache::Cache;
use std::collections::HashMap;

/// How high above the feet a projectile leaves and arrives, in tiles.
const LAUNCH_HEIGHT: f32 = 1.1;
const TARGET_HEIGHT: f32 = 0.9;
/// How far a flight bows upward at its middle, in tiles per tile travelled.
const ARC: f32 = 0.06;

/// A shot to draw. The shooter nocks and draws (`draw`, a graphic with its own animation that
/// ends as the arrow is let go), the projectile leaves when that animation has played and arrives
/// `hit_in` seconds after the shot began, as the hit lands. Without a draw graphic the arrow leaves
/// partway through the attack animation (`swing` seconds long).
pub struct Shot {
    pub from: Entity,
    pub to: Entity,
    pub graphic: u32,
    pub draw: Option<u32>,
    /// Whether it leaves the shooter's mouth, when it has no draw graphic (a monster's); `None`
    /// is chest height. Found when it leaves, from the pose the model is in then.
    pub from_mouth: bool,
    pub swing: f32,
    pub hit_in: f32,
}

/// How far through a swing without a draw graphic the arrow is let go.
const RELEASE_FRACTION: f32 = 0.55;
/// Least a projectile is seen flying, seconds.
const MIN_FLIGHT: f32 = 0.25;
/// The draw graphic is shown this high above the archer's feet (the game's "height 100").
const DRAW_HEIGHT: f32 = 100.0 / 128.0;
/// ...and this far back toward the archer: the animation holds the arrow with its head well ahead
/// of the bow's grip, so the drawn arrow is eased back until it rests on the grip.
const DRAW_BACK: f32 = 24.0 / 128.0;
/// How far below the top of a monster's model its head reaches, tiles: the vertices that high up
/// are taken as the head, and its mouth is on the front of them.
const HEAD_DEPTH: f32 = 1.2;
/// How high the released arrow leaves, tiles: chest height, not the eyes the draw reaches.
const RELEASE_HEIGHT: f32 = 0.95;

/// Shots waiting to be drawn.
#[derive(Resource, Default)]
pub struct ProjectileQueue(pub Vec<Shot>);

/// A graphic to play on a character at app time `at`, at its feet, moving with it, until its
/// animation has played.
pub struct Spot {
    pub on: Entity,
    pub graphic: u32,
    pub at: f32,
}

/// Graphics waiting for their time.
#[derive(Resource, Default)]
pub struct SpotQueue(pub Vec<Spot>);

/// How long a graphic without an animation of its own stays up, seconds.
const STILL_SPOT_SECONDS: f32 = 0.6;

/// When (app seconds) the last arrow aimed at an entity arrives. The server lands a hit a tick or two
/// after the attack, which for a short shot is before the animation lets the arrow go; the client
/// holds that hit's splat until the arrow is there.
#[derive(Resource, Default)]
pub struct Arrivals(pub HashMap<Entity, f32>);

/// An effect on a character that goes when its animation has played.
#[derive(Component)]
struct Until(f32);

/// The graphics' definitions and the models built from them so far.
pub(crate) struct ProjectileArt {
    cache: Cache,
    defs: HashMap<u32, SpotAnim>,
    texture_hsl: HashMap<u32, u16>,
    built: HashMap<u32, Option<LocMesh>>,
    /// Animated graphics (the arrow in the bow as it is drawn), with their baked sequence.
    animated: HashMap<u32, Option<Drawn>>,
}

impl ProjectileArt {
    fn open() -> osrs_cache::Result<ProjectileArt> {
        let cache = Cache::open_default()?;
        let (defs, texture_hsl) = {
            let configs = Configs::new(&cache)?;
            (configs.spotanims()?, configs.texture_average_hsl()?)
        };
        Ok(ProjectileArt { cache, defs, texture_hsl, built: HashMap::new(), animated: HashMap::new() })
    }

    /// A graphic that has its own animation: the model and the clip baked against it.
    fn animated(&mut self, graphic: u32) -> Option<&Drawn> {
        if !self.animated.contains_key(&graphic) {
            let built = (|| {
                let def = self.defs.get(&graphic)?;
                let sequence = def.sequence?;
                let mesh = build_spotanim_with_rig(&self.cache, def, &self.texture_hsl, true)?;
                let mut animations = Animations::new(&self.cache).ok()?;
                let clip = Clip::bake(&mut animations, sequence, mesh.rig.as_ref()?).ok()?;
                // where the arrow is when it is let go (near the end of its animation), as an
                // offset from the archer's feet
                let frame = clip.frame_at(clip.duration() as f32 * 0.78);
                let points = &clip.frames[frame];
                let n = points.len().max(1) as f32;
                let mut c = Vec3::ZERO;
                for p in points {
                    c += Vec3::new(p[0], -p[1], -p[2]) / (128.0 * n);
                }
                // (in the archer's frame +z is the way it faces, so easing back is -z)
                Some(Drawn { mesh, clip, release_point: Vec3::new(c.x, 0.0, c.z - DRAW_BACK) })
            })();
            self.animated.insert(graphic, built);
        }
        self.animated.get(&graphic).and_then(Option::as_ref)
    }

    /// A graphic as something to put in the scene: its model and, if it has an animation, the
    /// clip to loop on it.
    pub(crate) fn drawable(&mut self, graphic: u32) -> Option<(LocMesh, Option<Clip>)> {
        if let Some(drawn) = self.animated(graphic).cloned() {
            return Some((drawn.mesh, Some(drawn.clip)));
        }
        self.mesh(graphic).cloned().map(|mesh| (mesh, None))
    }

    fn mesh(&mut self, graphic: u32) -> Option<&LocMesh> {
        if !self.built.contains_key(&graphic) {
            let mesh = self.defs.get(&graphic).and_then(|d| build_spotanim(&self.cache, d, &self.texture_hsl));
            self.built.insert(graphic, mesh);
        }
        self.built.get(&graphic).and_then(Option::as_ref)
    }
}

/// An animated graphic: its model, its baked sequence and where its arrow is when released.
#[derive(Clone)]
struct Drawn {
    mesh: LocMesh,
    clip: Clip,
    release_point: Vec3,
}

#[derive(Component)]
struct Projectile {
    from: Vec3,
    target: Entity,
    last_target: Vec3,
    born: f32,
    seconds: f32,
}

pub struct ProjectilePlugin;

impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProjectileQueue>()
            .init_resource::<SpotQueue>()
            .init_resource::<Arrivals>()
            .add_systems(Startup, open_art)
            .add_systems(Update, (spawn_projectiles, fly, play_spots).chain());
    }
}

fn open_art(world: &mut World) {
    match ProjectileArt::open() {
        Ok(art) => {
            world.insert_non_send(art);
        }
        Err(e) => eprintln!("[client] projectiles cannot be drawn: {e}"),
    }
}

/// A projectile that has not left yet.
struct Waiting {
    shot: Shot,
    leaves_at: f32,
    /// Seconds it flies once it leaves.
    flight: f32,
    /// Where it leaves from, relative to the shooter (the nocked arrow's place at the release).
    launch: Option<Vec3>,
}

#[allow(clippy::too_many_arguments)]
fn spawn_projectiles(
    mut commands: Commands,
    time: Res<Time>,
    mut queue: ResMut<ProjectileQueue>,
    mut waiting: Local<Vec<Waiting>>,
    mut arrivals: ResMut<Arrivals>,
    art: Option<NonSendMut<ProjectileArt>>,
    textures: Res<Textures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    transforms: Query<&Transform>,
    mouths: Query<(&super::npcs::NpcView, &osrs_render::AnimatedModel)>,
    effects: Query<(Entity, &Until)>,
) {
    let now = time.elapsed_secs();
    // effects whose animation has played
    for (entity, until) in &effects {
        if now >= until.0 {
            commands.entity(entity).despawn();
        }
    }
    let Some(mut art) = art else {
        queue.0.clear();
        return;
    };

    // new shots: the draw starts now; the projectile leaves when it has played
    for shot in std::mem::take(&mut queue.0) {
        let mut release = shot.swing * RELEASE_FRACTION;
        let mut launch = None;
        if let Some(g) = shot.draw {
            if art.animated(g).is_none() {
                eprintln!("[client] the draw graphic {g} cannot be drawn");
            }
        }
        if let Some(drawn) = shot.draw.and_then(|g| art.animated(g).cloned()) {
            let seconds = drawn.clip.duration() as f32 * 0.02;
            release = seconds;
            launch = Some(drawn.release_point);
            let model = osrs_render::spawn_animated(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut images,
                &textures.0 .0,
                &drawn.mesh,
                vec![drawn.clip],
                // shown above the archer's feet, as the game's "height 100" graphic, eased back
                // onto the bow's grip
                Transform::from_translation(Vec3::new(0.0, DRAW_HEIGHT, -DRAW_BACK)),
            );
            // a child of the shooter, so it turns and moves with them
            commands.entity(shot.from).add_child(model);
            commands.entity(model).insert(Until(now + seconds));
        }
        // it flies until the hit lands, or a short while if the draw takes longer than that
        let flight = (shot.hit_in - release).max(MIN_FLIGHT);
        let arrives = now + release + flight;
        let entry = arrivals.0.entry(shot.to).or_insert(arrives);
        *entry = entry.max(arrives);
        waiting.push(Waiting { leaves_at: now + release, flight, launch, shot });
    }

    // the ones whose time has come leave
    let mut still_waiting = Vec::new();
    for w in std::mem::take(&mut *waiting) {
        if now < w.leaves_at {
            still_waiting.push(w);
            continue;
        }
        let shot = w.shot;
        let (Ok(shooter), Ok(target)) = (transforms.get(shot.from), transforms.get(shot.to)) else { continue };
        let Some(mesh) = art.mesh(shot.graphic).cloned() else { continue };
        // from the bow, where the drawn arrow was (or chest height without a draw graphic)
        let start = match w.launch {
            Some(offset) => shooter.translation + shooter.rotation * offset + Vec3::Y * RELEASE_HEIGHT,
            None => match mouths.get(shot.from) {
                Ok((view, model)) if shot.from_mouth => {
                    // the front of the head as posed right now (rearing, crouching), else a fixed
                    // point on the model
                    let local = model.head_front(HEAD_DEPTH).unwrap_or_else(|| view.mouth_at(model.top()));
                    shooter.translation + shooter.rotation * local
                }
                _ => shooter.translation + Vec3::Y * LAUNCH_HEIGHT,
            },
        };
        let flight = w.flight;
        let model = osrs_render::spawn_loc_mesh(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &textures.0 .0,
            &mesh,
            Transform::from_translation(start),
        );
        commands.entity(model).insert(Projectile {
            from: start,
            target: shot.to,
            last_target: target.translation + Vec3::Y * TARGET_HEIGHT,
            born: now,
            seconds: flight,
        });
    }
    *waiting = still_waiting;
}

/// Plays each graphic whose time has come on its character (gone again once its animation has
/// played; `spawn_projectiles` removes it).
#[allow(clippy::too_many_arguments)]
fn play_spots(
    mut commands: Commands,
    time: Res<Time>,
    mut queue: ResMut<SpotQueue>,
    art: Option<NonSendMut<ProjectileArt>>,
    textures: Res<Textures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    characters: Query<(), With<Transform>>,
) {
    let now = time.elapsed_secs();
    let Some(mut art) = art else {
        queue.0.clear();
        return;
    };
    let (due, waiting): (Vec<Spot>, Vec<Spot>) = std::mem::take(&mut queue.0).into_iter().partition(|s| now >= s.at);
    queue.0 = waiting;
    for spot in due {
        if characters.get(spot.on).is_err() {
            continue;
        }
        let (model, seconds) = if let Some(drawn) = art.animated(spot.graphic).cloned() {
            let seconds = drawn.clip.duration() as f32 * 0.02;
            let model = osrs_render::spawn_animated(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut images,
                &textures.0 .0,
                &drawn.mesh,
                vec![drawn.clip],
                Transform::IDENTITY,
            );
            (model, seconds)
        } else if let Some(mesh) = art.mesh(spot.graphic).cloned() {
            let model = osrs_render::spawn_loc_mesh(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut images,
                &textures.0 .0,
                &mesh,
                Transform::IDENTITY,
            );
            (model, STILL_SPOT_SECONDS)
        } else {
            eprintln!("[client] graphic {} cannot be drawn", spot.graphic);
            continue;
        };
        commands.entity(spot.on).add_child(model);
        commands.entity(model).insert(Until(now + seconds));
    }
}

/// Moves each projectile along its flight, head first, and removes it on arrival.
fn fly(
    mut commands: Commands,
    time: Res<Time>,
    transforms: Query<&Transform, Without<Projectile>>,
    mut projectiles: Query<(Entity, &mut Projectile, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    for (entity, mut projectile, mut transform) in &mut projectiles {
        if let Ok(target) = transforms.get(projectile.target) {
            projectile.last_target = target.translation + Vec3::Y * TARGET_HEIGHT;
        }
        let t = ((now - projectile.born) / projectile.seconds.max(0.05)).clamp(0.0, 1.0);
        let (from, to) = (projectile.from, projectile.last_target);
        let distance = from.distance(to);
        let at = |t: f32| from.lerp(to, t) + Vec3::Y * (ARC * distance * 4.0 * t * (1.0 - t));
        transform.translation = at(t);
        // the head leads: the model's head is toward +z, so look the other way
        let ahead = at((t + 0.02).min(1.0)) - at((t - 0.02).max(0.0));
        if ahead.length_squared() > 1e-8 {
            transform.look_to(-ahead.normalize(), Vec3::Y);
        }
        if t >= 1.0 {
            commands.entity(entity).despawn();
        }
    }
}
