//! What the game's scripts put into the interface, ported pane by pane.
//!
//! The cache holds the interface's static widgets; the game's client scripts (CS2) fill in the rest
//! at run time: the inventory's 28 slots, the worn items, the prayer cells, the orb values, the
//! chat lines. Each file here is the port of one family of those scripts (the script numbers are in
//! the file headers) and writes its results into the [`UiState`] the software renderer draws:
//! script-created children (`UiState::created`) and changes to static widgets
//! (`UiState::overrides`).
//!
//! [`sync`] runs every pane's port from the latest server snapshot. The caller draws again only when
//! [`PaneInput::fingerprint`] changed.

pub mod chat;
pub mod combat;
pub mod emotes;
pub mod equipment;
pub mod inventory;
pub mod logout;
pub mod magic;
pub mod minimap;
pub mod music;
pub mod orbs;
pub mod prayer;
pub mod skills;
pub mod tabs;

use super::interface::ItemIcons;
use osrs_cache::interface::Component;
use osrs_cache::textures::Texture;
use osrs_cache::ui::{Assets, CacheAssets, Canvas, Layout, Rect, TopLevel, UiState};
use osrs_core::sim::PlayerSnapshot;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// A script-created sprite child at (`x`, `y`) of size `w` x `h`.
pub(crate) fn sprite_child(x: i32, y: i32, w: i32, h: i32, sprite: i32) -> Component {
    Component {
        is_if3: true,
        kind: 5,
        x: x as i16,
        y: y as i16,
        width: w as u16,
        height: h,
        sprite_id: sprite,
        ..Default::default()
    }
}

/// A script-created one-line text child at (`x`, `y`) of size `w` x `h`: centred, shadowed.
pub(crate) fn text_child(x: i32, y: i32, w: i32, h: i32, font: i32, colour: u32, text: &str) -> Component {
    Component {
        is_if3: true,
        kind: 4,
        x: x as i16,
        y: y as i16,
        width: w as u16,
        height: h,
        font_id: font,
        colour,
        text: text.to_owned(),
        x_align: 1,
        y_align: 0,
        shadowed: true,
        ..Default::default()
    }
}

/// The scroll bar script 31 builds in a layer `bar_height` tall for a pane that shows `view_height`
/// of `content_height` pixels, scrolled to the top: a track (sprite 792) between two arrows (773
/// up, 788 down, 16 px each) and the thumb (cap 789, body 790, cap 791) as tall as the visible
/// share of the track, at least 10 px, just under the up arrow.
pub(crate) fn scrollbar(bar_height: i32, view_height: i32, content_height: i32) -> Vec<Component> {
    let thumb = if content_height > 0 { (bar_height - 32) * view_height / content_height } else { bar_height - 32 }.max(10);
    // (y, height, y mode, height mode, sprite): all as wide as the bar, centred
    let part = |x_y: (i32, i32), size_h: i32, modes: (i8, i8), sprite: i32| Component {
        is_if3: true,
        kind: 5,
        x: x_y.0 as i16,
        y: x_y.1 as i16,
        width: 0,
        height: size_h,
        width_mode: 1,
        height_mode: modes.1,
        x_mode: 1,
        y_mode: modes.0,
        sprite_id: sprite,
        ..Default::default()
    };
    vec![
        part((0, 0), 32, (1, 1), 792),
        part((0, 16), thumb, (0, 0), 790),
        part((0, 16), 5, (0, 0), 789),
        part((0, 16 + thumb - 5), 5, (0, 0), 791),
        part((0, 0), 16, (0, 0), 773),
        part((0, 0), 16, (2, 0), 788),
    ]
}

/// Component id `file` of `group`.
pub(crate) const fn comp(group: u32, file: u32) -> u32 {
    group << 16 | file
}

/// What the panes are drawn from.
pub struct PaneInput<'a> {
    pub player: Option<&'a PlayerSnapshot>,
    /// NPC dots for the minimap: world position in tiles (the middle of the footprint).
    pub npc_dots: &'a [(f32, f32)],
    pub icons: &'a ItemIcons,
    /// Each book prayer's level (`None`: the server does not have it).
    pub levels: &'a [Option<i32>],
    /// Chat messages, oldest first.
    pub chat: &'a [String],
    pub name: &'a str,
    pub top: &'a TopLevel,
    pub layout: Layout,
    pub selected: usize,
    pub open: bool,
    /// The inventory slot picked with "Use", waiting for a second item.
    pub use_selected: Option<usize>,
    /// Where the minimap is centred (world tiles) and how the camera is turned.
    pub map: minimap::MapParams,
}

impl PaneInput<'_> {
    /// A number that changes whenever anything the panes draw changes.
    pub fn fingerprint(&self) -> u64 {
        let mut h = DefaultHasher::new();
        (self.selected, self.open, self.layout as u8, self.name, self.chat.len(), self.use_selected).hash(&mut h);
        self.chat.last().hash(&mut h);
        self.map.quantised().hash(&mut h);
        for (x, z) in self.npc_dots {
            ((*x * 4.0) as i32, (*z * 4.0) as i32).hash(&mut h);
        }
        if let Some(p) = self.player {
            (p.health, p.max_health, p.prayer, p.max_prayer, p.run_energy, p.is_running).hash(&mut h);
            (p.special_energy, p.special_enabled).hash(&mut h);
            for stack in &p.inventory {
                stack.as_ref().map(|s| (&s.name, s.quantity)).hash(&mut h);
            }
            for (slot, item) in &p.equipped {
                (*slot as u8, &item.name).hash(&mut h);
            }
            p.active_prayers.hash(&mut h);
            p.spell.hash(&mut h);
            (p.poisoned, p.venomed, p.combat_level, &p.weapon_category, &p.stances, p.stance_index, p.special_cost).hash(&mut h);
            for skill in &p.skills {
                (skill.skill.index(), skill.base, skill.current).hash(&mut h);
            }
        }
        h.finish()
    }
}

/// Runs every pane's port.
pub fn sync(state: &mut UiState, input: &PaneInput, assets: &mut dyn Assets) {
    tabs::sync(state, input);
    inventory::sync(state, input);
    equipment::sync(state, input);
    prayer::sync(state, input, assets);
    orbs::sync(state, input);
    skills::sync(state, input);
    combat::sync(state, input, assets);
    match input.selected {
        super::interface::MAGIC_TAB => magic::sync(state, input, assets),
        10 => logout::sync(state, input),
        12 => emotes::sync(state, input),
        13 => music::sync(state, input),
        _ => {}
    }
    chat::sync(state, input, assets);
}

/// The renderer's assets plus what only this client can draw: the minimap and the compass.
pub struct ClientAssets {
    pub inner: CacheAssets<'static>,
    pub minimap: minimap::MinimapView,
    masks: HashMap<i32, Option<Texture>>,
}

impl ClientAssets {
    pub fn new(inner: CacheAssets<'static>, minimap: minimap::MinimapView) -> ClientAssets {
        ClientAssets { inner, minimap, masks: HashMap::new() }
    }

    fn mask(&mut self, sprite: i32) -> Option<Texture> {
        if !self.masks.contains_key(&sprite) {
            let tex = self.inner.sprite(sprite).cloned();
            self.masks.insert(sprite, tex);
        }
        self.masks.get(&sprite).cloned().flatten()
    }
}

impl Assets for ClientAssets {
    fn sprite(&mut self, id: i32) -> Option<&Texture> {
        self.inner.sprite(id)
    }

    fn font(&mut self, id: i32) -> Option<&osrs_cache::font::Font> {
        self.inner.font(id)
    }

    fn item(&mut self, obj: u32, qty: u32) -> Option<&Texture> {
        self.inner.item(obj, qty)
    }

    fn content(&mut self, content_type: u16, sprite: i32, rect: Rect, canvas: &mut Canvas) {
        match content_type {
            1338 => {
                let mask = self.mask(sprite);
                self.minimap.draw_map(rect, mask.as_ref(), canvas, &mut self.inner);
            }
            1339 => {
                let mask = self.mask(sprite);
                self.minimap.draw_compass(rect, mask.as_ref(), canvas, &mut self.inner);
            }
            _ => {}
        }
    }
}
