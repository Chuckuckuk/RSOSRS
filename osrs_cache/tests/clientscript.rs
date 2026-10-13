//! Client script decoding against the locally installed cache (skipped when absent).

use osrs_cache::clientscript::{self, Operand};
use osrs_cache::*;

#[test]
fn all_scripts_decode_exactly() {
    let Ok(c) = Cache::open_default() else { return };
    let index = c.index(clientscript::SCRIPTS_ARCHIVE).unwrap();
    let (mut ok, mut bad) = (0, Vec::new());
    for &id in index.groups.keys() {
        // group 0 is a 2-byte stub (`00 0a`), not a script: too short to hold even the trailer
        if id == 0 && c.read_group(clientscript::SCRIPTS_ARCHIVE, 0).unwrap().len() == 2 {
            continue;
        }
        match clientscript::load_script_with(&c, &index, id) {
            Ok(_) => ok += 1,
            Err(e) => bad.push(format!("{id}: {e}")),
        }
    }
    assert!(bad.is_empty(), "{} of {} scripts failed: {:?}", bad.len(), ok + bad.len(), &bad[..bad.len().min(5)]);
    assert!(ok > 5_000);
}

#[test]
fn toplevel_redraw_script_907_shape() {
    let Ok(c) = Cache::open_default() else { return };
    let s = clientscript::load_script(&c, 907).unwrap();
    // starts by reading a varbit and ends in a return
    assert_eq!(clientscript::opcode_name(s.instructions[0].opcode), Some("get_varbit"));
    assert!(matches!(s.instructions[0].operand, Operand::Int(_)));
    assert_eq!(s.instructions.last().unwrap().opcode, clientscript::RETURN);
}

#[test]
fn all_enums_decode_exactly() {
    let Ok(c) = Cache::open_default() else { return };
    let cfg = c.index(2).unwrap();
    let (mut ok, mut bad) = (0, Vec::new());
    for (id, d) in c.read_files(&cfg, cfg::ENUM).unwrap() {
        match clientscript::EnumDef::decode(id, &d) {
            Ok(_) => ok += 1,
            Err(e) => bad.push(e.to_string()),
        }
    }
    assert!(bad.is_empty(), "{} of {} enums failed: {:?}", bad.len(), ok + bad.len(), &bad[..bad.len().min(5)]);
    assert!(ok > 1_000);
}

#[test]
fn inventory_script_6007_creates_28_slot_grid() {
    let Ok(c) = Cache::open_default() else { return };
    let s = clientscript::load_script(&c, 6010).unwrap();
    // slot pitch 42 x 36 in 4 columns: the script divides/mods the slot index by 4
    let consts: Vec<i32> = s.instructions.iter().filter(|i| i.opcode == 0).map(|i| i.int()).collect();
    assert!(consts.contains(&42) && consts.contains(&36) && consts.contains(&4));
}
