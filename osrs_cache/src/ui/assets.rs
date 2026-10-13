//! [`Assets`] backed by the real cache, loading lazily and remembering results.

use super::tree::Assets;
use crate::config::Configs;
use crate::font::Font;
use crate::item::ItemDef;
use crate::textures::{load_sprite, Texture};
use crate::{idx, Cache, Index};
use std::collections::HashMap;

pub struct CacheAssets<'a> {
    cache: &'a Cache,
    sprite_index: Index,
    sprites: HashMap<i32, Option<Texture>>,
    fonts: HashMap<i32, Option<Font>>,
    items: Option<(HashMap<u32, ItemDef>, HashMap<u32, u16>)>,
    icons: HashMap<u32, Option<Texture>>,
}

impl<'a> CacheAssets<'a> {
    pub fn new(cache: &'a Cache) -> crate::Result<CacheAssets<'a>> {
        Ok(CacheAssets {
            cache,
            sprite_index: cache.index(idx::SPRITES)?,
            sprites: HashMap::new(),
            fonts: HashMap::new(),
            items: None,
            icons: HashMap::new(),
        })
    }
}

impl Assets for CacheAssets<'_> {
    fn sprite(&mut self, id: i32) -> Option<&Texture> {
        if id < 0 {
            return None;
        }
        let (cache, index) = (self.cache, &self.sprite_index);
        self.sprites.entry(id).or_insert_with(|| load_sprite(cache, index, id as u32)).as_ref()
    }

    fn font(&mut self, id: i32) -> Option<&Font> {
        if id < 0 {
            return None;
        }
        let cache = self.cache;
        self.fonts.entry(id).or_insert_with(|| Font::load_id(cache, id as u32).ok().flatten()).as_ref()
    }

    fn item(&mut self, obj: u32, qty: u32) -> Option<&Texture> {
        if self.items.is_none() {
            let configs = Configs::new(self.cache).ok()?;
            self.items = Some((configs.items().ok()?, configs.texture_average_hsl().ok()?));
        }
        let (defs, hsl) = self.items.as_ref()?;
        let def = defs.get(&obj)?;
        let shown = def.pile_item(qty).and_then(|p| defs.get(&p)).unwrap_or(def);
        let key = shown.id;
        let cache = self.cache;
        self.icons.entry(key).or_insert_with(|| crate::item_icon::item_icon(cache, shown, hsl)).as_ref()
    }
}
