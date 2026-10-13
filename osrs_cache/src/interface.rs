//! Interface components (archive 3): one group per interface, one file per component ("widget").
//!
//! Files starting with 0xff are the IF3 format; anything else is the legacy IF1 format. Layout
//! follows RuneLite's `InterfaceLoader`; see the README for how this revision differs (if at all).
//! [`Component::decode`] is strict: it fails unless it consumes exactly all the bytes.

use crate::buf::Buf;
use crate::{idx, Cache, Error, Result};

/// One argument of a listener (script call): an int or a string. Event listeners are
/// `[script id, args...]` where the first entry is normally the script id.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Int(i32),
    Str(String),
}

/// An event listener: the argument list handed to a client script. `args[0]` is the script id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listener {
    pub args: Vec<Arg>,
}

impl Listener {
    pub fn script_id(&self) -> Option<i32> {
        match self.args.first() {
            Some(Arg::Int(i)) => Some(*i),
            _ => None,
        }
    }
}

/// IF3 listener slots, in file order.
pub const LISTENER_NAMES: [&str; 18] = [
    "onLoad", "onMouseOver", "onMouseLeave", "onTargetLeave", "onTargetEnter", "onVarTransmit", "onInvTransmit",
    "onStatTransmit", "onTimer", "onOp", "onMouseRepeat", "onClick", "onClickRepeat", "onRelease", "onHold",
    "onDrag", "onDragComplete", "onScrollWheel",
];

#[derive(Debug, Clone, Default)]
pub struct Component {
    /// `group << 16 | file`
    pub id: u32,
    pub group: u32,
    pub file: u32,
    pub is_if3: bool,
    /// 0 layer, 3 rectangle, 4 text, 5 sprite, 6 model, 9 line (IF1 adds 1, 2 inventory, 7 text inventory, 8 tooltip).
    pub kind: u8,
    pub content_type: u16,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    /// Signed for lines (type 9: end point offset).
    pub height: i32,
    /// 0 absolute, 1 minus parent, 2 fraction (/ 1<<14), 3 per-three-quarters, 4 aspect (IF3 modes).
    pub width_mode: i8,
    pub height_mode: i8,
    /// x: 0 left, 1 centre, 2 right, 3 proportional... y: 0 top, 1 centre, 2 bottom.
    pub x_mode: i8,
    pub y_mode: i8,
    /// Full id of the parent layer, if any.
    pub parent: Option<u32>,
    pub hidden: bool,

    // layer
    pub scroll_width: u16,
    pub scroll_height: u16,
    pub no_click_through: bool,

    // rectangle / line / text colour (0xRRGGBB; alpha in the top byte for some)
    pub colour: u32,
    pub filled: bool,
    /// Rectangle and sprite transparency byte (0 = opaque, 255 = invisible).
    pub transparency: u8,

    // sprite
    pub sprite_id: i32,
    pub texture_id: u16,
    pub tiling: bool,
    pub border: u8,
    pub shadow_colour: u32,
    pub flip_v: bool,
    pub flip_h: bool,

    // text
    pub font_id: i32,
    pub text: String,
    pub line_height: u8,
    pub x_align: u8,
    pub y_align: u8,
    pub shadowed: bool,

    // model
    pub model_id: i32,
    pub model_offset_x: i16,
    pub model_offset_y: i16,
    pub model_rot_x: u16,
    pub model_rot_z: u16,
    pub model_rot_y: u16,
    pub model_zoom: u16,
    pub model_anim: i32,
    pub model_orthogonal: bool,
    pub model_unknown: u16,
    pub model_height_override: u16,
    pub model_extra: u16,

    // line
    pub line_width: u8,
    pub line_direction: bool,

    // interaction
    pub click_mask: u32,
    pub name: String,
    pub ops: Vec<String>,
    pub drag_dead_zone: u8,
    pub drag_dead_time: u8,
    pub drag_render_behaviour: bool,
    pub target_verb: String,
    /// One entry per [`LISTENER_NAMES`] slot; `None` where absent.
    pub listeners: Vec<Option<Listener>>,
    pub var_triggers: Vec<i32>,
    pub inv_triggers: Vec<i32>,
    pub stat_triggers: Vec<i32>,

    // IF1 only
    pub if1: Option<If1>,
}

/// Extra IF1 (legacy) fields.
#[derive(Debug, Clone, Default)]
pub struct If1 {
    pub menu_type: u8,
    pub hovered_sibling: i32,
    pub alt_operators: Vec<(u8, u16)>,
    pub scripts: Vec<Vec<u16>>,
    pub alt_text: String,
    pub alt_colour: u32,
    pub hover_colour: u32,
    pub alt_hover_colour: u32,
    pub alt_sprite: i32,
    pub alt_model: i32,
    pub alt_anim: i32,
    pub x_pitch: i32,
    pub y_pitch: i32,
    pub slot_sprites: Vec<(i16, i16, i32)>,
    pub config_actions: Vec<String>,
    pub spell_name: String,
    pub tooltip: String,
}

impl Component {
    pub fn has_listener(&self) -> bool {
        self.listeners.iter().any(|l| l.is_some())
    }

    pub fn listener(&self, name: &str) -> Option<&Listener> {
        let i = LISTENER_NAMES.iter().position(|n| *n == name)?;
        self.listeners.get(i)?.as_ref()
    }

    /// Script ids of every listener present, as `(slot name, id)`.
    pub fn script_ids(&self) -> Vec<(&'static str, i32)> {
        self.listeners
            .iter()
            .enumerate()
            .filter_map(|(i, l)| Some((LISTENER_NAMES[i], l.as_ref()?.script_id()?)))
            .collect()
    }

    /// Decode one component file. Errors unless every byte is consumed.
    pub fn decode(group: u32, file: u32, data: &[u8]) -> Result<Component> {
        let mut c = Component { id: group << 16 | file, group, file, ..Default::default() };
        let mut b = Buf::new(data);
        if data.first() == Some(&0xff) {
            c.is_if3 = true;
            decode_if3(&mut c, &mut b);
        } else {
            decode_if1(&mut c, &mut b);
        }
        if b.pos != data.len() {
            return Err(Error::Format(format!(
                "interface {group}/{file} (type {}, if3={}): consumed {} of {} bytes",
                c.kind,
                c.is_if3,
                b.pos,
                data.len()
            )));
        }
        Ok(c)
    }
}

fn parent_id(group: u32, v: u16) -> Option<u32> {
    (v != 0xffff).then_some(group << 16 | v as u32)
}

fn listener(b: &mut Buf) -> Option<Listener> {
    let n = b.u8();
    if n == 0 {
        return None;
    }
    let mut args = Vec::with_capacity(n as usize);
    for _ in 0..n {
        match b.u8() {
            0 => args.push(Arg::Int(b.i32())),
            1 => args.push(Arg::Str(b.string())),
            _ => {} // unknown kind: RuneLite leaves a null; no payload
        }
    }
    Some(Listener { args })
}

fn triggers(b: &mut Buf) -> Vec<i32> {
    let n = b.u8();
    (0..n).map(|_| b.i32()).collect()
}

fn decode_if3(c: &mut Component, b: &mut Buf) {
    b.u8(); // 0xff
    c.kind = b.u8();
    c.content_type = b.u16();
    c.x = b.i16();
    c.y = b.i16();
    c.width = b.u16();
    c.height = if c.kind == 9 { b.i16() as i32 } else { b.u16() as i32 };
    c.width_mode = b.i8();
    c.height_mode = b.i8();
    c.x_mode = b.i8();
    c.y_mode = b.i8();
    c.parent = parent_id(c.group, b.u16());
    c.hidden = b.u8() == 1;
    match c.kind {
        0 => {
            c.scroll_width = b.u16();
            c.scroll_height = b.u16();
            c.no_click_through = b.u8() == 1;
        }
        5 => {
            c.sprite_id = b.i32();
            c.texture_id = b.u16();
            c.tiling = b.u8() == 1;
            c.transparency = b.u8();
            c.border = b.u8();
            c.shadow_colour = b.u32();
            c.flip_v = b.u8() == 1;
            c.flip_h = b.u8() == 1;
        }
        6 => {
            c.model_id = b.i32();
            c.model_offset_x = b.i16();
            c.model_offset_y = b.i16();
            c.model_rot_x = b.u16();
            c.model_rot_z = b.u16();
            c.model_rot_y = b.u16();
            c.model_zoom = b.u16();
            let a = b.u16();
            c.model_anim = if a == 0xffff { -1 } else { a as i32 };
            c.model_orthogonal = b.u8() == 1;
            c.model_unknown = b.u16();
            // RuneLite reads one short per non-zero mode; here any non-zero mode brings two (every
            // (1,0), (1,1) and (2,1) model in the cache fits; (0,1) never occurs)
            if c.width_mode != 0 || c.height_mode != 0 {
                c.model_height_override = b.u16();
                c.model_extra = b.u16();
            }
        }
        4 => {
            let f = b.u16();
            c.font_id = if f == 0xffff { -1 } else { f as i32 };
            c.text = b.string();
            c.line_height = b.u8();
            c.x_align = b.u8();
            c.y_align = b.u8();
            c.shadowed = b.u8() == 1;
            c.colour = b.u32();
        }
        3 => {
            c.colour = b.u32();
            c.filled = b.u8() == 1;
            c.transparency = b.u8();
        }
        9 => {
            c.line_width = b.u8();
            c.colour = b.u32();
            c.line_direction = b.u8() == 1;
        }
        _ => {}
    }
    c.click_mask = b.u24();
    c.name = b.string();
    let n = b.u8();
    c.ops = (0..n).map(|_| b.string()).collect();
    c.drag_dead_zone = b.u8();
    c.drag_dead_time = b.u8();
    c.drag_render_behaviour = b.u8() == 1;
    c.target_verb = b.string();
    c.listeners = (0..LISTENER_NAMES.len()).map(|_| listener(b)).collect();
    c.var_triggers = triggers(b);
    c.inv_triggers = triggers(b);
    c.stat_triggers = triggers(b);
}

fn opt16(v: u16) -> i32 {
    if v == 0xffff {
        -1
    } else {
        v as i32
    }
}

fn decode_if1(c: &mut Component, b: &mut Buf) {
    let mut f = If1::default();
    c.kind = b.u8();
    f.menu_type = b.u8();
    c.content_type = b.u16();
    c.x = b.i16();
    c.y = b.i16();
    c.width = b.u16();
    c.height = b.u16() as i32;
    c.transparency = b.u8();
    c.parent = parent_id(c.group, b.u16());
    f.hovered_sibling = opt16(b.u16());
    let n = b.u8();
    f.alt_operators = (0..n).map(|_| (b.u8(), b.u16())).collect();
    let n = b.u8();
    for _ in 0..n {
        let len = b.u16();
        f.scripts.push((0..len).map(|_| b.u16()).collect());
    }
    let kind = c.kind;
    if kind == 0 {
        c.scroll_height = b.u16();
        c.hidden = b.u8() == 1;
    }
    if kind == 1 {
        b.u16();
        b.u8();
    }
    if kind == 2 {
        for bit in [1u32 << 28, 1 << 30, 1 << 31, 1 << 29] {
            if b.u8() == 1 {
                c.click_mask |= bit;
            }
        }
        f.x_pitch = b.u8() as i32;
        f.y_pitch = b.u8() as i32;
        for _ in 0..20 {
            if b.u8() == 1 {
                f.slot_sprites.push((b.i16(), b.i16(), b.i32()));
            } else {
                f.slot_sprites.push((0, 0, -1));
            }
        }
        for i in 0..5 {
            let s = b.string();
            if !s.is_empty() {
                c.click_mask |= 1 << (i + 23);
            }
            f.config_actions.push(s);
        }
    }
    if kind == 3 {
        c.filled = b.u8() == 1;
    }
    if kind == 4 || kind == 1 {
        c.x_align = b.u8();
        c.y_align = b.u8();
        c.line_height = b.u8();
        c.font_id = opt16(b.u16());
        c.shadowed = b.u8() == 1;
    }
    if kind == 4 {
        c.text = b.string();
        f.alt_text = b.string();
    }
    if matches!(kind, 1 | 3 | 4) {
        c.colour = b.u32();
    }
    if kind == 3 || kind == 4 {
        f.alt_colour = b.u32();
        f.hover_colour = b.u32();
        f.alt_hover_colour = b.u32();
    }
    if kind == 5 {
        c.sprite_id = b.i32();
        f.alt_sprite = b.i32();
    }
    if kind == 6 {
        c.model_id = b.i32();
        f.alt_model = b.i32();
        c.model_anim = opt16(b.u16());
        f.alt_anim = opt16(b.u16());
        c.model_zoom = b.u16();
        c.model_rot_x = b.u16();
        c.model_rot_z = b.u16();
    }
    if kind == 7 {
        c.x_align = b.u8();
        c.font_id = opt16(b.u16());
        c.shadowed = b.u8() == 1;
        c.colour = b.u32();
        f.x_pitch = b.i16() as i32;
        f.y_pitch = b.i16() as i32;
        if b.u8() == 1 {
            c.click_mask |= 1 << 30;
        }
        for i in 0..5 {
            let s = b.string();
            if !s.is_empty() {
                c.click_mask |= 1 << (i + 23);
            }
            f.config_actions.push(s);
        }
    }
    if kind == 8 {
        c.text = b.string();
    }
    if f.menu_type == 2 || kind == 2 {
        c.target_verb = b.string();
        f.spell_name = b.string();
        c.click_mask |= ((b.u16() & 63) as u32) << 11;
    }
    if matches!(f.menu_type, 1 | 4 | 5 | 6) {
        f.tooltip = b.string();
    }
    c.if1 = Some(f);
}

/// Every interface group id in the cache.
pub fn group_ids(cache: &Cache) -> Result<Vec<u32>> {
    Ok(cache.index(idx::INTERFACES)?.groups.keys().copied().collect())
}

/// Decode all components of one interface group, in file order.
pub fn load_group(cache: &Cache, group: u32) -> Result<Vec<Component>> {
    let index = cache.index(idx::INTERFACES)?;
    load_group_with(cache, &index, group)
}

/// Like [`load_group`] with the reference table already read (for sweeps).
pub fn load_group_with(cache: &Cache, index: &crate::Index, group: u32) -> Result<Vec<Component>> {
    cache.read_files(index, group)?.into_iter().map(|(file, d)| Component::decode(group, file, &d)).collect()
}
