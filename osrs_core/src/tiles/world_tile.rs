use super::tile_coord::TileCoord;

/// World-unit-per-tile conversion: 1 world unit = 1 tile. A tile's centre sits at
/// `(x + 0.5, z + 0.5)` per `TileCoord`'s documented convention, so world-to-tile must floor
/// (never truncate) to round negative coordinates toward negative infinity like every other
/// tile in the grid. Takes plain floats so the core stays engine-agnostic (the C# version takes
/// a Unity `Vector3`; `x`/`z` here are its horizontal components).
pub fn world_to_tile(world_x: f32, world_z: f32, plane: i32) -> TileCoord {
    TileCoord::with_plane(world_x.floor() as i32, world_z.floor() as i32, plane)
}

/// The `(x, z)` world position of the centre of `tile`.
pub fn tile_to_world_center(tile: TileCoord) -> (f32, f32) {
    (tile.x as f32 + 0.5, tile.z as f32 + 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_to_tile_floors_rather_than_truncates() {
        assert_eq!(world_to_tile(2.9, 0.1, 0), TileCoord::new(2, 0));
        assert_eq!(world_to_tile(-0.1, -2.5, 0), TileCoord::new(-1, -3));
    }

    #[test]
    fn tile_centre_round_trips_back_to_the_same_tile() {
        let tile = TileCoord::new(-4, 7);

        let (x, z) = tile_to_world_center(tile);

        assert_eq!((x, z), (-3.5, 7.5));
        assert_eq!(world_to_tile(x, z, 0), tile);
    }
}
