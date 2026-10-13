//! Compose a widget tree into one canvas and a hit list.

use super::layout::{resolve_raw, RawLayout, Rect};
use super::raster::{decorate, Canvas};
use super::text::draw_text;
use crate::font::Font;
use crate::interface::Component;
use crate::textures::Texture;
use std::collections::HashMap;

/// Script-driven changes to a static component (what cc_set* / if_set* would do).
#[derive(Default, Clone, Debug)]
pub struct Override {
    pub hidden: Option<bool>,
    pub sprite: Option<i32>,
    pub text: Option<String>,
    pub colour: Option<u32>,
    pub transparency: Option<u8>,
    pub pos: Option<(i32, i32)>,
    pub size: Option<(i32, i32)>,
    /// Cache object id and quantity: draws the item icon (and stack text) instead of a sprite.
    pub item: Option<(u32, u32)>,
}

#[derive(Default)]
pub struct UiState {
    pub groups: HashMap<u32, Vec<Component>>,
    pub top: u32,
    /// component id -> group mounted into it
    pub mounts: HashMap<u32, u32>,
    pub overrides: HashMap<u32, Override>,
    /// parent component id -> script-created children (positions relative to the parent)
    pub created: HashMap<u32, Vec<Component>>,
    /// (parent component id, index in `created[parent]`) -> script changes to that created child
    /// (the item it shows, its position, text...).
    pub created_overrides: HashMap<(u32, usize), Override>,
}

pub trait Assets {
    fn sprite(&mut self, id: i32) -> Option<&Texture>;
    fn font(&mut self, id: i32) -> Option<&Font>;
    fn item(&mut self, obj: u32, qty: u32) -> Option<&Texture>;
    /// Content-type components (minimap, compass, ...): draw into `canvas` at `rect`. `sprite` is the
    /// component's own sprite (the minimap's mask shape).
    fn content(&mut self, _content_type: u16, _sprite: i32, _rect: Rect, _canvas: &mut Canvas) {}
}

#[derive(Debug, Clone)]
pub struct Hit {
    /// Component id (for a created component: its parent's id).
    pub id: u32,
    pub created: Option<usize>,
    /// Canvas coordinates, clipped by the enclosing layers.
    pub rect: Rect,
    pub kind: u8,
    pub content_type: u16,
    pub ops: Vec<String>,
    pub name: String,
    pub no_click_through: bool,
    /// A fully opaque filled rectangle (a background that swallows clicks).
    pub opaque: bool,
    /// The component has a target verb (a spell icon: "Cast"), so it takes clicks even with no ops.
    pub targetable: bool,
}

impl Hit {
    pub fn interactive(&self) -> bool {
        self.ops.iter().any(|o| !o.is_empty()) || self.no_click_through || self.opaque || self.targetable
    }
}

/// The topmost interactive hit under (x, y).
pub fn hit_at(hits: &[Hit], x: i32, y: i32) -> Option<&Hit> {
    hits.iter().rev().find(|h| h.interactive() && h.rect.contains(x, y))
}

struct GroupIndex {
    roots: Vec<usize>,
    children: HashMap<u32, Vec<usize>>,
}

struct Ctx<'a> {
    state: &'a UiState,
    assets: &'a mut dyn Assets,
    canvas: Canvas,
    hits: Vec<Hit>,
    index: HashMap<u32, GroupIndex>,
    depth: u32,
}

pub fn draw(state: &UiState, assets: &mut dyn Assets, width: i32, height: i32) -> (Canvas, Vec<Hit>) {
    let mut index = HashMap::new();
    for (g, comps) in &state.groups {
        let mut gi = GroupIndex { roots: Vec::new(), children: HashMap::new() };
        for (i, c) in comps.iter().enumerate() {
            match c.parent {
                None => gi.roots.push(i),
                Some(p) => gi.children.entry(p).or_default().push(i),
            }
        }
        index.insert(*g, gi);
    }
    let mut cx = Ctx { state, assets, canvas: Canvas::new(width, height), hits: Vec::new(), index, depth: 0 };
    cx.draw_group(state.top, Rect::new(0, 0, width, height));
    (cx.canvas, cx.hits)
}

impl Ctx<'_> {
    fn draw_group(&mut self, group: u32, host: Rect) {
        if self.depth > 8 {
            return;
        }
        let Some(comps) = self.state.groups.get(&group) else { return };
        let roots = self.index.get(&group).map(|g| g.roots.clone()).unwrap_or_default();
        self.depth += 1;
        for r in roots {
            self.draw_component(&comps[r], group, host, None);
        }
        self.depth -= 1;
    }

    /// `host` is the parent's absolute rect; `created` is the index when this is a script-made component.
    fn draw_component(&mut self, c: &Component, group: u32, host: Rect, created: Option<usize>) {
        let ov = match created {
            Some(i) => self.state.created_overrides.get(&(c.id, i)).cloned().unwrap_or_default(),
            None => self.state.overrides.get(&c.id).cloned().unwrap_or_default(),
        };
        if ov.hidden.unwrap_or(c.hidden) {
            return;
        }
        let mut raw = RawLayout::of(c);
        if let Some((x, y)) = ov.pos {
            raw.x = x;
            raw.y = y;
        }
        if let Some((w, h)) = ov.size {
            raw.width = w;
            raw.height = h;
        }
        let rel = resolve_raw(&raw, c.is_if3, host.w, host.h);
        let rect = rel.translate(host.x, host.y);
        let visible = rect.intersect(&self.canvas.clip());

        let colour = ov.colour.unwrap_or(c.colour) & 0xff_ffff;
        let transparency = ov.transparency.unwrap_or(c.transparency);
        let alpha = 255 - transparency;
        let mut opaque = false;

        if c.content_type == 1337 {
            // the 3D viewport shows through
        } else if c.content_type != 0 && c.kind != 0 && c.kind != 4 {
            self.assets.content(c.content_type, ov.sprite.unwrap_or(c.sprite_id), rect, &mut self.canvas);
        } else {
            match c.kind {
                3 => {
                    if c.filled {
                        self.canvas.fill_rect(rect, colour, alpha);
                        opaque = alpha == 255;
                    } else {
                        self.canvas.outline_rect(rect, colour, alpha);
                    }
                }
                4 => {
                    let text = ov.text.as_deref().unwrap_or(&c.text);
                    if !text.is_empty() {
                        if let Some(font) = self.assets.font(c.font_id) {
                            draw_text(&mut self.canvas, font, text, rect, colour, c.x_align, c.y_align, c.line_height, c.shadowed);
                        }
                    }
                }
                5 => self.draw_sprite(c, &ov, rect, alpha),
                9 => {
                    let (x0, y0, x1, y1) = if c.line_direction {
                        (rect.x + rect.w, rect.y, rect.x, rect.y + rect.h)
                    } else {
                        (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h)
                    };
                    self.canvas.line(x0, y0, x1, y1, colour, alpha, c.line_width as i32);
                }
                _ => {}
            }
        }

        if !visible.is_empty() {
            self.hits.push(Hit {
                id: c.id,
                created,
                rect: visible,
                kind: c.kind,
                content_type: c.content_type,
                ops: c.ops.clone(),
                name: c.name.clone(),
                no_click_through: c.no_click_through,
                opaque,
                targetable: !c.target_verb.is_empty(),
            });
        }

        if c.kind == 0 && c.content_type != 1337 {
            self.canvas.push_clip(rect);
            if created.is_none() {
                let kids = self.index.get(&group).and_then(|g| g.children.get(&c.id)).cloned().unwrap_or_default();
                if let Some(comps) = self.state.groups.get(&group) {
                    for k in kids {
                        self.draw_component(&comps[k], group, rect, None);
                    }
                }
                if let Some(extra) = self.state.created.get(&c.id) {
                    for (i, k) in extra.iter().enumerate() {
                        // created children carry the parent's id for hits
                        let mut k2 = k.clone();
                        k2.id = c.id;
                        self.draw_component(&k2, group, rect, Some(i));
                    }
                }
                if let Some(&g) = self.state.mounts.get(&c.id) {
                    self.draw_group(g, rect);
                }
            }
            self.canvas.pop_clip();
        } else if created.is_none() {
            if let Some(&g) = self.state.mounts.get(&c.id) {
                self.canvas.push_clip(rect);
                self.draw_group(g, rect);
                self.canvas.pop_clip();
            }
        }
    }

    fn draw_sprite(&mut self, c: &Component, ov: &Override, rect: Rect, alpha: u8) {
        if let Some((obj, qty)) = ov.item {
            let tex = self.assets.item(obj, qty).cloned();
            if let Some(t) = tex {
                let x = rect.x + (rect.w - t.width as i32) / 2;
                let y = rect.y + (rect.h - t.height as i32) / 2;
                self.canvas.blit(&t, x, y, alpha, false, false);
            }
            if qty > 1 {
                let (txt, col) = if qty < 100_000 {
                    (qty.to_string(), 0xFFFF00)
                } else if qty < 10_000_000 {
                    (format!("{}K", qty / 1000), 0xFFFFFF)
                } else {
                    (format!("{}M", qty / 1_000_000), 0x00FF80)
                };
                if let Some(font) = self.assets.font(494) {
                    draw_text(&mut self.canvas, font, &txt, Rect::new(rect.x, rect.y, 40, 12), col, 0, 0, 0, true);
                }
            }
            return;
        }
        let id = ov.sprite.unwrap_or(c.sprite_id);
        if id < 0 {
            return;
        }
        let Some(tex) = self.assets.sprite(id) else { return };
        let decorated;
        let tex = if c.border != 0 || c.shadow_colour & 0xff_ffff != 0 {
            decorated = decorate(tex, c.border, c.shadow_colour & 0xff_ffff);
            &decorated
        } else {
            tex
        };
        if c.tiling {
            self.canvas.blit_tiled(tex, rect, alpha);
        } else {
            self.canvas.blit_scaled(tex, rect, alpha, c.flip_h, c.flip_v);
        }
    }
}
