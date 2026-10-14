//! Print objects' menu options, to tell a closed door ("Open") from an open one ("Close"):
//! `obj_actions <id>...`, or `obj_actions --named <name>` for every object with that name.
use osrs_cache::config::{Configs, ObjectDef};
use osrs_cache::*;

fn line(d: &ObjectDef) -> String {
    let actions: Vec<&str> = d.actions.iter().map(String::as_str).filter(|a| !a.is_empty()).collect();
    format!(
        "{} '{}' actions {:?} size {}x{} interact_type {} models {:?}",
        d.id, d.name, actions, d.size_x, d.size_y, d.interact_type, d.models
    )
}

fn main() -> Result<()> {
    let c = Cache::open_default()?;
    let defs = Configs::new(&c)?.objects()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--named") {
        let name = args[1..].join(" ").to_lowercase();
        let mut found: Vec<&ObjectDef> = defs.values().filter(|d| d.name.to_lowercase() == name).collect();
        found.sort_by_key(|d| d.id);
        for d in found {
            println!("{}", line(d));
        }
        return Ok(());
    }
    for a in &args {
        let id: u32 = a.parse().unwrap();
        match defs.get(&id) {
            Some(d) => println!("{}", line(d)),
            None => println!("{id}: no such object"),
        }
    }
    Ok(())
}
