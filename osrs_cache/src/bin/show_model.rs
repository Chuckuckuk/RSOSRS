use osrs_cache::model::Model;
use osrs_cache::*;
fn main() -> Result<()> {
    let id: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    let c = Cache::open_default()?;
    let data = c.read_group(idx::MODELS, id)?;
    let m = Model::decode(id, &data)?;
    println!("model {id} v{} verts {} faces {}", m.version, m.vertex_count(), m.face_count());
    for f in 0..m.face_count() {
        println!(
            "face {f:3}: color {:04x} render {:?} tex {:?} alpha {:?} prio {} skin {:?}",
            m.colors[f],
            m.render_type.get(f),
            m.textures.get(f),
            m.alpha.get(f),
            m.priority[f],
            m.face_skin.get(f)
        );
    }
    println!("raw len {}", data.len());
    Ok(())
}
