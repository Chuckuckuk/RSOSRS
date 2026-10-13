//! Compares where the bow is during the attack animation with where the drawn-arrow graphic is.
use osrs_cache::anim::{Animations, Clip};
use osrs_cache::config::Configs;
use osrs_cache::item::ids_by_name;
use osrs_cache::kit::{build_player, Appearance, WornPart};
use osrs_cache::spotanim::build_spotanim_with_rig;
use osrs_cache::*;

fn centroid(positions: &[[f32; 3]]) -> [f32; 3] {
    let n = positions.len().max(1) as f32;
    let mut c = [0.0; 3];
    for p in positions {
        for k in 0..3 {
            c[k] += p[k] / n;
        }
    }
    c
}

fn main() -> Result<()> {
    let cache = Cache::open_default()?;
    let configs = Configs::new(&cache)?;
    let items = configs.items()?;
    let names = ids_by_name(&items);
    let hsl = configs.texture_average_hsl()?;
    let spots = configs.spotanims()?;
    let bow = WornPart::of(&items[&names["magic shortbow"]], false).expect("bow");
    println!("bow models {:?} slot {}", bow.models, bow.slot);
    // the whole character with and without the bow: the bow's vertices are the first ones
    // (nothing is worn in the slots before the weapon's), so the difference counts them
    let with = build_player(&cache, &Appearance::default_male(), &[bow.clone()], &hsl)?.expect("with");
    let without = build_player(&cache, &Appearance::default_male(), &[], &hsl)?.expect("without");
    let bow_vertices = with.rig.as_ref().unwrap().positions.len() - without.rig.as_ref().unwrap().positions.len();
    println!("the bow is the first {bow_vertices} of {} vertices", with.rig.as_ref().unwrap().positions.len());
    let bow_mesh = with;
    let arrow_def = &spots[&19];
    println!("arrow graphic {arrow_def:?}");
    let arrow_mesh = build_spotanim_with_rig(&cache, arrow_def, &hsl, true).expect("arrow mesh");
    let mut animations = Animations::new(&cache)?;
    let attack = Clip::bake(&mut animations, 426, bow_mesh.rig.as_ref().unwrap())?;
    let draw = Clip::bake(&mut animations, arrow_def.sequence.unwrap(), arrow_mesh.rig.as_ref().unwrap())?;
    println!("attack 426: {} frames {} ticks; draw {}: {} frames {} ticks", attack.frames.len(), attack.duration(), arrow_def.sequence.unwrap(), draw.frames.len(), draw.duration());
    let bounds = |pts: &[[f32; 3]]| {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in pts {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    };
    for tick in [0usize, 20, 32, 40] {
        let (bl, bh) = bounds(&attack.frames[attack.frame_at(tick as f32)][..bow_vertices]);
        let (al, ah) = bounds(&draw.frames[draw.frame_at(tick as f32)]);
        println!("t={tick}: bow x {:.0}..{:.0} y {:.0}..{:.0} z {:.0}..{:.0}", bl[0], bh[0], bl[1], bh[1], bl[2], bh[2]);
        println!("      arrow x {:.0}..{:.0} y {:.0}..{:.0} z {:.0}..{:.0} (y +100 up: {:.0}..{:.0})", al[0], ah[0], al[1], ah[1], al[2], ah[2], al[1] - 100.0, ah[1] - 100.0);
    }
    let rows = attack.duration().max(draw.duration()) as usize;
    for tick in (0..rows).step_by(2) {
        let a = centroid(&attack.frames[attack.frame_at(tick as f32)][..bow_vertices]);
        let d = centroid(&draw.frames[draw.frame_at(tick as f32)]);
        println!(
            "t={tick:3} bow ({:7.1},{:7.1},{:7.1})  arrow ({:7.1},{:7.1},{:7.1})  diff ({:6.1},{:6.1},{:6.1})",
            a[0], a[1], a[2], d[0], d[1], d[2], d[0] - a[0], d[1] - a[1], d[2] - a[2]
        );
    }
    Ok(())
}
