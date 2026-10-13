//! Lists which synth features a sound uses: sound_features <id>...  (reads ids from stdin lines if none)
use osrs_cache::*;
fn main() -> Result<()> {
    let c = Cache::open_default()?;
    let ix = c.index(sound::ARCHIVE)?;
    let ids: Vec<u32> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    for id in ids {
        let fx = sound::load_from(&c, &ix, id)?;
        let mut f = Vec::new();
        for i in &fx.instruments {
            let mut s = format!("form{}", i.pitch.form);
            if i.pitch_modifier.is_some() { s += "+vib"; }
            if i.volume_modifier.is_some() { s += "+trem"; }
            if i.gate.is_some() { s += "+gate"; }
            if i.delay_time > 0 && i.delay_decay > 0 { s += "+echo"; }
            if i.filter.pairs[0] > 0 || i.filter.pairs[1] > 0 { s += &format!("+filter{:?}", i.filter.pairs); }
            s += &format!("+osc{}", i.oscillators.len());
            f.push(s);
        }
        println!("{id}: {}", f.join(" | "));
    }
    Ok(())
}
