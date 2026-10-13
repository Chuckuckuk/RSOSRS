//! Prints decoded item definitions: item_dump <id>...
use osrs_cache::config::Configs;
use osrs_cache::item::ItemDef;
use osrs_cache::*;

fn main() -> Result<()> {
    let cache = Cache::open_default()?;
    let wanted: Vec<u32> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    for (id, data) in Configs::new(&cache)?.group_files(10)? {
        if wanted.contains(&id) {
            let (def, stop) = ItemDef::decode_checked(id, &data);
            println!("{def:?}\nstopped: {stop:?}");
        }
    }
    Ok(())
}
