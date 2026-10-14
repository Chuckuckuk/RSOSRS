//! The game interface: the cache's own widget tree (resizable classic by default), drawn by the
//! software renderer `osrs_cache::ui` into one picture that covers the window.
//!
//! * [`InterfacePlugin`] builds the interface at start-up (`build_interface`): the top-level layout
//!   (`OSRS_LAYOUT=classic|modern|fixed`), the standard side panes mounted, one `ImageNode` that
//!   shows the picture.
//! * `refresh` runs every frame, builds the panes' state from the server's snapshot (`panes`), and
//!   draws again only when something it shows changed (`PaneInput::fingerprint`). The picture's
//!   `Image` is reused. The list of widgets drawn goes to [`UiHits`].
//! * [`UiHits::target_at`] says what a window position is (a tab, an inventory slot...), which is
//!   what `ground_click` and `tile_marks` ask. `None` means the world shows there.
//! * In the fixed layout the 3D camera is cut down to the viewport widget (`fit_viewport`).
//!
//! Item pictures are the items' models, drawn like the game does (`osrs_cache::item_icon`) when the
//! renderer first needs them.

use super::chat::ChatLog;
use super::panes::{self, minimap::MapParams, minimap::MinimapView, ClientAssets, PaneInput};
use super::{OrbitCamera, Player, ServerLink};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use osrs_cache::config::Configs;
use osrs_cache::item::{ids_by_name, ItemDef};
use osrs_cache::ui::{draw, standard_state, CacheAssets, Hit, Layout, Rect, TopLevel, UiState};
use osrs_cache::Cache;
use osrs_core::data::EquipSlot;
use osrs_core::tiles::TileCoord;
use std::collections::HashMap;
use std::time::Instant;

pub const INVENTORY_TAB: usize = 3;
#[allow(dead_code)]
pub const EQUIPMENT_TAB: usize = 4;
pub const PRAYER_TAB: usize = 5;
pub const MAGIC_TAB: usize = 6;

/// The width and height of the fixed interface, game pixels.
const FIXED_SIZE: (i32, i32) = (765, 503);

/// One prayer in the prayer book.
pub struct BookPrayer {
    /// The id the server knows it by (`data/prayers.json`).
    pub id: &'static str,
    pub name: &'static str,
    /// Its icon, and the greyed icon shown while the player's level is too low for it.
    pub icon: u32,
    pub greyed: u32,
}

const fn prayer(id: &'static str, name: &'static str, icon: u32, greyed: u32) -> BookPrayer {
    BookPrayer { id, name, icon, greyed }
}

/// The standard prayer book, in the order the game lays it out (by level, five to a row,
/// `panes::prayer`). Each icon is the cache item's param 1757 and its greyed copy param 1756.
pub const PRAYER_BOOK: [BookPrayer; 29] = [
    prayer("thick_skin", "Thick Skin", 115, 135),
    prayer("burst_of_strength", "Burst of Strength", 116, 136),
    prayer("clarity_of_thought", "Clarity of Thought", 117, 137),
    prayer("sharp_eye", "Sharp Eye", 133, 153),
    prayer("mystic_will", "Mystic Will", 134, 154),
    prayer("rock_skin", "Rock Skin", 118, 138),
    prayer("superhuman_strength", "Superhuman Strength", 119, 139),
    prayer("improved_reflexes", "Improved Reflexes", 120, 140),
    prayer("rapid_restore", "Rapid Restore", 121, 141),
    prayer("rapid_heal", "Rapid Heal", 122, 142),
    prayer("protect_item", "Protect Item", 123, 143),
    prayer("hawk_eye", "Hawk Eye", 502, 506),
    prayer("mystic_lore", "Mystic Lore", 503, 507),
    prayer("steel_skin", "Steel Skin", 124, 144),
    prayer("ultimate_strength", "Ultimate Strength", 125, 145),
    prayer("incredible_reflexes", "Incredible Reflexes", 126, 146),
    prayer("protect_from_magic", "Protect from Magic", 127, 147),
    prayer("protect_from_missiles", "Protect from Missiles", 128, 148),
    prayer("protect_from_melee", "Protect from Melee", 129, 149),
    prayer("eagle_eye", "Eagle Eye", 504, 508),
    prayer("mystic_might", "Mystic Might", 505, 509),
    prayer("retribution", "Retribution", 131, 151),
    prayer("redemption", "Redemption", 130, 150),
    prayer("smite", "Smite", 132, 152),
    prayer("preserve", "Preserve", 947, 951),
    prayer("chivalry", "Chivalry", 945, 949),
    prayer("piety", "Piety", 946, 950),
    prayer("rigour", "Rigour", 1420, 1424),
    prayer("augury", "Augury", 1421, 1425),
];

/// What the prayer pane needs from the game's rules: each book prayer's level, or `None` for one
/// the server does not have yet (drawn greyed, and does nothing).
#[derive(Resource)]
pub struct PrayerLevels(pub Vec<Option<i32>>);

/// The inventory slot the player has picked with "Use": the next item clicked (in the inventory or
/// on the ground) is what it is used on.
#[derive(Resource, Default)]
pub struct UseSelection(pub Option<usize>);

impl Default for PrayerLevels {
    fn default() -> Self {
        let registry = osrs_core::data::Registry::builtin();
        PrayerLevels(PRAYER_BOOK.iter().map(|p| registry.prayer(p.id).map(|def| def.level)).collect())
    }
}

/// What a point of the interface is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiTarget {
    Tab(usize),
    InventorySlot(usize),
    EquipmentSlot(EquipSlot),
    /// A prayer in the prayer book, by its place in `PRAYER_BOOK`.
    Prayer(usize),
    /// The minimap, at this world tile.
    Minimap { x: i32, z: i32 },
    /// The compass.
    Compass,
    /// A spell of the Magic tab, by its place in the standard spellbook (`panes::magic`).
    Spell(usize),
    /// The attack style button in slot 0..3 of the Combat Options tab (`panes::combat`).
    CombatStyle(usize),
    /// The special attack bar of the Combat Options tab.
    SpecialBar,
    /// The Auto Retaliate button (the server has no such setting yet).
    AutoRetaliate,
    /// One of the minimap's orbs.
    Orb(Orb),
    /// The Logout button of the Logout tab, and its World Switcher button.
    Logout,
    WorldSwitcher,
    /// Somewhere else on the interface: it takes the click so it does not reach the world.
    Background,
}

/// The minimap orbs that take clicks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orb {
    Prayer,
    Run,
    Special,
}

/// The open tab, and the layout in use.
#[derive(Resource)]
pub struct Interface {
    pub selected: usize,
    /// Whether the side panel is open. Only the modern resizable layout closes it (by clicking the
    /// open tab again); the others always have a pane open.
    pub open: bool,
    pub layout: Layout,
}

/// The layout asked for by `OSRS_LAYOUT` (`classic` by default).
pub fn layout_from_env() -> Layout {
    match std::env::var("OSRS_LAYOUT").ok().as_deref() {
        Some("modern") => Layout::Modern,
        Some("fixed") => Layout::Fixed,
        _ => Layout::Classic,
    }
}

impl Default for Interface {
    fn default() -> Self {
        // OSRS_TAB opens another tab at the start (a debugging aid for screenshots)
        let selected = std::env::var("OSRS_TAB").ok().and_then(|v| v.parse::<usize>().ok()).filter(|t| *t < 14);
        Interface { selected: selected.unwrap_or(INVENTORY_TAB), open: true, layout: layout_from_env() }
    }
}

impl Interface {
    /// A click on tab `index`'s stone (or its key): open it, or in the modern layout close the
    /// panel when it was already open.
    pub fn click_tab(&mut self, index: usize) {
        if self.layout == Layout::Modern && self.open && self.selected == index {
            self.open = false;
        } else {
            self.selected = index;
            self.open = true;
        }
    }
}

/// Item definitions, for finding the cache item the server's item names mean and for its menu.
pub struct ItemIcons {
    cache: Cache,
    defs: HashMap<u32, ItemDef>,
    by_name: HashMap<String, u32>,
    texture_hsl: HashMap<u32, u16>,
    /// What the Combat Options tab offers per weapon type, and what the Skills tab's cells show.
    pub styles: panes::combat::WeaponStyles,
    pub skills: panes::skills::SkillTable,
    /// The standard spellbook of the Magic tab.
    pub spells: panes::magic::SpellBook,
    pub emotes: panes::emotes::EmoteTable,
    pub music: panes::music::MusicTable,
}

impl ItemIcons {
    fn open() -> osrs_cache::Result<ItemIcons> {
        let cache = Cache::open_default()?;
        let configs = Configs::new(&cache)?;
        let defs = configs.items()?;
        let by_name = ids_by_name(&defs);
        let texture_hsl = configs.texture_average_hsl()?;
        let styles = panes::combat::WeaponStyles::load(&cache).unwrap_or_default();
        let skills = panes::skills::SkillTable::load(&cache).unwrap_or_default();
        let spells = panes::magic::SpellBook::load(&cache, &defs).unwrap_or_default();
        let emotes = panes::emotes::EmoteTable::load(&cache).unwrap_or_default();
        let music = panes::music::MusicTable::load(&cache).unwrap_or_default();
        Ok(ItemIcons { cache, defs, by_name, texture_hsl, styles, skills, spells, emotes, music })
    }

    /// The cache item called `name` (as the server names it).
    pub fn item_named(&self, name: &str) -> Option<&ItemDef> {
        let lower = name.to_lowercase();
        let id = self
            .by_name
            .get(&lower)
            .or_else(|| self.by_name.get(&format!("{lower}(4)")))
            .or_else(|| self.by_name.get(&format!("{lower}(1)")))?;
        self.defs.get(id)
    }
}

/// Draws the picture of each named item once, discarding it: a recording run
/// (`OSRS_RECORD_BUNDLE`) uses this so the groups those pictures need are noted.
pub fn touch_icons(names: &[String]) {
    let Ok(icons) = ItemIcons::open() else { return };
    for name in names {
        if let Some(def) = icons.item_named(name) {
            let _ = osrs_cache::item_icon::item_icon(&icons.cache, def, &icons.texture_hsl);
        }
    }
}

/// How a stack's size is written over it: whole up to 99,999, then in thousands and millions, in
/// the game's colours.
#[allow(dead_code)]
pub fn quantity_label(quantity: u32) -> (String, u32) {
    match quantity {
        0..=99_999 => (quantity.to_string(), 0xFFFF00),
        100_000..=9_999_999 => (format!("{}K", quantity / 1000), 0xFFFFFF),
        _ => (format!("{}M", quantity / 1_000_000), 0x00FF80),
    }
}

/// Which window pixels belong to which widget, as of the last time the interface was drawn.
#[derive(Resource, Default)]
pub struct UiHits {
    hits: Vec<Hit>,
    /// The picture's alpha, row by row: an opaque pixel is interface even where no widget listens.
    alpha: Vec<u8>,
    /// The picture's size in its own pixels.
    size: (i32, i32),
    /// Window pixels per picture pixel.
    scale: i32,
    /// Where the picture's corner is in the window.
    origin: Vec2,
    stones: HashMap<u32, usize>,
    equipment: HashMap<u32, EquipSlot>,
    /// The Magic tab's spell widgets and their place in the spellbook.
    spells: HashMap<u32, usize>,
    /// The 3D viewport widget's rectangle in picture pixels (fixed layout only).
    viewport: Option<Rect>,
    /// The minimap's centre and turn as drawn.
    map: MapParams,
    /// Physical pixels per logical pixel of the window (the picture is sized in physical pixels).
    factor: f32,
    /// Counts redraws.
    pub generation: u32,
}

/// The first of the 29 prayer cells (541:9).
const FIRST_PRAYER_CELL: u32 = 9;

impl UiHits {
    /// What is under `cursor` (a window position, logical pixels), if it is the interface.
    pub fn target_at(&self, cursor: Vec2) -> Option<UiTarget> {
        if self.scale <= 0 {
            return None;
        }
        let p = (cursor * self.factor - self.origin) / self.scale as f32;
        let (x, y) = (p.x.floor() as i32, p.y.floor() as i32);
        if p.x < 0.0 || p.y < 0.0 || x >= self.size.0 || y >= self.size.1 {
            return None;
        }
        let opaque = self.alpha[(y * self.size.0 + x) as usize] > 0;
        let widget = self
            .hits
            .iter()
            .rev()
            .find(|h| (h.interactive() || h.content_type == 1338 || h.content_type == 1339) && h.rect.contains(x, y));
        match widget {
            Some(hit) => self.classify(hit, x, y, opaque),
            None => opaque.then_some(UiTarget::Background),
        }
    }

    fn classify(&self, hit: &Hit, x: i32, y: i32, opaque: bool) -> Option<UiTarget> {
        match hit.content_type {
            // the corners of the minimap's square show the world
            1338 if opaque => {
                let (cx, cy) = (hit.rect.x as f32 + hit.rect.w as f32 / 2.0, hit.rect.y as f32 + hit.rect.h as f32 / 2.0);
                let (wx, wz) = self.map.tile_at(x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                return Some(UiTarget::Minimap { x: wx.floor() as i32, z: wz.floor() as i32 });
            }
            1339 if opaque => return Some(UiTarget::Compass),
            1338 | 1339 => return None,
            _ => {}
        }
        if let Some(tab) = self.stones.get(&hit.id) {
            return Some(UiTarget::Tab(*tab));
        }
        if hit.created.is_some() && hit.id == panes::inventory::INVENTORY {
            return hit.created.map(UiTarget::InventorySlot);
        }
        if let Some(slot) = self.equipment.get(&hit.id) {
            return Some(UiTarget::EquipmentSlot(*slot));
        }
        if hit.id >> 16 == 541 {
            let cell = (hit.id & 0xffff).checked_sub(FIRST_PRAYER_CELL).map(|c| c as usize);
            if let Some(cell) = cell.filter(|c| *c < PRAYER_BOOK.len()) {
                return Some(UiTarget::Prayer(cell));
            }
        }
        if let Some(spell) = self.spells.get(&hit.id) {
            return Some(UiTarget::Spell(*spell));
        }
        if let Some(target) = orb_or_combat_target(hit.id) {
            return Some(target);
        }
        Some(UiTarget::Background)
    }
}

/// The click layers of the Combat Options tab and of the orbs.
fn orb_or_combat_target(id: u32) -> Option<UiTarget> {
    let (group, file) = (id >> 16, id & 0xffff);
    match (group, file) {
        (593, 32) => Some(UiTarget::AutoRetaliate),
        (593, 39) => Some(UiTarget::SpecialBar),
        (593, f) if (6..=18).contains(&f) && (f - 6) % 4 == 0 => Some(UiTarget::CombatStyle(((f - 6) / 4) as usize)),
        (182, 8) => Some(UiTarget::Logout),
        (182, 3) => Some(UiTarget::WorldSwitcher),
        (160, 20) => Some(UiTarget::Orb(Orb::Prayer)),
        (160, 28) => Some(UiTarget::Orb(Orb::Run)),
        (160, 36) => Some(UiTarget::Orb(Orb::Special)),
        _ => None,
    }
}

/// The interface picture's node and the state that draws it. A non-send resource: the cache's
/// assets and the minimap thread's channels live in it.
struct ClientUi {
    /// The cache the interface was built from (leaked: it lives as long as the program).
    cache: &'static Cache,
    state: UiState,
    assets: ClientAssets,
    top: TopLevel,
    image: Handle<Image>,
    node: Entity,
    last: u64,
    draws: u32,
    total_ms: f64,
}

/// The node showing the picture.
#[derive(Component)]
struct UiPicture;

/// The 2D camera the fixed layout lays its interface out through.
#[derive(Component)]
struct FixedUiCamera;

/// The picture's scale (physical pixels per picture pixel): `OSRS_UI_SCALE`, else the largest whole number that fits the 765x503
/// interface in the window.
fn ui_scale(window: Vec2) -> i32 {
    if let Some(v) = std::env::var("OSRS_UI_SCALE").ok().and_then(|v| v.parse::<i32>().ok()) {
        return v.max(1);
    }
    ((window.x / FIXED_SIZE.0 as f32).min(window.y / FIXED_SIZE.1 as f32).floor() as i32).max(1)
}

pub struct InterfacePlugin;

impl Plugin for InterfacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Interface>()
            .init_resource::<UseSelection>()
            .init_resource::<PrayerLevels>()
            .init_resource::<UiHits>()
            .add_systems(Startup, build_interface)
            .add_systems(Update, (tab_keys, switch_layout_key))
            .add_systems(Update, (refresh, fit_viewport).chain().after(super::orbit_camera));
    }
}

/// Keys that open a tab, as the game's default bindings do: Escape the inventory, F1 the prayer
/// tab, F2 the spellbook (the Magic tab).
const TAB_KEYS: [(KeyCode, usize); 3] = [
    (KeyCode::Escape, INVENTORY_TAB),
    (KeyCode::F1, PRAYER_TAB),
    (KeyCode::F2, MAGIC_TAB),
];

fn tab_keys(keys: Res<ButtonInput<KeyCode>>, mut interface: ResMut<Interface>) {
    for (key, tab) in TAB_KEYS {
        if keys.just_pressed(key) {
            interface.click_tab(tab);
        }
    }
}

/// The fixed layout needs a plain 2D camera over the whole window to lay the interface out (see
/// `build_interface`); the other layouts do not. Spawns or removes it to match `layout`.
fn sync_ui_camera(world: &mut World, layout: Layout) {
    let existing: Vec<Entity> = world.query_filtered::<Entity, With<FixedUiCamera>>().iter(world).collect();
    if layout == Layout::Fixed && existing.is_empty() {
        world.spawn((
            Camera2d,
            Camera { order: 1, clear_color: ClearColorConfig::None, ..default() },
            IsDefaultUiCamera,
            FixedUiCamera,
        ));
    } else if layout != Layout::Fixed {
        for e in existing {
            world.despawn(e);
        }
    }
}

/// The layout after `layout` in the F10 cycle: Fixed, Classic, Modern.
pub fn next_layout(layout: Layout) -> Layout {
    match layout {
        Layout::Fixed => Layout::Classic,
        Layout::Classic => Layout::Modern,
        Layout::Modern => Layout::Fixed,
    }
}

/// F10 cycles the layout (the game's Settings pane, which would choose it, is not built).
fn switch_layout_key(keys: Res<ButtonInput<KeyCode>>, mut commands: Commands) {
    if keys.just_pressed(KeyCode::F10) {
        commands.queue(switch_layout);
    }
}

/// Changes to the next layout at run time: rebuilds the widget state for it (as `build_interface` does
/// at start-up), forgets the old stones, adds or removes the fixed layout's UI camera and makes the next
/// frame redraw. The camera's viewport follows in `fit_viewport`.
fn switch_layout(world: &mut World) {
    let layout = next_layout(world.resource::<Interface>().layout);
    let selected = world.resource::<Interface>().selected;
    let rebuilt = {
        let Some(mut ui) = world.get_non_send_mut::<ClientUi>() else { return };
        let cache = ui.cache;
        match standard_state(cache, layout, Some(selected)) {
            Ok(state) => {
                ui.state = state;
                ui.top = TopLevel::of(layout);
                ui.last = u64::MAX;
                Some(ui.top.stone)
            }
            Err(e) => {
                eprintln!("[client] could not switch the layout: {e}");
                None
            }
        }
    };
    let Some(stones) = rebuilt else { return };
    {
        let mut interface = world.resource_mut::<Interface>();
        interface.layout = layout;
        // only the modern layout can have its panel closed
        interface.open = true;
    }
    let mut hits = world.resource_mut::<UiHits>();
    hits.stones.clear();
    for (i, id) in stones.iter().enumerate() {
        hits.stones.insert(*id, i);
    }
    println!("[ui] layout {layout:?}");
    sync_ui_camera(world, layout);
}

fn build_interface(world: &mut World) {
    let layout = world.resource::<Interface>().layout;
    let selected = world.resource::<Interface>().selected;
    let built = (|| -> osrs_cache::Result<(ClientUi, ItemIcons)> {
        let icons = ItemIcons::open()?;
        // the interface lives as long as the program
        let cache: &'static Cache = Box::leak(Box::new(Cache::open_default()?));
        let state = standard_state(cache, layout, Some(selected))?;
        let mut assets = ClientAssets::new(CacheAssets::new(cache)?, MinimapView::spawn(cache));
        // read every item definition now, not on the first frame that shows an item
        let _ = osrs_cache::ui::Assets::item(&mut assets, 4151, 1);
        let blank = Image::new_fill(
            bevy::render::render_resource::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            bevy::render::render_resource::TextureDimension::D2,
            &[0, 0, 0, 0],
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::default(),
        );
        let image = world.resource_mut::<Assets<Image>>().add(blank);
        let node = world
            .spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), ..default() },
                ImageNode::new(image.clone()),
                GlobalZIndex(20),
                UiPicture,
            ))
            .id();
        // A camera with a viewport lays UI out inside that viewport, so the fixed layout (where the 3D
        // camera has one) draws the interface through a camera of its own that spans the window.
        sync_ui_camera(world, layout);
        let top = TopLevel::of(layout);
        Ok((ClientUi { cache, state, assets, top, image, node, last: u64::MAX, draws: 0, total_ms: 0.0 }, icons))
    })();
    match built {
        Ok((ui, icons)) => {
            // which widget is which, for the clicks
            let mut hits = world.resource_mut::<UiHits>();
            for (i, id) in ui.top.stone.iter().enumerate() {
                hits.stones.insert(*id, i);
            }
            for (slot, file, _) in panes::equipment::SLOTS {
                hits.equipment.insert(panes::comp(387, file), slot);
            }
            for (i, spell) in icons.spells.spells.iter().enumerate() {
                hits.spells.insert(spell.comp, i);
            }
            world.insert_non_send(ui);
            world.insert_non_send(icons);
        }
        Err(e) => eprintln!("[client] the interface could not be built: {e}"),
    }
}

/// Where the player is in world tiles (x east, z north), for the minimap.
fn player_position(players: &Query<&Transform, With<Player>>, server: &ServerLink) -> (f32, f32) {
    if let Ok(t) = players.single() {
        return (t.translation.x, -t.translation.z);
    }
    server.latest.as_ref().map_or((0.0, 0.0), |s| (s.player.tile.x as f32 + 0.5, s.player.tile.z as f32 + 0.5))
}

#[allow(clippy::too_many_arguments)]
fn refresh(
    windows: Query<&Window, With<PrimaryWindow>>,
    server: NonSend<ServerLink>,
    interface: Res<Interface>,
    chat: Res<ChatLog>,
    levels: Res<PrayerLevels>,
    use_selection: Res<UseSelection>,
    icons: Option<NonSend<ItemIcons>>,
    ui: Option<NonSendMut<ClientUi>>,
    mut hits: ResMut<UiHits>,
    mut images: ResMut<Assets<Image>>,
    players: Query<&Transform, With<Player>>,
    cameras: Query<&OrbitCamera>,
    mut nodes: Query<&mut Node, With<UiPicture>>,
) {
    let (Some(mut ui), Some(icons)) = (ui, icons) else { return };
    let Ok(window) = windows.single() else { return };
    // everything is sized in physical pixels, so the picture's pixels are whole screen pixels
    let factor = window.scale_factor();
    let win = Vec2::new(window.physical_width() as f32, window.physical_height() as f32);
    if win.x < 1.0 || win.y < 1.0 {
        return;
    }
    let scale = ui_scale(win);
    let (cw, ch) = match interface.layout {
        Layout::Fixed => FIXED_SIZE,
        _ => ((win.x / scale as f32).ceil() as i32, (win.y / scale as f32).ceil() as i32),
    };
    let origin = match interface.layout {
        Layout::Fixed => Vec2::new(((win.x - (cw * scale) as f32) / 2.0).floor().max(0.0), ((win.y - (ch * scale) as f32) / 2.0).floor().max(0.0)),
        _ => Vec2::ZERO,
    };

    // the minimap's picture follows the player and the camera
    let (px, pz) = player_position(&players, &server);
    let yaw = cameras.single().map_or(0.0, |c| c.yaw);
    let map = MapParams { x: px, z: pz, yaw };
    let npc_dots: Vec<(f32, f32)> = server.latest.as_ref().map_or(Vec::new(), |s| {
        s.entities
            .iter()
            .filter(|e| !e.is_dead)
            .map(|e| (e.tile.x as f32 + e.size as f32 / 2.0, e.tile.z as f32 + e.size as f32 / 2.0))
            .collect()
    });
    let arrived = ui.assets.minimap.update(map, &npc_dots);

    let player = server.latest.as_ref().map(|s| &s.player);
    let messages = chat.messages();
    let name = std::env::var("OSRS_NAME").unwrap_or_else(|_| "Player".into());
    let top = ui.top.clone();
    let input = PaneInput {
        player,
        npc_dots: &npc_dots,
        icons: &icons,
        levels: &levels.0,
        chat: &messages,
        name: &name,
        top: &top,
        layout: interface.layout,
        selected: interface.selected,
        open: interface.open,
        use_selected: use_selection.0,
        map,
    };
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (input.fingerprint(), cw, ch, scale, ui.assets.minimap.ready(), arrived as u8).hash(&mut h);
    let key = h.finish();
    if key == ui.last {
        return;
    }
    ui.last = key;

    let started = Instant::now();
    let ClientUi { state, assets, .. } = &mut *ui;
    panes::sync(state, &input, assets);
    osrs_cache::ui::arrange(state, interface.layout, cw);
    let (canvas, drawn) = draw(state, assets, cw, ch);
    let ms = started.elapsed().as_secs_f64() * 1000.0;

    hits.alpha = canvas.rgba.chunks_exact(4).map(|p| p[3]).collect();
    hits.viewport = drawn.iter().find(|h| h.content_type == 1337).map(|h| h.rect);
    hits.hits = drawn;
    hits.size = (cw, ch);
    hits.scale = scale;
    hits.origin = origin;
    hits.factor = factor;
    hits.map = map;
    hits.generation += 1;

    // reuse the Image: same size, same asset, new pixels
    let size = bevy::render::render_resource::Extent3d { width: cw as u32, height: ch as u32, depth_or_array_layers: 1 };
    let same = images.get(&ui.image).is_some_and(|i| i.texture_descriptor.size == size);
    if same {
        if let Some(mut image) = images.get_mut(&ui.image) {
            image.data = Some(canvas.rgba);
        }
    } else {
        let mut image = Image::new(
            size,
            bevy::render::render_resource::TextureDimension::D2,
            canvas.rgba,
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::default(),
        );
        image.sampler = bevy::image::ImageSampler::nearest();
        let _ = images.insert(ui.image.id(), image);
    }
    if let Ok(mut node) = nodes.get_mut(ui.node) {
        node.left = Val::Px(origin.x / factor);
        node.top = Val::Px(origin.y / factor);
        node.width = Val::Px((cw * scale) as f32 / factor);
        node.height = Val::Px((ch * scale) as f32 / factor);
    }

    ui.draws += 1;
    ui.total_ms += ms;
    if ui.draws <= 3 || ui.draws % 50 == 0 {
        println!(
            "[ui] draw {ms:.2} ms ({cw}x{ch} at scale {scale}; {} draws, average {:.2} ms)",
            ui.draws,
            ui.total_ms / ui.draws as f64
        );
    }
}

/// In the fixed layout the 3D view fills only the viewport widget; elsewhere the whole window.
fn fit_viewport(
    hits: Res<UiHits>,
    interface: Res<Interface>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Camera, With<OrbitCamera>>,
    mut applied: Local<(u32, bool)>,
) {
    let fixed = interface.layout == Layout::Fixed;
    if hits.generation == applied.0 && fixed == applied.1 {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Ok(mut camera) = cameras.single_mut() else { return };
    *applied = (hits.generation, fixed);
    let rect = hits.viewport.filter(|_| fixed);
    camera.viewport = rect.map(|r| {
        let k = hits.scale as f32;
        let position = hits.origin + Vec2::new(r.x as f32, r.y as f32) * k;
        let size = Vec2::new(r.w as f32, r.h as f32) * k;
        let target = Vec2::new(window.physical_width() as f32, window.physical_height() as f32);
        let position = position.min(target - Vec2::ONE).max(Vec2::ZERO);
        let size = size.min(target - position).max(Vec2::ONE);
        bevy::camera::Viewport {
            physical_position: UVec2::new(position.x as u32, position.y as u32),
            physical_size: UVec2::new(size.x as u32, size.y as u32),
            ..default()
        }
    });
}

/// The tile a world position is on.
#[allow(dead_code)]
fn tile_of(x: f32, z: f32) -> TileCoord {
    TileCoord::new(x.floor() as i32, z.floor() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: u32, created: Option<usize>, rect: Rect, ops: &[&str]) -> Hit {
        Hit {
            id,
            created,
            rect,
            kind: 5,
            content_type: 0,
            ops: ops.iter().map(|s| s.to_string()).collect(),
            name: String::new(),
            no_click_through: false,
            opaque: false,
            targetable: false,
        }
    }

    fn hits_with(list: Vec<Hit>) -> UiHits {
        let mut h = UiHits { size: (100, 100), scale: 2, factor: 1.0, alpha: vec![0; 100 * 100], ..Default::default() };
        h.stones.insert(panes::comp(161, 60), 1);
        h.equipment.insert(panes::comp(387, 18), EquipSlot::Weapon);
        h.hits = list;
        // an opaque panel
        for y in 0..50 {
            for x in 0..50 {
                h.alpha[y * 100 + x] = 255;
            }
        }
        h
    }

    #[test]
    fn a_click_finds_the_widget_it_is_on() {
        let h = hits_with(vec![
            hit(panes::comp(161, 60), None, Rect::new(0, 0, 10, 10), &["*"]),
            hit(panes::inventory::INVENTORY, Some(5), Rect::new(10, 0, 10, 10), &["*"]),
            hit(panes::comp(387, 18), None, Rect::new(20, 0, 10, 10), &["*"]),
            hit(panes::comp(541, 9 + 18), None, Rect::new(30, 0, 10, 10), &["*"]),
        ]);
        let at = |x: f32, y: f32| h.target_at(Vec2::new(x * 2.0 + 1.0, y * 2.0 + 1.0));
        assert_eq!(at(5.0, 5.0), Some(UiTarget::Tab(1)));
        assert_eq!(at(15.0, 5.0), Some(UiTarget::InventorySlot(5)));
        assert_eq!(at(25.0, 5.0), Some(UiTarget::EquipmentSlot(EquipSlot::Weapon)));
        assert_eq!(at(35.0, 5.0), Some(UiTarget::Prayer(18)));
        // opaque interface with no widget on it takes the click; transparent pixels are the world
        assert_eq!(at(45.0, 45.0), Some(UiTarget::Background));
        assert_eq!(at(70.0, 70.0), None);
        // outside the picture
        assert_eq!(h.target_at(Vec2::new(500.0, 5.0)), None);
    }

    #[test]
    fn the_combat_buttons_and_orbs_are_found_by_component() {
        let id = panes::comp;
        assert_eq!(orb_or_combat_target(id(593, 6)), Some(UiTarget::CombatStyle(0)));
        assert_eq!(orb_or_combat_target(id(593, 10)), Some(UiTarget::CombatStyle(1)));
        assert_eq!(orb_or_combat_target(id(593, 14)), Some(UiTarget::CombatStyle(2)));
        assert_eq!(orb_or_combat_target(id(593, 18)), Some(UiTarget::CombatStyle(3)));
        assert_eq!(orb_or_combat_target(id(593, 7)), None);
        assert_eq!(orb_or_combat_target(id(593, 39)), Some(UiTarget::SpecialBar));
        assert_eq!(orb_or_combat_target(id(593, 32)), Some(UiTarget::AutoRetaliate));
        assert_eq!(orb_or_combat_target(id(160, 28)), Some(UiTarget::Orb(Orb::Run)));
        assert_eq!(orb_or_combat_target(id(160, 36)), Some(UiTarget::Orb(Orb::Special)));
        assert_eq!(orb_or_combat_target(id(160, 20)), Some(UiTarget::Orb(Orb::Prayer)));
    }

    #[test]
    fn the_modern_layout_closes_the_panel_on_the_open_tab() {
        let mut modern = Interface { selected: 3, open: true, layout: Layout::Modern };
        modern.click_tab(3);
        assert!(!modern.open);
        modern.click_tab(5);
        assert!(modern.open && modern.selected == 5);
        let mut classic = Interface { selected: 3, open: true, layout: Layout::Classic };
        classic.click_tab(3);
        assert!(classic.open);
    }

    #[test]
    fn the_scale_is_the_largest_whole_number_that_fits() {
        assert_eq!(ui_scale(Vec2::new(1280.0, 720.0)), 1);
        assert_eq!(ui_scale(Vec2::new(1920.0, 1080.0)), 2);
        assert_eq!(ui_scale(Vec2::new(3000.0, 600.0)), 1);
    }

    #[test]
    fn the_prayer_book_matches_the_server() {
        let registry = osrs_core::data::Registry::builtin();
        for def in registry.prayers() {
            assert!(PRAYER_BOOK.iter().any(|p| p.id == def.id), "{} is missing from the prayer book", def.id);
        }
        assert_eq!(PRAYER_BOOK[18].id, "protect_from_melee");
    }

    #[test]
    fn stack_sizes_are_written_the_games_way() {
        assert_eq!(quantity_label(25), ("25".to_owned(), 0xFFFF00));
        assert_eq!(quantity_label(100_000), ("100K".to_owned(), 0xFFFFFF));
        assert_eq!(quantity_label(10_000_000), ("10M".to_owned(), 0x00FF80));
    }
}
