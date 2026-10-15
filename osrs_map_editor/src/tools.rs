//! What clicks and keys do in the world, and the outlines drawn over it.

use crate::catalog::{footprint, Catalog};
use crate::editor::{Brush, Editor, Selected, Tool};
use crate::world::{placing_object, Hover};
use bevy::prelude::*;
use osrs_cache::locs::Placement;
use osrs_cache::project::NpcSpawn;
use osrs_cache::squares::SQUARE;
use osrs_render::streaming::{square_of, StreamedWorld};

const HOVER: Color = Color::srgb(1.0, 1.0, 1.0);
const SELECTED: Color = Color::srgb(1.0, 0.85, 0.1);
const GHOST: Color = Color::srgb(0.2, 1.0, 0.4);
const ERASE: Color = Color::srgb(1.0, 0.25, 0.2);
const NPC: Color = Color::srgb(1.0, 0.55, 0.0);
const ICON: Color = Color::srgb(0.2, 0.85, 1.0);

/// Lift of outlines above the ground, tiles.
const LIFT: f32 = 0.04;

/// What a click on the world (or the Delete key) acts on: the clicked object if its placement can
/// be found, else an NPC spawn on the tile, else the topmost placement covering the tile on the
/// plane being edited.
pub fn target_under(editor: &mut Editor, catalog: &Catalog, hover: &Hover) -> Option<Selected> {
    let click = hover.click.as_ref()?;
    if let Some(object) = &click.object {
        let found = editor.placements_at(object.tile, 0, catalog).into_iter().find(|(_, p)| p.id == object.id);
        if let Some((square, placement)) = found {
            return Some(Selected::Placement { square, placement });
        }
    }
    if let Some(&i) = editor.npcs_at(click.tile).first() {
        return Some(Selected::Npc(i));
    }
    let plane = editor.plane;
    editor
        .placements_at(click.tile, plane, catalog)
        .into_iter()
        .next()
        .map(|(square, placement)| Selected::Placement { square, placement })
}

pub fn remove(editor: &mut Editor, catalog: &Catalog, target: Selected) {
    match target {
        Selected::Placement { square, placement } => {
            editor.remove_placement(square, placement);
            editor.status = format!("Removed {} ({})", catalog.object_name(placement.id), placement.id);
        }
        Selected::Npc(i) => {
            let name = editor.project.npc_spawns.get(i).map(|s| s.npc.clone()).unwrap_or_default();
            editor.remove_npc(i);
            editor.status = format!("Removed NPC {name}");
        }
    }
}

pub fn world_clicks(buttons: Res<ButtonInput<MouseButton>>, hover: Res<Hover>, catalog: Res<Catalog>, mut editor: ResMut<Editor>) {
    if !hover.in_world || !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(tile) = hover.tile() else { return };
    match editor.tool {
        Tool::Select => {
            editor.selected = target_under(&mut editor, &catalog, &hover);
        }
        Tool::Erase => match target_under(&mut editor, &catalog, &hover) {
            Some(target) => remove(&mut editor, &catalog, target),
            None => editor.status = "Nothing to remove there".into(),
        },
        Tool::Place => place(&mut editor, &catalog, tile),
    }
}

fn place(editor: &mut Editor, catalog: &Catalog, tile: (i32, i32)) {
    let square = square_of(tile.0, tile.1);
    let (x, z) = (tile.0.rem_euclid(SQUARE) as u8, tile.1.rem_euclid(SQUARE) as u8);
    match editor.brush.clone() {
        Some(Brush::Object(id)) => {
            let Some((_, kind, orientation)) = placing_object(editor, catalog) else { return };
            let placement = Placement { id, plane: editor.plane, x, z, kind, orientation };
            editor.add_placement(square, placement);
            editor.selected = Some(Selected::Placement { square, placement });
            editor.status = format!("Placed {} ({id}) at {}, {}", catalog.object_name(id), tile.0, tile.1);
        }
        Some(Brush::Npc(npc)) => {
            editor.add_npc(NpcSpawn { npc: npc.clone(), x: tile.0, z: tile.1, plane: editor.plane });
            editor.selected = Some(Selected::Npc(editor.project.npc_spawns.len() - 1));
            editor.status = format!("Placed NPC {npc} at {}, {}", tile.0, tile.1);
        }
        Some(Brush::Icon(area)) => {
            editor.place_icon(square, x, z, area, catalog);
            editor.status = format!("Placed the {} icon at {}, {}", catalog.area_name(area), tile.0, tile.1);
        }
        None => editor.status = "Choose something in the palette to place".into(),
    }
}

pub fn shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    hover: Res<Hover>,
    catalog: Res<Catalog>,
    mut editor: ResMut<Editor>,
    mut view: ResMut<crate::camera::ViewState>,
    mut contexts: bevy_egui::EguiContexts,
) {
    if contexts.ctx_mut().is_ok_and(|ctx| ctx.egui_wants_keyboard_input()) {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if ctrl {
        if keys.just_pressed(KeyCode::KeyZ) && !shift {
            editor.undo();
        }
        if keys.just_pressed(KeyCode::KeyY) || (keys.just_pressed(KeyCode::KeyZ) && shift) {
            editor.redo();
        }
        if keys.just_pressed(KeyCode::KeyS) {
            editor.save();
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        editor.rotation = (editor.rotation + 1) & 3;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        view.roofs = !view.roofs;
    }
    for (i, key) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4].into_iter().enumerate() {
        if keys.just_pressed(key) {
            editor.plane = i as u8;
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        editor.tool = Tool::Select;
        editor.selected = None;
    }
    if keys.just_pressed(KeyCode::KeyV) {
        editor.tool = Tool::Select;
    }
    if keys.just_pressed(KeyCode::KeyB) {
        editor.tool = Tool::Place;
    }
    if keys.just_pressed(KeyCode::KeyX) {
        editor.tool = Tool::Erase;
    }
    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        // the selection, else whatever is under the mouse
        let target = editor.selected.clone().or_else(|| target_under(&mut editor, &catalog, &hover));
        if let Some(target) = target {
            remove(&mut editor, &catalog, target);
        }
    }
}

/// The four corners of a `w` x `h` block of tiles from (`x`, `z`), on the ground.
fn block(world: &StreamedWorld, x: i32, z: i32, w: i32, h: i32) -> Option<[Vec3; 4]> {
    let y = |tx: i32, tz: i32, corner: usize| world.tile_corners(tx, tz).map(|c| -(c[corner] as f32) / 128.0 + LIFT);
    Some([
        Vec3::new(x as f32, y(x, z, 0)?, -(z as f32)),
        Vec3::new((x + w) as f32, y(x + w - 1, z, 1)?, -(z as f32)),
        Vec3::new((x + w) as f32, y(x + w - 1, z + h - 1, 3)?, -((z + h) as f32)),
        Vec3::new(x as f32, y(x, z + h - 1, 2)?, -((z + h) as f32)),
    ])
}

fn outline(gizmos: &mut Gizmos, world: &StreamedWorld, x: i32, z: i32, w: i32, h: i32, colour: Color) {
    if let Some([a, b, c, d]) = block(world, x, z, w, h) {
        gizmos.linestrip([a, b, c, d, a], colour);
    }
}

/// A post standing on tile (`x`, `z`): NPC spawns and icon markers, which have no model.
fn post(gizmos: &mut Gizmos, world: &StreamedWorld, x: i32, z: i32, height: f32, colour: Color) {
    let Some(ground) = world.ground_at(x, z) else { return };
    gizmos.line(ground, ground + Vec3::Y * height, colour);
    gizmos.circle(Isometry3d::new(ground + Vec3::Y * LIFT, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), 0.4, colour);
}

/// Where each minimap icon of the loaded squares stands: world tile and area.
pub fn icons_in_view(editor: &mut Editor, catalog: &Catalog, world: &StreamedWorld) -> Vec<((i32, i32), u16)> {
    let mut out = Vec::new();
    let squares: Vec<(i32, i32)> = world.squares().collect();
    for square in squares {
        for p in editor.placements(square) {
            if p.plane != 0 {
                continue;
            }
            let icon = catalog
                .defs
                .get(&p.id)
                .and_then(|d| d.map_icon)
                .or_else(|| editor.project.marker_defs.iter().find(|m| m.id == p.id).map(|m| m.map_icon));
            if let Some(area) = icon.filter(|&a| a != 0xFFFF) {
                out.push(((square.0 * SQUARE + p.x as i32, square.1 * SQUARE + p.z as i32), area));
            }
        }
    }
    out
}

/// Icons of the loaded squares, recomputed when the world or the project changes.
#[derive(Resource, Default)]
pub struct IconsInView(pub Vec<((i32, i32), u16)>);

pub fn track_icons(mut editor: ResMut<Editor>, catalog: Res<Catalog>, world: Res<StreamedWorld>, mut icons: ResMut<IconsInView>) {
    if world.is_changed() || editor.is_changed() {
        icons.0 = icons_in_view(&mut editor, &catalog, &world);
    }
}

pub fn draw_marks(mut gizmos: Gizmos, editor: Res<Editor>, catalog: Res<Catalog>, hover: Res<Hover>, world: Res<StreamedWorld>, icons: Res<IconsInView>) {
    for s in &editor.project.npc_spawns {
        post(&mut gizmos, &world, s.x, s.z, 2.0, NPC);
    }
    for &((x, z), _) in &icons.0 {
        post(&mut gizmos, &world, x, z, 1.0, ICON);
    }
    match &editor.selected {
        Some(Selected::Placement { square, placement }) => {
            let (w, h) = footprint(catalog.defs.get(&placement.id), placement.orientation);
            outline(&mut gizmos, &world, square.0 * SQUARE + placement.x as i32, square.1 * SQUARE + placement.z as i32, w, h, SELECTED);
        }
        Some(Selected::Npc(i)) => {
            if let Some(s) = editor.project.npc_spawns.get(*i) {
                outline(&mut gizmos, &world, s.x, s.z, 1, 1, SELECTED);
            }
        }
        None => {}
    }
    let Some((x, z)) = hover.tile() else { return };
    match editor.tool {
        Tool::Place => match placing_object(&editor, &catalog) {
            Some((id, _, orientation)) => {
                let (w, h) = footprint(catalog.defs.get(&id), orientation);
                outline(&mut gizmos, &world, x, z, w, h, GHOST);
            }
            None => outline(&mut gizmos, &world, x, z, 1, 1, GHOST),
        },
        Tool::Erase => outline(&mut gizmos, &world, x, z, 1, 1, ERASE),
        Tool::Select => outline(&mut gizmos, &world, x, z, 1, 1, HOVER),
    }
}
