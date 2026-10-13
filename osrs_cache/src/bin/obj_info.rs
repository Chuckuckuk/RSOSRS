//! Describe an object definition's models and texture mapping: obj_info <object id>
use osrs_cache::config::Configs;
use osrs_cache::model::Model;
use osrs_cache::*;
fn main() -> Result<()> {
    let id: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    let c = Cache::open_default()?;
    let defs = Configs::new(&c)?.objects()?;
    let d = &defs[&id];
    println!("object {id}: models {:?} types {:?} size {}x{} rotated {} offsets {} {} {} scale {} {} {}",
        d.models, d.model_types, d.size_x, d.size_y, d.rotated, d.offset_x, d.offset_y, d.offset_z,
        d.model_size_x, d.model_size_y, d.model_size_z);
    println!("  retexture {:?} -> {:?}", d.retexture_from, d.retexture_to);
    for mid in &d.models {
        let m = Model::decode(*mid, &c.read_group(idx::MODELS, *mid)?)?;
        println!("model {mid}: v{} verts {} faces {} tex_tris {:?}", m.version, m.vertex_count(), m.face_count(), m.tex_tris);
        for f in 0..m.face_count().min(12) {
            println!("  face {f}: v({},{},{}) tex {} coord {:?} render {:?} alpha {:?}",
                m.fa[f], m.fb[f], m.fc[f],
                m.textures.get(f).copied().unwrap_or(-1), m.face_coord.get(f), m.render_type.get(f), m.alpha.get(f));
        }
        println!("  verts: {:?}", (0..m.vertex_count().min(std::env::var("ALLV").map_or(8, |_| 10000))).map(|i| (m.vx[i], m.vy[i], m.vz[i])).collect::<Vec<_>>());
    }
    Ok(())
}
