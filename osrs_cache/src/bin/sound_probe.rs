//! Probe sound storage: sound_probe [archive] [group] [out.wav]
//! With archive 4 and a wav path, the synth sound is mixed and written out to listen to.
use osrs_cache::*;

fn hex(d: &[u8], n: usize) -> String {
    d.iter().take(n).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let archive: u8 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let c = Cache::open_default()?;
    let ix = c.index(archive)?;
    let named = ix.groups.values().filter(|g| g.name_hash != 0).count();
    let multi = ix.groups.values().filter(|g| g.files.len() > 1).count();
    println!("archive {archive}: {} groups, {named} named, {multi} with several files, last id {:?}", ix.groups.len(), ix.groups.keys().last());
    if let Some(g) = a.get(2).and_then(|s| s.parse::<u32>().ok()) {
        let d = c.read_group(archive, g)?;
        println!("group {g}: {} bytes: {}", d.len(), hex(&d, 160));
        if let (4, Some(path)) = (archive, a.get(3)) {
            let fx = sound::load(&c, g)?;
            let pcm = fx.mix();
            let peak = pcm.iter().map(|s| (*s as i32).abs()).max().unwrap_or(0);
            println!("{} instruments, {} ms, {} samples, peak {peak}/127", fx.instruments.len(), fx.duration_ms(), pcm.len());
            std::fs::write(path, wav(&pcm))?;
        }
    }
    Ok(())
}

/// A mono 8-bit WAV file (WAV 8-bit samples are unsigned).
fn wav(pcm: &[i8]) -> Vec<u8> {
    let rate = sound::SAMPLE_RATE;
    let mut w = Vec::new();
    w.extend(b"RIFF");
    w.extend((36 + pcm.len() as u32).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(rate.to_le_bytes());
    w.extend(rate.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(8u16.to_le_bytes());
    w.extend(b"data");
    w.extend((pcm.len() as u32).to_le_bytes());
    w.extend(pcm.iter().map(|s| (*s as i16 + 128) as u8));
    w
}
