//! Which well-known sprite names exist in the cache, with frame counts and sizes: sprite_names [name]...
use osrs_cache::font::decode_frames;
use osrs_cache::*;

fn main() -> Result<()> {
    let cache = Cache::open_default()?;
    let sprites = cache.index(idx::SPRITES)?;
    let mut names: Vec<String> = std::env::args().skip(1).collect();
    if names.is_empty() {
        names = "sideicons redstone1 redstone2 redstone3 backbase1 backbase2 backhmid1 backhmid2 backleft1 backleft2 backright1 backright2 backtop1 backtop2 backvmid1 backvmid2 backvmid3 mapback compass mapedge scrollbar chatback steelborder steelborder2 mapfunction mapdots magicon magicon2 staticons staticons2 invback tabs orbs hitmarks headicons_prayer headicons_pk mod_icons prayeron prayeroff logout worldmap_icons miscgraphics sideicons2 miscgraphics2 miscgraphics3 chatbuttons chat_buttons"
            .split(' ')
            .map(String::from)
            .collect();
    }
    for name in names {
        match sprites.group_by_name(&name) {
            Some(group) => {
                let files = cache.read_files(&sprites, group.id)?;
                let frames = files.first().map(|(_, f)| decode_frames(f)).unwrap_or_default();
                let sizes: Vec<String> = frames.iter().take(6).map(|f| format!("{}x{}", f.width, f.height)).collect();
                println!("{name}: group {} frames {} [{}]", group.id, frames.len(), sizes.join(" "));
            }
            None => println!("{name}: -"),
        }
    }
    Ok(())
}
