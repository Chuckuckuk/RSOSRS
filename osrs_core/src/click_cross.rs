//! The cross that plays where the world was clicked: yellow for a walk, red for a click on
//! something (an NPC, an object, a ground item). It is the cache's "cross" sprite, whose first
//! four frames are the yellow animation and the last four the red one.
//!
//! As in the game, each frame shows for 100 ms (the client steps the cross 20 a 20 ms cycle and
//! moves on a frame every 100), so the whole cross lasts 400 ms, centred on the click.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use osrs_cache::font::decode_frames;
use osrs_cache::textures::Texture;
use osrs_cache::{idx, Cache};

/// Seconds each frame of the cross shows.
const FRAME_SECONDS: f32 = 0.1;
/// Frames in each colour's animation.
const FRAMES: usize = 4;
/// How much bigger than the interface's pixels the cross is drawn.
const CROSS_SCALE: f32 = 2.0;

/// Which cross a click plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossColour {
    /// A walk: the ground was clicked.
    Yellow,
    /// The click went to something: an NPC, an object, a ground item.
    Red,
}

/// The frame of the "cross" sprite to show `elapsed` seconds after a click, or `None` once the
/// cross has finished.
pub fn cross_frame(colour: CrossColour, elapsed: f32) -> Option<usize> {
    if elapsed < 0.0 {
        return None;
    }
    let step = (elapsed / FRAME_SECONDS) as usize;
    let first = match colour {
        CrossColour::Yellow => 0,
        CrossColour::Red => FRAMES,
    };
    (step < FRAMES).then_some(first + step)
}

/// The cross playing now, if any: where (logical window pixels), which colour and when it began.
#[derive(Resource, Default)]
pub struct ClickCross(Option<(Vec2, CrossColour, f32)>);

impl ClickCross {
    /// Starts the cross for a click at `at` (logical window pixels) at time `now`; `None` (a click
    /// that plays no cross) leaves the one already playing alone.
    pub fn show(&mut self, at: Vec2, colour: Option<CrossColour>, now: f32) {
        if let Some(colour) = colour {
            self.0 = Some((at, colour, now));
        }
    }
}

/// The eight frames of the cross, each on the sprite's full canvas so the cross stays centred.
#[derive(Resource)]
struct CrossArt {
    frames: Vec<Handle<Image>>,
    size: Vec2,
}

/// The node drawing the cross.
#[derive(Component)]
struct CrossNode;

pub struct ClickCrossPlugin;

impl Plugin for ClickCrossPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClickCross>().add_systems(Startup, load_art).add_systems(Update, draw_cross);
    }
}

fn load_art(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let loaded = (|| -> osrs_cache::Result<(Vec<Texture>, Vec2)> {
        let cache = Cache::open_default()?;
        let sprites = cache.index(idx::SPRITES)?;
        let group = sprites.group_by_name("cross").ok_or_else(|| osrs_cache::Error::Missing("sprite cross".into()))?.id;
        let files = cache.read_files(&sprites, group)?;
        let frames = files.first().map(|(_, data)| decode_frames(data)).unwrap_or_default();
        if frames.len() < 2 * FRAMES {
            return Err(osrs_cache::Error::Missing(format!("frames of sprite cross (has {})", frames.len())));
        }
        let size = Vec2::new(frames[0].canvas_width as f32, frames[0].canvas_height as f32);
        let textures = frames
            .iter()
            .map(|f| {
                // put the frame on its canvas: the frames are trimmed to their pixels
                let (cw, ch) = (f.canvas_width, f.canvas_height);
                let mut rgba = vec![0u8; cw * ch * 4];
                for y in 0..f.height.min(ch.saturating_sub(f.y)) {
                    for x in 0..f.width.min(cw.saturating_sub(f.x)) {
                        let (from, to) = ((y * f.width + x) * 4, ((y + f.y) * cw + x + f.x) * 4);
                        rgba[to..to + 4].copy_from_slice(&f.rgba[from..from + 4]);
                    }
                }
                Texture { width: cw as u32, height: ch as u32, rgba, has_alpha: true, scroll_dir: 0, scroll_speed: 0 }
            })
            .collect();
        Ok((textures, size))
    })();
    match loaded {
        Ok((textures, size)) => {
            let frames = textures.iter().map(|t| super::combat_fx::pixel_image(t, &mut images)).collect();
            commands.insert_resource(CrossArt { frames, size });
            commands.spawn((
                Node { position_type: PositionType::Absolute, ..default() },
                ImageNode::default(),
                CrossNode,
                // over the world and its overlays, under the right-click menu
                GlobalZIndex(60),
                Visibility::Hidden,
            ));
        }
        Err(e) => eprintln!("[client] the click cross cannot be drawn: {e}"),
    }
}

/// Shows the frame of the cross that is due, at the interface's pixel size, or hides it.
fn draw_cross(
    cross: Res<ClickCross>,
    art: Option<Res<CrossArt>>,
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut nodes: Query<(&mut Node, &mut ImageNode, &mut Visibility), With<CrossNode>>,
) {
    let (Some(art), Ok((mut node, mut image, mut visibility))) = (art, nodes.single_mut()) else { return };
    let due = cross.0.and_then(|(at, colour, began)| Some((at, cross_frame(colour, time.elapsed_secs() - began)?)));
    let (Some((at, frame)), Ok(window)) = (due, windows.single()) else {
        *visibility = Visibility::Hidden;
        return;
    };
    let Some(handle) = art.frames.get(frame) else { return };
    // the interface's pixels are whole screen pixels; the cross is drawn CROSS_SCALE of them a pixel
    let physical = Vec2::new(window.physical_width() as f32, window.physical_height() as f32);
    let size = art.size * CROSS_SCALE * super::interface::ui_scale(physical) as f32 / window.scale_factor();
    if image.image != *handle {
        image.image = handle.clone();
    }
    node.width = Val::Px(size.x);
    node.height = Val::Px(size.y);
    node.left = Val::Px(at.x - size.x / 2.0);
    node.top = Val::Px(at.y - size.y / 2.0);
    *visibility = Visibility::Inherited;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_colour_plays_its_four_frames_for_a_tenth_of_a_second_each() {
        assert_eq!(cross_frame(CrossColour::Yellow, 0.0), Some(0));
        assert_eq!(cross_frame(CrossColour::Yellow, 0.15), Some(1));
        assert_eq!(cross_frame(CrossColour::Yellow, 0.39), Some(3));
        assert_eq!(cross_frame(CrossColour::Red, 0.0), Some(4));
        assert_eq!(cross_frame(CrossColour::Red, 0.25), Some(6));
        assert_eq!(cross_frame(CrossColour::Red, 0.39), Some(7));
    }

    #[test]
    fn the_cross_is_gone_after_four_hundred_milliseconds() {
        assert_eq!(cross_frame(CrossColour::Yellow, 0.4), None);
        assert_eq!(cross_frame(CrossColour::Red, 1.0), None);
    }

    #[test]
    fn a_click_that_plays_no_cross_leaves_the_last_one_playing() {
        let mut cross = ClickCross::default();
        cross.show(Vec2::new(10.0, 20.0), Some(CrossColour::Yellow), 1.0);
        cross.show(Vec2::new(50.0, 50.0), None, 1.1);
        assert_eq!(cross.0, Some((Vec2::new(10.0, 20.0), CrossColour::Yellow, 1.0)));
        cross.show(Vec2::new(50.0, 50.0), Some(CrossColour::Red), 1.2);
        assert_eq!(cross.0, Some((Vec2::new(50.0, 50.0), CrossColour::Red, 1.2)));
    }
}
