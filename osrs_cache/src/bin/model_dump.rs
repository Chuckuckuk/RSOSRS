//! Dump a model's vertices and faces: model_dump <model id>
use osrs_cache::model::Model;
use osrs_cache::*;
fn main() -> Result<()> {
    let id: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    let c = Cache::open_default()?;
    let m = Model::decode(id, &c.read_group(idx::MODELS, id)?)?;
    println!("model {id}: v{} {} verts {} faces", m.version, m.vertex_count(), m.face_count());
    for i in 0..m.vertex_count() { println!("v{i}: {} {} {}", m.vx[i], m.vy[i], m.vz[i]); }
    for f in 0..m.face_count() {
        println!("f{f}: {} {} {} color {} alpha {:?} render {:?} pri {:?}", m.fa[f], m.fb[f], m.fc[f], m.colors[f], m.alpha.get(f), m.render_type.get(f), m.priority.get(f));
    }
    Ok(())
}
