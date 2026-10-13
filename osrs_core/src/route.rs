//! Drawing a character's movement: the server says which tiles it stepped through in a tick, and
//! the client walks the model through those tile centres in order.

use bevy::prelude::*;
use osrs_core::tiles::TileCoord;
use std::collections::VecDeque;

/// A tile centre to walk to and the heading (radians about the up axis, 0 faces south) of the
/// step into it.
pub type Route = VecDeque<(Vec3, f32)>;

/// Lays out the route through `steps`, starting from `start` (the tile the character was last
/// said to stand on) and drawn from `at`. Returns it with its flat length in tiles.
///
/// The character goes through the tile centres the server stepped over, in order, exactly as the
/// game draws it (a straight leg then a diagonal one is correct); stepping tile by tile also keeps
/// it clear of anything the path avoids. The facing for a step is the direction of the tile move,
/// one of the eight compass points, not the angle from wherever it happens to be (tile z grows
/// north; the model front is +z in the render space, south).
pub fn lay_route(
    steps: &[TileCoord],
    start: TileCoord,
    mut at: Vec3,
    ground: impl Fn(TileCoord) -> Option<Vec3>,
) -> (Route, f32) {
    let mut route = Route::new();
    let mut length = 0.0;
    let mut previous = start;
    for tile in steps {
        if let Some(p) = ground(*tile) {
            length += (p - at).with_y(0.0).length();
            at = p;
            let heading = ((tile.x - previous.x) as f32).atan2(-(tile.z - previous.z) as f32);
            route.push_back((p, heading));
        }
        previous = *tile;
    }
    (route, length)
}

/// Moves `transform` along `route` by `budget` tiles, dropping the tile centres it reaches.
/// Returns the heading of the step it is on, if it was on one. `surface` gives the ground height
/// at a flat position, so a long straight run over uneven ground stays on it.
pub fn follow_route(
    route: &mut Route,
    transform: &mut Transform,
    mut budget: f32,
    surface: impl Fn(f32, f32) -> Option<f32>,
) -> Option<f32> {
    let mut heading = None;
    while let Some(&(next, step_heading)) = route.front() {
        let to_go = next - transform.translation;
        let flat = to_go.with_y(0.0).length();
        heading = Some(step_heading);
        if flat <= budget {
            transform.translation = next;
            budget -= flat;
            route.pop_front();
        } else {
            transform.translation += to_go * (budget / flat);
            if let Some(y) = surface(transform.translation.x, -transform.translation.z) {
                transform.translation.y = y;
            }
            break;
        }
    }
    heading
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(tile: TileCoord) -> Option<Vec3> {
        Some(Vec3::new(tile.x as f32 + 0.5, 0.0, -(tile.z as f32 + 0.5)))
    }

    #[test]
    fn a_route_goes_through_each_tile_centre_with_the_heading_of_its_step() {
        let start = TileCoord::new(10, 10);
        let steps = [TileCoord::new(11, 10), TileCoord::new(11, 11)];
        let (route, length) = lay_route(&steps, start, flat(start).unwrap(), flat);
        assert_eq!(route.len(), 2);
        assert!((length - 2.0).abs() < 1e-5);
        // east, then north (which is turned all the way round from the south-facing model front)
        assert!((route[0].1 - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((route[1].1.abs() - std::f32::consts::PI).abs() < 1e-5);
    }

    #[test]
    fn following_a_route_reaches_the_centres_and_reports_the_heading() {
        let start = TileCoord::new(0, 0);
        let (mut route, _) = lay_route(&[TileCoord::new(1, 0)], start, flat(start).unwrap(), flat);
        let mut transform = Transform::from_translation(flat(start).unwrap());

        let heading = follow_route(&mut route, &mut transform, 0.4, |_, _| None);
        assert!(heading.is_some() && !route.is_empty());
        assert!((transform.translation.x - 0.9).abs() < 1e-5);

        follow_route(&mut route, &mut transform, 5.0, |_, _| None);
        assert!(route.is_empty());
        assert_eq!(transform.translation, flat(TileCoord::new(1, 0)).unwrap());
    }
}
