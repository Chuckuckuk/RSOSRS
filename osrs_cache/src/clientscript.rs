//! Client scripts (CS2), archive 12: one group per script id. Decoder only; there is no VM.
//!
//! Layout (RuneLite `ScriptLoader`, "rev237" form, which this cache follows):
//! `[name: string or a single 0 byte][instructions...][switch tables][trailer]` where the trailer is
//! `[u32 instruction count][u16 local ints][u16 local strings][u16 local longs][u16 int args]
//! [u16 string args][u16 long args][u8 switch table count; per table u16 n, n x (i32 key, i32 offset)]
//! [u16 byte length of the switch section]`. An instruction is `u16 opcode` plus an operand: a
//! null-terminated string for `sconst`, a u64 for `lconst`, a u8 for `return`/`pop_*`/`push_null`, a
//! u32 for the other opcodes below 100 and a u8 for opcodes 100 and up. [`decode`] is strict: it fails
//! unless it consumes every byte and finds exactly the stated instruction count.

use crate::buf::Buf;
use crate::{fmt_err, Cache, Result};

/// Archive (index) holding client scripts.
pub const SCRIPTS_ARCHIVE: u8 = 12;
use std::collections::HashMap;

include!("clientscript_names.rs");

pub const SCONST: u16 = 3;
pub const LCONST: u16 = 61;
pub const RETURN: u16 = 21;
pub const POP_INT: u16 = 38;
pub const POP_OBJECT: u16 = 39;
pub const PUSH_NULL: u16 = 63;
pub const SWITCH: u16 = 60;
pub const JUMP: u16 = 6;
pub const INVOKE: u16 = 40;

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    Int(i32),
    Long(i64),
    Str(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instr {
    pub opcode: u16,
    pub operand: Operand,
}

impl Instr {
    pub fn int(&self) -> i32 {
        match self.operand {
            Operand::Int(v) => v,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Script {
    pub id: u32,
    /// Name hash of the group (0 when the archive has no names).
    pub name_hash: i32,
    /// Name stored in the script itself (usually absent).
    pub name: Option<String>,
    pub int_args: usize,
    pub str_args: usize,
    pub long_args: usize,
    pub int_locals: usize,
    pub str_locals: usize,
    pub long_locals: usize,
    pub instructions: Vec<Instr>,
    /// Switch tables: key -> jump offset (relative to the instruction after the `switch`).
    pub switches: Vec<HashMap<i32, i32>>,
    /// Switch tables in file order (HashMap loses it).
    pub switch_order: Vec<Vec<(i32, i32)>>,
}

/// Name of an opcode, if RuneLite's table knows it.
pub fn opcode_name(op: u16) -> Option<&'static str> {
    OPCODE_NAMES.iter().find(|(o, _)| *o == op).map(|(_, n)| *n)
}

/// Opcode for a name (case-insensitive), accepting `op_1234` for unnamed ones.
pub fn opcode_by_name(name: &str) -> Option<u16> {
    let n = name.to_ascii_lowercase();
    if let Some(num) = n.strip_prefix("op_") {
        return num.parse().ok();
    }
    OPCODE_NAMES.iter().find(|(_, s)| *s == n).map(|(o, _)| *o)
}

enum Kind {
    Str,
    Long,
    Byte,
    Int,
}

fn operand_kind(op: u16) -> Kind {
    match op {
        SCONST => Kind::Str,
        LCONST => Kind::Long,
        RETURN | POP_INT | POP_OBJECT | PUSH_NULL => Kind::Byte,
        o if o < 100 => Kind::Int,
        _ => Kind::Byte,
    }
}

/// Decode one script's bytes, failing unless they are consumed exactly.
pub fn decode(id: u32, name_hash: i32, data: &[u8]) -> Result<Script> {
    let n = data.len();
    if n < 2 + 16 {
        return fmt_err(format!("script {id}: {n} bytes is too short"));
    }
    let switch_len = u16::from_be_bytes([data[n - 2], data[n - 1]]) as usize;
    if n < 2 + switch_len + 16 + 1 {
        return fmt_err(format!("script {id}: switch section {switch_len} overruns"));
    }
    let end = n - 2 - switch_len - 16;
    let mut t = Buf::at(data, end);
    let count = t.u32() as usize;
    let int_locals = t.u16() as usize;
    let str_locals = t.u16() as usize;
    let long_locals = t.u16() as usize;
    let int_args = t.u16() as usize;
    let str_args = t.u16() as usize;
    let long_args = t.u16() as usize;
    let mut switches = Vec::new();
    let mut switch_order = Vec::new();
    if switch_len > 0 || t.pos < n - 2 {
        let tables = t.u8() as usize;
        for _ in 0..tables {
            let k = t.u16() as usize;
            let mut m = HashMap::new();
            let mut v = Vec::new();
            for _ in 0..k {
                let key = t.i32();
                let off = t.i32();
                m.insert(key, off);
                v.push((key, off));
            }
            switches.push(m);
            switch_order.push(v);
        }
    }
    if t.pos != n - 2 {
        return fmt_err(format!("script {id}: switch tables end at {} not {}", t.pos, n - 2));
    }
    let mut b = Buf::new(&data[..end]);
    let name = if b.peek() != 0 {
        Some(b.string())
    } else {
        b.pos += 1;
        None
    };
    let mut instructions = Vec::with_capacity(count);
    while b.pos < end {
        if instructions.len() >= count {
            return fmt_err(format!("script {id}: more than {count} instructions"));
        }
        let opcode = b.u16();
        let operand = match operand_kind(opcode) {
            Kind::Str => Operand::Str(b.string()),
            Kind::Long => Operand::Long(((b.u32() as u64) << 32 | b.u32() as u64) as i64),
            Kind::Byte => Operand::Int(b.u8() as i32),
            Kind::Int => Operand::Int(b.i32()),
        };
        instructions.push(Instr { opcode, operand });
    }
    if b.pos != end {
        return fmt_err(format!("script {id}: instructions end at {} not {end}", b.pos));
    }
    if instructions.len() != count {
        return fmt_err(format!("script {id}: {} instructions, trailer says {count}", instructions.len()));
    }
    Ok(Script {
        id,
        name_hash,
        name,
        int_args,
        str_args,
        long_args,
        int_locals,
        str_locals,
        long_locals,
        instructions,
        switches,
        switch_order,
    })
}

/// All script ids present in the cache.
pub fn script_ids(cache: &Cache) -> Result<Vec<u32>> {
    Ok(cache.index(SCRIPTS_ARCHIVE)?.groups.keys().copied().collect())
}

/// Load one script by id.
pub fn load_script(cache: &Cache, id: u32) -> Result<Script> {
    let index = cache.index(SCRIPTS_ARCHIVE)?;
    load_script_with(cache, &index, id)
}

/// Like [`load_script`] with the reference table already read (for sweeps).
pub fn load_script_with(cache: &Cache, index: &crate::Index, id: u32) -> Result<Script> {
    let g = index.groups.get(&id).ok_or_else(|| crate::Error::Missing(format!("script {id}")))?;
    let data = cache.read_group(SCRIPTS_ARCHIVE, id)?;
    decode(id, g.name_hash, &data)
}

impl Script {
    /// Instruction index a branch/jump at `pc` lands on.
    pub fn target(&self, pc: usize, offset: i32) -> i64 {
        pc as i64 + offset as i64 + 1
    }
}

/// An enum definition (config group 8), the key/value tables scripts read with `enum`.
#[derive(Debug, Clone, Default)]
pub struct EnumDef {
    pub id: u32,
    /// Script var type characters (`i` = 105 int, `s` = 115 string, `o` obj, `I` component...).
    pub key_type: u8,
    pub val_type: u8,
    pub default_int: i32,
    pub default_str: String,
    pub entries: Vec<(i32, Operand)>,
}

impl EnumDef {
    /// Strict: fails unless it reads to the end exactly.
    pub fn decode(id: u32, data: &[u8]) -> Result<EnumDef> {
        let mut b = Buf::new(data);
        let mut e = EnumDef { id, ..Default::default() };
        loop {
            match b.u8() {
                0 => break,
                1 => e.key_type = b.u8(),
                2 => e.val_type = b.u8(),
                3 => e.default_str = b.string(),
                4 => e.default_int = b.i32(),
                5 => {
                    for _ in 0..b.u16() {
                        let k = b.i32();
                        e.entries.push((k, Operand::Str(b.string())));
                    }
                }
                6 => {
                    for _ in 0..b.u16() {
                        let k = b.i32();
                        e.entries.push((k, Operand::Int(b.i32())));
                    }
                }
                op @ (7 | 8) => {
                    let _size = b.u16();
                    for _ in 0..b.u16() {
                        let k = b.u16() as i32;
                        let v = if op == 7 { Operand::Str(b.string()) } else { Operand::Int(b.i32()) };
                        e.entries.push((k, v));
                    }
                }
                op => return fmt_err(format!("enum {id}: opcode {op} at {}", b.pos - 1)),
            }
            if b.pos > data.len() {
                return fmt_err(format!("enum {id}: overran"));
            }
        }
        if b.pos != data.len() {
            return fmt_err(format!("enum {id}: {} bytes left", data.len() - b.pos));
        }
        Ok(e)
    }
}

/// Load one enum (config group 8, file = enum id).
pub fn load_enum(cache: &Cache, id: u32) -> Result<EnumDef> {
    let cfg = cache.index(2)?;
    let d = cache.read_file(&cfg, crate::cfg::ENUM, id)?;
    EnumDef::decode(id, &d)
}
