//! Clicking the world: walking, and the right-click menu.
//!
//! Left-click follows the click's path from the camera: the first object that has a left-click
//! option other than walking selects that option (objects have no interactions yet, so it is
//! logged); otherwise it walks to the ground tile where the path ends.
//!
//! Right-click opens a "Choose Option" menu at the cursor (a right-click that is really a camera
//! drag opens nothing). An NPC in the way lists its options in the cache's order ("Talk-to",
//! "Attack", ...) and "Examine"; then comes the left-click option of an object in the way, if any,
//! and "Examine" for the nearest named object; the menu always ends with "Walk here", which walks
//! to the ground tile at the end of the click's path exactly as a left-click on that spot with
//! nothing in front would. Left-click does the first option of whatever is nearest.
//!
//! "Attack" on an NPC asks the server to attack it; the server walks the player up to it and
//! fights. Its other options have nothing to do yet.
//!
//! The client only names the tile it clicked; routing is the server's (`WalkTo`).

use super::chat::ChatLog;
use super::interface::{Interface, ItemIcons, Orb, PrayerLevels, UiHits, UiTarget, UseSelection, PRAYER_BOOK};
use super::npcs::{pick_npc, NpcHit, NpcView};
use super::{ObjectInfo, OrbitCamera, ServerLink};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use osrs_core::actors::EntityId;
use osrs_core::data::EquipSlot;
use osrs_core::sim::{ClientCommand, GroundItemView, ItemView};
use osrs_core::tiles::TileCoord;
use osrs_render::pick::{Click, PickedObject, Picker};
use osrs_render::streaming::StreamedWorld;

/// The narrowest the menu is; it grows to fit its longest line.
const MIN_MENU_WIDTH: f32 = 132.0;
/// The UI font is monospaced: a character is about 0.6 of the 14 px text size wide.
const CHARACTER_WIDTH: f32 = 8.5;
/// Space the rows keep either side of their text.
const ROW_PADDING: f32 = 6.0;
const FONT_PX: f32 = 14.0;
const HEADER_HEIGHT: f32 = 20.0;
const ENTRY_HEIGHT: f32 = 22.0;
/// How far the cursor may stray outside an open menu before it closes: the game allows 10 pixels,
/// where its rows are 15 pixels tall.
const MENU_LEAVE_MARGIN: f32 = ENTRY_HEIGHT * 10.0 / 15.0;
/// A right-click that moved the cursor this far (logical pixels) was an orbit drag.

pub struct GroundClickPlugin;

impl Plugin for GroundClickPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RightPress>()
            .init_resource::<ContextMenu>()
            .add_systems(Update, (ground_clicks, close_menu_when_left, highlight_menu_entry).chain());
    }
}

/// Click boxes of the loaded plane-0 objects; map squares add theirs as they stream in.
#[derive(Resource)]
pub struct GroundPicker(pub Picker);

impl GroundPicker {
    /// An empty picker that knows which objects take clicks; squares add their boxes as they load.
    pub fn new(info: &ObjectInfo) -> GroundPicker {
        GroundPicker(Picker::streamed(0, info.left_click.clone(), info.names.clone()))
    }
}

/// Where the right button went down: the menu opens there when it comes up.
#[derive(Resource, Default)]
struct RightPress {
    at: Option<Vec2>,
}

/// What choosing a menu entry does.
#[derive(Clone)]
enum MenuAction {
    /// Walk to a world tile.
    Walk(TileCoord),
    /// The left-click option of an object.
    Object(PickedObject),
    /// Ask the server what an object is.
    Examine { id: u32 },
    /// An option of an NPC. "Attack" is a request to the server; the rest have no effect yet.
    Npc { id: EntityId, name: String, option: String },
    /// Ask the server what an NPC is.
    ExamineNpc { id: EntityId },
    /// Choose an option of the item in an inventory slot ("Wield", "Eat", "Drop", ...).
    InventoryOption { slot: usize, option: String },
    /// "Use" on an inventory item: it waits for the second item (nothing is sent).
    SelectUse { slot: usize },
    /// Put the picked item down without using it.
    CancelUse,
    /// Use the item in `used` on the one in `target`.
    UseOnItem { used: usize, target: usize },
    /// Use the item in `slot` on a ground stack.
    UseOnGround { slot: usize, uid: u32 },
    /// Use the item in `slot` on the object `id` covering `tile` (a log on a fire).
    UseOnObject { slot: usize, id: u32, tile: TileCoord },
    /// Choose an option ("Take", "Light") of a ground stack.
    GroundOption { uid: u32, option: String },
    /// Take off what is worn in a slot.
    Unequip { slot: EquipSlot },
    /// Ask the server what an item is.
    ExamineItem { item: String },
    /// Switch a prayer on or off (by its id in the prayer book).
    TogglePrayer { prayer: &'static str },
    /// Choose the combat style at this index of the wielded weapon's list.
    SetStance { index: usize },
    /// Switch the special attack on or off.
    SetSpecial { enabled: bool },
    /// Switch running on or off.
    SetRunning { running: bool },
    /// Select (or with `None` clear) the spell the wielded staff casts.
    SelectSpell { spell: Option<String> },
    /// Say something in the chat box that only this client knows (an unavailable button).
    Say(String),
}

/// What a menu entry is about; its name is drawn in the game's colour for that kind of thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Subject {
    /// Scenery and other inanimate objects: blue.
    Object,
    /// Non-player characters: yellow.
    Npc,
    /// Items: orange.
    Item,
    /// Spells: green.
    Spell,
}

impl Subject {
    fn colour(self) -> Color {
        match self {
            // the game's 0x00FFFF
            Subject::Object => Color::srgb(0.0, 1.0, 1.0),
            // the game's 0xFFFF00
            Subject::Npc => Color::srgb(1.0, 1.0, 0.0),
            // the game's 0xFF9040
            Subject::Item => Color::srgb_u8(0xFF, 0x90, 0x40),
            // the game's 0x00FF00
            Subject::Spell => Color::srgb(0.0, 1.0, 0.0),
        }
    }
}

/// A line of the menu before it is drawn: the option text, then the name of what it acts on.
struct MenuItem {
    option: String,
    subject: Option<(String, Subject)>,
    /// Text after the name in its own colour (an NPC's level).
    suffix: Option<(String, Color)>,
    action: MenuAction,
}

/// The colour the game draws an NPC's level in: green when it is well below the player's combat
/// level, through yellow when equal, to red when well above.
fn level_colour(npc: i32, player: i32) -> Color {
    let rgb = match npc - player {
        d if d < -9 => (0x00, 0xFF, 0x00),
        -9..=-7 => (0x40, 0xFF, 0x00),
        -6..=-4 => (0x80, 0xFF, 0x00),
        -3..=-1 => (0xC0, 0xFF, 0x00),
        0 => (0xFF, 0xFF, 0x00),
        1..=3 => (0xFF, 0xB0, 0x00),
        4..=6 => (0xFF, 0x70, 0x00),
        7..=9 => (0xFF, 0x30, 0x00),
        _ => (0xFF, 0x00, 0x00),
    };
    Color::srgb_u8(rgb.0, rgb.1, rgb.2)
}

struct MenuEntry {
    row: Entity,
    action: MenuAction,
}

struct OpenMenu {
    root: Entity,
    entries: Vec<MenuEntry>,
    top_left: Vec2,
    width: f32,
}

/// How wide a menu must be to show every line (and its header) without clipping.
fn menu_width(items: &[MenuItem]) -> f32 {
    let longest = items
        .iter()
        .map(|item| {
            let name = item.subject.as_ref().map_or(0, |(name, _)| 1 + name.chars().count());
            let suffix = item.suffix.as_ref().map_or(0, |(text, _)| text.chars().count());
            item.option.chars().count() + name + suffix
        })
        .chain(std::iter::once("Choose Option".len()))
        .max()
        .unwrap_or(0);
    (longest as f32 * CHARACTER_WIDTH + ROW_PADDING * 2.0).max(MIN_MENU_WIDTH)
}

impl OpenMenu {
    fn entry_rect(&self, index: usize) -> Rect {
        let min = self.top_left + Vec2::new(0.0, HEADER_HEIGHT + ENTRY_HEIGHT * index as f32);
        Rect::from_corners(min, min + Vec2::new(self.width, ENTRY_HEIGHT))
    }

    /// Whether `cursor` is still near enough to keep the menu open: inside it, or within
    /// `MENU_LEAVE_MARGIN` of its edge.
    fn keeps_open(&self, cursor: Vec2) -> bool {
        let size = Vec2::new(self.width, HEADER_HEIGHT + ENTRY_HEIGHT * self.entries.len() as f32);
        Rect::from_corners(self.top_left, self.top_left + size).inflate(MENU_LEAVE_MARGIN).contains(cursor)
    }

    /// The entry under `cursor`, if any.
    fn entry_at(&self, cursor: Vec2) -> Option<usize> {
        (0..self.entries.len()).find(|i| self.entry_rect(*i).contains(cursor))
    }
}

#[derive(Resource, Default)]
struct ContextMenu {
    open: Option<OpenMenu>,
}

/// Ask the server to walk the player to `tile`. Left-click and "Walk here" both end up here.
/// `inverse` is a ctrl-click: this walk is a run if the player is walking, and a walk if running.
fn request_walk(server: &ServerLink, tile: TileCoord, inverse: bool) {
    println!("[client] {} to ({}, {})", if inverse { "walk (run/walk inverted)" } else { "walk" }, tile.x, tile.z);
    server.link.send(ClientCommand::WalkTo { tile, force_inverse: inverse });
}

/// Everything under the cursor: the ground tile at the end of the click's path, the objects in
/// the way and the NPC in the way, if it is nearer than the ground.
struct Under {
    tile: TileCoord,
    click: Click,
    npc: Option<NpcHit>,
}

fn pick_at(
    cursor: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    world: &StreamedWorld,
    picker: &Picker,
    npcs: &Query<(&NpcView, &Transform)>,
) -> Option<Under> {
    let origin = camera.logical_viewport_rect().map_or(Vec2::ZERO, |r| r.min);
    let ray = camera.viewport_to_world(camera_transform, cursor - origin).ok()?;
    let click = picker.pick_all(world, ray)?;
    let npc = pick_npc(ray.origin, *ray.direction, npcs.iter()).filter(|n| n.distance < click.ground_distance);
    Some(Under { tile: TileCoord::new(click.tile.0, click.tile.1), click, npc })
}

/// Whether the NPC is what a click should go to rather than the intercepting object behind it.
fn npc_comes_first(under: &Under) -> bool {
    match (&under.npc, under.click.object_distance) {
        (Some(npc), Some(object)) => npc.distance <= object,
        (Some(_), None) => true,
        _ => false,
    }
}

/// The option a left-click on an NPC does: "Attack" when it has one, else its first.
fn default_option<'a>(mut options: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let all: Vec<&str> = options.by_ref().collect();
    all.iter().copied().find(|o| o.eq_ignore_ascii_case("attack")).or_else(|| all.first().copied())
}

/// An NPC's options in menu order: the default (the left-click one) first, the rest as the cache
/// lists them.
fn ordered_options<'a>(options: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let all: Vec<&str> = options.collect();
    let Some(first) = default_option(all.iter().copied()) else { return all };
    let mut out = vec![first];
    out.extend(all.iter().copied().filter(|o| *o != first));
    out
}

/// The action of choosing `option` of an NPC.
fn npc_action(npc: &NpcHit, option: &str) -> MenuAction {
    MenuAction::Npc { id: npc.id, name: npc.name.clone(), option: option.to_owned() }
}

/// Does what choosing `action` means.
/// The options an inventory item's menu offers, top to bottom: its cache definition's, less
/// "Wield"/"Wear"/"Equip"/"Hold" when the server says it cannot be worn (it has no slot here).
fn inventory_options(icons: Option<&ItemIcons>, stack: &ItemView) -> Vec<String> {
    let mut options = icons
        .and_then(|i| i.item_named(&stack.name).map(|d| d.menu_options()))
        .unwrap_or_else(|| vec!["Use".to_owned(), "Drop".to_owned()]);
    if !stack.wearable {
        options.retain(|option| !is_wear_option(option));
    }
    options
}

/// Whether a menu option puts the item on (the words the server's inventory handling accepts).
fn is_wear_option(option: &str) -> bool {
    ["wield", "wear", "equip", "hold"].iter().any(|word| option.eq_ignore_ascii_case(word))
}

/// What choosing `option` on the inventory item in `slot` does: "Use" picks it up for a second
/// item, anything else is the server's.
fn inventory_action(slot: usize, option: String) -> MenuAction {
    if option.eq_ignore_ascii_case("use") {
        MenuAction::SelectUse { slot }
    } else {
        MenuAction::InventoryOption { slot, option }
    }
}

/// The options of the ground item `item` the server handles, in the cache's order ("Take" and
/// "Light"; the left click is the first).
fn ground_options(icons: Option<&ItemIcons>, item: &GroundItemView) -> Vec<String> {
    let listed = icons.and_then(|i| i.item_named(&item.name)).map(|d| d.ground_options.iter().flatten().cloned().collect::<Vec<_>>());
    let mut options: Vec<String> = listed
        .unwrap_or_else(|| vec!["Take".to_owned()])
        .into_iter()
        .filter(|o| o.eq_ignore_ascii_case("take") || o.eq_ignore_ascii_case("light"))
        .collect();
    if options.is_empty() {
        options.push("Take".to_owned());
    }
    options
}

/// The stack on `tile` the player would click: the one that landed last.
fn ground_item_at(snapshot: Option<&osrs_core::sim::Snapshot>, tile: TileCoord) -> Option<&GroundItemView> {
    snapshot?.ground_items.iter().rev().find(|g| g.tile == tile)
}

/// The object under a click that an item picked from the inventory would be used on: the object
/// in the way, or the named one (a fire has no option to intercept the click with). Objects a
/// player made are not on the map, so the server's list says which tile they stand on.
fn use_target(snapshot: Option<&osrs_core::sim::Snapshot>, under: &Under) -> Option<(u32, String, TileCoord)> {
    if let Some(object) = &under.click.object {
        return Some((object.id, object.name.clone(), TileCoord::new(object.tile.0, object.tile.1)));
    }
    let (id, name) = under.click.examinable.clone()?;
    let made = snapshot?.spawned.iter().filter(|s| s.loc.id == id).min_by_key(|s| {
        let (dx, dz) = (s.loc.tile.x - under.tile.x, s.loc.tile.z - under.tile.z);
        dx.abs().max(dz.abs())
    })?;
    Some((id, name, made.loc.tile))
}

/// The menu of a Combat Options button or an orb, top line first (a left click takes it): the
/// widget's own option, from the cache's `ops` ("Toggle Run", "Use" + "Special Attack").
fn widget_menu(target: UiTarget, snapshot: Option<&osrs_core::sim::Snapshot>, icons: Option<&ItemIcons>) -> Vec<MenuItem> {
    let Some(p) = snapshot.map(|s| &s.player) else { return Vec::new() };
    let plain = |option: &str, action: MenuAction| MenuItem { option: option.to_owned(), subject: None, suffix: None, action };
    let special = |p: &osrs_core::sim::PlayerSnapshot| {
        p.special_cost.map(|_| MenuItem {
            option: "Use".to_owned(),
            subject: Some(("Special Attack".to_owned(), Subject::Item)),
            suffix: None,
            action: MenuAction::SetSpecial { enabled: !p.special_enabled },
        })
    };
    match target {
        UiTarget::CombatStyle(slot) => {
            let Some(icons) = icons else { return Vec::new() };
            let Some(index) = super::panes::combat::stance_for_slot(&icons.styles, p, slot) else { return Vec::new() };
            let ty = super::panes::combat::weapon_type(&p.weapon_category);
            let name = icons.styles.of(ty).iter().find(|e| e.slot == slot).map_or("Attack style", |e| e.name.as_str());
            vec![plain(name, MenuAction::SetStance { index })]
        }
        UiTarget::SpecialBar | UiTarget::Orb(Orb::Special) => special(p).into_iter().collect(),
        UiTarget::Orb(Orb::Run) => vec![plain("Toggle Run", MenuAction::SetRunning { running: !p.is_running })],
        UiTarget::Logout => vec![plain("Logout", MenuAction::Say("Logging out is not available in this game.".to_owned()))],
        UiTarget::WorldSwitcher => vec![plain("World Switcher", MenuAction::Say("World switching is not available in this game.".to_owned()))],
        UiTarget::Spell(i) => {
            let Some(spell) = icons.and_then(|i2| i2.spells.spells.get(i)) else { return Vec::new() };
            let selected = p.spell.as_deref() == Some(spell.server_id().as_str());
            let known = icons.is_some_and(|i2| i2.spells.known.get(i).copied().unwrap_or(false));
            let (option, action) = match (known, selected) {
                (true, false) => ("Cast", MenuAction::SelectSpell { spell: Some(spell.server_id()) }),
                (true, true) => ("Deselect", MenuAction::SelectSpell { spell: None }),
                (false, _) => ("Cast", MenuAction::Say(format!("{} is not available in this game yet.", spell.name))),
            };
            vec![MenuItem { option: option.to_owned(), subject: Some((spell.name.clone(), Subject::Spell)), suffix: None, action }]
        }
        // quick prayers are not in the game yet
        _ => Vec::new(),
    }
}

fn perform(server: &ServerLink, chat: &mut ChatLog, selection: &mut UseSelection, now: f32, action: &MenuAction) {
    // anything but picking an item, and the clicks that finish the use, puts the picked item down
    if !matches!(action, MenuAction::SelectUse { .. }) {
        selection.0 = None;
    }
    match action {
        MenuAction::SelectUse { slot } => selection.0 = Some(*slot),
        MenuAction::CancelUse => {}
        MenuAction::UseOnItem { used, target } => server.link.send(ClientCommand::UseItemOnItem { used: *used, target: *target }),
        MenuAction::UseOnGround { slot, uid } => server.link.send(ClientCommand::UseItemOnGroundItem { slot: *slot, uid: *uid }),
        MenuAction::UseOnObject { slot, id, tile } => {
            server.link.send(ClientCommand::UseItemOnObject { slot: *slot, id: *id, tile: *tile })
        }
        MenuAction::GroundOption { uid, option } => {
            server.link.send(ClientCommand::GroundItemOption { uid: *uid, option: option.clone() })
        }
        MenuAction::Walk(tile) => request_walk(server, *tile, false),
        // the server finds the object from its id and a tile it covers, and decides what happens
        MenuAction::Object(PickedObject { id, name, action, tile }) => {
            println!("[client] {action} {name} (object {id} at {}, {})", tile.0, tile.1);
            server.link.send(ClientCommand::InteractObject {
                id: *id,
                tile: TileCoord::new(tile.0, tile.1),
                action: action.clone(),
            });
        }
        MenuAction::Examine { id } => server.link.send(ClientCommand::ExamineObject { id: *id }),
        MenuAction::ExamineNpc { id } => server.link.send(ClientCommand::ExamineNpc { target: *id }),
        MenuAction::InventoryOption { slot, option } => {
            server.link.send(ClientCommand::InventoryOption { slot: *slot, option: option.clone() })
        }
        MenuAction::Unequip { slot } => server.link.send(ClientCommand::Unequip { slot: *slot }),
        MenuAction::ExamineItem { item } => server.link.send(ClientCommand::ExamineItem { item: item.clone() }),
        MenuAction::TogglePrayer { prayer } => server.link.send(ClientCommand::TogglePrayer { prayer: (*prayer).to_owned() }),
        MenuAction::SetStance { index } => server.link.send(ClientCommand::SetStance { index: *index }),
        MenuAction::SetSpecial { enabled } => server.link.send(ClientCommand::SetSpecial { enabled: *enabled }),
        MenuAction::SetRunning { running } => server.link.send(ClientCommand::SetRunning(*running)),
        MenuAction::SelectSpell { spell } => server.link.send(ClientCommand::SelectSpell { spell: spell.clone() }),
        MenuAction::Say(text) => chat.push(text.clone(), now),
        MenuAction::Npc { id, option, .. } if option.eq_ignore_ascii_case("attack") => {
            println!("[client] attack entity {}", id.0);
            server.link.send(ClientCommand::Attack { target: *id });
        }
        // anything else: walk up to it and face it (there is nothing to do there yet)
        MenuAction::Npc { id, name, option } => {
            println!("[client] {option} {name} (entity {}): walking up to it", id.0);
            server.link.send(ClientCommand::InteractNpc { target: *id });
        }
    }
}

/// Parameters of `ground_clicks` that did not fit among its sixteen.
#[derive(bevy::ecs::system::SystemParam)]
struct Extras<'w> {
    levels: Res<'w, PrayerLevels>,
    chat: ResMut<'w, ChatLog>,
    time: Res<'w, Time>,
    use_selection: ResMut<'w, UseSelection>,
}

fn ground_clicks(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<OrbitCamera>>,
    npcs: Query<(&NpcView, &Transform)>,
    world: Res<StreamedWorld>,
    picker: Res<GroundPicker>,
    server: NonSend<ServerLink>,
    mut right: ResMut<RightPress>,
    mut menu: ResMut<ContextMenu>,
    mut interface: ResMut<Interface>,
    ui: Res<UiHits>,
    mut orbits: Query<&mut OrbitCamera>,
    icons: Option<NonSend<ItemIcons>>,
    mut extra: Extras,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let now = extra.time.elapsed_secs();
    // an item that is gone (dropped, burnt) cannot stay picked
    if let Some(slot) = extra.use_selection.0 {
        let held = server.latest.as_ref().is_some_and(|s| s.player.inventory.get(slot).is_some_and(Option::is_some));
        if !held {
            extra.use_selection.0 = None;
        }
    }
    let prayer_levels = &extra.levels;
    let Ok(window) = windows.single() else { return };
    let Ok((camera, camera_transform)) = cameras.single() else { return };

    if buttons.just_pressed(MouseButton::Left) {
        let Some(cursor) = window.cursor_position() else { return };
        // While a menu is open the click belongs to it: take the entry, or just dismiss it.
        if let Some(open) = menu.open.take() {
            commands.entity(open.root).despawn();
            if let Some(index) = open.entry_at(cursor) {
                perform(&server, &mut extra.chat, &mut extra.use_selection, now, &open.entries[index].action);
            }
            return;
        }
        // the interface takes its own clicks: tabs open, items do their first option
        if let Some(target) = ui.target_at(cursor) {
            let snapshot = server.latest.as_ref();
            match target {
                UiTarget::Tab(index) => interface.click_tab(index),
                UiTarget::Minimap { x, z } => request_walk(&server, TileCoord::new(x, z), false),
                // the compass turns the camera back to face north
                UiTarget::Compass => {
                    for mut orbit in &mut orbits {
                        orbit.yaw = 0.0;
                    }
                }
                UiTarget::InventorySlot(slot) => {
                    let stack = snapshot.and_then(|s| s.player.inventory.get(slot)).and_then(Option::as_ref);
                    if let Some(stack) = stack {
                        // the left click is the menu's top option
                        let option = inventory_options(icons.as_deref(), stack).into_iter().next().unwrap_or_else(|| "Use".to_owned());
                        let action = match extra.use_selection.0 {
                            Some(used) if used == slot => MenuAction::CancelUse,
                            Some(used) => MenuAction::UseOnItem { used, target: slot },
                            None => inventory_action(slot, option),
                        };
                        perform(&server, &mut extra.chat, &mut extra.use_selection, now, &action);
                    }
                }
                UiTarget::EquipmentSlot(slot) => {
                    if snapshot.is_some_and(|s| s.player.equipped.iter().any(|(worn, _)| *worn == slot)) {
                        perform(&server, &mut extra.chat, &mut extra.use_selection, now, &MenuAction::Unequip { slot });
                    }
                }
                // a prayer the server does not have yet does nothing; the server answers a level
                // that is too low itself
                UiTarget::Prayer(index) => {
                    if prayer_levels.0[index].is_some() {
                        perform(&server, &mut extra.chat, &mut extra.use_selection, now, &MenuAction::TogglePrayer { prayer: PRAYER_BOOK[index].id });
                    }
                }
                // the Combat Options buttons and the orbs: the top line of their menu
                UiTarget::CombatStyle(_) | UiTarget::SpecialBar | UiTarget::Orb(_) | UiTarget::Spell(_) | UiTarget::Logout | UiTarget::WorldSwitcher => {
                    if let Some(first) = widget_menu(target, snapshot, icons.as_deref()).into_iter().next() {
                        perform(&server, &mut extra.chat, &mut extra.use_selection, now, &first.action);
                    }
                }
                UiTarget::AutoRetaliate | UiTarget::Background => {}
            }
            return;
        }
        // the nearest NPC or object in the way with a left-click option, else walk to where the
        // path ends
        if let Some(under) = pick_at(cursor, camera, camera_transform, &world, &picker.0, &npcs) {
            // shift-click only walks, whatever is under the cursor; ctrl-click flips run/walk
            let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
            let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
            if shift {
                extra.use_selection.0 = None;
                request_walk(&server, under.tile, ctrl);
                return;
            }
            let first_option = under.npc.as_ref().and_then(|n| default_option(n.options()).map(str::to_owned));
            let ground = ground_item_at(server.latest.as_ref(), under.tile)
                .filter(|_| under.click.object.is_none() && !npc_comes_first(&under));
            if let Some(item) = ground {
                let action = match extra.use_selection.0 {
                    Some(slot) => MenuAction::UseOnGround { slot, uid: item.uid },
                    None => {
                        let option = ground_options(icons.as_deref(), item).remove(0);
                        MenuAction::GroundOption { uid: item.uid, option }
                    }
                };
                perform(&server, &mut extra.chat, &mut extra.use_selection, now, &action);
                return;
            }
            // a picked item used on an object
            if let (Some(slot), Some((id, _, tile))) = (extra.use_selection.0, use_target(server.latest.as_ref(), &under)) {
                perform(&server, &mut extra.chat, &mut extra.use_selection, now, &MenuAction::UseOnObject { slot, id, tile });
                return;
            }
            extra.use_selection.0 = None; // a click on the world puts the picked item down
            match (npc_comes_first(&under), first_option, under.click.object) {
                (true, Some(option), _) => {
                    perform(&server, &mut extra.chat, &mut extra.use_selection, now, &npc_action(under.npc.as_ref().expect("an NPC came first"), &option))
                }
                (_, _, Some(object)) => perform(&server, &mut extra.chat, &mut extra.use_selection, now, &MenuAction::Object(object)),
                _ => request_walk(&server, under.tile, ctrl),
            }
        }
    }

    if buttons.just_pressed(MouseButton::Right) {
        right.at = window.cursor_position();
    }

    if buttons.just_released(MouseButton::Right) {
        let press = right.at.take();
        // the right button no longer orbits the camera, so any press-and-release opens the menu
        if let Some(cursor) = press {
            if let Some(old) = menu.open.take() {
                commands.entity(old.root).despawn();
            }
            // the interface's own menus: an item's options, then Examine
            if let Some(target) = ui.target_at(cursor) {
                let snapshot = server.latest.as_ref();
                let item_line = |option: String, name: &str, action: MenuAction| MenuItem {
                    option,
                    subject: Some((name.to_owned(), Subject::Item)),
                    suffix: None,
                    action,
                };
                let mut items: Vec<MenuItem> = Vec::new();
                match target {
                    UiTarget::InventorySlot(slot) => {
                        let stack = snapshot.and_then(|s| s.player.inventory.get(slot)).and_then(Option::as_ref);
                        if let Some(stack) = stack {
                            let picked = extra.use_selection.0.filter(|used| *used != slot);
                            let picked_name = picked
                                .and_then(|used| snapshot.and_then(|s| s.player.inventory.get(used)).and_then(Option::as_ref))
                                .map(|used| used.name.clone());
                            if let (Some(used), Some(picked_name)) = (picked, picked_name) {
                                // "Use Tinderbox -> Logs" comes first while an item is picked
                                let action = MenuAction::UseOnItem { used, target: slot };
                                items.push(item_line("Use".to_owned(), &format!("{picked_name} -> {}", stack.name), action));
                            } else {
                                for option in inventory_options(icons.as_deref(), stack) {
                                    let action = inventory_action(slot, option.clone());
                                    items.push(item_line(option, &stack.name, action));
                                }
                            }
                            items.push(item_line(
                                "Examine".to_owned(),
                                &stack.name,
                                MenuAction::ExamineItem { item: stack.id.clone() },
                            ));
                            if extra.use_selection.0.is_some() {
                                items.push(MenuItem { option: "Cancel".to_owned(), subject: None, suffix: None, action: MenuAction::CancelUse });
                            }
                        }
                    }
                    UiTarget::Prayer(index) if prayer_levels.0[index].is_some() => {
                        let book = &PRAYER_BOOK[index];
                        let on = snapshot.is_some_and(|s| s.player.active_prayers.iter().any(|id| id == book.id));
                        let option = if on { "Deactivate" } else { "Activate" };
                        items.push(item_line(option.to_owned(), book.name, MenuAction::TogglePrayer { prayer: book.id }));
                    }
                    UiTarget::EquipmentSlot(slot) => {
                        let worn = snapshot.and_then(|s| s.player.equipped.iter().find(|(s, _)| *s == slot));
                        if let Some((_, item)) = worn {
                            items.push(item_line("Remove".to_owned(), &item.name, MenuAction::Unequip { slot }));
                            items.push(item_line(
                                "Examine".to_owned(),
                                &item.name,
                                MenuAction::ExamineItem { item: item.id.clone() },
                            ));
                        }
                    }
                    UiTarget::CombatStyle(_) | UiTarget::SpecialBar | UiTarget::Orb(_) | UiTarget::Spell(_) | UiTarget::Logout | UiTarget::WorldSwitcher => {
                        items.extend(widget_menu(target, snapshot, icons.as_deref()));
                    }
                    _ => {}
                }
                if !items.is_empty() {
                    menu.open = Some(spawn_menu(&mut commands, window, cursor, items));
                }
                return;
            }
            if let Some(under) = pick_at(cursor, camera, camera_transform, &world, &picker.0, &npcs) {
                let player_level = server.latest.as_ref().map_or(3, |s| s.player.combat_level);
                // an NPC's options and Examine
                let mut npc_items: Vec<MenuItem> = Vec::new();
                if let Some(npc) = &under.npc {
                    let suffix = npc
                        .combat_level
                        .map(|l| (format!(" (level-{l})"), level_colour(l as i32, player_level)));
                    let item = |option: &str, action: MenuAction| MenuItem {
                        option: option.to_owned(),
                        subject: Some((npc.name.clone(), Subject::Npc)),
                        suffix: suffix.clone(),
                        action,
                    };
                    for option in ordered_options(npc.options()) {
                        npc_items.push(item(option, npc_action(npc, option)));
                    }
                    npc_items.push(item("Examine", MenuAction::ExamineNpc { id: npc.id }));
                }
                // an object's option, then Examine
                let mut object_items: Vec<MenuItem> = Vec::new();
                // a picked item can be used on the object first
                if let Some(slot) = extra.use_selection.0 {
                    let snapshot = server.latest.as_ref();
                    let used = snapshot.and_then(|s| s.player.inventory.get(slot)).and_then(Option::as_ref);
                    if let (Some(used), Some((id, name, tile))) = (used, use_target(snapshot, &under)) {
                        object_items.push(MenuItem {
                            option: "Use".to_owned(),
                            subject: Some((format!("{} -> {name}", used.name), Subject::Item)),
                            suffix: None,
                            action: MenuAction::UseOnObject { slot, id, tile },
                        });
                    }
                }
                if let Some(object) = under.click.object.clone() {
                    object_items.push(MenuItem {
                        option: object.action.clone(),
                        subject: Some((object.name.clone(), Subject::Object)),
                        suffix: None,
                        action: MenuAction::Object(object),
                    });
                }
                if let Some((id, name)) = under.click.examinable.clone() {
                    object_items.push(MenuItem {
                        option: "Examine".to_owned(),
                        subject: Some((name, Subject::Object)),
                        suffix: None,
                        action: MenuAction::Examine { id },
                    });
                }
                // the item lying on the tile
                let mut ground_items: Vec<MenuItem> = Vec::new();
                let ground = ground_item_at(server.latest.as_ref(), under.tile).filter(|_| under.click.object.is_none());
                if let Some(item) = ground {
                    let line = |option: String, action: MenuAction| MenuItem {
                        option,
                        subject: Some((item.name.clone(), Subject::Item)),
                        suffix: None,
                        action,
                    };
                    if let Some(slot) = extra.use_selection.0 {
                        let used = server.latest.as_ref().and_then(|s| s.player.inventory.get(slot)).and_then(Option::as_ref);
                        let used = used.map_or_else(String::new, |u| u.name.clone());
                        ground_items.push(MenuItem {
                            option: "Use".to_owned(),
                            subject: Some((format!("{used} -> {}", item.name), Subject::Item)),
                            suffix: None,
                            action: MenuAction::UseOnGround { slot, uid: item.uid },
                        });
                    } else {
                        for option in ground_options(icons.as_deref(), item) {
                            ground_items.push(line(option.clone(), MenuAction::GroundOption { uid: item.uid, option }));
                        }
                    }
                    ground_items.push(line("Examine".to_owned(), MenuAction::ExamineItem { item: item.item.clone() }));
                }
                // whichever is nearer first, and "Walk here" always last as the fallback
                let mut items: Vec<MenuItem> = Vec::new();
                items.extend(ground_items);
                if npc_comes_first(&under) || under.click.object.is_none() {
                    items.extend(npc_items);
                    items.extend(object_items);
                } else {
                    items.extend(object_items);
                    items.extend(npc_items);
                }
                items.push(MenuItem {
                    option: "Walk here".to_owned(),
                    subject: None,
                    suffix: None,
                    action: MenuAction::Walk(under.tile),
                });
                if extra.use_selection.0.is_some() {
                    items.push(MenuItem { option: "Cancel".to_owned(), subject: None, suffix: None, action: MenuAction::CancelUse });
                }
                menu.open = Some(spawn_menu(&mut commands, window, cursor, items));
            }
        }
    }
}

fn spawn_menu(commands: &mut Commands, window: &Window, cursor: Vec2, items: Vec<MenuItem>) -> OpenMenu {
    let width = menu_width(&items);
    let size = Vec2::new(width, HEADER_HEIGHT + ENTRY_HEIGHT * items.len() as f32);
    let top_left = cursor.min(Vec2::new(window.width(), window.height()) - size).max(Vec2::ZERO);

    let font = || TextFont { font_size: bevy::text::FontSize::Px(FONT_PX), ..default() };
    let label = |text: &str, color: Color| (Text::new(text.to_owned()), font(), TextColor(color));
    let row = |height: f32, background: Color| {
        (
            Node {
                height: Val::Px(height),
                padding: UiRect::horizontal(Val::Px(ROW_PADDING)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(background),
        )
    };

    let mut entries = Vec::new();
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(top_left.x),
                top: Val::Px(top_left.y),
                width: Val::Px(width),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(MENU_ENTRY),
            GlobalZIndex(100),
        ))
        .with_children(|menu| {
            menu.spawn(row(HEADER_HEIGHT, Color::BLACK)).with_children(|header| {
                header.spawn(label("Choose Option", Color::srgb(0.78, 0.74, 0.6)));
            });
            for item in items {
                let row = menu
                    .spawn(row(ENTRY_HEIGHT, MENU_ENTRY))
                    .with_children(|e| {
                        // the option in white, then what it acts on in that kind of thing's colour
                        let mut text = e.spawn(label(&item.option, Color::WHITE));
                        if let Some((name, subject)) = &item.subject {
                            text.with_child((TextSpan::new(format!(" {name}")), font(), TextColor(subject.colour())));
                        }
                        if let Some((text_after, colour)) = &item.suffix {
                            text.with_child((TextSpan::new(text_after.clone()), font(), TextColor(*colour)));
                        }
                    })
                    .id();
                entries.push(MenuEntry { row, action: item.action });
            }
        })
        .id();
    OpenMenu { root, entries, top_left, width }
}

const MENU_ENTRY: Color = Color::srgb(0.36, 0.33, 0.27);
const MENU_ENTRY_HOVER: Color = Color::srgb(0.5, 0.45, 0.3);

/// The entry under the cursor lights up.
/// Closes the menu once the cursor wanders away from it, as the game does.
fn close_menu_when_left(mut commands: Commands, mut menu: ResMut<ContextMenu>, windows: Query<&Window, With<PrimaryWindow>>) {
    let Ok(window) = windows.single() else { return };
    let left = match (&menu.open, window.cursor_position()) {
        (Some(open), Some(cursor)) => !open.keeps_open(cursor),
        // the cursor left the window altogether
        (Some(_), None) => true,
        (None, _) => false,
    };
    if left {
        if let Some(open) = menu.open.take() {
            commands.entity(open.root).despawn();
        }
    }
}

fn highlight_menu_entry(
    menu: Res<ContextMenu>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut backgrounds: Query<&mut BackgroundColor>,
) {
    let Some(open) = &menu.open else { return };
    let Ok(window) = windows.single() else { return };
    let hovered = window.cursor_position().and_then(|c| open.entry_at(c));
    for (index, entry) in open.entries.iter().enumerate() {
        if let Ok(mut background) = backgrounds.get_mut(entry.row) {
            background.0 = if hovered == Some(index) { MENU_ENTRY_HOVER } else { MENU_ENTRY };
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_item_that_cannot_be_worn_offers_no_wear_options() {
        assert!(is_wear_option("Wield") && is_wear_option("wear") && is_wear_option("Hold"));
        assert!(!is_wear_option("Drink") && !is_wear_option("Drop"));
        let vial = ItemView { id: "vial".into(), name: "Vial".into(), quantity: 1, wearable: false };
        assert!(inventory_options(None, &vial).iter().all(|o| !is_wear_option(o)));
    }

    use super::*;

    #[test]
    fn the_menu_stays_open_near_it_and_closes_once_the_cursor_strays() {
        let open = OpenMenu { root: Entity::PLACEHOLDER, entries: Vec::new(), top_left: Vec2::new(100.0, 100.0), width: 120.0 };
        // header only (no entries): 120 x 20 at (100, 100)
        assert!(open.keeps_open(Vec2::new(150.0, 110.0)));
        assert!(open.keeps_open(Vec2::new(100.0 - MENU_LEAVE_MARGIN + 1.0, 110.0)));
        assert!(!open.keeps_open(Vec2::new(100.0 - MENU_LEAVE_MARGIN - 1.0, 110.0)));
        assert!(!open.keeps_open(Vec2::new(150.0, 120.0 + MENU_LEAVE_MARGIN + 1.0)));
    }

    #[test]
    fn the_menu_grows_to_fit_its_longest_line_and_never_shrinks_below_the_minimum() {
        let item = |option: &str, name: Option<&str>, suffix: Option<&str>| MenuItem {
            option: option.to_owned(),
            subject: name.map(|n| (n.to_owned(), Subject::Object)),
            suffix: suffix.map(|s| (s.to_owned(), Color::WHITE)),
            action: MenuAction::Walk(TileCoord::new(0, 0)),
        };
        assert_eq!(menu_width(&[item("Walk here", None, None)]), MIN_MENU_WIDTH);

        let long = item("Pickpocket", Some("Man"), Some(" (level-2)"));
        let width = menu_width(&[item("Walk here", None, None), long]);
        // "Pickpocket Man (level-2)" is 24 characters
        assert!((width - (24.0 * CHARACTER_WIDTH + 2.0 * ROW_PADDING)).abs() < 1e-3, "{width}");
        assert!(width > MIN_MENU_WIDTH);
    }

    #[test]
    fn an_npcs_level_is_coloured_by_how_it_compares_with_the_player() {
        let rgb = |c: Color| {
            let c = c.to_srgba();
            ((c.red * 255.0).round() as u8, (c.green * 255.0).round() as u8, (c.blue * 255.0).round() as u8)
        };
        assert_eq!(rgb(level_colour(2, 3)), (0xC0, 0xFF, 0x00), "a little below: yellow-green");
        assert_eq!(rgb(level_colour(30, 30)), (0xFF, 0xFF, 0x00), "equal: yellow");
        assert_eq!(rgb(level_colour(2, 60)), (0x00, 0xFF, 0x00), "far below: green");
        assert_eq!(rgb(level_colour(120, 3)), (0xFF, 0x00, 0x00), "far above: red");
    }

    #[test]
    fn an_npcs_attack_option_is_its_left_click_and_leads_its_menu() {
        let options = ["Talk-to", "Attack", "Pickpocket"];
        assert_eq!(default_option(options.into_iter()), Some("Attack"));
        assert_eq!(ordered_options(options.into_iter()), vec!["Attack", "Talk-to", "Pickpocket"]);
        // without one, the first stays the default and the order is the cache's
        let peaceful = ["Talk-to", "Pickpocket"];
        assert_eq!(default_option(peaceful.into_iter()), Some("Talk-to"));
        assert_eq!(ordered_options(peaceful.into_iter()), vec!["Talk-to", "Pickpocket"]);
    }

    #[test]
    fn objects_are_named_in_blue_and_npcs_in_yellow() {
        let rgb = |c: Color| {
            let c = c.to_srgba();
            (c.red, c.green, c.blue)
        };
        let (r, g, b) = rgb(Subject::Object.colour());
        assert!(b > 0.9 && r < 0.1, "objects are blue-ish: {r} {g} {b}");
        assert_eq!(rgb(Subject::Npc.colour()), (1.0, 1.0, 0.0));
    }
}
