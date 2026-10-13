//! Picking against the real Lumbridge map (skipped when the game cache is absent).

use bevy::prelude::{Dir3, Ray3d, Vec3};
use osrs_render::pick::{Pick, Picker};
use osrs_render::world::{load_world, WorldData};

fn lumbridge() -> Option<WorldData> {
    load_world(49, 49, 3, 3).ok()
}

fn down_at(x: f32, y: f32, z: f32) -> Ray3d {
    Ray3d::new(Vec3::new(x, y, z), Dir3::NEG_Y)
}

#[test]
fn a_ray_straight_down_picks_the_tile_under_it() {
    let Some(world) = lumbridge() else { return };
    let picker = Picker::new(&world, 0);
    // grid tile (86, 82) is Lumbridge square; bevy z is minus north
    match picker.pick(&world, down_at(86.4, 60.0, -82.6)) {
        Pick::Ground { x, z } => assert_eq!((x, z), (86, 82)),
        other => panic!("expected ground, got {other:?}"),
    }
}

#[test]
fn an_object_with_actions_takes_the_click_from_the_ground_behind_it() {
    let Some(world) = lumbridge() else { return };
    let picker = Picker::new(&world, 0);

    // find an interactive plane-0 object and aim at the middle of its top from straight above
    let (key, origin, _) = world
        .objects
        .instances
        .iter()
        .find(|(k, _, p)| *p == 0 && world.objects.left_click.contains_key(&k.0) && !world.objects.meshes[k].opaque.is_empty())
        .expect("Lumbridge has interactive scenery");
    let mesh = &world.objects.meshes[key].opaque;
    let top = mesh.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
    let tri = mesh.indices.chunks_exact(3).find(|t| {
        // a triangle whose centroid is near the top: a point we know the ray can reach
        let y = t.iter().map(|i| mesh.positions[*i as usize][1]).sum::<f32>() / 3.0;
        y > top - 0.3
    });
    let tri = tri.expect("a top triangle");
    let c: Vec3 = tri.iter().map(|i| Vec3::from(mesh.positions[*i as usize])).sum::<Vec3>() / 3.0 + *origin;

    match picker.pick(&world, down_at(c.x, c.y + 20.0, c.z)) {
        Pick::Object { id, .. } => assert_eq!(id, key.0),
        other => panic!("expected the object {} to take the click, got {other:?}", key.0),
    }

    // with the object's click box gone (no interactive ids), the same ray falls through to ground
    let mut open = lumbridge().unwrap();
    std::sync::Arc::make_mut(&mut open.objects).left_click.clear();
    let picker = Picker::new(&open, 0);
    assert!(matches!(picker.pick(&open, down_at(c.x, c.y + 20.0, c.z)), Pick::Ground { .. }));
}

#[test]
fn scenery_without_actions_does_not_block_clicks() {
    let Some(world) = lumbridge() else { return };
    let picker = Picker::new(&world, 0);
    let boxed = picker.pick_targets();
    let plain = world
        .objects
        .instances
        .iter()
        .filter(|(k, _, p)| *p == 0 && !world.objects.left_click.contains_key(&k.0))
        .count();
    assert!(plain > 0 && boxed > 0, "both kinds of scenery exist on plane 0");
    assert!(boxed < world.objects.instances.len());
}

#[test]
fn objects_with_no_left_click_option_or_plain_walk_here_are_passed_through() {
    use osrs_cache::config::ObjectDef;
    let def = |actions: &[&str]| ObjectDef { actions: actions.iter().map(|s| s.to_string()).collect(), ..Default::default() };
    assert_eq!(def(&["Open", "Examine"]).left_click(), Some("Open"));
    // only a second option: the left-click is empty, so the object does not intercept
    assert_eq!(def(&["", "Pick-lock"]).left_click(), None);
    assert_eq!(def(&["Walk here"]).left_click(), None);
    assert_eq!(def(&["walk here", "Examine"]).left_click(), None);
    assert_eq!(def(&["Hidden"]).left_click(), None);
    assert_eq!(def(&[]).left_click(), None);
}

#[test]
fn walk_here_under_an_object_goes_to_the_ground_at_the_end_of_the_path() {
    let Some(world) = lumbridge() else { return };
    let picker = Picker::new(&world, 0);
    let (key, origin, _) = world
        .objects
        .instances
        .iter()
        .find(|(k, _, p)| *p == 0 && world.objects.left_click.contains_key(&k.0) && !world.objects.meshes[k].opaque.is_empty())
        .expect("Lumbridge has interactive scenery");
    let mesh = &world.objects.meshes[key].opaque;
    let top = mesh.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
    let tri = mesh
        .indices
        .chunks_exact(3)
        .find(|t| t.iter().map(|i| mesh.positions[*i as usize][1]).sum::<f32>() / 3.0 > top - 0.3)
        .expect("a top triangle");
    let c: Vec3 = tri.iter().map(|i| Vec3::from(mesh.positions[*i as usize])).sum::<Vec3>() / 3.0 + *origin;
    let ray = down_at(c.x, c.y + 20.0, c.z);

    let click = picker.pick_all(&world, ray).expect("the ray reaches ground");
    let object = click.object.expect("the object is in the way");
    assert_eq!(object.id, key.0);
    assert!(!object.action.is_empty());

    // the same ray with no click boxes ends on the same tile: that is where "Walk here" goes
    let mut open = lumbridge().unwrap();
    std::sync::Arc::make_mut(&mut open.objects).left_click.clear();
    let bare = Picker::new(&open, 0).pick_all(&open, ray).unwrap();
    assert!(bare.object.is_none());
    assert_eq!(bare.tile, click.tile);
}

#[test]
fn a_named_object_with_no_left_click_option_can_still_be_examined() {
    let Some(world) = lumbridge() else { return };
    let picker = Picker::new(&world, 0);
    // a named plane-0 object that a left-click passes through
    let (key, origin, _) = world
        .objects
        .instances
        .iter()
        .find(|(k, _, p)| {
            *p == 0
                && world.objects.names.contains_key(&k.0)
                && !world.objects.left_click.contains_key(&k.0)
                && world.objects.meshes[k].opaque.positions.len() > 30
        })
        .expect("Lumbridge has named scenery with no left-click option");
    let mesh = &world.objects.meshes[key].opaque;
    let top = mesh.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
    let tri = mesh
        .indices
        .chunks_exact(3)
        .find(|t| t.iter().map(|i| mesh.positions[*i as usize][1]).sum::<f32>() / 3.0 > top - 0.2)
        .expect("a top triangle");
    let c: Vec3 = tri.iter().map(|i| Vec3::from(mesh.positions[*i as usize])).sum::<Vec3>() / 3.0 + *origin;
    let click = picker.pick_all(&world, down_at(c.x, c.y + 20.0, c.z)).expect("ground");

    // a left-click is not intercepted by it ...
    assert!(
        click.object.as_ref().is_none_or(|object| object.id != key.0),
        "an object without a left-click option must not intercept"
    );
    // ... but it is the nearest named thing in the way, so it can be examined
    let (id, name) = click.examinable.expect("examinable");
    assert_eq!(id, key.0);
    assert_eq!(name, world.objects.names[&key.0]);
}
