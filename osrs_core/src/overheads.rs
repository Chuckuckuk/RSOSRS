//! The prayer icon over the player's head: the cache's own overhead pictures (the sprite
//! "headicons_prayer"), shown while one of the prayers that has one is on.

use super::combat_fx::{pixel_image, screen_point, HEAD_HEIGHT};
use super::{OrbitCamera, Player, ServerLink};
use bevy::prelude::*;
use osrs_cache::font::decode_frames;
use osrs_cache::textures::Texture;
use osrs_cache::{idx, Cache};

/// The prayers that show an icon overhead, with their frame of the "headicons_prayer" sprite
/// (identified by eye from a contact sheet of its frames).
pub const OVERHEAD_FRAMES: [(&str, usize); 6] = [
    ("protect_from_melee", 0),
    ("protect_from_missiles", 1),
    ("protect_from_magic", 2),
    ("retribution", 3),
    ("smite", 4),
    ("redemption", 5),
];

/// How much bigger than the cache's 25-pixel picture the icon is drawn.
const ICON_SCALE: f32 = 1.44;
/// Pixels between the icon and the health bar under it.
const ICON_GAP: f32 = 2.0;

/// The frame of the overhead picture for the prayers that are on, if one of them has one.
pub fn overhead_frame(active_prayers: &[String]) -> Option<usize> {
    OVERHEAD_FRAMES.iter().find(|(id, _)| active_prayers.iter().any(|a| a == id)).map(|(_, frame)| *frame)
}

/// The overhead pictures, by frame, with their size in pixels.
#[derive(Resource)]
struct OverheadArt(Vec<(Handle<Image>, Vec2)>);

/// The one icon node, over the player.
#[derive(Component)]
struct OverheadIcon;

pub struct OverheadPlugin;

impl Plugin for OverheadPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_art).add_systems(Update, place_icon.after(super::sync_player));
    }
}

fn load_art(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let loaded = (|| -> osrs_cache::Result<Vec<(Handle<Image>, Vec2)>> {
        let cache = Cache::open_default()?;
        let sprites = cache.index(idx::SPRITES)?;
        let group = sprites
            .group_by_name("headicons_prayer")
            .ok_or_else(|| osrs_cache::Error::Missing("sprite headicons_prayer".into()))?
            .id;
        let files = cache.read_files(&sprites, group)?;
        let frames = files.first().map(|(_, data)| decode_frames(data)).unwrap_or_default();
        Ok(frames
            .into_iter()
            .map(|f| {
                let size = Vec2::new(f.width as f32, f.height as f32);
                let has_alpha = f.rgba.chunks(4).any(|p| p[3] == 0);
                let texture = Texture { width: f.width as u32, height: f.height as u32, rgba: f.rgba, has_alpha, scroll_dir: 0, scroll_speed: 0 };
                (pixel_image(&texture, &mut images), size)
            })
            .collect())
    })();
    match loaded {
        Ok(frames) => {
            commands.insert_resource(OverheadArt(frames));
            commands.spawn((
                Node { position_type: PositionType::Absolute, ..default() },
                ImageNode::default(),
                OverheadIcon,
                GlobalZIndex(40),
                Visibility::Hidden,
            ));
        }
        Err(e) => eprintln!("[client] overhead prayer icons cannot be drawn: {e}"),
    }
}

/// Shows the icon of the protection prayer that is on, over the player's head, above the health
/// bar; hides it when none is.
fn place_icon(
    server: NonSend<ServerLink>,
    art: Option<Res<OverheadArt>>,
    cameras: Query<(&Camera, &GlobalTransform), With<OrbitCamera>>,
    players: Query<&Transform, With<Player>>,
    mut icons: Query<(&mut Node, &mut ImageNode, &mut Visibility), With<OverheadIcon>>,
) {
    let Some(art) = art else { return };
    let Ok((mut node, mut image, mut visibility)) = icons.single_mut() else { return };
    let frame = server.latest.as_ref().and_then(|s| overhead_frame(&s.player.active_prayers));
    let (Some((handle, size)), Ok((camera, camera_transform)), Ok(player)) =
        (frame.and_then(|f| art.0.get(f)), cameras.single(), players.single())
    else {
        *visibility = Visibility::Hidden;
        return;
    };
    // the health bar's top sits at this point; the icon sits on top of it
    let Some(at) = screen_point(camera, camera_transform, player, HEAD_HEIGHT + 0.2) else {
        *visibility = Visibility::Hidden;
        return;
    };
    let (w, h) = (size.x * ICON_SCALE, size.y * ICON_SCALE);
    if image.image != *handle {
        image.image = handle.clone();
    }
    node.width = Val::Px(w);
    node.height = Val::Px(h);
    node.left = Val::Px(at.x - w / 2.0);
    node.top = Val::Px(at.y - h - ICON_GAP);
    *visibility = Visibility::Inherited;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_protection_prayer_that_is_on_picks_the_icon() {
        assert_eq!(overhead_frame(&[]), None);
        assert_eq!(overhead_frame(&["piety".to_owned()]), None, "piety has no overhead");
        assert_eq!(overhead_frame(&["piety".to_owned(), "protect_from_magic".to_owned()]), Some(2));
        assert_eq!(overhead_frame(&["protect_from_melee".to_owned()]), Some(0));
    }
}
