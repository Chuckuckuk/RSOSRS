//! Lists NPC definitions: npc_probe [name] prints the ones with that name; with no name it
//! reports how many definitions stop at an opcode the decoder does not know.
use osrs_cache::config::Configs;
use osrs_cache::*;
use std::collections::BTreeMap;

fn main() -> Result<()> {
    let c = Cache::open_default()?;
    let npcs = Configs::new(&c)?.npcs()?;
    match std::env::args().nth(1) {
        Some(name) => {
            let mut ids: Vec<_> = npcs.values().filter(|n| n.name.eq_ignore_ascii_case(&name)).collect();
            ids.sort_by_key(|n| n.id);
            let index = c.index(idx::CONFIG)?;
            let raw: std::collections::HashMap<u32, Vec<u8>> = c.read_files(&index, cfg::NPC)?.into_iter().collect();
            for n in ids {
                println!("{n:?}");
                let hex: String = raw[&n.id].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
                println!("   raw: {hex}");
            }
        }
        None => {
            let mut unknown: BTreeMap<u8, Vec<u32>> = BTreeMap::new();
            for n in npcs.values() {
                if let Some(op) = n.unknown_opcode {
                    unknown.entry(op).or_default().push(n.id);
                }
            }
            println!("{} npc definitions", npcs.len());
            for (op, ids) in unknown {
                println!("opcode {op}: {} definitions, e.g. {:?}", ids.len(), &ids[..ids.len().min(5)]);
            }
        }
    }
    Ok(())
}
