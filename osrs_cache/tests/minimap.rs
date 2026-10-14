//! The minimap picture of the Lumbridge square (skipped when the cache is absent).

use osrs_cache::minimap::{render_square, SIZE};
use osrs_cache::*;

fn cache() -> Option<Cache> {
    Cache::open_default().ok()
}

fn count(rgba: &[u8], f: impl Fn(u8, u8, u8) -> bool) -> usize {
    rgba.chunks_exact(4).filter(|p| p[3] == 255 && f(p[0], p[1], p[2])).count()
}

#[test]
fn lumbridge_minimap_has_water_walls_doors_and_icons() {
    let Some(c) = cache() else { return };
    let m = render_square(&c, 50, 50).unwrap();
    let img = &m.image;
    assert_eq!((img.width as usize, img.height as usize), (SIZE, SIZE));
    assert_eq!(img.rgba.len(), SIZE * SIZE * 4);

    // the river Lum and the pond: plenty of blue-ish pixels
    let blue = count(&img.rgba, |r, g, b| b as i32 > r as i32 + 30 && b as i32 > g as i32 + 10);
    assert!(blue > 1500, "{blue} blue pixels");
    // fences and castle walls are white lines, doors red ones
    let white = count(&img.rgba, |r, g, b| (r, g, b) == (0xEE, 0xEE, 0xEE));
    let red = count(&img.rgba, |r, g, b| (r, g, b) == (0xEE, 0, 0));
    assert!(white > 300, "{white} white pixels");
    assert!(red > 10, "{red} red pixels");

    // the castle (tiles about x 3205..3215, z 3205..3228) has wall pixels on its edges
    let (x0, x1, z0, z1) = (3205, 3216, 3205, 3229);
    let mut castle_white = 0;
    for z in z0..z1 {
        for x in x0..x1 {
            let (px, py) = (((x - 3200) * 4) as usize, ((63 - (z - 3200)) * 4) as usize);
            for dy in 0..4 {
                for dx in 0..4 {
                    let o = ((py + dy) * SIZE + px + dx) * 4;
                    if img.rgba[o..o + 3] == [0xEE, 0xEE, 0xEE] {
                        castle_white += 1;
                    }
                }
            }
        }
    }
    assert!(castle_white > 20, "{castle_white} white pixels around the castle");

    // map function icons are listed with tiles inside the square
    assert!(!m.icons.is_empty());
    assert!(m.icons.iter().all(|i| (3200..3264).contains(&i.x) && (3200..3264).contains(&i.z)));
}
