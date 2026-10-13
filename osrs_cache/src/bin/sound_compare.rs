//! Compares synth renders with recordings named <id>.wav (8-bit mono 22050): sound_compare <dir>
use osrs_cache::*;
fn main() -> Result<()> {
    let dir = std::env::args().nth(1).unwrap();
    let c = Cache::open_default()?;
    let ix = c.index(sound::ARCHIVE)?;
    let (mut exact, mut near, mut bad) = (0, 0, Vec::new());
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        let Some(id) = p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u32>().ok()) else { continue };
        if p.extension().and_then(|s| s.to_str()) != Some("wav") { continue; }
        let b = std::fs::read(&p)?;
        // 44-byte header, 8-bit mono 22050 only
        if b.len() < 45 || b[22] != 1 || b[34] != 8 || u32::from_le_bytes(b[24..28].try_into().unwrap()) != 22050 { continue; }
        let rec: Vec<i32> = b[44..].iter().map(|x| *x as i32 - 128).collect();
        let fx = sound::load_from(&c, &ix, id)?;
        let pcm: Vec<i32> = fx.mix().iter().map(|s| *s as i32).collect();
        let n = rec.len().min(pcm.len());
        let diff: i64 = (0..n).map(|i| (rec[i] - pcm[i]).abs() as i64).sum();
        let energy: i64 = (0..n).map(|i| rec[i].abs() as i64).sum::<i64>().max(1);
        let rel = diff as f64 / energy as f64;
        if rec.len() == pcm.len() && diff == 0 { exact += 1 } else if rel < 0.05 { near += 1 } else { bad.push((id, rel, rec.len(), pcm.len())) }
    }
    println!("exact {exact}, near (<5% error) {near}, different {}", bad.len());
    for (id, rel, a, b) in bad { println!("  {id}: error {:.0}%, recording {a} samples, render {b}", rel * 100.0); }
    Ok(())
}
