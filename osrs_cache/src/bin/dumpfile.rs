use osrs_cache::*;
use std::io::Write;

/// dumpfile model-tails <out.tsv>   : id, len, hex of last 40 bytes for every model
/// dumpfile group <archive> <group> <out.bin>
fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let c = Cache::open_default()?;
    match a[1].as_str() {
        "model-tails" => {
            let idx = c.index(idx::MODELS)?;
            let mut out = std::io::BufWriter::new(std::fs::File::create(&a[2])?);
            for &id in idx.groups.keys() {
                let Ok(d) = c.read_group(idx::MODELS, id) else { continue };
                let n = d.len();
                let tail: String = d[n.saturating_sub(40)..].iter().map(|b| format!("{b:02x}")).collect();
                let head: String = d[..n.min(16)].iter().map(|b| format!("{b:02x}")).collect();
                writeln!(out, "{id}\t{n}\t{tail}\t{head}")?;
            }
        }
        "model-pack" => {
            // [u32 id][u32 len][bytes]...
            let idx = c.index(idx::MODELS)?;
            let mut out = std::io::BufWriter::new(std::fs::File::create(&a[2])?);
            for &id in idx.groups.keys() {
                let Ok(d) = c.read_group(idx::MODELS, id) else { continue };
                out.write_all(&id.to_le_bytes())?;
                out.write_all(&(d.len() as u32).to_le_bytes())?;
                out.write_all(&d)?;
            }
        }
        "group" => {
            let arch: u8 = a[2].parse().unwrap();
            let gid: u32 = a[3].parse().unwrap();
            let d = c.read_group(arch, gid)?;
            std::fs::write(&a[4], &d)?;
            println!("wrote {} bytes", d.len());
        }
        _ => eprintln!("usage"),
    }
    Ok(())
}
