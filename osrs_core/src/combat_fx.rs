//! What a fight looks like: the server reports attacks, hits and deaths as events, and this turns
//! them into attack, block and death animations, a turn to face the opponent, hit splats and
//! health bars over the fighters' heads.
//!
//! The animation clips themselves are chosen by `sync_player` and `sync_npcs`, which read each
//! fighter's [`Fighter`] state; this module keeps that state up to date.

use super::npcs::NpcViews;
use super::{OrbitCamera, Player, ServerLink};
use bevy::prelude::*;
use osrs_core::sim::{Combatant, WorldEvent};

/// How much bigger than the cache's 25-pixel picture a hit splat is drawn (whole numbers keep the
/// pixel art crisp).
const SPLAT_SCALE: f32 = 1.44;
/// A server tick, seconds.
const TICK_SECONDS: f32 = 0.6;
/// Seconds a health bar stays up after the fighter last fought (300 client cycles in the cache).
const BAR_SECONDS: f32 = 6.0;
/// How much bigger than the cache's picture a health bar is drawn.
const BAR_SCALE: f32 = 2.0;

/// A one-shot animation a fighter is playing instead of walking or standing.
pub struct Action {
    pub clip: usize,
    /// When it is over (seconds of app time).
    pub until: f32,
    /// Whether the model has been told to play it yet.
    pub started: bool,
}

/// A fighter's combat state, kept up to date from the server's events.
#[derive(Component, Default)]
pub struct Fighter {
    pub action: Option<Action>,
    pub dead: bool,
    /// Current and maximum hitpoints as of the latest snapshot.
    pub health: Option<(i32, i32)>,
    /// Show the health bar until this time.
    pub bar_until: f32,
    /// Heading (radians, 0 faces south) to turn to: toward the opponent when it attacks.
    pub face: Option<f32>,
}

impl Fighter {
    /// The one-shot clip this fighter should be playing at `now`, if its fight state says so: the
    /// death (held for good) or an attack/block that has not finished. Returns (clip, first time it
    /// is asked for, is the death); `None` when the fighter is free to walk or stand.
    pub fn clip(&mut self, now: f32, death: usize) -> Option<(usize, bool, bool)> {
        if self.dead {
            let first = self.action.as_ref().is_none_or(|a| a.clip != death || !a.started);
            self.action = Some(Action { clip: death, until: f32::MAX, started: true });
            return Some((death, first, true));
        }
        match &mut self.action {
            Some(action) if now < action.until => {
                let first = !action.started;
                action.started = true;
                Some((action.clip, first, false))
            }
            Some(_) => {
                self.action = None;
                None
            }
            None => None,
        }
    }
}

/// A damage number over a fighter.
#[derive(Component)]
struct HitSplat {
    target: Entity,
    born: f32,
    /// How long it stays up, from its cache definition.
    seconds: f32,
    /// Its size in pixels.
    size: f32,
}

/// A cache picture as a pixel-art image (nearest-neighbour).
pub(crate) fn pixel_image(texture: &osrs_cache::textures::Texture, images: &mut Assets<Image>) -> Handle<Image> {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::ImageSampler;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut image = Image::new(
        Extent3d { width: texture.width, height: texture.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        texture.rgba.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    images.add(image)
}

/// The cache's fight pictures, as images: the two splats, the numbers drawn on them (made as they
/// are first needed) and the health bar.
#[derive(Resource)]
struct SplatImages {
    damage: SplatImage,
    block: SplatImage,
    font: osrs_cache::font::Font,
    /// What is written on a splat, per amount: the image and its size in pixels on screen.
    numbers: std::collections::HashMap<i32, (Handle<Image>, Vec2)>,
    text_color: u32,
    bar: BarImages,
}

struct SplatImage {
    handle: Handle<Image>,
    /// Pixels on screen.
    size: f32,
    seconds: f32,
}

struct BarImages {
    front: Handle<Image>,
    back: Handle<Image>,
    /// The cache picture's size, pixels.
    width: u32,
    height: u32,
}

fn load_splat_images(mut commands: Commands, scene: Res<super::Scene>, mut images: ResMut<Assets<Image>>) {
    let splat = |art: &osrs_cache::hitsplat::HitSplatArt, images: &mut Assets<Image>| SplatImage {
        handle: pixel_image(&art.sprite, images),
        size: art.sprite.width as f32 * SPLAT_SCALE,
        seconds: art.seconds,
    };
    let bar = &scene.health_bar;
    commands.insert_resource(SplatImages {
        damage: splat(&scene.damage_splat, &mut images),
        block: splat(&scene.block_splat, &mut images),
        font: scene.splat_font.clone(),
        numbers: Default::default(),
        text_color: scene.damage_splat.text_color,
        bar: BarImages {
            front: pixel_image(&bar.front, &mut images),
            back: pixel_image(&bar.back, &mut images),
            width: bar.width,
            height: bar.back.height,
        },
    });
}

impl SplatImages {
    /// The picture of `amount` in the game's font, made the first time it is asked for.
    fn number(&mut self, amount: i32, images: &mut Assets<Image>) -> (Handle<Image>, Vec2) {
        let font = &self.font;
        let colour = self.text_color;
        self.numbers
            .entry(amount)
            .or_insert_with(|| {
                let text = font.render(&amount.to_string(), colour, true);
                let size = Vec2::new(text.width as f32, text.height as f32) * SPLAT_SCALE;
                (pixel_image(&text, images), size)
            })
            .clone()
    }
}

/// A health bar over a fighter: the node is the red part, its child the green.
#[derive(Component)]
struct HealthBar {
    target: Entity,
    fill: Entity,
}

pub struct CombatFxPlugin;

impl Plugin for CombatFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_splat_images).add_systems(
            Update,
            apply_events.after(super::poll_server).before(super::sync_player).before(super::npcs::sync_npcs),
        )
        .add_systems(Update, (place_splats, place_bars).after(super::map_stream::stream_map));
    }
}

pub fn heading_between(from: Vec3, to: Vec3) -> f32 {
    // render space: the model front is +z at heading 0
    (to.x - from.x).atan2(to.z - from.z)
}

/// Turns the server's events into fighter state, hit splats and sounds.
#[allow(clippy::too_many_arguments)]
fn apply_events(
    mut commands: Commands,
    time: Res<Time>,
    mut splat_images: ResMut<SplatImages>,
    mut projectiles: ResMut<super::projectiles::ProjectileQueue>,
    mut spots: ResMut<super::projectiles::SpotQueue>,
    mut sounds: ResMut<super::sound_fx::SoundQueue>,
    arrivals: Res<super::projectiles::Arrivals>,
    mut held: Local<Vec<(f32, WorldEvent)>>,
    mut images: ResMut<Assets<Image>>,
    mut server: NonSendMut<ServerLink>,
    views: NonSend<NpcViews>,
    players: Query<Entity, With<Player>>,
    mut fighters: Query<&mut Fighter>,
    transforms: Query<&Transform>,
    models: Query<&osrs_render::AnimatedModel>,
    mouths: Query<&super::npcs::NpcView>,
) {
    let now = time.elapsed_secs();
    let new_events = std::mem::take(&mut server.new_events);
    // hits (and the deaths they cause) that were held back until an arrow arrived, once it has
    let mut events: Vec<WorldEvent> = Vec::new();
    let mut still_held = Vec::new();
    for (due, event) in std::mem::take(&mut *held) {
        if now >= due {
            events.push(event);
        } else {
            still_held.push((due, event));
        }
    }
    *held = still_held;
    events.extend(new_events);
    if events.is_empty() {
        return;
    }
    let entity_of = |who: Combatant| -> Option<Entity> {
        match who {
            Combatant::Player => players.single().ok(),
            Combatant::Npc(id) => views.entity_of(id),
        }
    };
    // (attack, block, death) clip indices and how long a clip lasts, for a fighter
    let clips_of = |who: Combatant| -> (usize, usize, usize) {
        match who {
            Combatant::Player => (crate::CLIP_ATTACK, crate::CLIP_BLOCK, crate::CLIP_DEATH),
            Combatant::Npc(_) => (crate::npcs::CLIP_ATTACK, crate::npcs::CLIP_DEFEND, crate::npcs::CLIP_DEATH),
        }
    };
    let length_of = |entity: Entity, clip: usize| -> f32 {
        models.get(entity).map_or(0.6, |m| m.clip_seconds(clip))
    };

    // the fighter whose hit (or death) was just held back: the sound that follows it waits too
    let mut held_victim: Option<Combatant> = None;
    for event in events {
        // a hit aimed at something an arrow has not reached yet waits for it, and so does what
        // follows from it, so the splat is not before the arrow
        let victim = match event {
            WorldEvent::Hit { target, .. } => Some(target),
            WorldEvent::Died { who } => Some(who),
            WorldEvent::Sound { from, delay_ticks: 0, .. } if held_victim == Some(from) => Some(from),
            _ => None,
        };
        held_victim = None;
        if let Some(entity) = victim.and_then(entity_of) {
            let due = arrivals.0.get(&entity).copied().unwrap_or(0.0);
            // (held events of one target all share its arrival time, so they keep their order)
            if due > now {
                held.push((due, event));
                held_victim = victim;
                continue;
            }
        }
        match event {
            WorldEvent::Attack { attacker, target, animation } => {
                let (Some(a), Some(t)) = (entity_of(attacker), entity_of(target)) else { continue };
                // an attack with an animation of its own (Jad's stomp, his rearing up) plays that,
                // when the model has it baked; anything else plays the fighter's usual attack
                let usual = clips_of(attacker).0;
                let attack = animation
                    .and_then(|sequence| models.get(a).ok().and_then(|m| m.clip_of_sequence(sequence)))
                    .unwrap_or(usual);
                let toward = match (transforms.get(a), transforms.get(t)) {
                    (Ok(from), Ok(to)) => Some(heading_between(from.translation, to.translation)),
                    _ => None,
                };
                let length = length_of(a, attack);
                if let Ok(mut fighter) = fighters.get_mut(a) {
                    if !fighter.dead {
                        fighter.action = Some(Action { clip: attack, until: now + length, started: false });
                        fighter.face = toward;
                        fighter.bar_until = now + BAR_SECONDS;
                    }
                }
                if let Ok(mut fighter) = fighters.get_mut(t) {
                    fighter.bar_until = now + BAR_SECONDS;
                }
            }
            WorldEvent::Projectile { from, to, graphic, draw, ticks, released } => {
                if let (Some(a), Some(t)) = (entity_of(from), entity_of(to)) {
                    // `projectiles` times the release from the draw graphic (or, without one, from
                    // the attack animation) and has the arrow arrive as the hit lands
                    projectiles.0.push(super::projectiles::Shot {
                        from: a,
                        to: t,
                        graphic,
                        draw,
                        // a monster's attack leaves its mouth (a shooter with a draw graphic, the
                        // player's bow, has its own release point)
                        from_mouth: draw.is_none() && mouths.get(a).is_ok(),
                        // a wound-up attack (Jad's) lets go now; otherwise partway through the swing
                        swing: if released { 0.0 } else { length_of(a, clips_of(from).0) },
                        hit_in: ticks as f32 * 0.6,
                    });
                }
            }
            WorldEvent::Hit { target, damage } => {
                let Some(t) = entity_of(target) else { continue };
                let (_, block, _) = clips_of(target);
                let length = length_of(t, block);
                if let Ok(mut fighter) = fighters.get_mut(t) {
                    fighter.bar_until = now + BAR_SECONDS;
                    // being hit while swinging does not interrupt the swing
                    let swinging = fighter.action.as_ref().is_some_and(|a| now < a.until);
                    if !fighter.dead && !swinging {
                        fighter.action = Some(Action { clip: block, until: now + length, started: false });
                    }
                }
                spawn_splat(&mut commands, &mut splat_images, &mut images, t, damage, now);
            }
            WorldEvent::Graphic { on, graphic, delay_ticks } => {
                if let Some(e) = entity_of(on) {
                    spots.0.push(super::projectiles::Spot { on: e, graphic, at: now + delay_ticks as f32 * TICK_SECONDS });
                }
            }
            WorldEvent::Heal { healer, target, animation, .. } => {
                // the healer turns to the one it heals and plays its healing animation; the
                // healed one's health bar shows the hitpoints coming back
                let (Some(h), Some(t)) = (entity_of(healer), entity_of(target)) else { continue };
                let Some(motion) = animation.and_then(|sequence| models.get(h).ok().and_then(|m| m.clip_of_sequence(sequence))) else {
                    continue;
                };
                let toward = match (transforms.get(h), transforms.get(t)) {
                    (Ok(from), Ok(to)) => Some(heading_between(from.translation, to.translation)),
                    _ => None,
                };
                let length = length_of(h, motion);
                if let Ok(mut fighter) = fighters.get_mut(h) {
                    if !fighter.dead {
                        fighter.action = Some(Action { clip: motion, until: now + length, started: false });
                        fighter.face = toward;
                    }
                }
                if let Ok(mut fighter) = fighters.get_mut(t) {
                    fighter.bar_until = now + BAR_SECONDS;
                }
            }
            WorldEvent::Sound { sound, from, delay_ticks } => {
                if let Some(e) = entity_of(from) {
                    sounds.0.push(super::sound_fx::PendingSound { sound, from: e, at: now + delay_ticks as f32 * TICK_SECONDS });
                }
            }
            // experience drops and level-up fireworks are not drawn yet
            WorldEvent::Experience { .. } | WorldEvent::LevelUp { .. } => {}
            WorldEvent::Died { who } => {
                let Some(e) = entity_of(who) else { continue };
                if let Ok(mut fighter) = fighters.get_mut(e) {
                    fighter.dead = true;
                    fighter.bar_until = 0.0;
                }
            }
        }
    }
}

fn spawn_splat(
    commands: &mut Commands,
    all: &mut SplatImages,
    images: &mut Assets<Image>,
    target: Entity,
    damage: i32,
    now: f32,
) {
    // the game's red splat for a hit, its blue one for a block
    let (number, number_size) = all.number(damage, images);
    let art = if damage > 0 { &all.damage } else { &all.block };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(art.size),
                height: Val::Px(art.size),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            ImageNode::new(art.handle.clone()),
            GlobalZIndex(50),
            Visibility::Hidden,
            HitSplat { target, born: now, seconds: art.seconds, size: art.size },
        ))
        .with_children(|splat| {
            splat.spawn((
                Node { width: Val::Px(number_size.x), height: Val::Px(number_size.y), ..default() },
                ImageNode::new(number),
            ));
        });
}

/// Where the screen point above `target`'s head is, if it is on screen.
pub(crate) fn screen_point(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    transform: &Transform,
    lift: f32,
) -> Option<Vec2> {
    // viewport coordinates start at the viewport's corner; UI nodes are placed from the window's
    let origin = camera.logical_viewport_rect().map_or(Vec2::ZERO, |r| r.min);
    camera.world_to_viewport(camera_transform, transform.translation + Vec3::Y * lift).ok().map(|p| p + origin)
}

/// How high above its feet a fighter's head is, in tiles: the player's. An NPC is as tall as its
/// own model (`NpcView::top`).
pub(crate) const HEAD_HEIGHT: f32 = 1.75;

/// A health bar floats this far above the top of the model, tiles.
const BAR_CLEARANCE: f32 = 0.2;

/// How high a fighter reaches right now: an animated NPC model's highest vertex this frame (so a
/// bar follows a monster rearing up or crouching), else the model's resting height, else the
/// player's.
fn height_of(views: &Query<(Option<&super::npcs::NpcView>, &osrs_render::AnimatedModel)>, who: Entity) -> f32 {
    views.get(who).map_or(HEAD_HEIGHT, |(view, model)| model.top().max(view.map_or(0.0, |v| v.top()) * 0.25))
}

fn place_splats(
    mut commands: Commands,
    time: Res<Time>,
    cameras: Query<(&Camera, &GlobalTransform), With<OrbitCamera>>,
    targets: Query<&Transform>,
    heights: Query<(Option<&super::npcs::NpcView>, &osrs_render::AnimatedModel)>,
    mut splats: Query<(Entity, &HitSplat, &mut Node, &mut Visibility)>,
) {
    let now = time.elapsed_secs();
    let Ok((camera, camera_transform)) = cameras.single() else { return };
    for (entity, splat, mut node, mut visibility) in &mut splats {
        let age = now - splat.born;
        let Ok(transform) = targets.get(splat.target) else {
            commands.entity(entity).despawn();
            continue;
        };
        if age > splat.seconds {
            commands.entity(entity).despawn();
            continue;
        }
        // it stays where it landed, over the middle of the body
        match screen_point(camera, camera_transform, transform, height_of(&heights, splat.target) * 0.6) {
            Some(p) => {
                node.left = Val::Px(p.x - splat.size / 2.0);
                node.top = Val::Px(p.y - splat.size / 2.0);
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

fn place_bars(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<SplatImages>,
    cameras: Query<(&Camera, &GlobalTransform), With<OrbitCamera>>,
    fighters: Query<(Entity, &Fighter, &Transform)>,
    heights: Query<(Option<&super::npcs::NpcView>, &osrs_render::AnimatedModel)>,
    mut bars: Query<(Entity, &HealthBar, &mut Node, &mut Visibility), Without<BarFill>>,
    mut fills: Query<(&mut Node, &mut ImageNode), With<BarFill>>,
) {
    let now = time.elapsed_secs();
    let Ok((camera, camera_transform)) = cameras.single() else { return };
    let (width, height) = (art.bar.width as f32 * BAR_SCALE, art.bar.height as f32 * BAR_SCALE);

    // a bar for every fighter that should show one and has none
    for (entity, fighter, _) in &fighters {
        let wanted = fighter.bar_until > now && !fighter.dead && fighter.health.is_some();
        let has = bars.iter().any(|(_, bar, _, _)| bar.target == entity);
        if wanted && !has {
            let mut fill = Entity::PLACEHOLDER;
            let root = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(width),
                        height: Val::Px(height),
                        ..default()
                    },
                    // the game's red bar; the green is drawn over the part that is health
                    ImageNode::new(art.bar.back.clone()),
                    GlobalZIndex(40),
                    Visibility::Hidden,
                ))
                .with_children(|bar| {
                    fill = bar
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(0.0),
                                top: Val::Px(0.0),
                                width: Val::Px(width),
                                height: Val::Px(height),
                                ..default()
                            },
                            ImageNode::new(art.bar.front.clone()),
                            BarFill,
                        ))
                        .id();
                })
                .id();
            commands.entity(root).insert(HealthBar { target: entity, fill });
        }
    }

    for (bar_entity, bar, mut node, mut visibility) in &mut bars {
        let Ok((_, fighter, transform)) = fighters.get(bar.target) else {
            commands.entity(bar_entity).despawn();
            continue;
        };
        if fighter.bar_until <= now || fighter.dead {
            commands.entity(bar_entity).despawn();
            continue;
        }
        if let (Ok((mut fill, mut image)), Some((hp, max))) = (fills.get_mut(bar.fill), fighter.health) {
            // the game cuts the green off at a whole pixel of the picture
            let pixels = (art.bar.width as i32 * hp.max(0) / max.max(1)).clamp(0, art.bar.width as i32);
            fill.width = Val::Px(pixels as f32 * BAR_SCALE);
            image.rect = Some(Rect::new(0.0, 0.0, pixels as f32, art.bar.height as f32));
        }
        // above the top of the model, not inside it
        match screen_point(camera, camera_transform, transform, height_of(&heights, bar.target) + BAR_CLEARANCE) {
            Some(p) => {
                node.left = Val::Px(p.x - width / 2.0);
                node.top = Val::Px(p.y);
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

/// Marks the green part of a health bar.
#[derive(Component)]
struct BarFill;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fighter_plays_its_action_once_then_hands_back() {
        let mut fighter = Fighter::default();
        fighter.action = Some(Action { clip: 7, until: 2.0, started: false });

        assert_eq!(fighter.clip(1.0, 9), Some((7, true, false)), "first ask starts it");
        assert_eq!(fighter.clip(1.5, 9), Some((7, false, false)), "then it just carries on");
        assert_eq!(fighter.clip(2.5, 9), None, "and when it is over the fighter is free");
        assert!(fighter.action.is_none());
    }

    #[test]
    fn death_is_started_once_and_held() {
        let mut fighter = Fighter { dead: true, ..Default::default() };
        fighter.action = Some(Action { clip: 7, until: f32::MAX, started: true });

        assert_eq!(fighter.clip(1.0, 9), Some((9, true, true)));
        assert_eq!(fighter.clip(50.0, 9), Some((9, false, true)));
    }

    #[test]
    fn facing_an_opponent_uses_the_render_space_heading() {
        // an opponent to the east: heading a quarter turn (the model front is +z, south)
        let east = heading_between(Vec3::ZERO, Vec3::new(3.0, 0.0, 0.0));
        assert!((east - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        // north is -z in render space: half a turn
        let north = heading_between(Vec3::ZERO, Vec3::new(0.0, 0.0, -2.0));
        assert!((north.abs() - std::f32::consts::PI).abs() < 1e-5);
    }
}
