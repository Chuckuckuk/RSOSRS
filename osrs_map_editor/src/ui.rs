//! The editor's panels: menus, tools and palette on the left, what is selected and the minimap on
//! the right, a status line at the bottom, and labels over NPCs and icons in the world.

use crate::camera::{jump_to, FlyCamera};
use crate::catalog::{area_label, kind_name, star, Catalog, Search, FACINGS, KINDS};
use crate::editor::{default_spawns, world_tile, Brush, Editor, Selected, Tool};
use crate::preview::{MinimapPreview, Sprites};
use crate::tools::{remove, IconsInView};
use crate::world::{Focus, Hover};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use osrs_cache::active::{self, Selection};
use osrs_cache::squares::SQUARE;
use osrs_map_editor::builds::{self, CacheInfo, Manifest};
use osrs_map_editor::compile::{self, Stage};
use osrs_map_editor::project::MapProject;
use osrs_render::streaming::{square_of, StreamedWorld};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteTab {
    Objects,
    Resources,
    Npcs,
    Icons,
}

/// Where the game starts when it is played: a tile, or its usual spot.
type Spawn = Option<(i32, i32)>;

/// A compile running on its own thread.
pub struct CompileJob {
    pub name: String,
    pub stage: Stage,
    messages: Mutex<Receiver<JobMessage>>,
    /// Start the game on the cache once it is built.
    then_play: Option<Spawn>,
}

enum JobMessage {
    Stage(Stage),
    Done(Result<Manifest, String>),
}

/// The one dialog open, if any.
pub enum Dialog {
    NewProject(String),
    /// Compile into a cache of this name; with `then_play`, start the game on it once built (Play
    /// on a project that has no cache yet).
    Compile { name: String, then_play: Option<Spawn> },
    /// Play on a cache older than the project's edits: rebuild it first, or play it as it is.
    Rebuild { name: String, spawn: Spawn },
}

#[derive(Resource)]
pub struct UiState {
    pub tab: PaletteTab,
    pub search: String,
    pub goto: String,
    pub dialog: Option<Dialog>,
    pub job: Option<CompileJob>,
    /// The last compile's outcome, with the cache it made.
    pub compiled: Option<Result<Manifest, String>>,
    /// Play was asked for (menu or F5); handled once the camera is at hand.
    pub play_requested: bool,
    /// Whether the game starts where the camera is looking (else at its usual spot).
    pub play_here: bool,
    /// The NPC palette lists only the NPCs with combat stats.
    pub npcs_with_stats_only: bool,
    /// The NPC palette row whose models are being picked from.
    pub npc_picker: Option<usize>,
    /// The palette rows matching the search, for the tab, search and filter they were found for.
    rows: Option<((PaletteTab, String, bool), Vec<usize>)>,
    /// Counts the times the built caches or the active one changed (a compile finished, a cache
    /// was chosen), so what is read of them from disk is read again only then.
    builds_epoch: u32,
    /// The built caches and the active choice, as of `builds_epoch`.
    caches: Option<(u32, Selection, Vec<CacheInfo>)>,
    /// The project's built cache and whether it has all the edits, as of an editor generation and
    /// `builds_epoch`.
    play_status: Option<((u64, u32), Option<PathBuf>, bool)>,
    /// The world tile the mouse is resting on, and since when (egui time, seconds).
    hovered: Option<((i32, i32), f64)>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            tab: PaletteTab::Objects,
            search: String::new(),
            goto: String::new(),
            dialog: None,
            job: None,
            compiled: None,
            play_requested: false,
            play_here: true,
            npcs_with_stats_only: false,
            npc_picker: None,
            rows: None,
            builds_epoch: 0,
            caches: None,
            play_status: None,
            hovered: None,
        }
    }
}

impl UiState {
    /// The project's built cache, and whether it holds the project as it is now.
    fn play_status(&mut self, editor: &Editor) -> (Option<PathBuf>, bool) {
        let key = (editor.generation, self.builds_epoch);
        if self.play_status.as_ref().is_none_or(|(k, _, _)| *k != key) {
            let dir = editor.project.compiled_cache();
            let current = dir.as_deref().is_some_and(|d| editor.project.is_compiled_into(d));
            self.play_status = Some((key, dir, current));
        }
        let (_, dir, current) = self.play_status.as_ref().unwrap();
        (dir.clone(), *current)
    }

    /// The active choice and the built caches.
    fn caches(&mut self) -> (Selection, Vec<CacheInfo>) {
        if self.caches.as_ref().is_none_or(|(epoch, _, _)| *epoch != self.builds_epoch) {
            self.caches = Some((self.builds_epoch, active::selection(), builds::list_caches()));
        }
        let (_, selection, caches) = self.caches.as_ref().unwrap();
        (selection.clone(), caches.clone())
    }
}

#[derive(SystemParam)]
pub struct View<'w, 's> {
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform, &'static mut Transform, &'static mut FlyCamera)>,
    world: Res<'w, StreamedWorld>,
    focus: Res<'w, Focus>,
    hover: Res<'w, Hover>,
    icons: Res<'w, IconsInView>,
}

pub fn editor_ui(
    mut contexts: EguiContexts,
    mut editor: ResMut<Editor>,
    catalog: Res<Catalog>,
    mut state: ResMut<UiState>,
    mut minimap: ResMut<MinimapPreview>,
    mut sprites: ResMut<Sprites>,
    mut view: View,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    poll_job(&mut state, &mut editor);

    let mut root = egui::Ui::new(
        ctx.clone(),
        "editor_root".into(),
        egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()),
    );

    egui::Panel::top("menu").show(&mut root, |ui| menu_bar(ui, &mut editor, &mut state, &mut minimap));
    egui::Panel::bottom("status").show(&mut root, |ui| status_bar(ui, &editor, &view));
    egui::Panel::left("palette").resizable(true).default_size(300.0).show(&mut root, |ui| {
        tools_panel(ui, &mut editor);
        ui.separator();
        palette(ui, &mut editor, &catalog, &mut state, &mut sprites);
    });
    egui::Panel::right("inspector").resizable(true).default_size(280.0).show(&mut root, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            inspector(ui, &mut editor, &catalog);
            ui.separator();
            if minimap.open {
                minimap_panel(ui, &ctx, &mut editor, &catalog, &mut minimap, &mut sprites, &view);
                ui.separator();
            }
            project_panel(ui, &mut editor, &catalog, &mut view, &mut state.goto);
        });
    });

    // what the panels leave is the world: labels are drawn there only
    let painter = root.painter_at(root.available_rect_before_wrap());
    world_labels(&ctx, &painter, &editor, &catalog, &mut sprites, &view);
    hover_tooltip(&ctx, &mut editor, &catalog, &mut state, &view);
    if std::mem::take(&mut state.play_requested) {
        let spawn = state.play_here.then_some(view.focus.tile);
        request_play(&mut editor, &mut state, spawn);
    }
    dialogs(&ctx, &mut editor, &mut state);
    Ok(())
}

/// Plays the project: on its cache if it has one, else after asking to make one.
fn request_play(editor: &mut Editor, state: &mut UiState, spawn: Spawn) {
    if state.job.is_some() {
        editor.status = "A compile is running; play when it is done".into();
        return;
    }
    if editor.dirty {
        editor.save();
    }
    match state.play_status(editor) {
        (Some(dir), current) => {
            // a project found by its manifest names its cache from now on
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
            if editor.project.cache != name {
                editor.project.cache = name.clone();
                editor.save();
            }
            if current {
                launch(editor, &dir, spawn);
            } else {
                // edits made since it was built: offer to build it again first
                state.dialog = Some(Dialog::Rebuild { name: name.unwrap_or_default(), spawn });
            }
        }
        (None, _) => {
            let name = editor.project.cache.clone().unwrap_or_else(|| clean_name(&editor.project.name));
            state.dialog = Some(Dialog::Compile { name, then_play: Some(spawn) });
        }
    }
}

fn launch(editor: &mut Editor, dir: &std::path::Path, spawn: Spawn) {
    let name = dir.file_name().unwrap_or_default().to_string_lossy();
    editor.status = match crate::play::launch(dir, spawn) {
        Ok(_) => match spawn {
            Some((x, z)) => format!("Starting the game on cache {name} at {x}, {z}"),
            None => format!("Starting the game on cache {name}"),
        },
        Err(e) => e,
    };
}

fn menu_bar(ui: &mut egui::Ui, editor: &mut Editor, state: &mut UiState, minimap: &mut MinimapPreview) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("New project…").clicked() {
                state.dialog = Some(Dialog::NewProject(String::new()));
                ui.close();
            }
            ui.menu_button("Open project", |ui| {
                let projects = list_projects();
                if projects.is_empty() {
                    ui.label("No projects yet");
                }
                for (name, path) in projects {
                    if ui.button(&name).clicked() {
                        match MapProject::load(&path) {
                            Ok(project) => {
                                editor.switch_project(project, path);
                                editor.status = format!("Opened {name}");
                            }
                            Err(e) => editor.status = format!("could not open {name}: {e}"),
                        }
                        ui.close();
                    }
                }
            });
            if ui.button("Save   Ctrl+S").clicked() {
                editor.save();
                ui.close();
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui.add_enabled(editor.can_undo(), egui::Button::new("Undo   Ctrl+Z")).clicked() {
                editor.undo();
                ui.close();
            }
            if ui.add_enabled(editor.can_redo(), egui::Button::new("Redo   Ctrl+Y")).clicked() {
                editor.redo();
                ui.close();
            }
        });
        ui.menu_button("Cache", |ui| {
            if ui.add_enabled(state.job.is_none(), egui::Button::new("Compile to a new cache…")).clicked() {
                let name = editor.project.cache.clone().unwrap_or_else(|| clean_name(&editor.project.name));
                state.dialog = Some(Dialog::Compile { name, then_play: None });
                ui.close();
            }
            ui.separator();
            ui.label("The game reads:");
            let (selection, caches) = state.caches();
            let mut chosen = None;
            let mut pick = |ui: &mut egui::Ui, label: String, choice: Selection| {
                if ui.radio(selection == choice, label).clicked() {
                    chosen = Some(choice);
                }
            };
            pick(ui, "The installed game's cache (live)".into(), Selection::Live);
            for info in caches {
                let detail = info.manifest.as_ref().map(|m| format!(" (project {})", m.project)).unwrap_or_default();
                pick(ui, format!("{}{detail}", info.name), Selection::Dir(info.dir.clone()));
            }
            if let Some(choice) = chosen {
                set_active(editor, state, &choice, "The game will read that cache next time it starts".into());
            }
            if std::env::var(active::ENV_CACHE_DIR).is_ok() {
                ui.colored_label(egui::Color32::YELLOW, format!("{} is set and overrides this choice", active::ENV_CACHE_DIR));
            }
        });
        ui.menu_button("Play", |ui| {
            let (cache, current) = state.play_status(editor);
            let label = match &cache {
                Some(_) => "Play this project   F5",
                None => "Play this project (makes its cache first)   F5",
            };
            if ui.add_enabled(state.job.is_none(), egui::Button::new(label)).clicked() {
                state.play_requested = true;
                ui.close();
            }
            ui.checkbox(&mut state.play_here, "Start where the camera is looking");
            match &cache {
                Some(dir) => {
                    ui.label(format!("Plays on {}", dir.display()));
                    ui.label(if current { "It has all your edits." } else { "You have edited since it was built: Play offers to rebuild it." });
                }
                None => {
                    ui.label("This project has no cache yet.");
                }
            }
        });
        ui.menu_button("View", |ui| {
            ui.checkbox(&mut minimap.open, "Minimap preview");
        });
    });
}

/// Makes `choice` the cache the game reads, saying `done` when it worked.
fn set_active(editor: &mut Editor, state: &mut UiState, choice: &Selection, done: String) {
    state.builds_epoch += 1;
    editor.status = match active::set_active(choice) {
        Ok(()) => done,
        Err(e) => format!("could not choose the cache: {e}"),
    };
}

fn list_projects() -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(builds::projects_dir()) else { return Vec::new() };
    let mut out: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| (p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), p))
        .collect();
    out.sort();
    out
}

fn status_bar(ui: &mut egui::Ui, editor: &Editor, view: &View) {
    ui.horizontal(|ui| {
        if let Some((x, z)) = view.hover.tile() {
            let (sx, sy) = square_of(x, z);
            ui.monospace(format!("tile {x}, {z}  square {sx}_{sy}"));
            ui.separator();
        }
        let project = format!("{}{}", editor.project.name, if editor.dirty { " *" } else { "" });
        ui.label(project);
        ui.separator();
        ui.label(&editor.status);
    });
}

fn tools_panel(ui: &mut egui::Ui, editor: &mut Editor) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut editor.tool, Tool::Select, "Select (V)");
        ui.selectable_value(&mut editor.tool, Tool::Place, "Place (B)");
        ui.selectable_value(&mut editor.tool, Tool::Erase, "Erase (X)");
    });
    ui.horizontal(|ui| {
        ui.label("Plane (1-4)");
        for p in 0..4u8 {
            ui.selectable_value(&mut editor.plane, p, format!("{}", p + 1));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Rotation (R)");
        for (r, facing) in FACINGS.iter().enumerate() {
            ui.selectable_value(&mut editor.rotation, r as u8, *facing);
        }
    });
    let shown = editor.kind_override.map_or_else(|| "the object's own".to_string(), |k| format!("{k}: {}", kind_name(k)));
    egui::ComboBox::from_label("Shape").selected_text(shown).show_ui(ui, |ui| {
        ui.selectable_value(&mut editor.kind_override, None, "the object's own");
        for (k, name) in KINDS {
            ui.selectable_value(&mut editor.kind_override, Some(k), format!("{k}: {name}"));
        }
    });
}

/// The rows of the Objects or NPCs tab that match the search: worked out again only when the
/// tab, search or filter change (the lists are tens of thousands long).
fn palette_rows<'a>(state: &'a mut UiState, catalog: &Catalog) -> &'a [usize] {
    let key = (state.tab, state.search.clone(), state.npcs_with_stats_only);
    if state.rows.as_ref().is_none_or(|(k, _)| *k != key) {
        let search = Search::new(&state.search);
        let rows = match state.tab {
            PaletteTab::Objects => {
                (0..catalog.objects.len()).filter(|&i| search.matches(&catalog.objects[i].name_lower, Some(catalog.objects[i].id))).collect()
            }
            // a group matches by its name, or by the cache id of any of its models
            PaletteTab::Npcs => (0..catalog.npc_groups.len())
                .filter(|&g| !state.npcs_with_stats_only || catalog.npc_groups[g].has_stats)
                .filter(|&g| {
                    search.matches(&catalog.npc_groups[g].name_lower, None) || catalog.group_members(g).any(|n| search.matches("", n.cache_id))
                })
                .collect(),
            PaletteTab::Resources | PaletteTab::Icons => Vec::new(),
        };
        state.rows = Some((key, rows));
    }
    &state.rows.as_ref().unwrap().1
}

fn palette(ui: &mut egui::Ui, editor: &mut Editor, catalog: &Catalog, state: &mut UiState, sprites: &mut Sprites) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.tab, PaletteTab::Objects, "Objects");
        ui.selectable_value(&mut state.tab, PaletteTab::Resources, "Resources");
        ui.selectable_value(&mut state.tab, PaletteTab::Npcs, "NPCs");
        ui.selectable_value(&mut state.tab, PaletteTab::Icons, "Map icons");
    });
    ui.horizontal(|ui| {
        ui.label("Search");
        ui.text_edit_singleline(&mut state.search);
    });
    let search = Search::new(&state.search);
    let choose = |editor: &mut Editor, brush: Brush| {
        editor.brush = Some(brush);
        editor.tool = Tool::Place;
    };
    let row_height = ui.spacing().interact_size.y;
    match state.tab {
        PaletteTab::Objects => {
            let rows = palette_rows(state, catalog);
            ui.label(format!("{} objects", rows.len()));
            egui::ScrollArea::vertical().id_salt("objects").show_rows(ui, row_height, rows.len(), |ui, range| {
                for &i in &rows[range] {
                    let o = &catalog.objects[i];
                    let chosen = editor.brush == Some(Brush::Object(o.id));
                    if ui.selectable_label(chosen, format!("{}  {}  [{}]", o.id, o.name, kind_name(o.kind))).clicked() {
                        choose(editor, Brush::Object(o.id));
                    }
                }
            });
        }
        PaletteTab::Resources => {
            ui.label("Objects the game can gather from (data/gathering.json)");
            egui::ScrollArea::vertical().id_salt("resources").show(ui, |ui| {
                for (&id, node) in &catalog.resources {
                    let name = catalog.object_name(id);
                    if !search.matches(&name.to_lowercase(), Some(id)) && !search.matches(node, None) {
                        continue;
                    }
                    let chosen = editor.brush == Some(Brush::Object(id));
                    if ui.selectable_label(chosen, format!("{id}  {name}  ({node})")).clicked() {
                        choose(editor, Brush::Object(id));
                    }
                }
            });
        }
        PaletteTab::Npcs => {
            ui.checkbox(&mut state.npcs_with_stats_only, "Only NPCs with combat stats (\u{2605})");
            let mut open_picker = None;
            let rows = palette_rows(state, catalog);
            ui.label(format!("{} NPCs; \u{2605} = has combat stats (data/npcs.json), others stand and cannot be fought", rows.len()));
            egui::ScrollArea::vertical().id_salt("npcs").show_rows(ui, row_height, rows.len(), |ui, range| {
                for &g in &rows[range] {
                    let group = &catalog.npc_groups[g];
                    let chosen = matches!(&editor.brush, Some(Brush::Npc(key)) if catalog.group_members(g).any(|n| &n.key == key));
                    let count = if group.members.len() > 1 { format!("  \u{00d7}{}", group.members.len()) } else { String::new() };
                    if ui.selectable_label(chosen, format!("{}{count}{}", group.label(), star(group.has_stats))).clicked() {
                        match &group.members[..] {
                            // one model: nothing to pick
                            [only] => choose(editor, Brush::Npc(catalog.npcs[*only].key.clone())),
                            _ => open_picker = Some(g),
                        }
                    }
                }
            });
            if open_picker.is_some() {
                state.npc_picker = open_picker;
            }
        }
        PaletteTab::Icons => {
            ui.label("Minimap icons: placed as an invisible marker object");
            egui::ScrollArea::vertical().id_salt("icons").show(ui, |ui| {
                for a in catalog.areas.iter().filter(|a| search.matches(&area_label(a).to_lowercase(), Some(a.id as u32))) {
                    ui.horizontal(|ui| {
                        if let Some(tex) = sprites.get(ui.ctx(), a.sprite) {
                            ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(15.0, 15.0)));
                        }
                        let chosen = editor.brush == Some(Brush::Icon(a.id));
                        if ui.selectable_label(chosen, format!("{}  {}", a.id, area_label(a))).clicked() {
                            choose(editor, Brush::Icon(a.id));
                        }
                    });
                }
            });
        }
    }
}

fn inspector(ui: &mut egui::Ui, editor: &mut Editor, catalog: &Catalog) {
    ui.heading("Selected");
    match editor.selected.clone() {
        None => {
            ui.label("Nothing. Click something with the Select tool.");
        }
        Some(Selected::Placement { square, placement: p }) => {
            let (x, z) = world_tile(square, &p);
            ui.label(format!("{} (object {})", catalog.object_name(p.id), p.id));
            ui.label(format!("tile {x}, {z}, plane {}", p.plane + 1));
            ui.label(format!("shape {}: {}, facing {}", p.kind, kind_name(p.kind), FACINGS[p.orientation as usize & 3]));
            if let Some(node) = catalog.resources.get(&p.id) {
                ui.label(format!("gathering resource: {node}"));
            }
            if let Some(area) = catalog.defs.get(&p.id).and_then(|d| d.map_icon) {
                ui.label(format!("minimap icon: {}", catalog.area_name(area)));
            }
            if ui.button("Remove (Delete)").clicked() {
                remove(editor, catalog, Selected::Placement { square, placement: p });
            }
        }
        Some(Selected::Npc(i)) => {
            if let Some(s) = editor.project.npc_spawns.get(i).cloned() {
                ui.label(format!("NPC {} ({})", catalog.npc_name(&s.npc), s.npc));
                if !catalog.npc(&s.npc).is_some_and(|n| n.has_stats) {
                    ui.label("No combat stats yet: it stands where it is put and cannot be fought.");
                }
                ui.label(format!("tile {}, {}, plane {}", s.x, s.z, s.plane + 1));
                if ui.button("Remove (Delete)").clicked() {
                    remove(editor, catalog, Selected::Npc(i));
                }
            }
        }
    }
}

/// How long the mouse rests on a tile before what is on it is shown, seconds.
const HOVER_DELAY: f64 = 0.6;

/// Once the mouse has rested on a world tile for [`HOVER_DELAY`], a tooltip beside it lists what
/// stands there on the plane being edited. It floats over the world, so nothing else moves.
fn hover_tooltip(ctx: &egui::Context, editor: &mut Editor, catalog: &Catalog, state: &mut UiState, view: &View) {
    let now = ctx.input(|i| i.time);
    let resting = view.hover.in_world && !ctx.input(|i| i.pointer.any_down());
    let tile = view.hover.tile().filter(|_| resting);
    let since = match (tile, state.hovered) {
        (Some(t), Some((was, since))) if t == was => since,
        (Some(t), _) => {
            state.hovered = Some((t, now));
            now
        }
        (None, _) => {
            state.hovered = None;
            return;
        }
    };
    let waited = now - since;
    if waited < HOVER_DELAY {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(HOVER_DELAY - waited));
        return;
    }
    let (Some(tile), Some(pointer)) = (tile, ctx.pointer_hover_pos()) else { return };
    let here = editor.placements_at(tile, editor.plane, catalog);
    let npcs = editor.npcs_at(tile);
    if here.is_empty() && npcs.is_empty() {
        return;
    }
    egui::Area::new("under_the_mouse".into())
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fixed_pos(pointer + egui::vec2(16.0, 16.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                // one line per thing, however near the window's edge the tooltip is
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                for (_, p) in here.iter().take(8) {
                    let name = catalog.defs.get(&p.id).map(|d| d.name.as_str()).filter(|n| !n.is_empty() && *n != "null");
                    ui.label(format!("{} <{}> {}", name.unwrap_or("unnamed"), p.id, kind_name(p.kind)));
                }
                if here.len() > 8 {
                    ui.label(format!("and {} more", here.len() - 8));
                }
                for i in npcs {
                    ui.label(format!("NPC {}", catalog.npc_name(&editor.project.npc_spawns[i].npc)));
                }
            });
        });
}

fn minimap_panel(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    editor: &mut Editor,
    catalog: &Catalog,
    minimap: &mut MinimapPreview,
    sprites: &mut Sprites,
    view: &View,
) {
    let square = view.focus.square();
    minimap.refresh(ctx, square, editor.minimap_stale, &catalog.defs, &editor.project.marker_defs);
    if editor.minimap_stale {
        editor.minimap_stale = false;
    }
    ui.heading(format!("Minimap of square {}_{}", square.0, square.1));
    let Some(tex) = &minimap.texture else {
        ui.label("This square has no map.");
        return;
    };
    let size = 256.0;
    let response = ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(size, size)));
    let rect = response.rect;
    let painter = ui.painter_at(rect);
    let px = size / 256.0;
    for icon in &minimap.icons {
        // the icon's middle, north up
        let lx = (icon.x - square.0 * SQUARE) as f32 + icon.size_x as f32 / 2.0;
        let lz = (icon.z - square.1 * SQUARE) as f32 + icon.size_z as f32 / 2.0;
        let at = rect.min + egui::vec2(lx * 4.0 * px, (64.0 - lz) * 4.0 * px);
        match sprites.get(ctx, icon.sprite) {
            Some(tex) => {
                let half = egui::vec2(tex.size()[0] as f32, tex.size()[1] as f32) * px / 2.0;
                painter.image(tex.id(), egui::Rect::from_min_max(at - half, at + half), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
            None => {
                painter.circle_filled(at, 3.0, egui::Color32::from_rgb(50, 220, 255));
            }
        }
    }
    response.on_hover_text(minimap.icons.iter().map(|i| i.name.clone()).collect::<Vec<_>>().join(", "));
}

fn project_panel(ui: &mut egui::Ui, editor: &mut Editor, catalog: &Catalog, view: &mut View, goto_text: &mut String) {
    ui.heading(format!("Project \u{201c}{}\u{201d}", editor.project.name));
    let squares = editor.project.edited_squares();
    let (added, removed): (usize, usize) =
        editor.project.squares.values().fold((0, 0), |(a, r), e| (a + e.added.len(), r + e.removed.len()));
    ui.label(format!("{added} placed, {removed} removed in {} squares; {} NPC spawns", squares.len(), editor.project.npc_spawns.len()));
    let mut goto = None;
    ui.horizontal(|ui| {
        ui.label("Go to tile");
        let response = ui.add(egui::TextEdit::singleline(goto_text).hint_text("x, z").desired_width(90.0));
        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if entered || ui.button("Go").clicked() {
            let parts: Vec<i32> = goto_text.split([',', ' ']).filter_map(|s| s.trim().parse().ok()).collect();
            match parts[..] {
                [x, z] => goto = Some((x, z)),
                _ => editor.status = "type a tile as x, z (e.g. 3222, 3218)".into(),
            }
        }
    });
    ui.collapsing("NPC spawns", |ui| {
        let mut remove_at = None;
        for (i, s) in editor.project.npc_spawns.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.link(format!("{} at {}, {}", catalog.npc_name(&s.npc), s.x, s.z)).clicked() {
                    goto = Some((s.x, s.z));
                }
                if ui.small_button("✕").clicked() {
                    remove_at = Some(i);
                }
            });
        }
        if let Some(i) = remove_at {
            editor.remove_npc(i);
        }
    });
    ui.collapsing("Edited squares", |ui| {
        for (sx, sy) in &squares {
            if ui.link(format!("{sx}_{sy}")).clicked() {
                goto = Some((sx * SQUARE + SQUARE / 2, sy * SQUARE + SQUARE / 2));
            }
        }
    });
    if let Some((x, z)) = goto {
        if let Ok((_, _, mut transform, mut camera)) = view.cameras.single_mut() {
            jump_to(&mut transform, &mut camera, x, z);
        }
    }
}

/// A dialog window: `contents` draws it and says whether it is finished. Returns that.
fn modal(ctx: &egui::Context, title: &str, contents: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let mut done = false;
    egui::Window::new(title).collapsible(false).show(ctx, |ui| done = contents(ui));
    done
}

/// A cache name field, with where that cache goes and whether one is there already. Returns the
/// name made usable as a directory name (empty when it cannot be).
fn cache_name_field(ui: &mut egui::Ui, name: &mut String) -> String {
    ui.horizontal(|ui| {
        ui.label("Cache name");
        ui.text_edit_singleline(name);
    });
    let clean = clean_name(name);
    if !clean.is_empty() {
        let out = builds::cache_dir_of(&clean);
        ui.label(format!("Into: {}", out.display()));
        if out.exists() {
            ui.colored_label(egui::Color32::YELLOW, "A cache of that name exists and will be replaced.");
        }
    }
    clean
}

fn dialogs(ctx: &egui::Context, editor: &mut Editor, state: &mut UiState) {
    let mut dialog = state.dialog.take();
    let done = match &mut dialog {
        None => false,
        Some(Dialog::NewProject(name)) => modal(ctx, "New project", |ui| {
            ui.label("Name");
            ui.text_edit_singleline(name);
            let clean = clean_name(name);
            let mut done = false;
            ui.horizontal(|ui| {
                if ui.add_enabled(!clean.is_empty(), egui::Button::new("Create")).clicked() {
                    let path = MapProject::path_of(&clean);
                    if path.exists() {
                        editor.status = format!("a project called {clean} already exists; open it from File");
                    } else {
                        let mut project = MapProject::new(&clean);
                        project.npc_spawns = default_spawns();
                        editor.switch_project(project, path);
                        editor.save();
                    }
                    done = true;
                }
                done |= ui.button("Cancel").clicked();
            });
            done
        }),
        Some(Dialog::Compile { name, then_play }) => {
            let (title, about, button) = match then_play {
                None => (
                    "Compile to a new cache",
                    "Builds a full copy of the base cache with this project's edits. The base cache is only read.".to_string(),
                    "Compile",
                ),
                Some(_) => (
                    "Create a cache to play on?",
                    format!(
                        "The project \u{201c}{}\u{201d} has no cache yet. Playing it compiles a new cache from the base cache with this project's edits (the base cache is only read), then starts the game on it.",
                        editor.project.name
                    ),
                    "Create cache and play",
                ),
            };
            let then_play = *then_play;
            modal(ctx, title, |ui| {
                ui.label(about);
                ui.label(format!("Base: {}", editor.base().dir().display()));
                let clean = cache_name_field(ui, name);
                let mut done = false;
                ui.horizontal(|ui| {
                    if ui.add_enabled(!clean.is_empty(), egui::Button::new(button)).clicked() {
                        state.job = Some(start_compile(editor, clean.clone(), then_play));
                        done = true;
                    }
                    done |= ui.button("Cancel").clicked();
                });
                done
            })
        }
        Some(Dialog::Rebuild { name, spawn }) => {
            let (name, spawn) = (name.clone(), *spawn);
            modal(ctx, "Rebuild the map before playing?", |ui| {
                ui.label(format!("You have edited \u{201c}{}\u{201d} since its cache \u{201c}{name}\u{201d} was built.", editor.project.name));
                ui.label("Rebuilding takes a moment and puts your latest edits in the game.");
                let mut done = false;
                ui.horizontal(|ui| {
                    if ui.button("Rebuild and play").clicked() {
                        state.job = Some(start_compile(editor, name.clone(), Some(spawn)));
                        done = true;
                    }
                    if ui.button("Play the last build").clicked() {
                        launch(editor, &builds::cache_dir_of(&name), spawn);
                        done = true;
                    }
                    done |= ui.button("Cancel").clicked();
                });
                done
            })
        }
    };
    if !done {
        state.dialog = dialog;
    }

    if let Some(job) = &state.job {
        egui::Window::new("Compiling").collapsible(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!(
                    "{}: {}",
                    job.name,
                    match job.stage {
                        Stage::Building => "building the changed groups",
                        Stage::Copying => "copying the base cache",
                        Stage::Writing => "writing the edits",
                        Stage::Done => "done",
                    }
                ));
            });
        });
    }

    let Some(result) = &state.compiled else { return };
    let mut make_active = None;
    let closed = modal(ctx, "Compiled", |ui| match result {
        Ok(m) => {
            ui.label(format!("Built cache \u{201c}{}\u{201d}: {} squares, {} NPC spawns, {} new icon objects.", m.name, m.squares, m.npc_spawns, m.marker_defs));
            ui.label(builds::cache_dir_of(&m.name).display().to_string());
            let mut done = false;
            ui.horizontal(|ui| {
                if ui.button("Play on it (set active)").clicked() {
                    make_active = Some(m.name.clone());
                    done = true;
                }
                done |= ui.button("Close").clicked();
            });
            done
        }
        Err(e) => {
            ui.colored_label(egui::Color32::LIGHT_RED, format!("The compile failed: {e}"));
            ui.button("Close").clicked()
        }
    });
    if let Some(name) = make_active {
        set_active(editor, state, &Selection::Dir(builds::cache_dir_of(&name)), format!("The game will read {name} next time it starts"));
    }
    if closed {
        state.compiled = None;
    }
}

/// A name usable as a file or directory name: letters, digits, `-` and `_`.
pub fn clean_name(name: &str) -> String {
    name.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect()
}

fn start_compile(editor: &mut Editor, name: String, then_play: Option<Spawn>) -> CompileJob {
    if editor.dirty {
        editor.save();
    }
    let (tx, rx) = mpsc::channel();
    let project = editor.project.clone();
    let base = editor.base().dir().to_path_buf();
    let out = builds::cache_dir_of(&name);
    std::thread::spawn(move || {
        let progress = tx.clone();
        let result = compile::compile(&project, &base, &out, |stage| {
            let _ = progress.send(JobMessage::Stage(stage));
        });
        let _ = tx.send(JobMessage::Done(result.map_err(|e| e.to_string())));
    });
    CompileJob { name, stage: Stage::Building, messages: Mutex::new(rx), then_play }
}

fn poll_job(state: &mut UiState, editor: &mut Editor) {
    let Some(job) = &mut state.job else { return };
    let messages: Vec<JobMessage> = job.messages.lock().unwrap().try_iter().collect();
    for message in messages {
        match message {
            JobMessage::Stage(stage) => job.stage = stage,
            JobMessage::Done(result) => {
                let then_play = job.then_play;
                state.job = None;
                state.builds_epoch += 1;
                match &result {
                    Ok(m) => {
                        editor.status = format!("Compiled cache {}", m.name);
                        // the cache this project plays on from now on
                        editor.project.cache = Some(m.name.clone());
                        editor.save();
                        if let Some(spawn) = then_play {
                            launch(editor, &builds::cache_dir_of(&m.name), spawn);
                            return;
                        }
                    }
                    Err(e) => editor.status = format!("compile failed: {e}"),
                }
                state.compiled = Some(result);
                return;
            }
        }
    }
}

/// Names over the NPC spawns, and each minimap icon's sprite over its marker, near the camera.
fn world_labels(ctx: &egui::Context, painter: &egui::Painter, editor: &Editor, catalog: &Catalog, sprites: &mut Sprites, view: &View) {
    let Ok((camera, transform, _, _)) = view.cameras.single() else { return };
    let eye = transform.translation();
    let screen = |x: i32, z: i32, height: f32| -> Option<egui::Pos2> {
        let at = view.world.ground_at(x, z)? + Vec3::Y * height;
        if at.distance(eye) > 80.0 {
            return None;
        }
        camera.world_to_viewport(transform, at).ok().map(|p| egui::pos2(p.x, p.y))
    };
    for s in &editor.project.npc_spawns {
        let Some(p) = screen(s.x, s.z, 2.1) else { continue };
        painter.text(p, egui::Align2::CENTER_BOTTOM, catalog.npc_name(&s.npc), egui::FontId::proportional(13.0), egui::Color32::from_rgb(255, 160, 40));
    }
    for &((x, z), area) in &view.icons.0 {
        let Some(p) = screen(x, z, 1.1) else { continue };
        let sprite = catalog.areas.iter().find(|a| a.id == area).and_then(|a| sprites.get(ctx, a.sprite));
        match sprite {
            Some(tex) => {
                let half = egui::vec2(tex.size()[0] as f32, tex.size()[1] as f32) / 2.0;
                let rect = egui::Rect::from_min_max(p - egui::vec2(half.x, half.y * 2.0), p + egui::vec2(half.x, 0.0));
                painter.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
            None => {
                painter.text(p, egui::Align2::CENTER_BOTTOM, catalog.area_name(area), egui::FontId::proportional(13.0), egui::Color32::from_rgb(80, 220, 255));
            }
        }
    }
}
