//! The editor's state: the project being edited, what the tools are set to, and every change to
//! the project (each undoable).
//!
//! An edit changes only the [`MapProject`]. The world on screen follows it: after each change the
//! changed groups are rebuilt with [`osrs_cache::compile::build_with`] and put in the cache
//! [`overlay`](osrs_cache::overlay), and the squares they touch are decoded again, through the same
//! code the game uses, so what is drawn is what a compiled cache will hold.

use crate::catalog::{footprint, Catalog};
use bevy::prelude::*;
use osrs_cache::compile;
use osrs_cache::container::Compression;
use osrs_cache::locs::Placement;
use osrs_cache::project::{MapProject, MarkerDef, NpcSpawn};
use osrs_cache::squares::SQUARE;
use osrs_cache::{idx, Cache};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

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
    /// The cache edits are made on, read without the overlay.
    pub base: Cache,
    pub base_dir: PathBuf,
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
    /// Each square's placements as the base cache has them.
    base_placements: HashMap<(i32, i32), Vec<Placement>>,
}

impl Editor {
    pub fn new(project: MapProject, path: PathBuf, base: Cache, base_dir: PathBuf) -> Editor {
        let mut editor = Editor {
            project,
            path,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            base,
            base_dir,
            tool: Tool::Select,
            brush: None,
            plane: 0,
            rotation: 0,
            kind_override: None,
            selected: None,
            status: String::new(),
            stale: HashSet::new(),
            minimap_stale: true,
            base_placements: HashMap::new(),
        };
        editor.refresh_overlay();
        editor
    }

    /// The placements of `square` as they are now (the base cache's, with the project's edits).
    pub fn placements(&mut self, square: (i32, i32)) -> Vec<Placement> {
        let base = self.base_placements_of(square).to_vec();
        match self.project.square(square.0, square.1) {
            Some(edits) => edits.apply(&base),
            None => base,
        }
    }

    fn base_placements_of(&mut self, square: (i32, i32)) -> &[Placement] {
        let base = &self.base;
        self.base_placements.entry(square).or_insert_with(|| {
            let gid = ((square.0 << 8) | square.1) as u32;
            base.index(idx::MAPS)
                .and_then(|maps| base.read_files(&maps, gid))
                .ok()
                .and_then(|files| files.get(1).map(|(_, d)| osrs_cache::locs::decode(d)))
                .unwrap_or_default()
        })
    }

    /// Whether the base cache has map data for `square` (only those can be edited).
    pub fn square_exists(&self, square: (i32, i32)) -> bool {
        let gid = ((square.0 << 8) | square.1) as u32;
        self.base.index(idx::MAPS).is_ok_and(|maps| maps.groups.contains_key(&gid))
    }

    /// The placements on `plane` whose footprint covers world tile `tile`, nearest the top first
    /// (scenery before walls before floor decorations).
    pub fn placements_at(&mut self, tile: (i32, i32), plane: u8, catalog: &Catalog) -> Vec<((i32, i32), Placement)> {
        let mut out = Vec::new();
        // a big object can reach in from a square to the south or west
        for dz in -1..=0 {
            for dx in -1..=0 {
                let square = (tile.0.div_euclid(SQUARE) + dx, tile.1.div_euclid(SQUARE) + dz);
                let (x0, z0) = (square.0 * SQUARE, square.1 * SQUARE);
                for p in self.placements(square) {
                    if p.plane != plane {
                        continue;
                    }
                    let (w, h) = footprint(catalog.defs.get(&p.id), p.orientation);
                    let (px, pz) = (x0 + p.x as i32, z0 + p.z as i32);
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
    }

    /// Puts `placement` (square-local position) into `square`.
    pub fn add_placement(&mut self, square: (i32, i32), placement: Placement) {
        if !self.square_exists(square) {
            self.status = format!("map square {}_{} is not in the cache; only existing squares can be edited", square.0, square.1);
            return;
        }
        self.checkpoint();
        self.put(square, placement);
    }

    /// Puts minimap icon `area` on square-local tile (`x`, `z`) of `square`: an invisible floor
    /// decoration carrying the icon (see [`Editor::icon_object`]).
    pub fn place_icon(&mut self, square: (i32, i32), x: u8, z: u8, area: u16, catalog: &Catalog) {
        if !self.square_exists(square) {
            self.status = format!("map square {}_{} is not in the cache; only existing squares can be edited", square.0, square.1);
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
        self.changed(square);
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
        self.changed(square);
    }

    pub fn add_npc(&mut self, spawn: NpcSpawn) {
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
            self.after_jump(&now);
            self.redo.push(now);
            self.status = "Undone".into();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            let now = std::mem::replace(&mut self.project, next);
            self.project.cache = now.cache.clone();
            self.after_jump(&now);
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

    /// After the project jumped from `before` to what it is now: every square either of them
    /// edits may look different.
    fn after_jump(&mut self, before: &MapProject) {
        let squares: HashSet<(i32, i32)> = before.edited_squares().into_iter().chain(self.project.edited_squares()).collect();
        let changed: Vec<_> = squares
            .into_iter()
            .filter(|&(sx, sy)| before.square(sx, sy) != self.project.square(sx, sy))
            .collect();
        self.selected = None;
        self.dirty = true;
        self.refresh_overlay();
        for square in changed {
            self.mark_stale(square);
        }
        self.minimap_stale = true;
    }

    fn changed(&mut self, square: (i32, i32)) {
        self.refresh_overlay();
        self.mark_stale(square);
        self.minimap_stale = true;
    }

    /// A square is drawn from it and its eight neighbours (objects reach over borders, terrain
    /// colours blend over them), so all nine are decoded again.
    fn mark_stale(&mut self, square: (i32, i32)) {
        for dz in -1..=1 {
            for dx in -1..=1 {
                self.stale.insert((square.0 + dx, square.1 + dz));
            }
        }
    }

    /// Rebuilds the in-memory overlay from the project.
    pub fn refresh_overlay(&mut self) {
        match compile::build_with(&self.project, &self.base, Compression::None) {
            Ok(groups) => osrs_cache::overlay::set(groups),
            Err(e) => {
                self.status = format!("could not apply the edits: {e}");
                eprintln!("[editor] {}", self.status);
            }
        }
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
        let before = std::mem::replace(&mut self.project, project);
        self.path = path;
        self.undo.clear();
        self.redo.clear();
        self.after_jump(&before);
        self.dirty = false;
    }
}

/// The NPC a new project starts with: the man who stands in Lumbridge square in the open world
/// when the cache carries no spawns, so a compiled cache keeps him until he is removed.
pub fn default_spawns() -> Vec<NpcSpawn> {
    vec![NpcSpawn { npc: "man".into(), x: 3226, z: 3215, plane: 0 }]
}
