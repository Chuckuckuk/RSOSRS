//! Which item opcodes the decoder does not know: item_census
use osrs_cache::config::Configs;
use osrs_cache::item::ItemDef;
use osrs_cache::*;
use std::collections::BTreeMap;

fn main() -> Result<()> {
    let cache = Cache::open_default()?;
    let configs = Configs::new(&cache)?;
    let mut stops: BTreeMap<u8, (usize, u32, usize)> = BTreeMap::new();
    let mut total = 0;
    let mut named = 0;
    for (id, data) in configs.group_files(10)? {
        total += 1;
        let (def, stop) = ItemDef::decode_checked(id, &data);
        if !def.name.is_empty() {
            named += 1;
        }
        if let Some((opcode, at)) = stop {
            let e = stops.entry(opcode).or_insert((0, id, at));
            e.0 += 1;
        }
    }
    println!("{total} items, {named} with a name");
    for (opcode, (count, example, at)) in stops {
        println!("opcode {opcode}: stops {count} items, e.g. item {example} at byte {at}");
    }
    Ok(())
}
