//! Bevy-side rendering of meshes built by `osrs_cache`, shared by the viewer and the game client.
//!
//! Everything is unlit: the client's lighting is baked into vertex colours by `osrs_cache::scene`,
//! so a mesh looks the same wherever it is drawn.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};
use osrs_cache::anim::Clip;
use osrs_cache::scene::{LocMesh, MeshData};
use osrs_cache::textures::Texture;
use std::collections::HashMap;

/// Object faces are one-sided like the client's; flip to `Face::Front` if models look inside out.
pub const OBJECT_CULL: Option<Face> = Some(Face::Back);

pub fn to_bevy_mesh(m: &MeshData) -> Mesh {
    let normals = vec![[0.0, 1.0, 0.0]; m.positions.len()];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, m.colors.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(m.indices.clone()));
    if !m.uvs.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs.clone());
    }
    mesh
}

/// Cut-out textured material for one texture image (repeat wrapping, unlit; vertex colours carry
/// the baked shading).
pub fn texture_material(
    tex: &Texture,
    cull_mode: Option<Face>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Handle<StandardMaterial> {
    let mut image = Image::new(
        Extent3d { width: tex.width, height: tex.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        tex.rgba.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(images.add(image)),
        unlit: true,
        cull_mode,
        alpha_mode: if tex.has_alpha { AlphaMode::Mask(0.5) } else { AlphaMode::Opaque },
        ..default()
    })
}

/// Unlit vertex-coloured material for untextured object faces.
pub fn vertex_color_material(blend: bool, materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        cull_mode: OBJECT_CULL,
        alpha_mode: if blend { AlphaMode::Blend } else { AlphaMode::Opaque },
        ..default()
    })
}

/// Mesh handle of one spawned part plus, per vertex, the rig vertex it follows.
pub type Part = (Handle<Mesh>, Vec<u32>);

/// Spawn one object mesh (opaque, translucent and textured parts) as children of a new entity at
/// `transform`, and return that parent. Translucent faces are not depth-sorted here.
pub fn spawn_loc_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    textures: &HashMap<u32, Texture>,
    loc: &LocMesh,
    transform: Transform,
) -> Entity {
    spawn_parts(commands, meshes, materials, images, textures, loc, transform).0
}

fn spawn_parts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    textures: &HashMap<u32, Texture>,
    loc: &LocMesh,
    transform: Transform,
) -> (Entity, Vec<Part>) {
    let parent = commands.spawn((transform, Visibility::default())).id();
    let parts = add_parts(commands, meshes, materials, images, textures, loc, parent);
    (parent, parts)
}

/// Spawns the mesh parts of `loc` as children of `parent`.
fn add_parts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    textures: &HashMap<u32, Texture>,
    loc: &LocMesh,
    parent: Entity,
) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut add = |commands: &mut Commands, data: &MeshData, material: Handle<StandardMaterial>| {
        let handle = meshes.add(to_bevy_mesh(data));
        let child = commands
            .spawn((Mesh3d(handle.clone()), MeshMaterial3d(material), Transform::default()))
            .id();
        commands.entity(parent).add_child(child);
        parts.push((handle, data.source.clone()));
    };
    if !loc.opaque.is_empty() {
        let mat = vertex_color_material(false, materials);
        add(commands, &loc.opaque, mat);
    }
    if !loc.translucent.is_empty() {
        let mat = vertex_color_material(true, materials);
        add(commands, &loc.translucent, mat);
    }
    for (id, data) in &loc.textured {
        // a texture missing from the store falls back to untextured grey shading
        let mat = match textures.get(id) {
            Some(t) => texture_material(t, OBJECT_CULL, images, materials),
            None => vertex_color_material(false, materials),
        };
        add(commands, data, mat);
    }
    parts
}

/// A model whose vertices follow one of several baked animation [`Clip`]s, looping the active one.
/// Switch with [`AnimatedModel::play`]. Advanced by [`ModelAnimationPlugin`].
#[derive(Component)]
pub struct AnimatedModel {
    parts: Vec<Part>,
    clips: Vec<Clip>,
    active: usize,
    /// Client ticks (20 ms) of the active clip played so far.
    ticks: f32,
    /// Clip and frame currently written into the meshes.
    shown: Option<(usize, usize)>,
    /// Whether the active clip loops; a one-shot clip holds its last frame once it has played.
    looping: bool,
    /// The height above the feet (tiles) of the highest vertex of every frame of every clip.
    tops: Vec<Vec<f32>>,
}

/// The highest point of each frame of each clip, in tiles above the model's origin.
fn frame_tops(clips: &[Clip]) -> Vec<Vec<f32>> {
    clips
        .iter()
        .map(|clip| {
            clip.frames
                .iter()
                .map(|frame| frame.iter().map(|p| -p[1] / 128.0).fold(0.0f32, f32::max))
                .collect()
        })
        .collect()
}

impl AnimatedModel {
    /// Start clip `index` from its first frame; a no-op if it is already the active one.
    pub fn play(&mut self, index: usize) {
        if (index != self.active || !self.looping) && index < self.clips.len() {
            self.active = index;
            self.ticks = 0.0;
            self.looping = true;
            self.shown = None;
        }
    }

    /// Play clip `index` once from its first frame, then hold its last frame (an attack, a death).
    /// It starts over if it is asked for again. Use [`AnimatedModel::play`] to go back to looping.
    pub fn play_once(&mut self, index: usize) {
        if index < self.clips.len() {
            self.active = index;
            self.ticks = 0.0;
            self.looping = false;
            self.shown = None;
        }
    }

    /// How long clip `index` lasts, in seconds.
    pub fn clip_seconds(&self, index: usize) -> f32 {
        self.clips.get(index).map_or(0.0, |c| c.duration() as f32 / TICKS_PER_SECOND)
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// Where the front of the head is in the frame being shown, relative to the model's origin
    /// (tiles, render space, the front is +z): the vertices within `depth` of the top are the
    /// head, and this is their middle sideways, and the front-most of them forward, `depth / 2`
    /// below the top. A monster's spit, breath or fireball starts there.
    pub fn head_front(&self, depth: f32) -> Option<Vec3> {
        let clip = self.clips.get(self.active)?;
        let frame = match self.shown {
            Some((_, frame)) => frame,
            None => clip.frame_at(self.ticks),
        };
        let top = *self.tops.get(self.active)?.get(frame)?;
        let posed = clip.frames.get(frame)?;
        let (mut x, mut count, mut front) = (0.0f32, 0u32, f32::MIN);
        for p in posed {
            if -p[1] / 128.0 >= top - depth {
                x += p[0] / 128.0;
                count += 1;
                front = front.max(-p[2] / 128.0);
            }
        }
        (count > 0).then(|| Vec3::new(x / count as f32, top - depth * 0.5, front))
    }

    /// How high the model reaches above its feet right now, in tiles: the highest vertex of the
    /// frame being shown, so something hung above it (a health bar) follows the animation (a
    /// crouching or rearing monster) instead of a fixed height.
    pub fn top(&self) -> f32 {
        let clip = &self.clips[self.active];
        let frame = match self.shown {
            Some((_, frame)) => frame,
            None => clip.frame_at(self.ticks),
        };
        self.tops[self.active].get(frame).copied().unwrap_or(0.0)
    }

    /// The index of the clip baked from cache sequence `sequence`, if this model has one.
    pub fn clip_of_sequence(&self, sequence: u32) -> Option<usize> {
        self.clips.iter().position(|clip| clip.sequence == sequence)
    }
}

/// Like [`spawn_loc_mesh`], for a mesh built with a rig. Plays `clips[0]` until told otherwise.
pub fn spawn_animated(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    textures: &HashMap<u32, Texture>,
    loc: &LocMesh,
    clips: Vec<Clip>,
    transform: Transform,
) -> Entity {
    assert!(!clips.is_empty(), "an animated model needs at least one clip");
    let (parent, parts) = spawn_parts(commands, meshes, materials, images, textures, loc, transform);
    let tops = frame_tops(&clips);
    commands.entity(parent).insert(AnimatedModel { parts, clips, active: 0, ticks: 0.0, shown: None, looping: true, tops });
    parent
}

/// Swaps the mesh and animations of an animated model that is already in the scene (a character
/// that put something on): the entity, its transform and the clip it was playing are kept.
#[allow(clippy::too_many_arguments)]
pub fn replace_animated(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    textures: &HashMap<u32, Texture>,
    entity: Entity,
    model: &mut AnimatedModel,
    loc: &LocMesh,
    clips: Vec<Clip>,
) {
    assert!(!clips.is_empty(), "an animated model needs at least one clip");
    commands.entity(entity).despawn_children();
    model.parts = add_parts(commands, meshes, materials, images, textures, loc, entity);
    if model.active >= clips.len() {
        model.active = 0;
    }
    model.tops = frame_tops(&clips);
    model.clips = clips;
    model.shown = None;
}

pub struct ModelAnimationPlugin;

impl Plugin for ModelAnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, play_animations);
    }
}

/// One client tick is 20 ms.
const TICKS_PER_SECOND: f32 = 50.0;

fn play_animations(time: Res<Time>, mut models: Query<&mut AnimatedModel>, mut meshes: ResMut<Assets<Mesh>>) {
    for mut model in &mut models {
        model.ticks += time.delta_secs() * TICKS_PER_SECOND;
        let active = model.active;
        let ticks = if model.looping {
            model.ticks
        } else {
            // a one-shot clip stops on its last frame
            model.ticks.min(model.clips[active].duration().saturating_sub(1) as f32)
        };
        let frame = model.clips[active].frame_at(ticks);
        if model.shown == Some((active, frame)) {
            continue;
        }
        model.shown = Some((active, frame));
        let posed = &model.clips[active].frames[frame];
        for (handle, source) in &model.parts {
            let positions: Vec<[f32; 3]> = source
                .iter()
                .map(|v| {
                    let p = posed[*v as usize];
                    [p[0] / 128.0, -p[1] / 128.0, -p[2] / 128.0]
                })
                .collect();
            if let Some(mut mesh) = meshes.get_mut(handle) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            }
        }
    }
}

pub mod world;

pub mod pick;
pub mod streaming;
