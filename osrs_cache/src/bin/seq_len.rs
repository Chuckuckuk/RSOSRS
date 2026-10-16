//! Lengths of animation sequences: seq_len <sequence id>...
use osrs_cache::anim::Animations;
use osrs_cache::*;

fn main() -> Result<()> {
    let cache = Cache::open_default()?;
    let animations = Animations::new(&cache)?;
    for id in std::env::args().skip(1).filter_map(|a| a.parse::<u32>().ok()) {
        let seq = animations.sequence(id)?;
        println!("{id}: {} frames, delays {:?}, {} client ticks = {:.2} game ticks", seq.delays.len(), seq.delays, seq.duration(), seq.duration() as f32 * 0.02 / 0.6);
    }
    Ok(())
}
