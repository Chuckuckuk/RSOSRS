//! The editor's state: the project being edited, what the tools are set to, and every change to
//! the project (each undoable).
//!
//! An edit changes only the [`MapProject`]. The world on screen follows it: after each change the
//! changed groups are rebuilt with [`compile::build_with`] and put in the editor's cache
//! [`Overlay`], which the caches the world is drawn from read first, and the squares they touch
//! are decoded again, through the same code the game uses, so what is drawn is what a compiled
//! cache will hold.

use crate::catalog::{footprint, Catalog};
use bevy::prelude::*;
use osrs_cache::container::Compression;
use osrs_cache::locs::Placement;
use osrs_cache::overlay::Overlay;
use osrs_cache::squares::SQUARE;
use osrs_cache::Cache;
use osrs_core::data::NpcSpawnPoint;
use osrs_map_editor::compile::{self, Builder};
use osrs_map_editor::project::{MapProject, MarkerDef};
use osrs_render::streaming::square_of;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

/// What a left click in the world does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Pick a placement or an NPC to look at (Delete removes it).
    Select,
    /// Put down what the palette has chosen.
    Place,
    /// Remove whatever is clicked.
    Erase,
}

/// What the Place tool puts down.
#[derive(Debug, Clone, PartialEq)]
pub enum Brush {
    Object(u32),
    Npc(String),
    Icon(u16),
}

/// Something in the world the Select tool has picked.
#[derive(Debug, Clone, PartialEq)]
pub enum Selected {
    Placement { square: (i32, i32), placement: Placement },
    Npc(usize),
}

#[derive(Resource)]
pub struct Editor {
    pub project: MapProject,
    /// Where the project is saved.
    pub path: PathBuf,
    /// Changes since the last save.
    pub dirty: bool,
    undo: Vec<MapProject>,
    redo: Vec<MapProject>,
    /// Builds the project's groups on the base cache (as stored), keeping what it has read and
    /// built, so an edit encodes only what it changed.
    builder: Builder,
    /// The uncompiled edits, as cache groups: the caches the world and minimap are drawn from read
    /// this first.
    pub overlay: Arc<Overlay>,
    /// Counts changes to the project, for whatever is worked out from it to know when to redo it.
    pub generation: u64,
    pub tool: Tool,
    pub brush: Option<Brush>,
    /// Plane new placements go on, and the plane the Select tool looks at.
    pub plane: u8,
    /// Quarter turns of new placements.
    pub rotation: u8,
    /// Shape of new placements, instead of the object's own.
    pub kind_override: Option<u8>,
    pub selected: Option<Selected>,
    /// One line about the last thing that happened.
    pub status: String,
    /// Squares whose map has changed and must be decoded again.
    pub stale: HashSet<(i32, i32)>,
    /// Whether the minimap preview needs drawing again.
    pub minimap_stale: bool,
    /// Each square's placements as they are now (the base's with the edits applied), worked out
    /// the first time they are asked for after a change.
    placements: HashMap<(i32, i32), Arc<[Placement]>>,
}

impl Editor {
    pub fn new(project: MapProject, path: PathBuf, base: Cache, overlay: Arc<Overlay>) -> Editor {
        let mut editor = Editor {
            project,
            path,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            builder: Builder::new(base, Compression::None),
            overlay,
            generation: 0,
            tool: Tool::Select,
            brush: None,
            plane: 0,
            rotation: 0,
            kind_override: None,
            selected: None,
            status: String::new(),
            stale: HashSet::new(),
            minimap_stale: true,
            placements: HashMap::new(),
        };
        editor.rebuild();
        editor
    }

    /// The cache the edits are made on, as stored.
    pub fn base(&self) -> &Cache {
        self.builder.base()
    }

    /// The placements of `square` as they are now (the base cache's, with the project's edits).
    pub fn placements(&mut self, square: (i32, i32)) -> Arc<[Placement]> {
        if let Some(placed) = self.placements.get(&square) {
            return Arc::clone(placed);
        }
        let base = self.builder.base_placements(square.0, square.1);
        let placed: Arc<[Placement]> = match self.project.square(square.0, square.1) {
            Some(edits) => edits.apply(&base).into(),
            None => base.into(),
        };
        self.placements.insert(square, Arc::clone(&placed));
        placed
    }

    /// Whether `square` can be edited (the base cache has it); says why not when it cannot.
    fn editable(&mut self, square: (i32, i32)) -> bool {
        let exists = self.builder.square_exists(square.0, square.1);
        if !exists {
            self.status = format!("map square {}_{} is not in the cache; only existing squares can be edited", square.0, square.1);
        }
        exists
    }

    /// The placements on `plane` whose footprint covers world tile `tile`, nearest the top first
    /// (scenery before walls before floor decorations).
    pub fn placements_at(&mut self, tile: (i32, i32), plane: u8, catalog: &Catalog) -> Vec<((i32, i32), Placement)> {
        let mut out = Vec::new();
        let (sx, sz) = square_of(tile.0, tile.1);
        // a big object can reach in from a square to the south or west
        for dz in -1..=0 {
            for dx in -1..=0 {
                let square = (sx + dx, sz + dz);
                for &p in self.placements(square).iter() {
                    if p.plane != plane {
                        continue;
                    }
                    let (w, h) = footprint(catalog.defs.get(&p.id), p.orientation);
                    let (px, pz) = world_tile(square, &p);
                    if (px..px + w).contains(&tile.0) && (pz..pz + h).contains(&tile.1) {
                        out.push((square, p));
                    }
                }
            }
        }
        out.sort_by_key(|(_, p)| match p.kind {
            10 | 11 => 0,
            0..=9 => 1,
            22 => 3,
            _ => 2,
        });
        out
    }

    /// The NPC spawns standing on `tile`.
    pub fn npcs_at(&self, tile: (i32, i32)) -> Vec<usize> {
        self.project.npc_spawns.iter().enumerate().filter(|(_, s)| (s.x, s.z) == tile).map(|(i, _)| i).collect()
    }

    // ---- changes (each one undoable) ----

    fn checkpoint(&mut self) {
        self.undo.push(self.project.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
        self.generation += 1;
    }

    /// Puts `placement` (square-local position) into `square`.
    pub fn add_placement(&mut self, square: (i32, i32), placement: Placement) {
        if !self.editable(square) {
            return;
        }
        self.checkpoint();
        self.put(square, placement);
    }

    /// Puts minimap icon `area` on square-local tile (`x`, `z`) of `square`: an invisible floor
    /// decoration carrying the icon (see [`Editor::icon_object`]).
    pub fn place_icon(&mut self, square: (i32, i32), x: u8, z: u8, area: u16, catalog: &Catalog) {
        if !self.editable(square) {
            return;
        }
        self.checkpoint();
        let id = self.icon_object(area, catalog);
        self.put(square, Placement { id, plane: self.plane, x, z, kind: 22, orientation: 0 });
    }

    fn put(&mut self, square: (i32, i32), placement: Placement) {
        let edits = self.project.square_mut(square.0, square.1);
        // putting back something that was taken out undoes the removal
        if let Some(i) = edits.removed.iter().position(|p| *p == placement) {
            edits.removed.remove(i);
        } else {
            edits.added.push(placement);
        }
        self.rebuild();
    }

    /// Takes one `placement` out of `square`.
    pub fn remove_placement(&mut self, square: (i32, i32), placement: Placement) {
        self.checkpoint();
        let edits = self.project.square_mut(square.0, square.1);
        // taking out something the project added just forgets it
        if let Some(i) = edits.added.iter().position(|p| *p == placement) {
            edits.added.remove(i);
        } else {
            edits.removed.push(placement);
        }
        if self.selected == Some(Selected::Placement { square, placement }) {
            self.selected = None;
        }
        self.rebuild();
    }

    pub fn add_npc(&mut self, spawn: NpcSpawnPoint) {
        self.checkpoint();
        self.project.npc_spawns.push(spawn);
    }

    pub fn remove_npc(&mut self, index: usize) {
        if index >= self.project.npc_spawns.len() {
            return;
        }
        self.checkpoint();
        self.project.npc_spawns.remove(index);
        self.selected = None;
    }

    /// The object to place for minimap icon `area`: one the cache already has (invisible, blocks
    /// nothing), else one the project has minted for it, else a newly minted one.
    fn icon_object(&mut self, area: u16, catalog: &Catalog) -> u32 {
        if let Some(id) = compile::existing_marker(&catalog.defs, area) {
            return id;
        }
        if let Some(m) = self.project.marker_defs.iter().find(|m| m.map_icon == area) {
            return m.id;
        }
        let id = compile::next_object_id(&catalog.defs, &self.project);
        self.project.marker_defs.push(MarkerDef { id, map_icon: area });
        id
    }

    pub fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            let now = std::mem::replace(&mut self.project, previous);
            // which cache the project plays on is not an edit
            self.project.cache = now.cache.clone();
            self.after_jump();
            self.redo.push(now);
            self.status = "Undone".into();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            let now = std::mem::replace(&mut self.project, next);
            self.project.cache = now.cache.clone();
            self.after_jump();
            self.undo.push(now);
            self.status = "Redone".into();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// After the project jumped to another version (undo, redo, another project).
    fn after_jump(&mut self) {
        self.selected = None;
        self.dirty = true;
        self.generation += 1;
        self.rebuild();
    }

    /// Brings the overlay up to date with the project, encoding only the squares whose edits
    /// changed, and has those squares worked out and drawn again. A square is drawn from it and
    /// its eight neighbours (objects reach over borders, terrain colours blend over them), so all
    /// nine are decoded again.
    fn rebuild(&mut self) {
        let changed = match self.builder.sync(&self.project).and_then(|changed| Ok((changed, self.builder.containers()?))) {
            Ok((changed, containers)) => {
                self.overlay.set(containers);
                changed
            }
            Err(e) => {
                self.status = format!("could not apply the edits: {e}");
                eprintln!("[editor] {}", self.status);
                return;
            }
        };
        for square in changed {
            self.placements.remove(&square);
            for dz in -1..=1 {
                for dx in -1..=1 {
                    self.stale.insert((square.0 + dx, square.1 + dz));
                }
            }
        }
        self.minimap_stale = true;
    }

    pub fn save(&mut self) {
        match self.project.save(&self.path) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("Saved {}", self.path.display());
            }
            Err(e) => self.status = format!("could not save {}: {e}", self.path.display()),
        }
    }

    /// Replaces the project with another (opened or new); not undoable.
    pub fn switch_project(&mut self, project: MapProject, path: PathBuf) {
        self.project = project;
        self.path = path;
        self.undo.clear();
        self.redo.clear();
        self.after_jump();
        self.dirty = false;
    }
}

/// The tile the game enters the world on by default (Lumbridge square), and where the editor
/// starts looking.
pub const LUMBRIDGE: (i32, i32) = (3222, 3218);

/// The NPCs a new project starts with: the ones the open world has when the cache carries no
/// spawns (the man in Lumbridge square), so a compiled cache keeps them until they are removed.
pub fn default_spawns() -> Vec<NpcSpawnPoint> {
    osrs_core::data::default_npc_spawns(LUMBRIDGE)
}

/// The world tile of a placement in `square` (placements are stored square-local).
pub fn world_tile(square: (i32, i32), p: &Placement) -> (i32, i32) {
    (square.0 * SQUARE + p.x as i32, square.1 * SQUARE + p.z as i32)
}
