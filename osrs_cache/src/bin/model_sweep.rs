//! Decode every model and summarise failures and suspicious data (dev tool).
use osrs_cache::config::Configs;
use osrs_cache::model::{hsl_to_rgb, Model};
use osrs_cache::*;
use std::collections::{BTreeMap, HashMap};

fn main() -> Result<()> {
    let c = Cache::open_default()?;
    let cfg = Configs::new(&c)?;
    let tex = cfg.texture_average_hsl()?;
    let models = c.index(idx::MODELS)?;
    let mut by_err: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut ok = 0;
    let mut whites: Vec<u32> = Vec::new();
    let mut usage: HashMap<(u8, i16), u32> = HashMap::new();
    for &id in models.groups.keys() {
        let Ok(data) = c.read_group(idx::MODELS, id) else { continue };
        match Model::decode(id, &data) {
            Ok(m) => {
                ok += 1;
                let white = m.colors.iter().filter(|c| (**c & 127) >= 120 && (**c >> 7) & 7 == 0).count();
                if white >= 3 {
                    whites.push(id);
                }
                for t in m.textures.iter().filter(|t| **t >= 0) {
                    *usage.entry((m.version, *t)).or_default() += 1;
                }
            }
            Err(e) => by_err.entry(format!("v{} {}", Model::version_of(&data), e)).or_default().push(id),
        }
    }
    println!("ok {ok}; models with 3+ near-white faces: {}", whites.len());
    if let Some(p) = std::env::args().nth(1) {
        std::fs::write(p, whites.iter().map(|i| i.to_string()).collect::<Vec<_>>().join("
")).unwrap();
    }
    let mut u: Vec<_> = usage.into_iter().collect();
    u.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for ((ver, t), n) in u.into_iter().take(14) {
        let hsl = tex.get(&(t as u32)).copied();
        let rgb = hsl.map(|h| hsl_to_rgb(h, 0.8));
        println!("v{ver} texture {t}: {n} faces, avg hsl {:?} rgb {:.2?}", hsl, rgb);
    }
    for (e, ids) in by_err {
        println!("{e}: {} e.g. {:?}", ids.len(), &ids[..ids.len().min(6)]);
    }
    Ok(())
}
