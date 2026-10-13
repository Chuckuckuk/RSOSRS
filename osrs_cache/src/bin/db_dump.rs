//! Dumps database rows (config group 38) of one table, decoded: `db_dump <table> [column value]`.
//! `db_dump raw <group> <file>` hex-dumps one config file.
use osrs_cache::dbtable::{DbRow, DbTables};
use osrs_cache::Cache;

fn main() -> osrs_cache::Result<()> {
    let cache = Cache::open_default()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("raw") {
        let cfg = cache.index(2)?;
        let d = cache.read_file(&cfg, args[1].parse().unwrap(), args[2].parse().unwrap())?;
        println!("{} bytes: {:02x?}", d.len(), d);
        return Ok(());
    }
    let tables = DbTables::load(&cache)?;
    if args.first().map(String::as_str) == Some("tables") {
        let mut counts = std::collections::BTreeMap::new();
        for r in tables.rows_of_all() {
            *counts.entry(r.table).or_insert(0) += 1;
        }
        println!("{counts:?}");
        return Ok(());
    }
    let table: u32 = args.first().map_or(78, |a| a.parse().unwrap());
    for row in tables.rows_of(table) {
        print_row(row);
    }
    Ok(())
}

fn print_row(r: &DbRow) {
    println!("row {} table {}: {:?}", r.id, r.table, r.columns);
}
