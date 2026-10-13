//! Client script (CS2) disassembler.
//!   cs2_dump <id>...                 disassembly (labels, invoke targets, component ids, switch tables,
//!                                    and a best-effort "(args)" comment on consumers like cc_setposition)
//!   cs2_dump --find-calls <id>       scripts (and interface listeners) that call script <id>
//!   cs2_dump --grep <opcode name|op_N> [operand]   every use of an opcode, optionally with that int operand
//!   cs2_dump --census                opcodes the name table does not know, with counts
use osrs_cache::clientscript::{self, Instr, Operand, Script};
use osrs_cache::config::Configs;
use osrs_cache::interface::{self, Arg};
use osrs_cache::*;
use std::collections::{BTreeMap, BTreeSet, HashSet};

fn is_branch(op: u16) -> bool {
    matches!(op, 6..=10 | 31 | 32 | 68..=73)
}

fn comp_name(v: i32, groups: &HashSet<u32>) -> Option<String> {
    if v >= 65536 && groups.contains(&((v as u32) >> 16)) {
        Some(format!("{}:{}", (v as u32) >> 16, v & 0xffff))
    } else {
        None
    }
}

fn q(s: &str) -> String {
    format!("{s:?}")
}

/// Best-effort symbolic stacks to show the arguments a consumer op takes.
#[derive(Default)]
struct Sym {
    ints: Vec<String>,
    strs: Vec<String>,
}

impl Sym {
    fn pi(&mut self) -> String {
        self.ints.pop().unwrap_or_else(|| "?".into())
    }
    fn ps(&mut self) -> String {
        self.strs.pop().unwrap_or_else(|| "?".into())
    }
    fn pops_i(&mut self, n: usize) -> Vec<String> {
        let mut v: Vec<String> = (0..n).map(|_| self.pi()).collect();
        v.reverse();
        v
    }
    fn lost(&mut self) {
        self.ints.clear();
        self.strs.clear();
    }
}

/// (int pops, str pops, int pushes, str pushes) for the setter/getter families.
fn cc_arity(off: i32) -> Option<(usize, usize, usize, usize)> {
    Some(match off {
        0 | 1 => (4, 0, 0, 0),
        3 | 5 | 6 => (1, 0, 0, 0),
        100 => (2, 0, 0, 0),
        101..=108 | 110 | 111 | 115..=119 | 123 | 126 | 127 => (1, 0, 0, 0),
        109 => (6, 0, 0, 0),
        112 => (0, 1, 0, 0),
        113 => (1, 0, 0, 0),
        114 => (3, 0, 0, 0),
        120 => (2, 0, 0, 0),
        200 | 205 | 212 => (2, 0, 0, 0),
        201 => (1, 0, 0, 0),
        202 | 307 => (0, 0, 0, 0),
        300 => (1, 1, 0, 0),
        301 | 302 | 303 | 304 => (1, 0, 0, 0),
        305 | 306 => (0, 1, 0, 0),
        500 | 501 | 502 | 503 | 504 | 505 => (0, 0, 1, 0),
        600 | 601 | 603 | 604 | 605 | 606 | 607 | 608 | 609 | 611 | 612 | 614 => (0, 0, 1, 0),
        602 => (0, 0, 0, 1),
        700 | 701 => (0, 0, 1, 0), // cc_getinvobject etc. take nothing in the cc form
        702 => (0, 0, 1, 0),
        800 => (0, 0, 1, 0),
        801 => (1, 0, 0, 1),
        802 => (0, 0, 0, 1),
        _ => return None,
    })
}

fn describe(op: u16, operand: &Operand, sym: &mut Sym, groups: &HashSet<u32>) -> Option<String> {
    let o = match operand {
        Operand::Int(v) => *v,
        _ => 0,
    };
    let ci = |s: String| -> String {
        match s.parse::<i32>().ok().and_then(|v| comp_name(v, groups)) {
            Some(n) => n,
            None => s,
        }
    };
    match op {
        0 => {
            sym.ints.push(o.to_string());
            return None;
        }
        3 => {
            if let Operand::Str(s) = operand {
                sym.strs.push(q(s));
            }
            return None;
        }
        1 => sym.ints.push(format!("varp{o}")),
        25 => sym.ints.push(format!("varbit{o}")),
        42 => sym.ints.push(format!("varc{o}")),
        49 => sym.strs.push(format!("varcstr{o}")),
        33 => sym.ints.push(format!("$i{o}")),
        35 => sym.strs.push(format!("$s{o}")),
        2 | 27 | 43 | 34 => {
            let v = sym.pi();
            let tgt = match op {
                2 => format!("varp{o}"),
                27 => format!("varbit{o}"),
                43 => format!("varc{o}"),
                _ => format!("$i{o}"),
            };
            return Some(format!("{tgt} = {v}"));
        }
        50 | 36 => {
            let v = sym.ps();
            return Some(format!("{} = {v}", if op == 50 { format!("varcstr{o}") } else { format!("$s{o}") }));
        }
        37 => {
            let mut v: Vec<String> = (0..o).map(|_| sym.ps()).collect();
            v.reverse();
            sym.strs.push(format!("join({})", v.join(" + ")));
        }
        38 => {
            for _ in 0..o.max(1) {
                sym.pi();
            }
        }
        39 => {
            for _ in 0..o.max(1) {
                sym.ps();
            }
        }
        7..=10 | 31 | 32 => {
            let b = sym.pi();
            let a = sym.pi();
            let sym_op = match op {
                7 => "!=",
                8 => "==",
                9 => "<",
                10 => ">",
                31 => "<=",
                _ => ">=",
            };
            return Some(format!("{} {sym_op} {}", ci(a), ci(b)));
        }
        40 => {
            sym.lost();
            return None;
        }
        4000 | 4001 | 4002 | 4003 | 4011 | 4014 | 4015 => {
            let b = sym.pi();
            let a = sym.pi();
            let s = match op {
                4000 => "+",
                4001 => "-",
                4002 => "*",
                4003 => "/",
                4011 => "%",
                4014 => "&",
                _ => "|",
            };
            sym.ints.push(format!("({a} {s} {b})"));
        }
        4018 => {
            let c = sym.pi();
            let b = sym.pi();
            let a = sym.pi();
            sym.ints.push(format!("scale({a},{b},{c})"));
        }
        4100 => {
            let n = sym.pi();
            let s = sym.ps();
            sym.strs.push(format!("{s}+{n}"));
        }
        4101 => {
            let b = sym.ps();
            let a = sym.ps();
            sym.strs.push(format!("{a}+{b}"));
        }
        4106 => {
            let n = sym.pi();
            sym.strs.push(format!("tostring({n})"));
        }
        100 => {
            let a = sym.pops_i(4);
            return Some(format!("parent={} type={} index={} d={}", ci(a[0].clone()), a[1], a[2], a[3]));
        }
        101 => return None,
        102 => {
            let a = sym.pi();
            return Some(ci(a));
        }
        200 => {
            let a = sym.pops_i(2);
            sym.ints.push(format!("found({},{})", ci(a[0].clone()), a[1]));
        }
        201 => {
            let a = sym.pi();
            sym.ints.push(format!("found({})", ci(a)));
        }
        3100 => {
            let s = sym.ps();
            return Some(s);
        }
        3103 | 3104 => return None,
        3305 | 3306 | 3307 => {
            let a = sym.pi();
            sym.ints.push(format!("stat({a})"));
        }
        3301 | 3302 => {
            let a = sym.pops_i(2);
            sym.ints.push(format!("inv({},{})", a[0], a[1]));
        }
        3303 | 3304 => {
            let a = sym.pi();
            sym.ints.push(format!("invtotal({a})"));
        }
        3408 => {
            let a = sym.pops_i(4);
            sym.ints.push(format!("enum({})", a.join(",")));
        }
        1400..=1429 | 2400..=2429 => {
            let is_if = op >= 2000;
            let comp = if is_if { format!("{} ", ci(sym.pi())) } else { String::new() };
            let desc = sym.ps();
            let d = desc.trim_matches('"').to_string();
            let mut trig = String::new();
            let mut chars: Vec<char> = d.chars().collect();
            if chars.last() == Some(&'Y') {
                chars.pop();
                let n: usize = sym.pi().parse().unwrap_or(0);
                let t = sym.pops_i(n);
                trig = format!(" triggers=[{}]", t.join(","));
            }
            let mut args: Vec<String> = Vec::new();
            for ch in chars.iter().rev() {
                args.push(if *ch == 's' { sym.ps() } else { sym.pi() });
            }
            args.reverse();
            let script = sym.pi();
            return Some(format!("{comp}script={script} args=[{}]{trig}", args.join(", ")));
        }
        1000..=1499 | 2000..=2499 | 1500..=1999 | 2500..=2999 => {
            let is_if = op >= 2000;
            let key = (op % 1000) as i32;
            let Some((pi, ps, pushi, pushs)) = cc_arity(key) else {
                sym.lost();
                return None;
            };
            let comp = if is_if { Some(ci(sym.pi())) } else { None };
            let mut args: Vec<String> = Vec::new();
            for _ in 0..ps {
                args.push(sym.ps());
            }
            let mut ints = sym.pops_i(pi);
            args.reverse();
            ints.extend(args);
            for i in 0..pushi {
                sym.ints.push(if let Some(c) = &comp { format!("{}({c})#{i}", clientscript::opcode_name(op).unwrap_or("get")) } else { format!("{}()", clientscript::opcode_name(op).unwrap_or("get")) });
            }
            for _ in 0..pushs {
                sym.strs.push(format!("{}()", clientscript::opcode_name(op).unwrap_or("get")));
            }
            let mut shown: Vec<String> = ints.into_iter().map(|s| ci(s)).collect();
            if let Some(c) = comp {
                shown.insert(0, c);
            }
            if pushi + pushs > 0 {
                return None;
            }
            return Some(shown.join(", "));
        }
        _ => {
            if let Some((pi, ps, pushi, pushs)) = simple_effect(op) {
                let mut a: Vec<String> = Vec::new();
                for _ in 0..ps {
                    a.push(sym.ps());
                }
                let mut ints = sym.pops_i(pi);
                a.reverse();
                ints.extend(a);
                let name = clientscript::opcode_name(op).map(String::from).unwrap_or_else(|| format!("op_{op}"));
                let call = format!("{name}({})", ints.join(","));
                if pushi + pushs == 0 {
                    return Some(ints.join(", "));
                }
                for _ in 0..pushi {
                    sym.ints.push(call.clone());
                }
                for _ in 0..pushs {
                    sym.strs.push(call.clone());
                }
                return None;
            }
            sym.lost();
            return None;
        }
    }
    None
}

/// Stack effects of plain ops: (int pops, str pops, int pushes, str pushes).
fn simple_effect(op: u16) -> Option<(usize, usize, usize, usize)> {
    Some(match op {
        3300 | 3308 | 3309 | 3310 | 3311 | 3312 | 3316 | 3317 | 3318 | 3321 | 3322 | 3323 | 3324 | 3128 | 3132 => (0, 0, 1, 0),
        4200 => (1, 0, 0, 1),
        4201 | 4202 => (2, 0, 0, 1),
        4203..=4209 => (1, 0, 1, 0),
        4004 | 4005 | 4007 | 4010 | 4012 | 4013 | 4016 | 4017 | 4009 | 4008 => (2, 0, 1, 0),
        4025 => (1, 0, 1, 0),
        4106 => (1, 0, 0, 1),
        4117 => (0, 1, 1, 0),
        1312 | 1124 | 1125 => (1, 0, 0, 0),
        2702 => (1, 0, 1, 0),
        _ => return None,
    })
}

fn disasm(s: &Script, groups: &HashSet<u32>, scripts_args: &dyn Fn(u32) -> Option<(usize, usize)>) {
    println!(
        "script {}{}{}  args: {} int, {} str{}  locals: {} int, {} str{}  ({} instructions)",
        s.id,
        s.name.as_ref().map(|n| format!(" {n:?}")).unwrap_or_default(),
        if s.name_hash != 0 { format!(" hash={}", s.name_hash) } else { String::new() },
        s.int_args,
        s.str_args,
        if s.long_args > 0 { format!(", {} long", s.long_args) } else { String::new() },
        s.int_locals,
        s.str_locals,
        if s.long_locals > 0 { format!(", {} long", s.long_locals) } else { String::new() },
        s.instructions.len()
    );
    // labels: branch targets and switch targets
    let mut labels: BTreeSet<i64> = BTreeSet::new();
    for (pc, i) in s.instructions.iter().enumerate() {
        if is_branch(i.opcode) {
            labels.insert(s.target(pc, i.int()));
        }
        if i.opcode == clientscript::SWITCH {
            if let Some(t) = s.switch_order.get(i.int() as usize) {
                for (_, off) in t {
                    labels.insert(s.target(pc, *off));
                }
            }
        }
    }
    let mut sym = Sym::default();
    for (pc, i) in s.instructions.iter().enumerate() {
        if labels.contains(&(pc as i64)) {
            println!("L{pc}:");
            sym.lost();
        }
        let name = clientscript::opcode_name(i.opcode).map(String::from).unwrap_or_else(|| format!("op_{}", i.opcode));
        let mut line = format!("  {pc:4}  {name}");
        let mut note = String::new();
        match &i.operand {
            Operand::Str(t) => line += &format!(" {}", q(t)),
            Operand::Long(v) => line += &format!(" {v}L"),
            Operand::Int(v) => {
                if is_branch(i.opcode) {
                    line += &format!(" -> L{}", s.target(pc, *v));
                } else if i.opcode == clientscript::SWITCH {
                    line += &format!(" table {v}");
                } else if i.opcode == clientscript::INVOKE {
                    line += &format!(" script {v}");
                    if let Some((a, b)) = scripts_args(*v as u32) {
                        note = format!("callee takes {a} int, {b} str");
                    }
                } else if i.opcode == 0 {
                    line += &format!(" {v}");
                    if let Some(n) = comp_name(*v, groups) {
                        note = n;
                    }
                } else if i.opcode >= 100 || matches!(i.opcode, 21 | 38 | 39 | 63) {
                    if *v != 0 || i.opcode == 21 {
                        line += &format!(" {v}");
                    }
                } else {
                    line += &format!(" {v}");
                }
            }
        }
        if let Some(d) = describe(i.opcode, &i.operand, &mut sym, groups) {
            if note.is_empty() {
                note = format!("({d})");
            } else {
                note = format!("{note} ({d})");
            }
        }
        if i.opcode == clientscript::INVOKE && note.starts_with("callee") {
            // invoke: stack effect unknown; handled in describe via lost()
        }
        if note.is_empty() {
            println!("{line}");
        } else {
            println!("{line:<44}; {note}");
        }
    }
    for (n, t) in s.switch_order.iter().enumerate() {
        println!("  switch table {n}:");
        for (k, off) in t {
            // the pc of the switch itself is unknown here; find the instruction that references this table
            let pc = s.instructions.iter().position(|i| i.opcode == clientscript::SWITCH && i.int() as usize == n);
            match pc {
                Some(pc) => println!("    {k} -> L{}", s.target(pc, *off)),
                None => println!("    {k} -> +{off}"),
            }
        }
    }
}

fn instr_str(i: &Instr) -> String {
    match &i.operand {
        Operand::Int(v) => v.to_string(),
        Operand::Long(v) => format!("{v}L"),
        Operand::Str(s) => q(s),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: cs2_dump <id>... | --find-calls <id> | --grep <opcode> [operand] | --census");
        return Ok(());
    }
    let cache = Cache::open_default()?;
    let index = cache.index(clientscript::SCRIPTS_ARCHIVE)?;
    let groups: HashSet<u32> = interface::group_ids(&cache)?.into_iter().collect();
    let load_all = || -> Vec<Script> {
        index.groups.keys().filter_map(|&id| clientscript::load_script_with(&cache, &index, id).ok()).collect()
    };
    match args[0].as_str() {
        "--find-calls" => {
            let target: i32 = args[1].parse().unwrap();
            let mut callers: BTreeMap<u32, usize> = BTreeMap::new();
            for s in load_all() {
                let n = s.instructions.iter().filter(|i| i.opcode == clientscript::INVOKE && i.int() == target).count();
                if n > 0 {
                    callers.insert(s.id, n);
                }
            }
            println!("scripts invoking {target}: {callers:?}");
            let mut listeners = Vec::new();
            for g in interface::group_ids(&cache)? {
                for c in interface::load_group(&cache, g)? {
                    for (k, l) in c.listeners.iter().enumerate() {
                        if let Some(l) = l {
                            if l.script_id() == Some(target) {
                                let a: Vec<String> = l.args.iter().skip(1).map(|a| match a { Arg::Int(v) => v.to_string(), Arg::Str(s) => q(s) }).collect();
                                listeners.push(format!("  {}:{} {} args [{}]", c.group, c.file, interface::LISTENER_NAMES[k], a.join(",")));
                            }
                        }
                    }
                }
            }
            println!("interface listeners calling {target}: {}", listeners.len());
            for l in listeners {
                println!("{l}");
            }
        }
        "--grep" => {
            let op = clientscript::opcode_by_name(&args[1]).expect("unknown opcode name");
            let want: Option<i32> = args.get(2).and_then(|v| v.parse().ok());
            for s in load_all() {
                for (pc, i) in s.instructions.iter().enumerate() {
                    if i.opcode == op && want.map_or(true, |w| i.int() == w) {
                        println!("script {} pc {pc}: {} {}", s.id, args[1], instr_str(i));
                    }
                }
            }
        }
        "--find-comp" => {
            // scripts with an iconst naming a component: --find-comp <group>[:<file>]
            let (g, f) = match args[1].split_once(':') {
                Some((g, f)) => (g.parse::<u32>().unwrap(), f.parse::<u32>().ok()),
                None => (args[1].parse::<u32>().unwrap(), None),
            };
            for s in load_all() {
                let hits: Vec<String> = s
                    .instructions
                    .iter()
                    .filter(|i| i.opcode == 0 && i.int() >= 65536 && (i.int() as u32) >> 16 == g && f.map_or(true, |f| i.int() & 0xffff == f as i32))
                    .map(|i| format!("{}:{}", g, i.int() & 0xffff))
                    .collect();
                if !hits.is_empty() {
                    println!("script {}: {}", s.id, hits.join(" "));
                }
            }
        }
        "--enum" => {
            for a in &args[1..] {
                let id: u32 = a.parse().unwrap();
                let e = clientscript::load_enum(&cache, id)?;
                println!(
                    "enum {id}: key {} ({}) -> val {} ({}), default int {} str {:?}, {} entries",
                    e.key_type as char, e.key_type, e.val_type as char, e.val_type, e.default_int, e.default_str, e.entries.len()
                );
                for (k, v) in &e.entries {
                    let extra = match v {
                        Operand::Int(i) => comp_name(*i, &groups).map(|c| format!("  ; {c}")).unwrap_or_default(),
                        _ => String::new(),
                    };
                    let kc = if e.key_type == b'I' { comp_name(*k, &groups).map(|c| format!("[{c}] ")).unwrap_or_default() } else { String::new() };
                    println!("  {kc}{k} -> {}{extra}", instr_str(&Instr { opcode: 0, operand: v.clone() }));
                }
            }
        }
        "--item" => {
            let items = Configs::new(&cache)?.items()?;
            for a in &args[1..] {
                let id: u32 = a.parse().unwrap();
                let Some(it) = items.get(&id) else { println!("item {id}: none"); continue };
                println!("item {id} {:?}", it.name);
                for (k, v) in &it.params {
                    let extra = match v {
                        item::Param::Int(i) => comp_name(*i, &groups).map(|c| format!("  ; {c}")).unwrap_or_default(),
                        _ => String::new(),
                    };
                    println!("  param {k} = {v:?}{extra}");
                }
            }
        }
        "--census" => {
            let mut c: BTreeMap<u16, usize> = BTreeMap::new();
            for s in load_all() {
                for i in &s.instructions {
                    if clientscript::opcode_name(i.opcode).is_none() {
                        *c.entry(i.opcode).or_default() += 1;
                    }
                }
            }
            println!("{} unnamed opcodes: {c:?}", c.len());
        }
        _ => {
            let arg_counts = |id: u32| clientscript::load_script_with(&cache, &index, id).ok().map(|s| (s.int_args, s.str_args));
            for a in &args {
                let id: u32 = a.parse().expect("script id");
                match clientscript::load_script_with(&cache, &index, id) {
                    Ok(s) => disasm(&s, &groups, &arg_counts),
                    Err(e) => eprintln!("script {id}: {e}"),
                }
                println!();
            }
        }
    }
    Ok(())
}
