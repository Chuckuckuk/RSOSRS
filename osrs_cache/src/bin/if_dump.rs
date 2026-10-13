//! Prints an interface group as a component tree: if_dump <group> | if_dump --list | if_dump --sprites <group>
use osrs_cache::interface::{self, Arg, Component};
use osrs_cache::*;
use std::collections::HashMap;

fn q(s: &str) -> String {
    format!("{s:?}")
}

fn line(c: &Component) -> String {
    let kind = match c.kind {
        0 => "layer",
        1 => "if1-text?",
        2 => "inv",
        3 => "rect",
        4 => "text",
        5 => "sprite",
        6 => "model",
        7 => "text-inv",
        8 => "tooltip",
        9 => "line",
        _ => "?",
    };
    let mut s = format!(
        "{} {kind}{} {},{} {}x{} m[w{} h{} x{} y{}]",
        c.file,
        if c.is_if3 { "" } else { " (if1)" },
        c.x,
        c.y,
        c.width,
        c.height,
        c.width_mode,
        c.height_mode,
        c.x_mode,
        c.y_mode
    );
    if c.hidden {
        s += " HIDDEN";
    }
    if c.content_type != 0 {
        s += &format!(" content={}", c.content_type);
    }
    match c.kind {
        0 => {
            if c.scroll_width != 0 || c.scroll_height != 0 {
                s += &format!(" scroll={}x{}", c.scroll_width, c.scroll_height);
            }
            if c.no_click_through {
                s += " noClickThrough";
            }
        }
        3 => s += &format!(" colour={:06x} filled={} transp={}", c.colour, c.filled, c.transparency),
        4 => s += &format!(" font={} colour={:06x} text={} align={},{} lh={}{}", c.font_id, c.colour, q(&c.text), c.x_align, c.y_align, c.line_height, if c.shadowed { " shadow" } else { "" }),
        5 => {
            s += &format!(" sprite={}", c.sprite_id);
            if c.tiling {
                s += " tiled";
            }
            if c.transparency != 0 {
                s += &format!(" transp={}", c.transparency);
            }
            if c.border != 0 {
                s += &format!(" border={}", c.border);
            }
            if c.flip_v || c.flip_h {
                s += &format!(" flip={}{}", if c.flip_h { "h" } else { "" }, if c.flip_v { "v" } else { "" });
            }
        }
        6 => s += &format!(" model={} zoom={} rot={},{},{} off={},{} anim={}", c.model_id, c.model_zoom, c.model_rot_x, c.model_rot_y, c.model_rot_z, c.model_offset_x, c.model_offset_y, c.model_anim),
        9 => s += &format!(" colour={:06x} lw={}", c.colour, c.line_width),
        _ => {}
    }
    if !c.name.is_empty() {
        s += &format!(" name={}", q(&c.name));
    }
    if c.ops.iter().any(|o| !o.is_empty()) {
        s += &format!(" ops={:?}", c.ops);
    }
    if !c.target_verb.is_empty() {
        s += &format!(" target={}", q(&c.target_verb));
    }
    if c.click_mask != 0 {
        s += &format!(" mask={:x}", c.click_mask);
    }
    for (i, l) in c.listeners.iter().enumerate() {
        if let Some(l) = l {
            let args: Vec<String> = l
                .args
                .iter()
                .map(|a| match a {
                    Arg::Int(v) => v.to_string(),
                    Arg::Str(t) => q(t),
                })
                .collect();
            s += &format!(" {}=[{}]", interface::LISTENER_NAMES[i], args.join(","));
        }
    }
    for (n, t) in [("varTrig", &c.var_triggers), ("invTrig", &c.inv_triggers), ("statTrig", &c.stat_triggers)] {
        if !t.is_empty() {
            s += &format!(" {n}={t:?}");
        }
    }
    if let Some(f) = &c.if1 {
        s += &format!(" menu={}", f.menu_type);
        if !f.tooltip.is_empty() {
            s += &format!(" tip={}", q(&f.tooltip));
        }
        if c.kind == 2 {
            s += &format!(" pitch={},{}", f.x_pitch, f.y_pitch);
        }
    }
    s
}

fn walk(by_parent: &HashMap<Option<u32>, Vec<&Component>>, key: Option<u32>, depth: usize) {
    if let Some(kids) = by_parent.get(&key) {
        for c in kids {
            println!("{}{}", "  ".repeat(depth), line(c));
            walk(by_parent, Some(c.id), depth + 1);
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cache = Cache::open_default()?;
    if args.first().map(|s| s.as_str()) == Some("--list") {
        let ids = interface::group_ids(&cache)?;
        println!("{} groups: {ids:?}", ids.len());
        return Ok(());
    }
    let sprites = args.first().map(|s| s.as_str()) == Some("--sprites");
    let group: u32 = args.get(sprites as usize).and_then(|g| g.parse().ok()).unwrap_or(161);
    let comps = interface::load_group(&cache, group)?;
    if sprites {
        let mut ids: Vec<i32> = comps.iter().flat_map(|c| [c.sprite_id].into_iter().chain(c.if1.iter().map(|f| f.alt_sprite))).filter(|&s| s >= 0 && comps.iter().any(|c| c.kind == 5)).collect();
        ids.sort();
        ids.dedup();
        println!("{ids:?}");
        return Ok(());
    }
    let present: std::collections::HashSet<u32> = comps.iter().map(|c| c.id).collect();
    let mut by_parent: HashMap<Option<u32>, Vec<&Component>> = HashMap::new();
    for c in &comps {
        let p = c.parent.filter(|p| present.contains(p));
        by_parent.entry(p).or_default().push(c);
    }
    println!("group {group}: {} components", comps.len());
    walk(&by_parent, None, 0);
    Ok(())
}
