//! The minimap of the square being edited, drawn by the game's own minimap code
//! ([`MinimapSource`]) through the overlay, so placed icons and changed walls show as the game
//! will show them; and the icon sprites, for the palette and the preview.

use bevy::prelude::*;
use bevy_egui::egui;
use osrs_cache::minimap::{MapIcon, MinimapSource};
use osrs_cache::textures::Texture;
use osrs_cache::{idx, Cache, Index};
use std::collections::HashMap;

#[derive(Resource)]
pub struct MinimapPreview {
    pub open: bool,
    cache: &'static Cache,
    /// The definitions, and how many minted markers the project had when they were decoded (a new
    /// one needs them decoded again to show its icon).
    source: Option<(usize, MinimapSource<'static>)>,
    pub square: Option<(i32, i32)>,
    pub texture: Option<egui::TextureHandle>,
    pub icons: Vec<MapIcon>,
}

impl MinimapPreview {
    pub fn new(cache: &'static Cache) -> MinimapPreview {
        MinimapPreview { open: true, cache, source: None, square: None, texture: None, icons: Vec::new() }
    }

    /// Draws square `square` again if it is not the one shown or `stale`.
    pub fn refresh(&mut self, ctx: &egui::Context, square: (i32, i32), stale: bool, markers: usize) {
        if !stale && self.square == Some(square) && self.texture.is_some() {
            return;
        }
        if self.source.as_ref().is_none_or(|(n, _)| *n != markers) {
            match MinimapSource::new(self.cache) {
                Ok(source) => self.source = Some((markers, source)),
                Err(e) => {
                    eprintln!("[editor] minimap: {e}");
                    return;
                }
            }
        }
        let Some((_, source)) = &self.source else { return };
        self.square = Some(square);
        match source.render(square.0, square.1) {
            Ok(map) => {
                let size = [map.image.width as usize, map.image.height as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &map.image.rgba);
                self.texture = Some(ctx.load_texture("minimap", image, egui::TextureOptions::NEAREST));
                self.icons = map.icons;
            }
            Err(_) => {
                self.texture = None;
                self.icons.clear();
            }
        }
    }
}

/// Sprites by group id, as egui textures, loaded the first time each is asked for.
#[derive(Resource)]
pub struct Sprites {
    cache: &'static Cache,
    index: Option<Index>,
    loaded: HashMap<u32, Option<egui::TextureHandle>>,
}

impl Sprites {
    pub fn new(cache: &'static Cache) -> Sprites {
        Sprites { index: cache.index(idx::SPRITES).ok(), cache, loaded: HashMap::new() }
    }

    pub fn get(&mut self, ctx: &egui::Context, group: u32) -> Option<egui::TextureHandle> {
        let (cache, index) = (self.cache, &self.index);
        self.loaded
            .entry(group)
            .or_insert_with(|| {
                let texture: Texture = osrs_cache::textures::load_sprite(cache, index.as_ref()?, group)?;
                let image = egui::ColorImage::from_rgba_unmultiplied([texture.width as usize, texture.height as usize], &texture.rgba);
                Some(ctx.load_texture(format!("sprite{group}"), image, egui::TextureOptions::NEAREST))
            })
            .clone()
    }
}
