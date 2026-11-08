//! Editing: the working copy of the Map ([`Editor`]), what you can do to it, and how the live
//! view follows.
//!
//! **The working copy.** Opening a Map parses its `.map` file into a [`MapFile`] that the editor
//! owns; every edit changes that copy and pushes the previous state on an undo stack (snapshots:
//! a Map is a few thousand brushes, a clone per edit is cheap). Nothing touches the Map's own
//! file until Save, so a committed or generated Map is never changed by looking at it or
//! experimenting.
//!
//! **The live view.** The viewer shows the Map through the game's own Map loader, which reads an
//! asset path. While a Map is open MapEdit writes the working copy to a preview file next to the
//! Map's file (`maps/.mapedit-arena.map`, git-ignored, deleted when the Map is left or the
//! viewer exits) and points the Map's registry entry at it. After an edit the preview is written
//! (at most every 200 ms) and the existing reload path runs: the Map root is despawned and the
//! loader reads the preview again, so the view is exactly what the game would load. Save writes
//! the working copy to the real file.
//!
//! **Files.** [`Command`]s (open, save, save as, new Map, settings, quit) change files and the
//! Map registry; the Esc menu and the remote methods run the same [`execute`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bevy::ecs::message::Messages;
use bevy::input::keyboard::KeyboardInput;
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use frontline::atmosphere::Looks;
use frontline::data::GameData;
use frontline::maps::{IdleMap, MapId, MapInfo, MapRegistry, private_dir};

use crate::camera::FlyCamera;
use crate::mapfile::{
    self, Brush, Item, MapEntity, MapFile, PropBounds, Rise, SURFACES, Surface, tb_to_game,
};
use crate::mapsron::{self, Entry};
use crate::menu::{BlockPicking, InputCapture, MenuState};
use crate::palette::{self, PropModel};
use crate::playtest;
use crate::prefab;
use crate::selection::{self, Combine, Kind, Selection};
use crate::session::Reload;
use crate::transform::{self, MirrorAxis, Motion, TURN_STEP};

const UNDO_LIMIT: usize = 200;
const PREVIEW_PREFIX: &str = ".mapedit-";
/// The preview is written at most this often, in seconds.
const WRITE_EVERY: f64 = 0.2;
const WATCH_EVERY: f32 = 0.5;
const NOTICE_SECS: f32 = 8.0;
/// Grid sizes, metres; `G` cycles them.
pub const GRIDS: [f64; 4] = [0.25, 0.5, 1.0, 0.05];

// ----------------------------------------------------------------------------- the editor

/// Where Maps and their registry live on disk (the repository's, or a temporary folder in tests).
#[derive(Clone, Debug)]
pub struct Paths {
    /// The game's asset folder: Maps are `maps/*.map` in it.
    pub assets: PathBuf,
    /// The built-in Map registry, `data/maps.ron`.
    pub registry: PathBuf,
    /// The private content folder (`content/private`), when there is one.
    pub private: Option<PathBuf>,
}

impl Paths {
    pub fn live(assets: PathBuf) -> Self {
        Paths {
            assets,
            registry: PathBuf::from("data/maps.ron"),
            private: private_dir(),
        }
    }

    /// The `.map` file of a registry `file` on disk.
    pub fn source(&self, file: &str, private: bool) -> Option<PathBuf> {
        if let Some(rest) = file.strip_prefix("private://") {
            return Some(self.private.as_ref()?.join(rest));
        }
        if private {
            return Some(self.private.as_ref()?.join(file));
        }
        Some(self.assets.join(file))
    }

    /// The folders that hold Map files.
    fn map_folders(&self) -> Vec<(bool, PathBuf)> {
        let mut folders = vec![(false, self.assets.join("maps"))];
        if let Some(dir) = &self.private {
            folders.push((true, dir.join("maps")));
        }
        folders
    }

    /// Deletes previews a crashed run left behind.
    fn remove_stale_previews(&self) {
        for (_, folder) in self.map_folders() {
            let Ok(entries) = std::fs::read_dir(folder) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(PREVIEW_PREFIX) && name.ends_with(".map") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    /// The registry file a Map's entry lives in.
    fn registry_file(&self, private: bool) -> Result<PathBuf, String> {
        if private {
            Ok(self
                .private
                .as_ref()
                .ok_or("the private content folder is missing")?
                .join("maps.ron"))
        } else {
            Ok(self.registry.clone())
        }
    }

    /// The folder prefabs are saved in: `prefabs/` of the game's assets, or of the private
    /// content folder for a private Map (Homage content never goes in the public one).
    pub fn prefab_folder(&self, private: bool) -> Result<PathBuf, String> {
        if private {
            Ok(self
                .private
                .as_ref()
                .ok_or("the private content folder is missing")?
                .join("prefabs"))
        } else {
            Ok(self.assets.join("prefabs"))
        }
    }

    /// Every prefab, the public ones first.
    pub fn prefab_list(&self) -> Vec<PrefabInfo> {
        let mut all = Vec::new();
        for private in [false, true] {
            if let Ok(dir) = self.prefab_folder(private) {
                all.extend(
                    prefab::list(&dir)
                        .into_iter()
                        .map(|name| PrefabInfo { name, private }),
                );
            }
        }
        all
    }

    fn target(&self, id: &str, private: bool) -> Result<Target, String> {
        if private {
            let dir = self
                .private
                .as_ref()
                .ok_or("the private content folder is missing")?;
            Ok(Target {
                disk: dir.join("maps").join(format!("{id}.map")),
                runtime_file: format!("private://maps/{id}.map"),
                ron_file: format!("maps/{id}.map"),
            })
        } else {
            Ok(Target {
                disk: self.assets.join("maps").join(format!("{id}.map")),
                runtime_file: format!("maps/{id}.map"),
                ron_file: format!("maps/{id}.map"),
            })
        }
    }
}

/// Where the live preview of the open Map is.
#[derive(Clone, Debug)]
pub struct Preview {
    pub disk: PathBuf,
}

/// A line of feedback shown in the HUD for a few seconds.
#[derive(Clone, Debug)]
pub struct Notice {
    pub text: String,
    pub error: bool,
    pub age: f32,
}

struct Snapshot {
    map: MapFile,
    selection: Selection,
}

/// Which face of a brush an edit means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FaceRef {
    /// By index into the brush's faces.
    Index(usize),
    /// The face that looks most nearly in this direction (game space); it slides along that
    /// direction.
    Towards(DVec3),
}

/// A change of an entity's key/values, facing and position, applied as one edit.
#[derive(Clone, Debug, Default)]
pub struct EntityChange {
    /// `(key, value)` pairs; an empty value removes the key.
    pub set: Vec<(String, String)>,
    pub position: Option<DVec3>,
    /// The facing in degrees.
    pub angle: Option<f64>,
}

/// A prefab on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefabInfo {
    pub name: String,
    /// From the private content folder (never committed to the public repository).
    pub private: bool,
}

/// Whether an entity class faces somewhere, so that turning it turns its `angle`.
fn faces(class: &str) -> bool {
    palette::class(class).is_some_and(|c| c.faces)
}

/// The editing session of the open Map.
#[derive(Resource)]
pub struct Editor {
    pub paths: Paths,
    /// Names of the look presets (for validating settings).
    pub look_names: Vec<String>,
    pub id: MapId,
    /// The working copy; `None` when the Map's file could not be read or parsed (the viewer still
    /// shows it, but nothing can be edited: see `problem`).
    pub map: Option<MapFile>,
    pub problem: Option<String>,
    /// The file as last loaded or saved, for the unsaved-changes flag.
    saved: Option<MapFile>,
    /// The Map's `.map` file.
    pub source: Option<PathBuf>,
    source_mtime: Option<SystemTime>,
    /// The Map is from the private content folder.
    pub private: bool,
    /// The registry entry's own `file` while it points at the preview.
    original_file: Option<String>,
    pub preview: Option<Preview>,
    /// What the Map's entry in `maps.ron` said when opened or saved.
    baseline: Option<Entry>,
    /// Name, look, Game Modes or Factions differ from `baseline`.
    pub settings_dirty: bool,
    /// Maps registered at run time that `maps.ron` does not list yet.
    pub loose: HashSet<MapId>,
    dirty: bool,
    pub revision: u64,
    /// The revision the view (the preview file) shows.
    pub shown_revision: u64,
    last_write: f64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    pub selection: Selection,
    pub hover: Option<Item>,
    /// The surface new brushes get.
    pub surface: &'static Surface,
    pub grid: f64,
    pub notice: Option<Notice>,
    pub props: Vec<PropModel>,
    pub prop_bounds: PropBounds,
    /// What Ctrl+C copied: a fragment (see [`prefab`]).
    pub clipboard: Option<MapFile>,
    /// The prefabs on disk, for the palette (refreshed when one is saved).
    pub prefab_list: Vec<PrefabInfo>,
}

impl Editor {
    pub fn new(paths: Paths, props: Vec<PropModel>) -> Self {
        let prefab_list = paths.prefab_list();
        Editor {
            paths,
            look_names: Vec::new(),
            id: MapId::new(""),
            map: None,
            problem: None,
            saved: None,
            source: None,
            source_mtime: None,
            private: false,
            original_file: None,
            preview: None,
            baseline: None,
            settings_dirty: false,
            loose: HashSet::new(),
            dirty: false,
            revision: 0,
            shown_revision: 0,
            last_write: f64::MIN,
            undo: Vec::new(),
            redo: Vec::new(),
            selection: Selection::default(),
            hover: None,
            surface: &SURFACES[0],
            grid: GRIDS[0],
            notice: None,
            prop_bounds: palette::prop_bounds(&props),
            clipboard: None,
            prefab_list,
            props,
        }
    }

    /// An editor holding `map` as saved, with no files behind it, for tests of the UI.
    #[cfg(test)]
    pub fn for_tests(map: MapFile, props: Vec<PropModel>) -> Editor {
        let paths = Paths {
            assets: PathBuf::from("/nonexistent"),
            registry: PathBuf::new(),
            private: None,
        };
        let mut editor = Editor::new(paths, props);
        editor.saved = Some(map.clone());
        editor.map = Some(map);
        editor
    }

    /// Unsaved edits or settings.
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The view shows the working copy (no preview write is pending).
    pub fn synced(&self) -> bool {
        self.revision == self.shown_revision
    }

    /// The `tools/mapgen` script that generates this Map's file, whose next run overwrites edits.
    pub fn generator(&self) -> Option<PathBuf> {
        generator_for(self.source.as_deref()?)
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: false,
            age: 0.0,
        });
    }

    pub fn complain(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: true,
            age: 0.0,
        });
    }

    /// Shows the outcome of an edit.
    pub fn report(&mut self, result: &Result<String, String>) {
        match result {
            Ok(text) => self.say(text.clone()),
            Err(text) => self.complain(text.clone()),
        }
    }

    fn refresh_dirty(&mut self) {
        self.dirty = self.settings_dirty || self.map != self.saved;
    }

    fn bump(&mut self) {
        self.revision += 1;
        self.refresh_dirty();
    }

    fn require_map(&self) -> Result<(), String> {
        if self.map.is_some() {
            return Ok(());
        }
        Err(match &self.problem {
            Some(problem) => format!("this Map cannot be edited: {problem}"),
            None => "no Map is open for editing".to_owned(),
        })
    }

    /// The items an edit acts on: `item` when given, else the whole selection; each checked to
    /// exist.
    pub fn targets(&self, item: Option<Item>) -> Result<Vec<Item>, String> {
        self.require_map()?;
        let items: Vec<Item> = match item {
            Some(item) => vec![item],
            None => self.selection.items().to_vec(),
        };
        if items.is_empty() {
            return Err("nothing is selected (click a brush or entity, or pass `item`)".into());
        }
        let map = self.map.as_ref().expect("checked");
        match items.iter().find(|item| !map.contains(**item)) {
            Some(missing) => Err(format!("there is no {missing}")),
            None => Ok(items),
        }
    }

    /// Exactly one item: `item`, or the selection when that is a single item.
    pub fn resolve(&self, item: Option<Item>) -> Result<Item, String> {
        match self.targets(item)?.as_slice() {
            [one] => Ok(*one),
            many => Err(format!(
                "{} items are selected; this needs exactly one",
                many.len()
            )),
        }
    }

    /// Applies `change` to the working copy as one undoable step; on an error the copy is
    /// left as it was.
    pub fn edit<R>(
        &mut self,
        change: impl FnOnce(&mut MapFile) -> Result<R, String>,
    ) -> Result<R, String> {
        self.require_map()?;
        let map = self.map.as_mut().expect("checked");
        let before = map.clone();
        match change(map) {
            Ok(value) => {
                self.undo.push(Snapshot {
                    map: before,
                    selection: self.selection.clone(),
                });
                if self.undo.len() > UNDO_LIMIT {
                    self.undo.remove(0);
                }
                self.redo.clear();
                self.bump();
                Ok(value)
            }
            Err(err) => {
                *map = before;
                Err(err)
            }
        }
    }

    /// Makes `item` (or nothing) the selection.
    pub fn select(&mut self, item: Option<Item>) -> Result<String, String> {
        self.require_map()?;
        match item {
            None => {
                self.selection.clear();
                Ok("Selection cleared".into())
            }
            Some(item) => self.select_items(&[item], Combine::Replace),
        }
    }

    /// Picks `items` into the selection (replacing it, adding to it, or toggling them).
    pub fn select_items(&mut self, items: &[Item], how: Combine) -> Result<String, String> {
        self.require_map()?;
        let map = self.map.as_ref().expect("checked");
        if let Some(missing) = items.iter().find(|item| !map.contains(**item)) {
            return Err(format!("there is no {missing}"));
        }
        // A brush entity stands for its brushes.
        let units = map.units(items).or_else(|err| {
            if items.is_empty() {
                Ok(Vec::new())
            } else {
                Err(err)
            }
        })?;
        self.selection.combine(how, &units);
        Ok(self.selection_text())
    }

    /// What the selection holds, in a line.
    pub fn selection_text(&self) -> String {
        match self.selection.items() {
            [] => "Selection cleared".into(),
            [one] => format!("Selected {}", self.describe(*one)),
            many => format!("Selected {}", self.describe_many(many)),
        }
    }

    /// The items of a kind (see [`Kind`]).
    pub fn find_kind(&self, kind: &Kind) -> Result<Vec<Item>, String> {
        self.require_map()?;
        Ok(selection::of_kind(
            self.map.as_ref().expect("checked"),
            kind,
        ))
    }

    /// The items whose bounding box `test` accepts.
    pub fn find_where(&self, test: impl Fn(DVec3, DVec3) -> bool) -> Result<Vec<Item>, String> {
        self.require_map()?;
        let map = self.map.as_ref().expect("checked");
        Ok(selection::where_bounds(map, &self.prop_bounds, test))
    }

    /// The items that lie wholly inside the box `min`..`max`; `flat` ignores height.
    pub fn find_in_box(&self, min: DVec3, max: DVec3, flat: bool) -> Result<Vec<Item>, String> {
        self.find_where(|lo, hi| selection::box_inside((lo, hi), (min, max), flat))
    }

    /// The box around the selection (game space).
    pub fn selection_bounds(&self) -> Option<(DVec3, DVec3)> {
        self.map
            .as_ref()?
            .bounds_of(self.selection.items(), &self.prop_bounds)
    }

    /// Makes freshly added items the selection.
    fn select_added(&mut self, items: Vec<Item>) {
        let units = self
            .map
            .as_ref()
            .and_then(|map| map.units(&items).ok())
            .unwrap_or(items);
        self.selection = Selection::from(units);
    }

    /// One line about an item: its kind, size and place.
    pub fn describe(&self, item: Item) -> String {
        let Some(map) = &self.map else {
            return item.to_string();
        };
        let round = |v: f64| (v * 100.0).round() / 100.0 + 0.0;
        match item {
            Item::Brush { entity, brush } => match map.brush(entity, brush).and_then(Brush::aabb) {
                Some((min, max)) => {
                    let size = max - min;
                    let surface = map.brush(entity, brush).and_then(Brush::surface);
                    format!(
                        "{item} ({}, {} x {} x {} m at {}, {}, {})",
                        surface.map_or("?", |s| s.name),
                        round(size.x),
                        round(size.y),
                        round(size.z),
                        round(min.x),
                        round(min.y),
                        round(min.z),
                    )
                }
                None => item.to_string(),
            },
            Item::Entity(index) => match map.entities.get(index) {
                Some(entity) => {
                    let at = entity.origin().map_or(String::new(), |o| {
                        format!(" at {}, {}, {}", round(o.x), round(o.y), round(o.z))
                    });
                    let detail: Vec<String> = entity
                        .props
                        .iter()
                        .filter(|(k, _)| !matches!(k.as_str(), "classname" | "origin"))
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect();
                    format!(
                        "{item} ({}{at}{}{})",
                        entity.classname(),
                        if detail.is_empty() { "" } else { ": " },
                        detail.join(" ")
                    )
                }
                None => item.to_string(),
            },
        }
    }

    /// One line about several items: how many of what, and the box around them.
    pub fn describe_many(&self, items: &[Item]) -> String {
        if let [one] = items {
            return self.describe(*one);
        }
        let brushes = items
            .iter()
            .filter(|item| matches!(item, Item::Brush { .. }))
            .count();
        let round = |v: f64| (v * 100.0).round() / 100.0 + 0.0;
        let size = self
            .map
            .as_ref()
            .and_then(|map| map.bounds_of(items, &self.prop_bounds))
            .map_or(String::new(), |(min, max)| {
                let size = max - min;
                format!(
                    ", {} x {} x {} m at {}, {}, {}",
                    round(size.x),
                    round(size.y),
                    round(size.z),
                    round(min.x),
                    round(min.y),
                    round(min.z)
                )
            });
        format!(
            "{} items ({brushes} brushes, {} entities{size})",
            items.len(),
            items.len() - brushes
        )
    }

    pub fn move_item(&mut self, item: Option<Item>, delta: DVec3) -> Result<String, String> {
        let items = self.targets(item)?;
        self.edit(|map| map.translate_many(&items, delta))?;
        let what = match items.as_slice() {
            [one] => one.to_string(),
            many => format!("{} items", many.len()),
        };
        Ok(format!(
            "Moved {what} by {}, {}, {} m",
            delta.x, delta.y, delta.z
        ))
    }

    /// Scales a brush so its bounding box becomes `min`..`max`.
    pub fn resize(&mut self, item: Option<Item>, min: DVec3, max: DVec3) -> Result<String, String> {
        let item = self.resolve(item)?;
        self.edit(|map| map.resize(item, min, max))?;
        Ok(format!("Resized {}", self.describe(item)))
    }

    /// Moves one face of a brush (see [`Brush::move_face`]): every other face stays where it
    /// is. `FaceRef::Towards` slides the face along that axis (a ramp's slope straight up),
    /// `FaceRef::Index` along its own normal. `amount` is metres, outward when positive; with
    /// `snap` an axis-aligned face ends up on the grid.
    pub fn move_face(
        &mut self,
        item: Option<Item>,
        face: FaceRef,
        amount: f64,
        snap: bool,
    ) -> Result<String, String> {
        let item = self.resolve(item)?;
        let Item::Brush { entity, brush } = item else {
            return Err("only brushes have faces to move".into());
        };
        let (index, direction, amount) = self.face_move(entity, brush, face, amount, snap)?;
        self.edit(|map| map.entities[entity].brushes[brush].move_face(index, direction, amount))?;
        Ok(format!("Moved face {index} of {}", self.describe(item)))
    }

    /// Resolves a face move: the face index, the direction it slides, the (snapped) amount.
    pub fn face_move(
        &self,
        entity: usize,
        brush: usize,
        face: FaceRef,
        amount: f64,
        snap: bool,
    ) -> Result<(usize, DVec3, f64), String> {
        if !amount.is_finite() {
            return Err("`amount` is a number of metres".into());
        }
        let b = self
            .map
            .as_ref()
            .and_then(|map| map.brush(entity, brush))
            .ok_or("there is no such brush")?;
        let (index, direction) = match face {
            FaceRef::Index(index) => {
                let normal = b
                    .face_normal(index)
                    .ok_or_else(|| format!("the brush has no face {index}"))?;
                (index, normal)
            }
            FaceRef::Towards(direction) => {
                let index = b
                    .face_towards(direction)
                    .ok_or("no face of the brush looks that way")?;
                (index, direction.normalize_or_zero())
            }
        };
        let mut amount = amount;
        let normal = b.face_normal(index).unwrap_or(DVec3::ZERO);
        let axis = (0..3).find(|a| normal[*a].abs() > 0.999 && direction[*a].abs() > 0.999);
        if let (true, Some(axis)) = (snap, axis) {
            let at = mapfile::tb_to_game(b.faces[index].points[0])[axis];
            let sign = direction[axis].signum();
            let target = ((at + amount * sign) / self.grid).round() * self.grid;
            amount = (target - at) * sign;
        }
        Ok((index, direction, (amount * 1e5).round() / 1e5))
    }

    pub fn delete(&mut self, item: Option<Item>) -> Result<String, String> {
        let items = self.targets(item)?;
        let what = match items.as_slice() {
            [one] => self.describe(*one),
            many => format!("{} items", many.len()),
        };
        self.edit(|map| map.delete_many(&items))?;
        // Indices after the deleted ones moved.
        self.selection.clear();
        Ok(format!("Deleted {what}"))
    }

    pub fn duplicate(&mut self, item: Option<Item>, offset: DVec3) -> Result<String, String> {
        let items = self.targets(item)?;
        let copies = self.edit(|map| map.duplicate_many(&items, offset))?;
        self.select_added(copies);
        Ok(match self.selection.items() {
            [one] => format!("Duplicated into {}", self.describe(*one)),
            many => format!("Duplicated {} items", many.len()),
        })
    }

    /// `count` copies of the items, each `offset` further than the one before; the copies
    /// are selected.
    pub fn array(
        &mut self,
        item: Option<Item>,
        count: usize,
        offset: DVec3,
    ) -> Result<String, String> {
        let items = self.targets(item)?;
        let copies = self.edit(|map| map.linear_array(&items, count, offset))?;
        self.select_added(copies);
        Ok(format!(
            "Made {count} copies of {} item(s), {} m apart",
            items.len(),
            (offset.length() * 1000.0).round() / 1000.0
        ))
    }

    /// Paints the brushes among the items with `surface` and makes it the surface of new
    /// brushes.
    pub fn set_surface(&mut self, item: Option<Item>, surface: &str) -> Result<String, String> {
        let surface = mapfile::surface(surface).ok_or_else(|| {
            let names: Vec<&str> = SURFACES.iter().map(|s| s.name).collect();
            format!("unknown surface \"{surface}\"; known: {}", names.join(", "))
        })?;
        let items = self.targets(item)?;
        self.edit(|map| map.set_surface_many(&items, surface))?;
        self.surface = surface;
        Ok(match items.as_slice() {
            [one] => format!("Painted {} with {}", one, surface.name),
            many => format!("Painted {} items with {}", many.len(), surface.name),
        })
    }

    /// The next (or previous) surface of the first selected brush, for all of them.
    pub fn cycle_surface(&mut self, item: Option<Item>, step: i32) -> Result<String, String> {
        let items = self.targets(item)?;
        let current = items.iter().find_map(|item| match item {
            Item::Brush { entity, brush } => self
                .map
                .as_ref()
                .and_then(|m| m.brush(*entity, *brush))
                .and_then(Brush::surface),
            Item::Entity(_) => None,
        });
        let at = current
            .and_then(|s| SURFACES.iter().position(|x| x.name == s.name))
            .map_or(0, |i| i as i32);
        let next = (at + step).rem_euclid(SURFACES.len() as i32) as usize;
        self.set_surface(item, SURFACES[next].name)
    }

    /// Adds a box (`min`..`max`, metres) to the worldspawn and selects it.
    pub fn add_box(&mut self, min: DVec3, max: DVec3, surface: &Surface) -> Result<String, String> {
        if (max - min).min_element() < mapfile::MIN_SIZE {
            return Err(format!(
                "a brush needs at least {} m on every side",
                mapfile::MIN_SIZE
            ));
        }
        let item = self.edit(|map| Ok(map.add_brush(Brush::cuboid(min, max, surface))))?;
        self.selection = Selection::from(Some(item));
        Ok(format!("Added {}", self.describe(item)))
    }

    /// Adds a ramp filling `low`..`high`, climbing towards `rise`.
    pub fn add_ramp(
        &mut self,
        low: DVec3,
        high: DVec3,
        rise: Rise,
        surface: &Surface,
    ) -> Result<String, String> {
        if (high - low).min_element() < mapfile::MIN_SIZE {
            return Err(format!(
                "a brush needs at least {} m on every side",
                mapfile::MIN_SIZE
            ));
        }
        let item = self.edit(|map| Ok(map.add_brush(Brush::wedge(low, high, rise, surface))))?;
        self.selection = Selection::from(Some(item));
        Ok(format!("Added {}", self.describe(item)))
    }

    /// Adds an entity of a known class and selects it.
    pub fn add_entity(
        &mut self,
        class: &str,
        position: DVec3,
        angle: Option<f64>,
        props: &[(String, String)],
    ) -> Result<String, String> {
        let info = palette::class(class).ok_or_else(|| {
            let known: Vec<&str> = palette::CLASSES.iter().map(|c| c.class).collect();
            format!(
                "unknown entity class \"{class}\"; known: {}",
                known.join(", ")
            )
        })?;
        let entity: MapEntity = info.instantiate(position, angle, props)?;
        if let Some(model) = entity.get("model") {
            self.check_model(model)?;
        }
        let item = self.edit(|map| Ok(map.add_entity(entity)))?;
        self.selection = Selection::from(Some(item));
        Ok(format!("Added {}", self.describe(item)))
    }

    /// A prop's model must be one the Map can load: a known model, or one of the private
    /// content folder.
    fn check_model(&self, model: &str) -> Result<(), String> {
        if self.props.is_empty()
            || model.starts_with("private://")
            || self.props.iter().any(|p| p.model == model)
        {
            return Ok(());
        }
        Err(format!(
            "unknown model \"{model}\"; the Props tab of the palette lists the models (models/<pack>/<name>.glb)"
        ))
    }

    /// Whether `value` is acceptable for `key` of an entity of `class`; an empty value removes
    /// the key (then it must be removable).
    pub fn check_property(&self, class: &str, key: &str, value: &str) -> Result<(), String> {
        let Some(info) = palette::class(class) else {
            // A class the palette does not know (a hand-made Map's): only the basics.
            return if matches!(key, "classname" | "origin") {
                Err(format!("`{key}` cannot be set here"))
            } else {
                Ok(())
            };
        };
        if value.is_empty() {
            return info.check_removal(key);
        }
        info.check(key, value)?;
        if key == "model" {
            self.check_model(value)?;
        }
        Ok(())
    }

    /// Changes entities (the selection, or `item`): key/values, facing, position; all checked
    /// first against what the entity's class accepts, then applied as one undo step.
    pub fn edit_entity(
        &mut self,
        item: Option<Item>,
        change: EntityChange,
    ) -> Result<String, String> {
        let items = self.targets(item)?;
        if items.contains(&Item::Entity(0)) {
            return Err("the worldspawn's name and look are Map settings".into());
        }
        let map = self.map.as_ref().expect("checked");
        let units = map.units(&items)?;
        let mut indices = Vec::new();
        for unit in &units {
            match unit {
                Item::Entity(index) => indices.push(*index),
                Item::Brush { .. } => {
                    return Err("only entities have properties; use set_texture for a brush".into());
                }
            }
        }
        if change.position.is_some() && indices.len() != 1 {
            return Err("a position can be set on one entity at a time".into());
        }
        let mut set = change.set.clone();
        if let Some(angle) = change.angle {
            if !angle.is_finite() {
                return Err("`angle` is a number of degrees".into());
            }
            set.push((
                "angle".to_owned(),
                mapfile::format_number(transform::normal_angle(angle)),
            ));
        }
        if let Some(position) = change.position
            && !position.is_finite()
        {
            return Err("`position` is [x, y, z] in metres".into());
        }
        for &index in &indices {
            let class = map.entities[index].classname().to_owned();
            for (key, value) in &set {
                self.check_property(&class, key, value)?;
            }
        }
        self.edit(|map| {
            for &index in &indices {
                let entity = &mut map.entities[index];
                for (key, value) in &set {
                    if value.is_empty() {
                        entity.remove(key);
                    } else {
                        entity.set(key, value);
                    }
                }
                if let Some(position) = change.position {
                    entity.set_origin(position);
                }
            }
            Ok(())
        })?;
        Ok(match (indices.as_slice(), set.as_slice()) {
            ([one], [(key, _)]) if change.position.is_none() => {
                format!("Set {key} of {}", self.describe(Item::Entity(*one)))
            }
            ([one], _) => format!("Changed {}", self.describe(Item::Entity(*one))),
            (many, _) => format!("Changed {} entities", many.len()),
        })
    }

    /// Sets a property of entities (`angle`, `team`, `model`, ...); an empty value removes it.
    pub fn set_property(
        &mut self,
        item: Option<Item>,
        key: &str,
        value: &str,
    ) -> Result<String, String> {
        self.edit_entity(
            item,
            EntityChange {
                set: vec![(key.to_owned(), value.to_owned())],
                ..EntityChange::default()
            },
        )
    }

    /// Turns the items counter-clockwise (seen from above) by `degrees` about the vertical axis
    /// through `pivot` (x and z), by default the middle of their bounding box. Brushes keep
    /// their textures on their faces; entities that face somewhere turn their `angle`.
    pub fn turn(
        &mut self,
        item: Option<Item>,
        degrees: f64,
        pivot: Option<DVec3>,
    ) -> Result<String, String> {
        if !degrees.is_finite() {
            return Err("`degrees` is a number".into());
        }
        let items = self.targets(item)?;
        let pivot = match pivot {
            Some(pivot) => pivot,
            None => {
                let (min, max) = self
                    .map
                    .as_ref()
                    .and_then(|map| map.bounds_of(&items, &self.prop_bounds))
                    .ok_or("the items have no size")?;
                (min + max) / 2.0
            }
        };
        let transform = Motion::Turn { degrees, pivot };
        self.edit(|map| map.transform_many(&items, &transform, &faces))?;
        Ok(format!(
            "Turned {} item(s) {} degrees about {:.2}, {:.2}",
            items.len(),
            (degrees * 1e4).round() / 1e4,
            pivot.x,
            pivot.z
        ))
    }

    /// Mirrors the items in the plane perpendicular to `axis` at coordinate `at` (default: the
    /// middle of their bounding box). With `copy` the mirror images are added and selected, and
    /// the originals stay.
    pub fn mirror(
        &mut self,
        item: Option<Item>,
        axis: MirrorAxis,
        at: Option<f64>,
        copy: bool,
    ) -> Result<String, String> {
        let items = self.targets(item)?;
        let at = match at {
            Some(at) if at.is_finite() => at,
            Some(_) => return Err("`at` is a coordinate in metres".into()),
            None => {
                let (min, max) = self
                    .map
                    .as_ref()
                    .and_then(|map| map.bounds_of(&items, &self.prop_bounds))
                    .ok_or("the items have no size")?;
                (min[axis.index()] + max[axis.index()]) / 2.0
            }
        };
        let transform = Motion::Mirror { axis, at };
        if copy {
            let copies = self.edit(|map| {
                let copies = map.duplicate_many(&items, DVec3::ZERO)?;
                map.transform_many(&copies, &transform, &faces)?;
                Ok(copies)
            })?;
            self.select_added(copies);
        } else {
            self.edit(|map| map.transform_many(&items, &transform, &faces))?;
        }
        Ok(format!(
            "Mirrored {} item(s) about {} = {:.2}{}",
            items.len(),
            axis.key(),
            at,
            if copy { " (copies)" } else { "" }
        ))
    }

    /// Copies the items to the clipboard.
    pub fn copy(&mut self, item: Option<Item>) -> Result<String, String> {
        let items = self.targets(item)?;
        let fragment = self.map.as_ref().expect("checked").extract(&items)?;
        let count = fragment.items().len();
        self.clipboard = Some(fragment);
        Ok(format!("Copied {count} item(s)"))
    }

    /// Pastes the clipboard moved by `offset`; the pasted items are selected.
    pub fn paste(&mut self, offset: DVec3) -> Result<String, String> {
        let fragment = self
            .clipboard
            .clone()
            .ok_or("the clipboard is empty (copy first)")?;
        let added = self.edit(|map| Ok(map.insert_fragment(&fragment, offset)))?;
        self.select_added(added);
        Ok(format!("Pasted {} item(s)", self.selection.len()))
    }

    /// Where the clipboard's anchor (see [`prefab::anchor`]) is now, for pasting it elsewhere.
    pub fn clipboard_anchor(&self) -> Option<DVec3> {
        let bounds = self.clipboard.as_ref()?.content_bounds(&self.prop_bounds)?;
        Some(prefab::anchor(bounds))
    }

    /// Pastes the clipboard so that its anchor lands on `point`, in whole grid steps across
    /// the ground.
    pub fn paste_at(&mut self, point: DVec3) -> Result<String, String> {
        let anchor = self
            .clipboard_anchor()
            .ok_or("the clipboard is empty (copy first)")?;
        self.paste(prefab::paste_offset(anchor, point, self.grid))
    }

    /// Saves the items as prefab `name` (see [`Paths::prefab_folder`]).
    pub fn save_prefab(
        &mut self,
        item: Option<Item>,
        name: &str,
        overwrite: bool,
    ) -> Result<String, String> {
        let items = self.targets(item)?;
        let fragment = self.map.as_ref().expect("checked").extract(&items)?;
        let dir = self.paths.prefab_folder(self.private)?;
        let path = prefab::save(&dir, name, &fragment, &self.prop_bounds, overwrite)?;
        self.prefab_list = self.paths.prefab_list();
        Ok(format!("Saved prefab {name} to {}", path.display()))
    }

    /// The prefabs there are, public ones then private ones.
    pub fn prefabs(&self) -> Vec<PrefabInfo> {
        self.paths.prefab_list()
    }

    /// Places prefab `name` with its anchor at `point`, turned by `degrees` about it; the
    /// placed items are selected.
    pub fn place_prefab(
        &mut self,
        name: &str,
        point: DVec3,
        degrees: f64,
    ) -> Result<String, String> {
        self.require_map()?;
        let info = self
            .prefabs()
            .into_iter()
            .find(|p| p.name == name)
            .ok_or_else(|| {
                let names: Vec<String> = self.prefabs().into_iter().map(|p| p.name).collect();
                format!("no prefab \"{name}\"; known: {}", names.join(", "))
            })?;
        let mut fragment = prefab::load(&self.paths.prefab_folder(info.private)?, name)?;
        if degrees != 0.0 {
            let items = fragment.items();
            let turn = Motion::Turn {
                degrees,
                pivot: DVec3::ZERO,
            };
            fragment.transform_many(&items, &turn, &faces)?;
        }
        let added = self.edit(|map| Ok(map.insert_fragment(&fragment, point)))?;
        self.select_added(added);
        Ok(format!(
            "Placed prefab {name} ({} item(s))",
            self.selection.len()
        ))
    }

    pub fn undo(&mut self) -> Result<String, String> {
        self.require_map()?;
        let snapshot = self.undo.pop().ok_or("nothing to undo")?;
        let current = self.map.take().expect("checked");
        self.redo.push(Snapshot {
            map: current,
            selection: self.selection.clone(),
        });
        self.map = Some(snapshot.map);
        self.restore_selection(snapshot.selection);
        self.bump();
        Ok("Undone".into())
    }

    pub fn redo(&mut self) -> Result<String, String> {
        self.require_map()?;
        let snapshot = self.redo.pop().ok_or("nothing to redo")?;
        let current = self.map.take().expect("checked");
        self.undo.push(Snapshot {
            map: current,
            selection: self.selection.clone(),
        });
        self.map = Some(snapshot.map);
        self.restore_selection(snapshot.selection);
        self.bump();
        Ok("Redone".into())
    }

    fn restore_selection(&mut self, selection: Selection) {
        self.selection = selection;
        if let Some(map) = &self.map {
            self.selection.retain(|item| map.contains(item));
        }
    }

    /// Item under a ray (game space).
    pub fn pick(&self, origin: DVec3, direction: DVec3) -> Option<mapfile::Hit> {
        self.map
            .as_ref()?
            .pick(origin, direction, &self.prop_bounds)
    }

    /// Where a ray meets the Map, else the ground plane, else 8 m ahead: the point and surface
    /// normal new things are placed on.
    pub fn target(&self, origin: DVec3, direction: DVec3) -> (DVec3, DVec3) {
        let direction = direction.normalize_or_zero();
        if let Some(hit) = self.pick(origin, direction) {
            return (origin + direction * hit.distance, hit.normal);
        }
        if direction.y < -1e-6 {
            let t = -origin.y / direction.y;
            if t > 0.0 {
                return (origin + direction * t, DVec3::Y);
            }
        }
        (origin + direction * 8.0, DVec3::Y)
    }

    /// Where a primitive goes when placed on `point` with surface `normal`: the corner box,
    /// resting on or against the surface, snapped to the grid along the surface.
    pub fn primitive_bounds(
        &self,
        primitive: &palette::Primitive,
        rise: Rise,
        point: DVec3,
        normal: DVec3,
    ) -> (DVec3, DVec3) {
        let size = primitive.extents(rise);
        let half = size / 2.0;
        let cardinal = {
            let n = normal.abs();
            let axis = if n.x >= n.y && n.x >= n.z {
                0
            } else if n.y >= n.z {
                1
            } else {
                2
            };
            let mut c = DVec3::ZERO;
            c[axis] = normal[axis].signum();
            c
        };
        let centre = point + cardinal * half;
        let mut min = centre - half;
        for axis in 0..3 {
            let snapped = if cardinal[axis] == 0.0 {
                (min[axis] / self.grid).round() * self.grid
            } else {
                (min[axis] * 1000.0).round() / 1000.0
            };
            min[axis] = snapped;
        }
        (min, min + size)
    }

    /// Where an entity of `class` goes on `point`.
    pub fn entity_position(&self, class: &str, point: DVec3, normal: DVec3) -> DVec3 {
        let lift = palette::class(class).map_or(0.0, |c| c.lift);
        let lift = if normal.y < -0.5 { -0.3 } else { lift };
        DVec3::new(
            (point.x / self.grid).round() * self.grid,
            (point.y * 1000.0).round() / 1000.0 + lift,
            (point.z / self.grid).round() * self.grid,
        )
    }

    /// Forgets the open Map: restores its registry entry and deletes the preview.
    fn close(&mut self, registry: &mut MapRegistry) {
        if let Some(file) = self.original_file.take()
            && let Some(info) = registry.get_mut(&self.id)
        {
            info.file = file;
        }
        if let Some(preview) = self.preview.take() {
            let _ = std::fs::remove_file(preview.disk);
        }
        self.map = None;
        self.saved = None;
        self.problem = None;
        self.source = None;
        self.source_mtime = None;
        self.baseline = None;
        self.settings_dirty = false;
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        self.selection.clear();
        self.hover = None;
    }

    /// Reads the Map `id` from its file and starts a preview of it. Returns what happened.
    fn open(&mut self, registry: &mut MapRegistry, id: &MapId) -> String {
        let info = registry.get(id).cloned().expect("checked by the caller");
        self.id = id.clone();
        self.private = info.private;
        self.source = self.paths.source(&info.file, info.private);
        self.source_mtime = self.source.as_deref().and_then(mtime);
        self.baseline = Some(entry_of(&info, &info.file));
        let loaded = match &self.source {
            None => Err(
                "the Map's file cannot be found (is the private content folder there?)".to_owned(),
            ),
            Some(path) => std::fs::read_to_string(path)
                .map_err(|err| format!("{}: {err}", path.display()))
                .and_then(|text| {
                    MapFile::parse(&text).map_err(|err| format!("{}: {err}", path.display()))
                }),
        };
        let summary = match loaded {
            Ok(map) => {
                let brushes: usize = map.entities.iter().map(|e| e.brushes.len()).sum();
                let open = map
                    .entities
                    .iter()
                    .flat_map(|e| &e.brushes)
                    .filter(|b| !b.is_solid())
                    .count();
                let summary = format!(
                    "Opened {id}: {brushes} brushes, {} entities{}",
                    map.entities.len() - 1,
                    if open > 0 {
                        format!(" ({open} brushes do not close up)")
                    } else {
                        String::new()
                    }
                );
                match self.start_preview(registry, &info, &map) {
                    Ok(()) => {
                        self.saved = Some(map.clone());
                        self.map = Some(map);
                        summary
                    }
                    Err(problem) => {
                        self.problem = Some(problem.clone());
                        format!("Opened {id} view only: {problem}")
                    }
                }
            }
            Err(problem) => {
                self.problem = Some(problem.clone());
                format!("Opened {id} view only: {problem}")
            }
        };
        self.bump();
        self.shown_revision = self.revision;
        self.last_write = f64::MIN;
        self.dirty = false;
        summary
    }

    fn start_preview(
        &mut self,
        registry: &mut MapRegistry,
        info: &MapInfo,
        map: &MapFile,
    ) -> Result<(), String> {
        let source = self.source.as_deref().ok_or("no file")?;
        let (asset, name) = preview_name(&info.file);
        let disk = source.with_file_name(name);
        std::fs::write(&disk, map.to_text())
            .map_err(|err| format!("cannot write the live preview {}: {err}", disk.display()))?;
        self.original_file = Some(info.file.clone());
        if let Some(entry) = registry.get_mut(&self.id) {
            entry.file = asset;
        }
        self.preview = Some(Preview { disk });
        Ok(())
    }

    /// Writes the preview when the working copy moved on. Returns whether it did.
    fn write_preview(&mut self, now: f64) -> bool {
        if self.synced() || now - self.last_write < WRITE_EVERY {
            return false;
        }
        let (Some(map), Some(preview)) = (&self.map, &self.preview) else {
            return false;
        };
        if let Err(err) = std::fs::write(&preview.disk, map.to_text()) {
            let text = format!("cannot write the live preview: {err}");
            self.complain(text);
            return false;
        }
        self.shown_revision = self.revision;
        self.last_write = now;
        true
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        if let Some(preview) = self.preview.take() {
            let _ = std::fs::remove_file(preview.disk);
        }
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

/// The preview's asset path and file name for the Map file `file`: `maps/arena.map` gets
/// `maps/.mapedit-arena.map`.
fn preview_name(file: &str) -> (String, String) {
    let (dir, name) = file.rsplit_once('/').unwrap_or(("", file));
    let name = format!("{PREVIEW_PREFIX}{name}");
    let asset = if dir.is_empty() {
        name.clone()
    } else {
        format!("{dir}/{name}")
    };
    (asset, name)
}

/// The `tools/mapgen` script that writes this Map file, if there is one: saving over its output
/// is undone by the next run of the script.
pub fn generator_for(source: &Path) -> Option<PathBuf> {
    generator_in(Path::new("tools/mapgen"), source)
}

fn generator_in(scripts: &Path, source: &Path) -> Option<PathBuf> {
    let stem = source.file_stem()?.to_str()?;
    if matches!(stem, "mapgen" | "kit" | "lint") {
        return None;
    }
    let script = scripts.join(format!("{stem}.py"));
    script.is_file().then_some(script)
}

fn entry_of(info: &MapInfo, runtime_file: &str) -> Entry {
    Entry {
        name: info.name.clone(),
        file: runtime_file
            .strip_prefix("private://")
            .unwrap_or(runtime_file)
            .to_owned(),
        look: info.look.clone(),
        modes: info.modes.clone(),
        factions: info.factions.clone(),
    }
}

// ----------------------------------------------------------------------------- commands

/// A change of the Map's name, look, Game Modes or Factions; `None` leaves a field alone.
#[derive(Clone, Debug, Default)]
pub struct SettingsChange {
    pub name: Option<String>,
    /// `Some(None)`: the Map's own (worldspawn) look applies.
    pub look: Option<Option<String>>,
    /// Empty means every mode.
    pub modes: Option<Vec<String>>,
    pub factions: Option<[String; 2]>,
}

/// Something that changes files or the Map registry.
#[derive(Message, Clone, Debug)]
pub enum Command {
    /// Opens a Map of the registry. Refused while there are unsaved changes unless `discard`.
    Open {
        id: MapId,
        discard: bool,
    },
    /// Registers and opens a loose `.map` file (a stem in `maps/`, or in the private folder).
    OpenFile {
        stem: String,
        private: bool,
        discard: bool,
    },
    Save,
    /// Saves, then runs the command with unsaved changes no longer in the way. When the save
    /// fails the command does not run.
    SaveAnd(Box<Command>),
    /// Copies the open Map to a new Map `id` (registered in `maps.ron`) and opens the copy.
    SaveAs {
        id: String,
        name: String,
    },
    /// A new Map: a floor, two spawns per Team, a worldspawn with a look.
    NewMap {
        id: String,
        name: String,
        discard: bool,
    },
    Settings(SettingsChange),
    /// Starts the game on the Map as edited (see [`crate::playtest`]).
    Playtest,
    Quit {
        discard: bool,
    },
}

impl Command {
    /// The command with its "discard unsaved changes" answer set to yes.
    pub fn discarding(self) -> Command {
        match self {
            Command::Open { id, .. } => Command::Open { id, discard: true },
            Command::OpenFile { stem, private, .. } => Command::OpenFile {
                stem,
                private,
                discard: true,
            },
            Command::NewMap { id, name, .. } => Command::NewMap {
                id,
                name,
                discard: true,
            },
            Command::Quit { .. } => Command::Quit { discard: true },
            other => other,
        }
    }
}

/// Why a [`Command`] did not run.
#[derive(Clone, Debug)]
pub enum Failure {
    /// There are unsaved changes the command would lose.
    Dirty,
    Message(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Message(message)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Failure::Message(message.to_owned())
    }
}

fn send_reload(world: &mut World) {
    world.resource_mut::<Messages<Reload>>().write(Reload);
}

/// Ids are file names and `--map` arguments: lowercase letters, digits and underscores.
pub fn valid_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !id.starts_with('_');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "\"{id}\" is not a usable Map id: 1 to 40 of a-z, 0-9 and _, not starting with _"
        ))
    }
}

/// Executes a command now. The menu sends them as messages ([`run_commands`]); the remote
/// methods call this directly to answer with the outcome.
pub fn execute(world: &mut World, command: Command) -> Result<String, Failure> {
    match command {
        Command::Open { id, discard } => open(world, id, discard),
        Command::OpenFile {
            stem,
            private,
            discard,
        } => open_file(world, &stem, private, discard),
        Command::Save => save(world),
        Command::SaveAnd(then) => {
            let saved = save(world)?;
            let then = execute(world, then.discarding())?;
            Ok(format!("{saved}. {then}"))
        }
        Command::SaveAs { id, name } => save_as(world, &id, &name),
        Command::NewMap { id, name, discard } => new_map(world, &id, &name, discard),
        Command::Settings(change) => settings(world, change),
        Command::Playtest => playtest(world),
        Command::Quit { discard } => {
            if world.resource::<Editor>().dirty() && !discard {
                return Err(Failure::Dirty);
            }
            world
                .resource_mut::<Messages<AppExit>>()
                .write(AppExit::Success);
            Ok("Quitting".into())
        }
    }
}

/// Writes the working copy where the game can read it and starts the game on it.
fn playtest(world: &mut World) -> Result<String, Failure> {
    let editor = world.resource::<Editor>();
    editor.require_map()?;
    let map = editor.map.as_ref().expect("checked");
    let source = editor.source.as_ref().ok_or("the Map has no file")?;
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .ok_or("the Map has no file name")?;
    let file = source.with_file_name(format!("{PREVIEW_PREFIX}play-{stem}.map"));
    std::fs::write(&file, map.to_text())
        .map_err(|err| format!("cannot write {}: {err}", file.display()))?;
    let own = std::env::current_exe().map_err(|err| format!("cannot find MapEdit: {err}"))?;
    let game = playtest::game_executable(std::env::var_os(playtest::GAME_ENV), &own)?;
    let assets = editor.paths.assets.clone();
    let root = assets.parent().unwrap_or(&assets);
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let pid = playtest::launch(&game, &file, &root)?;
    Ok(format!(
        "Playtest started (process {pid}): {} on {}",
        game.display(),
        file.display()
    ))
}
fn open(world: &mut World, id: MapId, discard: bool) -> Result<String, Failure> {
    {
        let registry = world.resource::<MapRegistry>();
        if registry.get(&id).is_none() {
            return Err(format!("unknown Map \"{id}\"; known: {}", registry.known_ids()).into());
        }
    }
    if world.resource::<Editor>().dirty() && !discard {
        return Err(Failure::Dirty);
    }
    let summary = world.resource_scope(|world, mut editor: Mut<Editor>| {
        let mut registry = world.resource_mut::<MapRegistry>();
        editor.close(&mut registry);
        editor.open(&mut registry, &id)
    });
    world.resource_mut::<IdleMap>().0 = id;
    send_reload(world);
    Ok(summary)
}

/// A `.map` file not in the registry (see [`loose_maps`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LooseMap {
    pub stem: String,
    pub private: bool,
}

impl LooseMap {
    fn runtime_file(&self) -> String {
        if self.private {
            format!("private://maps/{}.map", self.stem)
        } else {
            format!("maps/{}.map", self.stem)
        }
    }

    fn disk(&self, paths: &Paths) -> Option<PathBuf> {
        let file = self.runtime_file();
        paths.source(&file, self.private)
    }
}

/// The `.map` files of the Map folders that no registry entry uses.
pub fn loose_maps(paths: &Paths, registry: &MapRegistry) -> Vec<LooseMap> {
    let used: HashSet<&str> = registry
        .iter()
        .map(|(_, info)| info.file.as_str())
        .collect();
    let mut found = Vec::new();
    for (private, folder) in paths.map_folders() {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        let mut stems: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let stem = name.strip_suffix(".map")?;
                (!stem.starts_with(PREVIEW_PREFIX)).then(|| stem.to_owned())
            })
            .collect();
        stems.sort();
        for stem in stems {
            let loose = LooseMap { stem, private };
            // The registry may point at a preview of it right now: compare by the real name.
            let file = loose.runtime_file();
            let previewed = registry
                .iter()
                .any(|(_, info)| info.file == preview_name(&file).0);
            if !used.contains(file.as_str()) && !previewed {
                found.push(loose);
            }
        }
    }
    found
}

fn open_file(
    world: &mut World,
    stem: &str,
    private: bool,
    discard: bool,
) -> Result<String, Failure> {
    let loose = LooseMap {
        stem: stem.to_owned(),
        private,
    };
    let on_disk = loose.disk(&world.resource::<Editor>().paths);
    if on_disk.is_none_or(|path| !path.is_file()) {
        return Err(format!("there is no {}", loose.runtime_file()).into());
    }
    if world.resource::<Editor>().dirty() && !discard {
        return Err(Failure::Dirty);
    }
    // An id that is free: the stem, made a valid id, with a number when taken.
    let base: String = stem
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let base = base.trim_start_matches('_');
    let base = if base.is_empty() { "map" } else { base };
    let factions = default_factions(world);
    let mut registry = world.resource_mut::<MapRegistry>();
    let mut id = MapId::new(base);
    let mut n = 2;
    while registry.get(&id).is_some() {
        id = MapId::new(format!("{base}_{n}"));
        n += 1;
    }
    registry.insert(
        id.clone(),
        MapInfo {
            name: stem.to_owned(),
            file: loose.runtime_file(),
            look: None,
            modes: Vec::new(),
            factions,
            private,
        },
    );
    world.resource_mut::<Editor>().loose.insert(id.clone());
    open(world, id, discard)
}

fn default_factions(world: &World) -> [String; 2] {
    let data = world.resource::<GameData>();
    [data.factions[0].id.clone(), data.factions[1].id.clone()]
}

fn write_registry(
    paths: &Paths,
    private: bool,
    id: &MapId,
    entry: &Entry,
) -> Result<PathBuf, String> {
    let path = paths.registry_file(private)?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) if private => mapsron::empty_private(),
        Err(_) => {
            return Err(format!(
                "{} was not found: run MapEdit from the repository root",
                path.display()
            ));
        }
    };
    let updated = mapsron::upsert(&text, &id.0, entry)?;
    std::fs::write(&path, updated).map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(path)
}

fn save(world: &mut World) -> Result<String, Failure> {
    world.resource_scope(|world, mut editor: Mut<Editor>| {
        editor.require_map()?;
        let source = editor
            .source
            .clone()
            .ok_or("the Map has no file to write")?;
        let text = editor.map.as_ref().expect("checked").to_text();
        std::fs::write(&source, text).map_err(|err| format!("{}: {err}", source.display()))?;
        let mut said = format!("Saved {}", source.display());
        let id = editor.id.clone();
        if editor.settings_dirty || editor.loose.contains(&id) {
            let registry = world.resource::<MapRegistry>();
            let info = registry
                .get(&id)
                .ok_or("the Map left the registry")?
                .clone();
            let file = editor.original_file.clone().unwrap_or(info.file.clone());
            let entry = entry_of(&info, &file);
            let path = write_registry(&editor.paths, editor.private, &id, &entry)?;
            editor.baseline = Some(entry);
            editor.settings_dirty = false;
            editor.loose.remove(&id);
            said.push_str(&format!(" and its entry in {}", path.display()));
        }
        editor.saved = editor.map.clone();
        editor.source_mtime = mtime(&source);
        editor.refresh_dirty();
        if let Some(script) = generator_for(&source) {
            said.push_str(&format!(
                ". Warning: {} generates this Map; running it overwrites these edits",
                script.display()
            ));
        }
        Ok(said)
    })
}

/// A Map's files: where its `.map` goes, and how the registry names it.
struct Target {
    disk: PathBuf,
    runtime_file: String,
    ron_file: String,
}

fn check_new_id(world: &World, id: &str, private: bool) -> Result<Target, Failure> {
    valid_id(id)?;
    if world
        .resource::<MapRegistry>()
        .get(&MapId::new(id))
        .is_some()
    {
        return Err(format!("a Map called \"{id}\" exists").into());
    }
    let target = world.resource::<Editor>().paths.target(id, private)?;
    if target.disk.exists() {
        return Err(format!("{} exists; pick another id", target.disk.display()).into());
    }
    Ok(target)
}

fn display_name(id: &str, name: &str) -> String {
    let name = name.trim();
    if !name.is_empty() {
        return name.to_owned();
    }
    id.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn register(world: &mut World, id: &str, target: &Target, info: MapInfo) -> Result<(), Failure> {
    let entry = Entry {
        name: info.name.clone(),
        file: target.ron_file.clone(),
        look: info.look.clone(),
        modes: info.modes.clone(),
        factions: info.factions.clone(),
    };
    let private = info.private;
    write_registry(
        &world.resource::<Editor>().paths,
        private,
        &MapId::new(id),
        &entry,
    )?;
    world
        .resource_mut::<MapRegistry>()
        .insert(MapId::new(id), info);
    Ok(())
}

fn save_as(world: &mut World, id: &str, name: &str) -> Result<String, Failure> {
    let (mut copy, mut info) = {
        let editor = world.resource::<Editor>();
        editor.require_map()?;
        let registry = world.resource::<MapRegistry>();
        let info = registry
            .get(&editor.id)
            .ok_or("the open Map left the registry")?
            .clone();
        (editor.map.clone().expect("checked"), info)
    };
    let private = info.private;
    let target = check_new_id(world, id, private)?;
    let name = display_name(id, name);
    if let Some(world_entity) = copy.worldspawn_mut() {
        world_entity.set("name", &name);
    }
    std::fs::write(&target.disk, copy.to_text())
        .map_err(|err| format!("{}: {err}", target.disk.display()))?;
    info.name = name;
    info.file = target.runtime_file.clone();
    register(world, id, &target, info)?;
    let mut said = format!("Saved a copy as {}", target.disk.display());
    if let Some(script) = generator_for(&target.disk) {
        said.push_str(&format!(
            ". Warning: {} also generates it",
            script.display()
        ));
    }
    open(world, MapId::new(id), true)?;
    Ok(said)
}

/// The look a new Map starts with.
const DEFAULT_LOOK: &str = "overcast_urban";

fn new_map(world: &mut World, id: &str, name: &str, discard: bool) -> Result<String, Failure> {
    if world.resource::<Editor>().dirty() && !discard {
        return Err(Failure::Dirty);
    }
    let target = check_new_id(world, id, false)?;
    let name = display_name(id, name);
    let look = {
        let looks = &world.resource::<Editor>().look_names;
        if looks.iter().any(|l| l == DEFAULT_LOOK) {
            DEFAULT_LOOK.to_owned()
        } else {
            looks.first().cloned().unwrap_or_default()
        }
    };
    let ground = mapfile::surface("ground").expect("ground is a surface");
    let mut map = MapFile::with_worldspawn(&name, &look);
    map.add_brush(Brush::cuboid(
        DVec3::new(-30.0, -1.0, -30.0),
        DVec3::new(30.0, 0.0, 30.0),
        ground,
    ));
    // Alpha at the south edge facing north (-z), Bravo at the north edge facing south.
    for (team, z, angle) in [("alpha", 24.0, 0.0), ("bravo", -24.0, 180.0)] {
        for x in [-4.0, 4.0] {
            map.add_entity(MapEntity::point(
                "spawn_point",
                DVec3::new(x, 0.0, z),
                Some(angle),
                &[("team", team)],
            ));
        }
    }
    std::fs::write(&target.disk, map.to_text())
        .map_err(|err| format!("{}: {err}", target.disk.display()))?;
    let info = MapInfo {
        name: name.clone(),
        file: target.runtime_file.clone(),
        look: Some(look),
        // Spawns serve every mode, but only Team Deathmatch needs nothing else.
        modes: vec!["tdm".into()],
        factions: default_factions(world),
        private: false,
    };
    register(world, id, &target, info)?;
    open(world, MapId::new(id), true)?;
    Ok(format!(
        "Created {} (\"{name}\"): a floor and two spawns per Team, listed in data/maps.ron for Team Deathmatch",
        target.disk.display()
    ))
}

fn settings(world: &mut World, change: SettingsChange) -> Result<String, Failure> {
    if let Some(Some(look)) = &change.look {
        let known = &world.resource::<Editor>().look_names;
        if !known.contains(look) {
            return Err(format!("unknown look \"{look}\"; known: {}", known.join(", ")).into());
        }
    }
    if let Some(modes) = &change.modes {
        for mode in modes {
            if !crate::overlay::MODE_KEYS.contains(&mode.as_str()) {
                return Err(format!(
                    "unknown Game Mode \"{mode}\"; known: {}",
                    crate::overlay::MODE_KEYS.join(", ")
                )
                .into());
            }
        }
    }
    if let Some(factions) = &change.factions {
        let data = world.resource::<GameData>();
        for faction in factions {
            if data.faction_by_id(faction).is_none() {
                let known: Vec<&str> = data.factions.iter().map(|f| f.id.as_str()).collect();
                return Err(
                    format!("unknown faction \"{faction}\"; known: {}", known.join(", ")).into(),
                );
            }
        }
    }
    if change.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err("the name cannot be empty".into());
    }
    let mut reload = false;
    let said = world.resource_scope(
        |world, mut editor: Mut<Editor>| -> Result<String, Failure> {
            let mut registry = world.resource_mut::<MapRegistry>();
            let id = editor.id.clone();
            let info = registry.get_mut(&id).ok_or("no Map is open")?;
            if let Some(name) = &change.name {
                info.name = name.trim().to_owned();
            }
            if let Some(look) = &change.look {
                if info.look != *look {
                    reload = true;
                }
                info.look = look.clone();
            }
            if let Some(modes) = &change.modes {
                info.modes = modes.clone();
            }
            if let Some(factions) = &change.factions {
                info.factions = factions.clone();
            }
            let info = info.clone();
            let file = editor.original_file.clone().unwrap_or(info.file.clone());
            editor.settings_dirty = editor.baseline.as_ref() != Some(&entry_of(&info, &file));
            // The worldspawn carries the name and look too.
            let name = change.name.as_ref().map(|n| n.trim().to_owned());
            let look = change.look.clone().flatten();
            if editor.map.is_some() && (name.is_some() || look.is_some()) {
                let differs = editor
                    .map
                    .as_ref()
                    .and_then(MapFile::worldspawn)
                    .is_some_and(|w| {
                        name.as_deref().is_some_and(|n| w.get("name") != Some(n))
                            || look.as_deref().is_some_and(|l| w.get("look") != Some(l))
                    });
                if differs {
                    editor.edit(|map| {
                        let world = map.worldspawn_mut().ok_or("the Map has no worldspawn")?;
                        if let Some(name) = &name {
                            world.set("name", name);
                        }
                        if let Some(look) = &look {
                            world.set("look", look);
                        }
                        Ok(())
                    })?;
                }
            }
            editor.refresh_dirty();
            Ok(format!(
                "Settings of {id} updated (Save writes data/maps.ron)"
            ))
        },
    )?;
    if reload {
        send_reload(world);
    }
    Ok(said)
}

// ------------------------------------------------------------------------------- systems

pub struct EditPlugin {
    pub assets: PathBuf,
}

impl Plugin for EditPlugin {
    fn build(&self, app: &mut App) {
        let paths = Paths::live(self.assets.clone());
        paths.remove_stale_previews();
        app.insert_resource(Editor::new(paths, palette::load_props(&self.assets)))
            .init_resource::<Interaction>()
            .add_message::<Command>()
            .add_message::<PlaceRequest>()
            .add_systems(Startup, (open_initial, spawn_box_rect))
            .add_systems(
                Update,
                (
                    run_commands,
                    watch_source,
                    edit_input,
                    place,
                    sync_view,
                    age_notice,
                    draw_selection,
                    show_box_rect,
                )
                    .chain(),
            );
    }
}

fn open_initial(world: &mut World) {
    let looks: Vec<String> = world
        .resource::<Looks>()
        .names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    world.resource_mut::<Editor>().look_names = looks;
    let id = world.resource::<IdleMap>().0.clone();
    if let Err(Failure::Message(message)) = execute(world, Command::Open { id, discard: true }) {
        error!("{message}");
    }
    // Nothing is loaded yet, so there is nothing to reload.
    world.resource_mut::<Messages<Reload>>().clear();
}

/// Runs the commands the menu sent.
fn run_commands(world: &mut World) {
    let commands: Vec<Command> = world.resource_mut::<Messages<Command>>().drain().collect();
    for command in commands {
        let stays_open = matches!(command, Command::Settings(_));
        match execute(world, command.clone()) {
            Ok(text) => {
                info!("{text}");
                world.resource_mut::<Editor>().say(text);
                if !stays_open {
                    world.resource_mut::<MenuState>().close();
                }
            }
            Err(Failure::Dirty) => world.resource_mut::<MenuState>().confirm_discard(command),
            Err(Failure::Message(text)) => {
                warn!("{text}");
                world.resource_mut::<Editor>().complain(text);
            }
        }
    }
}

/// Reloads the Map when its file changed on disk (a regenerated Map shows up at once), unless
/// there are unsaved edits, which are kept.
fn watch_source(
    time: Res<Time>,
    mut editor: ResMut<Editor>,
    mut since: Local<f32>,
    mut commands: MessageWriter<Command>,
) {
    *since += time.delta_secs();
    if *since < WATCH_EVERY {
        return;
    }
    *since = 0.0;
    let Some(source) = editor.source.clone() else {
        return;
    };
    let now = mtime(&source);
    if now == editor.source_mtime {
        return;
    }
    editor.source_mtime = now;
    if now.is_none() {
        return;
    }
    if editor.dirty() {
        editor.complain(format!(
            "{} changed on disk; your unsaved edits are kept (Save overwrites the file)",
            source.display()
        ));
    } else {
        info!("{} changed on disk; reloading", source.display());
        commands.write(Command::Open {
            id: editor.id.clone(),
            discard: true,
        });
    }
}

/// After an edit: writes the preview and has the viewer reload it.
fn sync_view(time: Res<Time>, mut editor: ResMut<Editor>, mut reload: MessageWriter<Reload>) {
    if editor.write_preview(time.elapsed_secs_f64()) {
        reload.write(Reload);
    }
}

fn age_notice(time: Res<Time>, mut editor: ResMut<Editor>) {
    if let Some(notice) = &mut editor.notice {
        notice.age += time.delta_secs();
        if notice.age > NOTICE_SECS {
            editor.notice = None;
        }
    }
}

// ----------------------------------------------------------------------------- the cursor

/// Something to place at the cursor (or in front of the camera when the cursor is on a panel).
#[derive(Clone, Debug)]
pub enum Place {
    Primitive(&'static str),
    Entity {
        class: &'static str,
        props: Vec<(String, String)>,
    },
    Prop(String),
    /// A prefab by name.
    Prefab(String),
}

#[derive(Message, Clone, Debug)]
pub struct PlaceRequest {
    pub place: Place,
    /// Place at the cursor ray rather than the middle of the screen.
    pub at_cursor: bool,
}

/// Pixels the cursor travels with the button down before a click is a drag.
const DRAG_PIXELS: f32 = 5.0;
/// A camera pitch (radians) below this is a top view.
const TOP_PITCH: f32 = -1.1;

/// What the left mouse button is doing.
#[derive(Clone, Debug)]
enum Drag {
    /// Went down: a click selects on release; it turns into a box when it travels and `box_ok`.
    Press {
        start: Vec2,
        hit: Option<Item>,
        box_ok: bool,
    },
    /// A rubber band, in window pixels.
    Box { start: Vec2, now: Vec2 },
    /// A face of the selected brush follows the cursor along its normal; `amount` is the
    /// snapped distance moved so far, in metres.
    Face {
        entity: usize,
        brush: usize,
        face: usize,
        centre: DVec3,
        normal: DVec3,
        grab: f64,
        amount: f64,
    },
}

/// The state of the mouse editing: a drag in progress, and the line about it for the HUD.
#[derive(Resource, Default)]
pub struct Interaction {
    drag: Option<Drag>,
    pub readout: Option<String>,
}

impl Interaction {
    /// The rubber band of a box selection (corners in window pixels).
    pub fn box_rect(&self) -> Option<(Vec2, Vec2)> {
        match self.drag {
            Some(Drag::Box { start, now }) => Some((start.min(now), start.max(now))),
            _ => None,
        }
    }

    /// The face being dragged: brush, face index and metres moved.
    fn face_drag(&self) -> Option<(usize, usize, usize, f64)> {
        match self.drag {
            Some(Drag::Face {
                entity,
                brush,
                face,
                amount,
                ..
            }) => Some((entity, brush, face, amount)),
            _ => None,
        }
    }

    fn cancel(&mut self) {
        self.drag = None;
        self.readout = None;
    }
}

/// The cursor's ray into the world (game space), and whether the cursor is over a panel.
fn cursor_ray(
    window: &Window,
    camera: &Camera,
    transform: &GlobalTransform,
    blockers: &Query<(&ComputedNode, &UiGlobalTransform), With<BlockPicking>>,
) -> Option<(DVec3, DVec3, bool)> {
    let cursor = window.cursor_position()?;
    let physical = cursor * window.scale_factor();
    let over_ui = blockers
        .iter()
        .any(|(node, ui_transform)| node.contains_point(*ui_transform, physical));
    let ray = camera.viewport_to_world(transform, cursor).ok()?;
    Some((ray.origin.as_dvec3(), ray.direction.as_dvec3(), over_ui))
}

/// The ray through the middle of the screen.
fn centre_ray(
    window: &Window,
    camera: &Camera,
    transform: &GlobalTransform,
) -> Option<(DVec3, DVec3)> {
    let size = Vec2::new(window.width(), window.height());
    let ray = camera.viewport_to_world(transform, size / 2.0).ok()?;
    Some((ray.origin.as_dvec3(), ray.direction.as_dvec3()))
}

/// The world axis a horizontal direction is closest to.
fn cardinal(v: Vec3) -> DVec3 {
    if v.x.abs() >= v.z.abs() {
        DVec3::new(f64::from(v.x.signum()), 0.0, 0.0)
    } else {
        DVec3::new(0.0, 0.0, f64::from(v.z.signum()))
    }
}

/// How far along the line through `point` in the direction of the unit vector `axis` the
/// pointer ray (`origin`, `direction`) passes closest to it, measured from `point`: what a
/// drag along an axis follows. `None` when the ray runs parallel to the axis.
fn along_axis(point: DVec3, axis: DVec3, origin: DVec3, direction: DVec3) -> Option<f64> {
    let w = point - origin;
    let (b, c) = (axis.dot(direction), direction.dot(direction));
    let (d, e) = (axis.dot(w), direction.dot(w));
    let denominator = c - b * b;
    (denominator.abs() > 1e-9).then(|| (b * e - c * d) / denominator)
}

/// A click: selects what was hit (a modifier key toggles it in and out of the selection
/// instead), or clears the selection on empty space.
fn click(editor: &mut Editor, hit: Option<Item>, toggle: bool) {
    let how = if toggle {
        Combine::Toggle
    } else {
        Combine::Replace
    };
    let items: Vec<Item> = hit.into_iter().collect();
    match editor.select_items(&items, how) {
        Ok(text) => {
            if !editor.selection.is_empty() {
                editor.say(text);
            }
        }
        Err(text) => editor.complain(text),
    }
}

/// Selects what a rubber band of the window rectangle `low`..`high` wholly encloses.
fn select_in_rect(
    editor: &mut Editor,
    camera: &Camera,
    transform: &GlobalTransform,
    (low, high): (Vec2, Vec2),
    how: Combine,
) {
    let project = |p: DVec3| camera.world_to_viewport(transform, p.as_vec3()).ok();
    let result = editor
        .find_where(|min, max| selection::corners_inside(min, max, project, low, high))
        .and_then(|items| editor.select_items(&items, how));
    editor.report(&result);
}

#[allow(clippy::too_many_arguments)]
fn edit_input(
    mut keyboard: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    capture: Res<InputCapture>,
    mut editor: ResMut<Editor>,
    mut interaction: ResMut<Interaction>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform, &FlyCamera)>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform), With<BlockPicking>>,
    mut commands: MessageWriter<Command>,
    mut places: MessageWriter<PlaceRequest>,
) {
    let events: Vec<KeyboardInput> = keyboard.read().cloned().collect();
    editor.hover = None;
    if capture.blocked() || editor.map.is_none() {
        interaction.cancel();
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        interaction.cancel();
    }
    let (Ok(window), Ok((camera, camera_transform, fly))) = (windows.single(), cameras.single())
    else {
        return;
    };
    let looking = buttons.pressed(MouseButton::Right);

    let held = |a: KeyCode, b: KeyCode| keys.pressed(a) || keys.pressed(b);
    let shift = held(KeyCode::ShiftLeft, KeyCode::ShiftRight);
    let alt = held(KeyCode::AltLeft, KeyCode::AltRight);
    let command = held(KeyCode::ControlLeft, KeyCode::ControlRight)
        || held(KeyCode::SuperLeft, KeyCode::SuperRight);

    // The mouse: hover; a press starts a click, a box or a face drag; a release finishes it.
    let ray = cursor_ray(window, camera, camera_transform, &blockers);
    let free_ray =
        ray.and_then(|(origin, direction, over_ui)| (!over_ui).then_some((origin, direction)));
    let cursor = window.cursor_position();
    if looking {
        interaction.cancel();
    } else {
        if interaction.drag.is_none() {
            editor.hover = free_ray
                .and_then(|(origin, direction)| editor.pick(origin, direction))
                .map(|hit| hit.item);
        }
        if buttons.just_pressed(MouseButton::Left)
            && let (Some((origin, direction)), Some(cursor)) = (free_ray, cursor)
        {
            let hit = editor.pick(origin, direction).map(|hit| hit.item);
            // Alt on a face of the one selected brush grabs the face.
            let face = alt
                .then(|| editor.selection.single())
                .flatten()
                .and_then(|item| match item {
                    Item::Brush { entity, brush } => Some((entity, brush)),
                    Item::Entity(_) => None,
                })
                .and_then(|(entity, brush)| {
                    let b = editor.map.as_ref()?.brush(entity, brush)?;
                    let (_, _, face) = b.ray_face(origin, direction)?;
                    let (centre, normal) = (b.face_centre(face)?, b.face_normal(face)?);
                    let grab = along_axis(centre, normal, origin, direction)?;
                    Some(Drag::Face {
                        entity,
                        brush,
                        face,
                        centre,
                        normal,
                        grab,
                        amount: 0.0,
                    })
                });
            interaction.readout = None;
            interaction.drag = Some(face.unwrap_or(Drag::Press {
                start: cursor,
                hit,
                box_ok: alt || (hit.is_none() && fly.pitch < TOP_PITCH),
            }));
        }
    }
    if let Some(drag) = interaction.drag.clone() {
        let down = buttons.pressed(MouseButton::Left) || buttons.just_released(MouseButton::Left);
        if !down || !window.focused {
            interaction.cancel();
        } else {
            match drag {
                Drag::Press {
                    start,
                    box_ok: true,
                    ..
                } => {
                    if let Some(now) = cursor
                        && now.distance(start) > DRAG_PIXELS
                    {
                        interaction.drag = Some(Drag::Box { start, now });
                    }
                }
                Drag::Press { .. } => {}
                Drag::Box { start, .. } => {
                    if let Some(now) = cursor {
                        interaction.drag = Some(Drag::Box { start, now });
                    }
                }
                Drag::Face {
                    entity,
                    brush,
                    face,
                    centre,
                    normal,
                    grab,
                    ..
                } => {
                    if let Some((origin, direction, _)) = ray
                        && let Some(at) = along_axis(centre, normal, origin, direction)
                    {
                        match editor.face_move(entity, brush, FaceRef::Index(face), at - grab, true)
                        {
                            Ok((_, _, amount)) => {
                                interaction.readout = Some(format!(
                                    "face {face}: {amount:+} m (grid {} m)",
                                    editor.grid
                                ));
                                interaction.drag = Some(Drag::Face {
                                    entity,
                                    brush,
                                    face,
                                    centre,
                                    normal,
                                    grab,
                                    amount,
                                });
                            }
                            Err(text) => editor.complain(text),
                        }
                    }
                }
            }
        }
    }
    if buttons.just_released(MouseButton::Left)
        && let Some(drag) = interaction.drag.take()
    {
        interaction.readout = None;
        match drag {
            Drag::Press { hit, .. } => click(&mut editor, hit, shift || command),
            Drag::Box { start, now } => {
                let how = if shift || command {
                    Combine::Add
                } else {
                    Combine::Replace
                };
                select_in_rect(
                    &mut editor,
                    camera,
                    camera_transform,
                    (start.min(now), start.max(now)),
                    how,
                );
            }
            Drag::Face {
                entity,
                brush,
                face,
                amount,
                ..
            } => {
                if amount.abs() > 1e-9 {
                    let result = editor.move_face(
                        Some(Item::Brush { entity, brush }),
                        FaceRef::Index(face),
                        amount,
                        true,
                    );
                    editor.report(&result);
                }
            }
        }
    }

    // Camera-relative axes, snapped to the world's.
    let forward = cardinal(Vec3::from(camera_transform.forward()));
    let right = cardinal(Vec3::from(camera_transform.right()));
    let step = editor.grid * if shift && !alt { 4.0 } else { 1.0 };

    for event in events {
        if !event.state.is_pressed() {
            continue;
        }
        let direction = match event.key_code {
            KeyCode::ArrowLeft => Some(-right),
            KeyCode::ArrowRight => Some(right),
            KeyCode::ArrowUp => Some(forward),
            KeyCode::ArrowDown => Some(-forward),
            KeyCode::PageUp => Some(DVec3::Y),
            KeyCode::PageDown => Some(DVec3::NEG_Y),
            _ => None,
        };
        let result = if let Some(direction) = direction {
            if editor.selection.is_empty() {
                continue;
            }
            if alt {
                // Alt moves the face the arrow points to out by a grid step (Alt+Shift: in),
                // keeping every other face where it is.
                let amount = if shift { -editor.grid } else { editor.grid };
                editor.move_face(None, FaceRef::Towards(direction), amount, true)
            } else {
                editor.move_item(None, direction * step)
            }
        } else if command {
            match event.key_code {
                KeyCode::KeyZ if shift => editor.redo(),
                KeyCode::KeyZ => editor.undo(),
                KeyCode::KeyY => editor.redo(),
                KeyCode::KeyD => {
                    if editor.selection.is_empty() {
                        continue;
                    }
                    if shift {
                        // Three copies side by side, each next to the one before.
                        let axis = if right.x != 0.0 { 0 } else { 2 };
                        let width = editor
                            .selection_bounds()
                            .map_or(1.0, |(min, max)| (max - min)[axis]);
                        let width = if width < 0.05 { 1.0 } else { width };
                        editor.array(None, 3, right * width)
                    } else {
                        let offset = right * (editor.grid * 4.0);
                        editor.duplicate(None, offset)
                    }
                }
                KeyCode::KeyC => {
                    if editor.selection.is_empty() {
                        continue;
                    }
                    editor.copy(None)
                }
                KeyCode::KeyV => match free_ray {
                    Some((origin, direction)) => {
                        let (point, _) = editor.target(origin, direction);
                        editor.paste_at(point)
                    }
                    None => editor.paste(right),
                },
                KeyCode::KeyA => {
                    // All of it; with Shift, all of the kind of what was picked last.
                    let kind = match editor.selection.primary() {
                        Some(item) if shift => Kind::Like(item),
                        _ => Kind::Everything,
                    };
                    editor
                        .find_kind(&kind)
                        .and_then(|items| editor.select_items(&items, Combine::Replace))
                }
                KeyCode::KeyS => {
                    commands.write(Command::Save);
                    continue;
                }
                _ => continue,
            }
        } else {
            match event.key_code {
                KeyCode::Delete | KeyCode::Backspace => {
                    if editor.selection.is_empty() {
                        continue;
                    }
                    editor.delete(None)
                }
                KeyCode::Tab => {
                    if editor.selection.is_empty() {
                        continue;
                    }
                    editor.cycle_surface(None, if shift { -1 } else { 1 })
                }
                KeyCode::KeyB => {
                    places.write(PlaceRequest {
                        place: Place::Primitive(if shift { "ramp" } else { "box" }),
                        at_cursor: true,
                    });
                    continue;
                }
                KeyCode::KeyJ | KeyCode::KeyK => {
                    if editor.selection.is_empty() {
                        continue;
                    }
                    let degrees = if shift { 90.0 } else { TURN_STEP };
                    editor.turn(
                        None,
                        if event.key_code == KeyCode::KeyK {
                            degrees
                        } else {
                            -degrees
                        },
                        None,
                    )
                }
                KeyCode::KeyM => {
                    if editor.selection.is_empty() {
                        continue;
                    }
                    let axis = if shift { MirrorAxis::Z } else { MirrorAxis::X };
                    editor.mirror(None, axis, None, false)
                }
                KeyCode::KeyG => {
                    let at = GRIDS.iter().position(|g| *g == editor.grid).unwrap_or(0);
                    editor.grid = GRIDS[(at + 1) % GRIDS.len()];
                    Ok(format!("Grid {} m", editor.grid))
                }
                KeyCode::F5 => {
                    commands.write(Command::Playtest);
                    continue;
                }
                _ => continue,
            }
        };
        editor.report(&result);
    }
}

/// Places what a [`PlaceRequest`] asks for at the cursor.
fn place(
    mut requests: MessageReader<PlaceRequest>,
    mut editor: ResMut<Editor>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform), With<BlockPicking>>,
) {
    let requests: Vec<PlaceRequest> = requests.read().cloned().collect();
    if requests.is_empty() {
        return;
    }
    let (Ok(window), Ok((camera, transform))) = (windows.single(), cameras.single()) else {
        return;
    };
    for request in requests {
        let ray = match cursor_ray(window, camera, transform, &blockers) {
            Some((origin, direction, false)) if request.at_cursor => Some((origin, direction)),
            _ => centre_ray(window, camera, transform),
        };
        let Some((origin, direction)) = ray else {
            continue;
        };
        let (point, normal) = editor.target(origin, direction);
        let result = match request.place {
            Place::Primitive(key) => {
                let Some(primitive) = palette::primitive(key) else {
                    continue;
                };
                let rise = Rise::towards(DVec3::new(direction.x, 0.0, direction.z));
                let (min, max) = editor.primitive_bounds(primitive, rise, point, normal);
                let surface = editor.surface;
                if primitive.ramp {
                    editor.add_ramp(min, max, rise, surface)
                } else {
                    editor.add_box(min, max, surface)
                }
            }
            Place::Entity { class, props } => {
                let position = editor.entity_position(class, point, normal);
                // Spawns face where the camera looks.
                let angle = palette::class(class)
                    .is_some_and(|c| c.faces)
                    .then(|| (-direction.x).atan2(-direction.z).to_degrees().round());
                editor.add_entity(class, position, angle, &props)
            }
            Place::Prop(model) => {
                let position = editor.entity_position("prop", point, normal);
                editor.add_entity("prop", position, None, &[("model".to_owned(), model)])
            }
            Place::Prefab(name) => {
                let position = editor.entity_position("prefab", point, normal);
                editor.place_prefab(&name, position, 0.0)
            }
        };
        editor.report(&result);
    }
}

// ------------------------------------------------------------------------------- drawing

const SELECTED: Color = Color::srgb(1.0, 0.85, 0.1);
const HANDLE: Color = Color::srgb(1.0, 0.55, 0.1);
const PREVIEW: Color = Color::srgb(0.3, 0.95, 0.9);

/// The outline of a brush, as lines along its faces.
fn outline_brush(gizmos: &mut Gizmos, brush: &Brush, color: Color) {
    for polygon in brush.polygons() {
        if polygon.len() < 2 {
            continue;
        }
        let mut points: Vec<Vec3> = polygon.iter().map(|p| tb_to_game(*p).as_vec3()).collect();
        points.push(points[0]);
        gizmos.linestrip(points, color);
    }
}

fn outline_box(gizmos: &mut Gizmos, min: Vec3, max: Vec3, color: Color) {
    gizmos.cube(
        Transform::from_translation((min + max) / 2.0).with_scale(max - min),
        color,
    );
}

fn draw_selection(mut gizmos: Gizmos, editor: Res<Editor>, interaction: Res<Interaction>) {
    let Some(map) = &editor.map else {
        return;
    };
    let outline = |gizmos: &mut Gizmos, item: Item, color: Color| match item {
        Item::Brush { entity, brush } => {
            if let Some(brush) = map.brush(entity, brush) {
                outline_brush(gizmos, brush, color);
            }
        }
        Item::Entity(_) => {
            if let Some((min, max)) = map.bounds(item, &editor.prop_bounds) {
                outline_box(gizmos, min.as_vec3(), max.as_vec3(), color);
            }
        }
    };
    if let Some(hover) = editor.hover
        && !editor.selection.contains(hover)
    {
        outline(&mut gizmos, hover, Color::srgba(1.0, 1.0, 1.0, 0.5));
    }
    for item in editor.selection.items() {
        outline(&mut gizmos, *item, SELECTED);
    }
    let Some((min, max)) = editor.selection_bounds() else {
        return;
    };
    if editor.selection.len() > 1 {
        outline_box(&mut gizmos, min.as_vec3(), max.as_vec3(), PREVIEW);
    }
    // The grid the selection snaps to, on the plane of its underside.
    let g = editor.grid;
    let cells = |lo: f64, hi: f64| {
        let first = (lo / g).floor() - 2.0;
        let last = (hi / g).ceil() + 2.0;
        let (first, last) = if last - first > 48.0 {
            let mid = ((lo + hi) / 2.0 / g).round();
            (mid - 24.0, mid + 24.0)
        } else {
            (first, last)
        };
        (first * g, last * g)
    };
    let ((x0, x1), (z0, z1)) = (cells(min.x, max.x), cells(min.z, max.z));
    let y = min.y as f32 + 0.01;
    let faint = Color::srgba(1.0, 1.0, 1.0, 0.16);
    let mut x = x0;
    while x <= x1 + 1e-9 {
        gizmos.line(
            Vec3::new(x as f32, y, z0 as f32),
            Vec3::new(x as f32, y, z1 as f32),
            faint,
        );
        x += g;
    }
    let mut z = z0;
    while z <= z1 + 1e-9 {
        gizmos.line(
            Vec3::new(x0 as f32, y, z as f32),
            Vec3::new(x1 as f32, y, z as f32),
            faint,
        );
        z += g;
    }
    // A lone brush shows its faces as handles: a stalk along the normal with a knob.
    if let Some(Item::Brush { entity, brush }) = editor.selection.single()
        && let Some(b) = map.brush(entity, brush)
    {
        for face in 0..b.faces.len() {
            if let (Some(centre), Some(normal)) = (b.face_centre(face), b.face_normal(face)) {
                let (from, to) = (centre.as_vec3(), (centre + normal * 0.4).as_vec3());
                gizmos.line(from, to, HANDLE);
                gizmos.sphere(Isometry3d::from_translation(to), 0.07, HANDLE);
            }
        }
        // What the face being dragged will give.
        if let Some((e, br, face, amount)) = interaction.face_drag()
            && (e, br) == (entity, brush)
            && let Some(direction) = b.face_normal(face)
        {
            let mut preview = b.clone();
            if preview.move_face(face, direction, amount).is_ok() {
                outline_brush(&mut gizmos, &preview, PREVIEW);
            }
        }
    }
}

/// The rubber band of a box selection, as a screen rectangle.
#[derive(Component)]
struct BoxRect;

fn spawn_box_rect(mut commands: Commands) {
    commands.spawn((
        BoxRect,
        GlobalZIndex(4),
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(px(1.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 0.85, 0.1, 0.12)),
        BorderColor::all(SELECTED),
        Visibility::Hidden,
    ));
}

fn show_box_rect(
    interaction: Res<Interaction>,
    mut rect: Query<(&mut Node, &mut Visibility), With<BoxRect>>,
) {
    let Ok((mut node, mut visibility)) = rect.single_mut() else {
        return;
    };
    match interaction.box_rect() {
        Some((low, high)) => {
            node.left = px(low.x);
            node.top = px(low.y);
            node.width = px(high.x - low.x);
            node.height = px(high.y - low.y);
            *visibility = Visibility::Visible;
        }
        None => *visibility = Visibility::Hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::ButtonState;
    use bevy::input::keyboard::Key;

    /// A folder under the system temp dir that is removed when it goes out of scope.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            let dir =
                std::env::temp_dir().join(format!("mapedit-test-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("assets/maps")).unwrap();
            std::fs::write(dir.join("maps.ron"), "// test registry\n{\n}\n").unwrap();
            Temp(dir)
        }

        fn paths(&self) -> Paths {
            Paths {
                assets: self.0.join("assets"),
                registry: self.0.join("maps.ron"),
                private: None,
            }
        }

        fn maps(&self) -> PathBuf {
            self.0.join("assets/maps")
        }

        fn previews(&self) -> Vec<String> {
            std::fs::read_dir(self.maps())
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with(PREVIEW_PREFIX))
                .collect()
        }

        fn registry(&self) -> std::collections::BTreeMap<MapId, MapInfo> {
            ron::from_str(&std::fs::read_to_string(self.0.join("maps.ron")).unwrap()).unwrap()
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn world_in(dir: &Temp) -> World {
        let mut world = World::new();
        fill_world(&mut world, dir);
        world
    }

    fn fill_world(world: &mut World, dir: &Temp) {
        let mut editor = Editor::new(dir.paths(), Vec::new());
        editor.look_names = vec!["overcast_urban".into(), "desert_dust".into()];
        world.insert_resource(editor);
        world.insert_resource(MapRegistry::default());
        world.insert_resource(IdleMap(MapId::new("")));
        world.insert_resource(GameData::load());
        world.init_resource::<Messages<Reload>>();
        world.init_resource::<Messages<AppExit>>();
    }

    fn ok(world: &mut World, command: Command) -> String {
        match execute(world, command.clone()) {
            Ok(text) => text,
            Err(failure) => panic!("{command:?}: {failure:?}"),
        }
    }

    fn refused(world: &mut World, command: Command) -> Failure {
        execute(world, command).expect_err("should be refused")
    }

    fn open(world: &mut World, id: &str) -> Result<String, Failure> {
        execute(
            world,
            Command::Open {
                id: MapId::new(id),
                discard: false,
            },
        )
    }

    fn new_map(world: &mut World, id: &str, discard: bool) -> Result<String, Failure> {
        execute(
            world,
            Command::NewMap {
                id: id.into(),
                name: "Test Map".into(),
                discard,
            },
        )
    }

    fn editor(world: &World) -> &Editor {
        world.resource::<Editor>()
    }

    fn file_of(world: &World, id: &str) -> String {
        world
            .resource::<MapRegistry>()
            .get(&MapId::new(id))
            .unwrap()
            .file
            .clone()
    }

    #[test]
    fn a_new_map_is_valid_registered_and_previewed() {
        let dir = Temp::new("new");
        let mut world = world_in(&dir);
        let said = ok(
            &mut world,
            Command::NewMap {
                id: "t_one".into(),
                name: "".into(),
                discard: false,
            },
        );
        assert!(said.contains("t_one.map"), "{said}");
        // The file: a floor, two spawns per Team, a worldspawn with a look.
        let text = std::fs::read_to_string(dir.maps().join("t_one.map")).unwrap();
        let map = MapFile::parse(&text).unwrap();
        assert_eq!(map.entities[0].brushes.len(), 1);
        assert!(map.entities[0].brushes[0].is_solid());
        let spawns = |team: &str| {
            map.entities
                .iter()
                .filter(|e| e.classname() == "spawn_point" && e.get("team") == Some(team))
                .count()
        };
        assert_eq!((spawns("alpha"), spawns("bravo")), (2, 2));
        let world_entity = map.worldspawn().unwrap();
        assert_eq!(world_entity.get("look"), Some("overcast_urban"));
        assert_eq!(
            world_entity.get("name"),
            Some("T One"),
            "the name comes from the id"
        );
        // The registry file: appended, comments kept, parses, sensible defaults.
        let ron = std::fs::read_to_string(dir.0.join("maps.ron")).unwrap();
        assert!(ron.starts_with("// test registry"));
        let entry = &dir.registry()[&MapId::new("t_one")];
        assert_eq!(entry.file, "maps/t_one.map");
        assert_eq!(entry.modes, ["tdm"]);
        assert_eq!(entry.look.as_deref(), Some("overcast_urban"));
        assert_ne!(entry.factions[0], entry.factions[1]);
        // The viewer is pointed at the live preview, which holds the working copy.
        assert_eq!(world.resource::<IdleMap>().0, MapId::new("t_one"));
        assert_eq!(file_of(&world, "t_one"), "maps/.mapedit-t_one.map");
        assert_eq!(dir.previews(), [".mapedit-t_one.map"]);
        assert_eq!(
            std::fs::read_to_string(dir.maps().join(".mapedit-t_one.map")).unwrap(),
            text
        );
        assert_eq!(world.resource::<Messages<Reload>>().len(), 1);
        assert!(!editor(&world).dirty());
        // Ids are file names.
        for bad in ["Bad Id", "", "_x", "has-dash", "t_one"] {
            assert!(
                matches!(new_map(&mut world, bad, true), Err(Failure::Message(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn edits_reach_the_preview_but_not_the_file_until_save() {
        let dir = Temp::new("edit");
        let mut world = world_in(&dir);
        ok(
            &mut world,
            Command::NewMap {
                id: "t_one".into(),
                name: "T".into(),
                discard: false,
            },
        );
        let original = std::fs::read_to_string(dir.maps().join("t_one.map")).unwrap();

        let surface = mapfile::surface("brick").unwrap();
        let mut editor = world.resource_mut::<Editor>();
        editor
            .add_box(
                DVec3::new(0.0, 0.0, 0.0),
                DVec3::new(2.0, 3.0, 0.5),
                surface,
            )
            .unwrap();
        assert!(editor.dirty());
        assert!(!editor.synced());
        assert!(editor.write_preview(10.0), "the preview is written");
        assert!(editor.synced());
        assert!(!editor.write_preview(10.05), "nothing new to write");
        let preview = std::fs::read_to_string(dir.maps().join(".mapedit-t_one.map")).unwrap();
        assert_eq!(
            MapFile::parse(&preview).unwrap(),
            editor.map.clone().unwrap()
        );
        assert_eq!(
            std::fs::read_to_string(dir.maps().join("t_one.map")).unwrap(),
            original
        );

        // Unsaved changes stop a switch; discarding drops them and the file is untouched.
        assert!(matches!(
            refused(
                &mut world,
                Command::Open {
                    id: MapId::new("t_one"),
                    discard: false
                }
            ),
            Failure::Dirty
        ));
        assert!(matches!(
            new_map(&mut world, "t_two", false),
            Err(Failure::Dirty)
        ));
        assert!(matches!(
            refused(&mut world, Command::Quit { discard: false }),
            Failure::Dirty
        ));
        new_map(&mut world, "t_two", true).unwrap();
        assert_eq!(
            file_of(&world, "t_one"),
            "maps/t_one.map",
            "the entry points at the real file again"
        );
        assert_eq!(
            dir.previews(),
            [".mapedit-t_two.map"],
            "the old preview is gone"
        );
        assert_eq!(
            std::fs::read_to_string(dir.maps().join("t_one.map")).unwrap(),
            original
        );

        // Back to t_one, edit, save: now the file has the brush.
        open(&mut world, "t_one").unwrap();
        let mut editor = world.resource_mut::<Editor>();
        editor
            .add_box(DVec3::ZERO, DVec3::new(1.0, 1.0, 1.0), surface)
            .unwrap();
        assert!(editor.dirty());
        let said = ok(&mut world, Command::Save);
        assert!(said.starts_with("Saved "), "{said}");
        assert!(!editor_dirty(&world));
        let saved = MapFile::parse(&std::fs::read_to_string(dir.maps().join("t_one.map")).unwrap())
            .unwrap();
        assert_eq!(saved.entities[0].brushes.len(), 2);
        // Our own save is not an outside change.
        let mut editor = world.resource_mut::<Editor>();
        assert_eq!(editor.source_mtime, mtime(&dir.maps().join("t_one.map")));
        // Undo makes it dirty again against the saved file; redo too.
        editor.undo().unwrap();
        assert!(editor.dirty());
        editor.redo().unwrap();
        assert!(!editor.dirty(), "back to what is saved");
    }

    fn editor_dirty(world: &World) -> bool {
        editor(world).dirty()
    }

    #[test]
    fn save_as_registers_a_copy_and_leaves_the_original() {
        let dir = Temp::new("saveas");
        let mut world = world_in(&dir);
        ok(
            &mut world,
            Command::NewMap {
                id: "t_one".into(),
                name: "One".into(),
                discard: false,
            },
        );
        let original = std::fs::read_to_string(dir.maps().join("t_one.map")).unwrap();
        world
            .resource_mut::<Editor>()
            .add_entity("dom_flag", DVec3::new(1.0, 0.0, 1.0), None, &[])
            .unwrap();
        ok(
            &mut world,
            Command::SaveAs {
                id: "t_copy".into(),
                name: "The Copy".into(),
            },
        );
        assert_eq!(world.resource::<IdleMap>().0, MapId::new("t_copy"));
        let copy = MapFile::parse(&std::fs::read_to_string(dir.maps().join("t_copy.map")).unwrap())
            .unwrap();
        assert!(copy.entities.iter().any(|e| e.classname() == "dom_flag"));
        assert_eq!(copy.worldspawn().unwrap().get("name"), Some("The Copy"));
        let registry = dir.registry();
        assert_eq!(registry[&MapId::new("t_copy")].name, "The Copy");
        assert_eq!(registry[&MapId::new("t_copy")].file, "maps/t_copy.map");
        assert_eq!(
            std::fs::read_to_string(dir.maps().join("t_one.map")).unwrap(),
            original
        );
        assert!(
            !editor(&world).dirty(),
            "the copy is what the editor now has"
        );
        // The id is taken.
        assert!(matches!(
            execute(
                &mut world,
                Command::SaveAs {
                    id: "t_copy".into(),
                    name: "".into()
                }
            ),
            Err(Failure::Message(_))
        ));
        // A file in the way, even when the registry does not know it, is not overwritten.
        std::fs::write(dir.maps().join("t_stray.map"), "x").unwrap();
        assert!(matches!(
            execute(
                &mut world,
                Command::SaveAs {
                    id: "t_stray".into(),
                    name: "".into()
                }
            ),
            Err(Failure::Message(_))
        ));
        assert_eq!(
            std::fs::read_to_string(dir.maps().join("t_stray.map")).unwrap(),
            "x"
        );
    }

    #[test]
    fn settings_apply_to_the_registry_and_the_worldspawn_and_save() {
        let dir = Temp::new("settings");
        let mut world = world_in(&dir);
        ok(
            &mut world,
            Command::NewMap {
                id: "t_one".into(),
                name: "One".into(),
                discard: false,
            },
        );
        let factions: Vec<String> = world
            .resource::<GameData>()
            .factions
            .iter()
            .map(|f| f.id.clone())
            .collect();
        ok(
            &mut world,
            Command::Settings(SettingsChange {
                name: Some("Renamed".into()),
                look: Some(Some("desert_dust".into())),
                modes: Some(vec!["tdm".into(), "dom".into(), "sd".into()]),
                factions: Some([factions[2].clone(), factions[3].clone()]),
            }),
        );
        let info = world
            .resource::<MapRegistry>()
            .get(&MapId::new("t_one"))
            .unwrap()
            .clone();
        assert_eq!(info.name, "Renamed");
        assert_eq!(info.modes, ["tdm", "dom", "sd"]);
        assert_eq!(info.factions, [factions[2].clone(), factions[3].clone()]);
        let map = editor(&world).map.clone().unwrap();
        assert_eq!(map.worldspawn().unwrap().get("name"), Some("Renamed"));
        assert_eq!(map.worldspawn().unwrap().get("look"), Some("desert_dust"));
        assert!(editor(&world).settings_dirty && editor(&world).dirty());
        // Nothing on disk yet.
        assert_eq!(dir.registry()[&MapId::new("t_one")].name, "One");
        ok(&mut world, Command::Save);
        let saved = &dir.registry()[&MapId::new("t_one")];
        assert_eq!((saved.name.as_str(), saved.modes.len()), ("Renamed", 3));
        assert_eq!(
            saved.file, "maps/t_one.map",
            "the preview path never reaches maps.ron"
        );
        assert_eq!(saved.look.as_deref(), Some("desert_dust"));
        assert!(!editor(&world).dirty());
        let file = MapFile::parse(&std::fs::read_to_string(dir.maps().join("t_one.map")).unwrap())
            .unwrap();
        assert_eq!(file.worldspawn().unwrap().get("look"), Some("desert_dust"));
        // Putting everything back is not a change.
        ok(
            &mut world,
            Command::Settings(SettingsChange {
                name: Some("Renamed".into()),
                ..default()
            }),
        );
        assert!(!editor(&world).settings_dirty);
        // Bad values are refused whole.
        for bad in [
            SettingsChange {
                look: Some(Some("no_such_look".into())),
                ..default()
            },
            SettingsChange {
                modes: Some(vec!["ctf".into()]),
                ..default()
            },
            SettingsChange {
                factions: Some(["nobody".into(), factions[0].clone()]),
                ..default()
            },
            SettingsChange {
                name: Some("  ".into()),
                ..default()
            },
        ] {
            assert!(matches!(
                execute(&mut world, Command::Settings(bad)),
                Err(Failure::Message(_))
            ));
        }
        assert_eq!(
            world
                .resource::<MapRegistry>()
                .get(&MapId::new("t_one"))
                .unwrap()
                .name,
            "Renamed"
        );
    }

    #[test]
    fn loose_and_broken_files_open() {
        let dir = Temp::new("loose");
        let mut world = world_in(&dir);
        ok(
            &mut world,
            Command::NewMap {
                id: "t_one".into(),
                name: "One".into(),
                discard: false,
            },
        );
        let text = std::fs::read_to_string(dir.maps().join("t_one.map")).unwrap();
        std::fs::write(dir.maps().join("My Loose-Map.map"), &text).unwrap();
        std::fs::write(
            dir.maps().join("broken.map"),
            "{ \"classname\" \"worldspawn\"\n{ ( 0 0 oops )",
        )
        .unwrap();
        let loose = loose_maps(&editor(&world).paths, world.resource::<MapRegistry>());
        let stems: Vec<&str> = loose.iter().map(|l| l.stem.as_str()).collect();
        assert_eq!(
            stems,
            ["My Loose-Map", "broken"],
            "registered Maps and previews are not loose"
        );

        ok(
            &mut world,
            Command::OpenFile {
                stem: "My Loose-Map".into(),
                private: false,
                discard: false,
            },
        );
        let id = world.resource::<IdleMap>().0.clone();
        assert_eq!(id, MapId::new("my_loose_map"));
        assert!(editor(&world).loose.contains(&id));
        assert!(editor(&world).map.is_some());
        assert_eq!(
            file_of(&world, "my_loose_map"),
            "maps/.mapedit-My Loose-Map.map"
        );
        // Saving registers it.
        assert!(!dir.registry().contains_key(&id));
        ok(&mut world, Command::Save);
        assert_eq!(dir.registry()[&id].file, "maps/My Loose-Map.map");
        assert!(!editor(&world).loose.contains(&id));

        // A file that does not parse is shown (the game's loader may know better) but not edited.
        ok(
            &mut world,
            Command::OpenFile {
                stem: "broken".into(),
                private: false,
                discard: false,
            },
        );
        let editor_ref = editor(&world);
        assert!(editor_ref.map.is_none());
        assert!(
            editor_ref.problem.as_deref().unwrap().contains("line 2"),
            "{:?}",
            editor_ref.problem
        );
        assert_eq!(
            file_of(&world, "broken"),
            "maps/broken.map",
            "no preview: the real file is shown"
        );
        let mut editor = world.resource_mut::<Editor>();
        assert!(
            editor
                .delete(Some(Item::Entity(1)))
                .unwrap_err()
                .contains("cannot be edited")
        );
        assert!(
            editor
                .add_box(DVec3::ZERO, DVec3::ONE, mapfile::surface("wood").unwrap())
                .is_err()
        );
        assert!(matches!(
            execute(&mut world, Command::Save),
            Err(Failure::Message(_))
        ));
        // Not there at all.
        assert!(matches!(
            execute(
                &mut world,
                Command::OpenFile {
                    stem: "ghost".into(),
                    private: false,
                    discard: true
                }
            ),
            Err(Failure::Message(_))
        ));
    }

    #[test]
    fn leaving_and_dropping_clean_up_the_preview() {
        let dir = Temp::new("cleanup");
        let mut world = world_in(&dir);
        ok(
            &mut world,
            Command::NewMap {
                id: "t_one".into(),
                name: "One".into(),
                discard: false,
            },
        );
        assert_eq!(dir.previews().len(), 1);
        // A crashed run's leftovers go at startup.
        std::fs::write(dir.maps().join(".mapedit-ghost.map"), "x").unwrap();
        dir.paths().remove_stale_previews();
        assert!(dir.previews().is_empty());
        drop(world);
        assert!(
            dir.previews().is_empty(),
            "the editor removes its preview when it goes"
        );
    }

    #[test]
    fn an_outside_change_reloads_a_clean_map_and_spares_unsaved_edits() {
        let dir = Temp::new("watch");
        let mut app = App::new();
        fill_world(app.world_mut(), &dir);
        app.add_message::<Command>()
            .init_resource::<Time>()
            .add_systems(Update, watch_source);
        ok(
            app.world_mut(),
            Command::NewMap {
                id: "t_one".into(),
                name: "".into(),
                discard: false,
            },
        );
        let file = dir.maps().join("t_one.map");
        let opened = MapFile::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let tick = |app: &mut App| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(600));
            app.update();
            app.world_mut()
                .resource_mut::<Messages<Command>>()
                .drain()
                .collect::<Vec<_>>()
        };
        assert!(tick(&mut app).is_empty(), "nothing changed");

        // The file is regenerated outside: the clean Map is read again.
        let mut regenerated = opened.clone();
        regenerated.add_entity(MapEntity::point(
            "dom_flag",
            DVec3::new(3.0, 0.0, 3.0),
            None,
            &[("label", "B")],
        ));
        std::fs::write(&file, regenerated.to_text()).unwrap();
        let sent = tick(&mut app);
        assert!(
            matches!(&sent[..], [Command::Open { discard: true, .. }]),
            "{sent:?}"
        );
        for command in sent {
            ok(app.world_mut(), command);
        }
        assert_eq!(editor(app.world()).map, Some(regenerated.clone()));
        assert!(tick(&mut app).is_empty(), "the new file is the one we have");

        // With unsaved edits the change is announced and the edits stay.
        let surface = mapfile::surface("wood").unwrap();
        app.world_mut()
            .resource_mut::<Editor>()
            .add_box(DVec3::ZERO, DVec3::ONE, surface)
            .unwrap();
        std::fs::write(&file, opened.to_text()).unwrap();
        assert!(tick(&mut app).is_empty(), "no reload over unsaved edits");
        let editor = editor(app.world());
        assert!(editor.dirty());
        assert!(
            editor
                .notice
                .as_ref()
                .unwrap()
                .text
                .contains("changed on disk")
        );
        assert_eq!(editor.map.as_ref().unwrap().entities[0].brushes.len(), 2);
    }

    #[test]
    fn the_generator_of_a_map_is_named() {
        let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/mapgen");
        let found = generator_in(&scripts, Path::new("assets/maps/arena.map")).unwrap();
        assert!(found.ends_with("arena.py"));
        assert!(generator_in(&scripts, Path::new("assets/maps/t_one.map")).is_none());
        assert!(
            generator_in(&scripts, Path::new("x/kit.map")).is_none(),
            "kit.py is a library, not a map"
        );
        assert!(generator_in(&scripts, Path::new("x/mapgen.map")).is_none());
    }

    // ---- keys

    fn key_app(map: MapFile) -> App {
        let mut app = App::new();
        let mut editor = Editor::new(
            Paths {
                assets: PathBuf::new(),
                registry: PathBuf::new(),
                private: None,
            },
            Vec::new(),
        );
        editor.saved = Some(map.clone());
        editor.map = Some(map);
        app.insert_resource(editor)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Interaction>()
            .init_resource::<InputCapture>()
            .add_message::<KeyboardInput>()
            .add_message::<Command>()
            .add_message::<PlaceRequest>()
            .add_systems(Update, edit_input);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        // Looking down -z: right is +x, forward is -z.
        app.world_mut().spawn((
            Camera::default(),
            GlobalTransform::IDENTITY,
            FlyCamera {
                yaw: 0.0,
                pitch: 0.0,
                speed: 1.0,
            },
        ));
        app
    }

    fn tap(app: &mut App, code: KeyCode, held: &[KeyCode]) {
        for key in held {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(*key);
        }
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<Window>>()
            .single(app.world())
            .unwrap();
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        for key in held {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(*key);
        }
    }

    fn two_brushes_and_a_spawn() -> MapFile {
        let mut map = MapFile::with_worldspawn("T", "x");
        let concrete = mapfile::surface("concrete").unwrap();
        map.add_brush(Brush::cuboid(
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(2.0, 2.0, 2.0),
            concrete,
        ));
        map.add_entity(MapEntity::point(
            "spawn_point",
            DVec3::new(5.0, 0.0, 5.0),
            Some(0.0),
            &[("team", "any")],
        ));
        map
    }

    fn min_of(app: &App, item: Item) -> DVec3 {
        let editor = app.world().resource::<Editor>();
        editor
            .map
            .as_ref()
            .unwrap()
            .bounds(item, &editor.prop_bounds)
            .unwrap()
            .0
    }

    fn near(a: DVec3, b: DVec3) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn arrow_keys_move_resize_and_the_shortcuts_edit() {
        let mut app = key_app(two_brushes_and_a_spawn());
        let brush = Item::Brush {
            entity: 0,
            brush: 0,
        };
        app.world_mut()
            .resource_mut::<Editor>()
            .select(Some(brush))
            .unwrap();

        // Camera-relative: right is +x, up-arrow is forward, which is -z; PgUp is +y.
        tap(&mut app, KeyCode::ArrowRight, &[]);
        assert!(near(min_of(&app, brush), DVec3::new(0.25, 0.0, 0.0)));
        tap(&mut app, KeyCode::ArrowUp, &[]);
        assert!(near(min_of(&app, brush), DVec3::new(0.25, 0.0, -0.25)));
        tap(&mut app, KeyCode::PageUp, &[KeyCode::ShiftLeft]);
        assert!(
            near(min_of(&app, brush), DVec3::new(0.25, 1.0, -0.25)),
            "Shift moves four grid steps"
        );
        // Alt grows the side the arrow points to; Alt+Shift shrinks it.
        tap(&mut app, KeyCode::ArrowRight, &[KeyCode::AltLeft]);
        let editor = app.world().resource::<Editor>();
        let (min, max) = editor
            .map
            .as_ref()
            .unwrap()
            .bounds(brush, &editor.prop_bounds)
            .unwrap();
        assert!(near(max - min, DVec3::new(2.25, 2.0, 2.0)));
        tap(
            &mut app,
            KeyCode::ArrowLeft,
            &[KeyCode::AltLeft, KeyCode::ShiftLeft],
        );
        let editor = app.world().resource::<Editor>();
        let (min2, max2) = editor
            .map
            .as_ref()
            .unwrap()
            .bounds(brush, &editor.prop_bounds)
            .unwrap();
        assert!(
            near(max2 - min2, DVec3::new(2.0, 2.0, 2.0)),
            "the -x side moved in"
        );
        assert!(near(min2, min + DVec3::new(0.25, 0.0, 0.0)));

        // Grid, surface, duplicate.
        tap(&mut app, KeyCode::KeyG, &[]);
        assert_eq!(app.world().resource::<Editor>().grid, 0.5);
        tap(&mut app, KeyCode::Tab, &[]);
        let surface = app
            .world()
            .resource::<Editor>()
            .map
            .as_ref()
            .unwrap()
            .brush(0, 0)
            .unwrap()
            .surface()
            .unwrap()
            .name;
        assert_eq!(
            surface, "plaster",
            "Tab goes from concrete to the next surface"
        );
        tap(&mut app, KeyCode::Tab, &[KeyCode::ShiftLeft]);
        assert_eq!(app.world().resource::<Editor>().surface.name, "concrete");
        tap(&mut app, KeyCode::KeyD, &[KeyCode::ControlLeft]);
        let editor = app.world().resource::<Editor>();
        assert_eq!(editor.map.as_ref().unwrap().entities[0].brushes.len(), 2);
        assert_eq!(
            editor.selection.items(),
            [Item::Brush {
                entity: 0,
                brush: 1
            }],
            "the copy is selected"
        );
        // Delete the copy, undo brings it back, redo removes it again.
        tap(&mut app, KeyCode::Delete, &[]);
        assert_eq!(
            app.world()
                .resource::<Editor>()
                .map
                .as_ref()
                .unwrap()
                .entities[0]
                .brushes
                .len(),
            1
        );
        assert!(app.world().resource::<Editor>().selection.is_empty());
        tap(&mut app, KeyCode::KeyZ, &[KeyCode::ControlLeft]);
        assert_eq!(
            app.world()
                .resource::<Editor>()
                .map
                .as_ref()
                .unwrap()
                .entities[0]
                .brushes
                .len(),
            2
        );
        assert!(
            !app.world().resource::<Editor>().selection.is_empty(),
            "the selection comes back with it"
        );
        tap(
            &mut app,
            KeyCode::KeyZ,
            &[KeyCode::SuperLeft, KeyCode::ShiftLeft],
        );
        assert_eq!(
            app.world()
                .resource::<Editor>()
                .map
                .as_ref()
                .unwrap()
                .entities[0]
                .brushes
                .len(),
            1,
            "Cmd+Shift+Z redoes"
        );
        tap(&mut app, KeyCode::KeyZ, &[KeyCode::ControlLeft]);
        tap(&mut app, KeyCode::KeyY, &[KeyCode::ControlLeft]);
        assert_eq!(
            app.world()
                .resource::<Editor>()
                .map
                .as_ref()
                .unwrap()
                .entities[0]
                .brushes
                .len(),
            1
        );

        // Entities turn with J and K, 15 degrees a press.
        let spawn = Item::Entity(1);
        app.world_mut()
            .resource_mut::<Editor>()
            .select(Some(spawn))
            .unwrap();
        tap(&mut app, KeyCode::KeyK, &[]);
        tap(&mut app, KeyCode::KeyK, &[]);
        tap(&mut app, KeyCode::KeyJ, &[]);
        assert_eq!(
            app.world()
                .resource::<Editor>()
                .map
                .as_ref()
                .unwrap()
                .entities[1]
                .angle(),
            15.0
        );
        // Ctrl+S asks for a save, B for a box.
        tap(&mut app, KeyCode::KeyS, &[KeyCode::ControlLeft]);
        assert_eq!(app.world().resource::<Messages<Command>>().len(), 1);
        tap(&mut app, KeyCode::KeyB, &[]);
        assert_eq!(app.world().resource::<Messages<PlaceRequest>>().len(), 1);
    }

    #[test]
    fn keys_do_nothing_while_a_menu_or_text_field_has_the_keyboard() {
        let mut app = key_app(two_brushes_and_a_spawn());
        let brush = Item::Brush {
            entity: 0,
            brush: 0,
        };
        app.world_mut()
            .resource_mut::<Editor>()
            .select(Some(brush))
            .unwrap();
        app.world_mut().resource_mut::<InputCapture>().menu = true;
        tap(&mut app, KeyCode::ArrowRight, &[]);
        tap(&mut app, KeyCode::Delete, &[]);
        app.world_mut().resource_mut::<InputCapture>().menu = false;
        app.world_mut().resource_mut::<InputCapture>().typing = true;
        tap(&mut app, KeyCode::Backspace, &[]);
        tap(&mut app, KeyCode::KeyG, &[]);
        let editor = app.world().resource::<Editor>();
        assert!(!editor.can_undo(), "nothing was edited");
        assert_eq!(editor.grid, GRIDS[0]);
        assert!(near(min_of(&app, brush), DVec3::ZERO));
    }

    #[test]
    fn placement_rests_on_what_the_ray_hits() {
        let map = two_brushes_and_a_spawn();
        let mut editor = Editor::new(
            Paths {
                assets: PathBuf::new(),
                registry: PathBuf::new(),
                private: None,
            },
            Vec::new(),
        );
        editor.map = Some(map);
        // Straight down onto the top of the box (y = 2) at x = 1.1: the point is on the top face.
        let (point, normal) = editor.target(DVec3::new(1.1, 10.0, 1.0), DVec3::NEG_Y);
        assert!(near(point, DVec3::new(1.1, 2.0, 1.0)) && near(normal, DVec3::Y));
        let primitive = palette::primitive("box").unwrap();
        let (min, max) = editor.primitive_bounds(primitive, Rise::PosX, point, normal);
        assert!((min.y - 2.0).abs() < 1e-9, "the box stands on it");
        assert!((max - min - DVec3::splat(2.0)).length() < 1e-9);
        assert_eq!(min.x % 0.25, 0.0, "snapped to the grid along the surface");
        // Missing everything: the ground at y = 0.
        let (point, normal) =
            editor.target(DVec3::new(20.0, 10.0, 20.0), DVec3::new(0.0, -1.0, -0.5));
        assert!((point.y).abs() < 1e-9 && normal == DVec3::Y);
        // A light hangs 2.9 m above where it is put, or just under a ceiling.
        assert!(
            (editor
                .entity_position("light_point", DVec3::new(1.0, 0.0, 1.0), DVec3::Y)
                .y
                - 2.9)
                .abs()
                < 1e-9
        );
        assert!(
            editor
                .entity_position("light_point", DVec3::new(1.0, 3.0, 1.0), DVec3::NEG_Y)
                .y
                < 3.0
        );
    }

    // ---- the selection as a list, turning, entities, clipboard, prefabs

    fn world() -> MapFile {
        let concrete = mapfile::surface("concrete").unwrap();
        let mut map = MapFile::with_worldspawn("T", "x");
        for x in [0.0, 4.0, 8.0] {
            map.add_brush(Brush::cuboid(
                DVec3::new(x, 0.0, 0.0),
                DVec3::new(x + 2.0, 2.0, 2.0),
                concrete,
            ));
        }
        map.add_entity(MapEntity::point(
            "spawn_point",
            DVec3::new(1.0, 0.0, 5.0),
            Some(0.0),
            &[("team", "alpha")],
        ));
        map.add_entity(MapEntity::point(
            "spawn_point",
            DVec3::new(5.0, 0.0, 5.0),
            Some(0.0),
            &[("team", "bravo")],
        ));
        map
    }

    fn editor_with(map: MapFile) -> Editor {
        Editor::for_tests(map, Vec::new())
    }

    #[test]
    fn every_edit_of_a_selection_is_one_undo_step() {
        let mut editor = editor_with(world());
        let all = editor.find_kind(&Kind::Everything).unwrap();
        assert_eq!(all.len(), 5);
        editor.select_items(&all, Combine::Replace).unwrap();

        editor.move_item(None, DVec3::new(0.0, 1.0, 0.0)).unwrap();
        assert_eq!(editor.undo.len(), 1, "five items, one step");
        let map = editor.map.as_ref().unwrap();
        for brush in 0..3 {
            assert!((map.brush(0, brush).unwrap().aabb().unwrap().0.y - 1.0).abs() < 1e-3);
        }
        assert!((map.entities[1].origin().unwrap().y - 1.0).abs() < 1e-3);

        // Paint: the brushes change, the spawns are skipped.
        editor.set_surface(None, "brick").unwrap();
        assert_eq!(editor.undo.len(), 2);
        let map = editor.map.as_ref().unwrap();
        assert!((0..3).all(|b| map.brush(0, b).unwrap().surface().unwrap().name == "brick"));

        // Turn about the middle of the five: brushes turn, spawns face another way.
        editor.turn(None, 90.0, None).unwrap();
        assert_eq!(editor.undo.len(), 3);
        assert_eq!(editor.map.as_ref().unwrap().entities[1].angle(), 90.0);

        // Duplicate selects the copies; delete removes them; undo brings each back whole.
        editor.duplicate(None, DVec3::new(0.0, 0.0, 20.0)).unwrap();
        assert_eq!(editor.selection.len(), 5);
        assert_eq!(editor.map.as_ref().unwrap().entities[0].brushes.len(), 6);
        editor.delete(None).unwrap();
        assert_eq!(editor.map.as_ref().unwrap().entities[0].brushes.len(), 3);
        assert!(editor.selection.is_empty());
        assert_eq!(editor.undo.len(), 5);
        editor.undo().unwrap();
        assert_eq!(editor.map.as_ref().unwrap().entities[0].brushes.len(), 6);
        assert_eq!(editor.selection.len(), 5, "the selection comes back");
        editor.undo().unwrap();
        editor.undo().unwrap();
        editor.undo().unwrap();
        editor.undo().unwrap();
        assert_eq!(
            *editor.map.as_ref().unwrap(),
            world(),
            "five undos, as it was"
        );
        editor.redo().unwrap();
        assert_eq!(editor.undo.len(), 1);
    }

    #[test]
    fn picks_combine_and_stale_items_are_refused() {
        let mut editor = editor_with(world());
        let brush = |n| Item::Brush {
            entity: 0,
            brush: n,
        };
        editor.select_items(&[brush(0)], Combine::Replace).unwrap();
        editor.select_items(&[brush(2)], Combine::Toggle).unwrap();
        editor.select_items(&[brush(0)], Combine::Toggle).unwrap();
        assert_eq!(editor.selection.items(), [brush(2)]);
        assert!(editor.select_items(&[brush(9)], Combine::Add).is_err());
        assert_eq!(
            editor.selection.items(),
            [brush(2)],
            "a refused pick changes nothing"
        );
        // A box wholly around the first two brushes (not the third) and the first spawn.
        let found = editor
            .find_in_box(
                DVec3::new(-1.0, -1.0, -1.0),
                DVec3::new(7.0, 3.0, 6.0),
                false,
            )
            .unwrap();
        assert_eq!(
            found,
            [brush(0), brush(1), Item::Entity(1), Item::Entity(2)]
        );
        // One brush alone is not "exactly one" when three are selected.
        let all = editor.find_kind(&Kind::Brushes).unwrap();
        editor.select_items(&all, Combine::Replace).unwrap();
        assert!(editor.resolve(None).is_err());
        assert!(
            editor
                .move_face(None, FaceRef::Index(0), 1.0, false)
                .is_err()
        );
        assert!(editor.targets(Some(Item::Entity(7))).is_err());
    }

    #[test]
    fn entity_edits_are_checked_against_the_class_and_leave_no_step_when_refused() {
        let mut editor = editor_with(world());
        editor
            .select_items(&[Item::Entity(1), Item::Entity(2)], Combine::Replace)
            .unwrap();
        // Both at once, one step.
        editor.set_property(None, "team", "any").unwrap();
        assert_eq!(editor.undo.len(), 1);
        let map = editor.map.as_ref().unwrap();
        assert_eq!(map.entities[1].get("team"), Some("any"));
        assert_eq!(map.entities[2].get("team"), Some("any"));
        // Refused: the class says so, nothing changes, no step.
        for (key, value) in [
            ("team", "red"),
            ("modes", "ctf"),
            ("angle", "north"),
            ("colour", "red"),
            ("origin", "0 0 0"),
            ("classname", "prop"),
        ] {
            assert!(
                editor.set_property(None, key, value).is_err(),
                "{key}={value}"
            );
        }
        assert_eq!(editor.undo.len(), 1);
        // A position needs one entity; facing and position go in one step.
        assert!(
            editor
                .edit_entity(
                    None,
                    EntityChange {
                        position: Some(DVec3::X),
                        ..EntityChange::default()
                    }
                )
                .is_err()
        );
        editor.select(Some(Item::Entity(1))).unwrap();
        editor
            .edit_entity(
                None,
                EntityChange {
                    set: vec![("modes".into(), "tdm sd".into())],
                    position: Some(DVec3::new(3.0, 0.0, 4.0)),
                    angle: Some(-90.0),
                },
            )
            .unwrap();
        assert_eq!(editor.undo.len(), 2);
        let e = &editor.map.as_ref().unwrap().entities[1];
        assert_eq!(e.get("modes"), Some("tdm sd"));
        assert_eq!(e.angle(), 270.0);
        assert!((e.origin().unwrap() - DVec3::new(3.0, 0.0, 4.0)).length() < 1e-3);
        // Removing a key is checked too: an empty value takes `modes` off again.
        editor.set_property(None, "modes", "").unwrap();
        assert_eq!(editor.map.as_ref().unwrap().entities[1].get("modes"), None);
        // A prop needs its model, and the model must be one that exists.
        let mut props = MapFile::with_worldspawn("T", "x");
        props.add_entity(MapEntity::point(
            "prop",
            DVec3::ZERO,
            None,
            &[("model", "models/downtown/Prop_Bollard.glb")],
        ));
        let model = PropModel {
            model: "models/downtown/Prop_Bollard.glb".into(),
            bounds: None,
        };
        let mut editor = Editor::for_tests(props, vec![model]);
        editor.select(Some(Item::Entity(1))).unwrap();
        assert!(
            editor.set_property(None, "model", "").is_err(),
            "cannot be removed"
        );
        assert!(
            editor
                .set_property(None, "model", "models/nowhere/Nothing.glb")
                .is_err()
        );
        assert!(editor.set_property(None, "model", "notamodel").is_err());
        assert!(editor.set_property(None, "collider", "sphere").is_err());
        editor.set_property(None, "collider", "convex").unwrap();
        editor
            .set_property(None, "model", "private://models/secret.glb")
            .unwrap();
        assert!(
            editor
                .add_entity(
                    "prop",
                    DVec3::ZERO,
                    None,
                    &[("model".into(), "models/nowhere/Nothing.glb".into())]
                )
                .is_err()
        );
        // A light: colour is three numbers, range is positive.
        editor
            .add_entity("light_point", DVec3::ZERO, None, &[])
            .unwrap();
        assert!(editor.set_property(None, "color", "1 0.5").is_err());
        assert!(editor.set_property(None, "color", "1 0.5 x").is_err());
        assert!(editor.set_property(None, "range", "0").is_err());
        assert!(editor.set_property(None, "range", "12.5").is_ok());
        assert!(editor.set_property(None, "shadows_enabled", "yes").is_err());
    }

    #[test]
    fn faces_move_on_the_grid_and_a_turned_box_stays_a_turned_box() {
        let mut editor = editor_with(world());
        let brush = Item::Brush {
            entity: 0,
            brush: 0,
        };
        editor.select(Some(brush)).unwrap();
        // Grid 0.25: the east face at 2.0 grows by 0.3 and lands on 2.25 when asked to snap.
        editor
            .move_face(None, FaceRef::Towards(DVec3::X), 0.3, true)
            .unwrap();
        let (_, max) = editor
            .map
            .as_ref()
            .unwrap()
            .bounds(brush, &editor.prop_bounds)
            .unwrap();
        assert!((max.x - 2.25).abs() < 1e-3, "{}", max.x);
        editor
            .move_face(None, FaceRef::Towards(DVec3::X), 0.3, false)
            .unwrap();
        let (_, max) = editor
            .map
            .as_ref()
            .unwrap()
            .bounds(brush, &editor.prop_bounds)
            .unwrap();
        assert!((max.x - 2.55).abs() < 1e-3, "{}", max.x);
        assert!(
            editor
                .move_face(None, FaceRef::Towards(DVec3::X), -9.0, false)
                .is_err()
        );
        assert_eq!(editor.undo.len(), 2, "the refused move left no step");
        // Turn 30 degrees, then push a face out along its own normal: still a closed box.
        editor.turn(None, 30.0, None).unwrap();
        let b = editor.map.as_ref().unwrap().brush(0, 0).unwrap().clone();
        assert!(b.is_solid());
        let face = b
            .face_towards(DVec3::new(
                30f64.to_radians().cos(),
                0.0,
                -30f64.to_radians().sin(),
            ))
            .unwrap();
        let before = b.vertices().len();
        editor
            .move_face(None, FaceRef::Index(face), 0.5, false)
            .unwrap();
        let b = editor.map.as_ref().unwrap().brush(0, 0).unwrap();
        assert!(b.is_solid() && b.vertices().len() == before);
    }

    #[test]
    fn copy_paste_arrays_and_prefabs_work_on_the_selection() {
        let dir = std::env::temp_dir().join(format!("mapedit-editor-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut editor = Editor::new(
            Paths {
                assets: dir.clone(),
                registry: PathBuf::new(),
                private: None,
            },
            Vec::new(),
        );
        editor.map = Some(world());
        editor
            .select_items(
                &[
                    Item::Brush {
                        entity: 0,
                        brush: 0,
                    },
                    Item::Entity(1),
                ],
                Combine::Replace,
            )
            .unwrap();
        assert!(editor.paste(DVec3::X).is_err(), "nothing copied yet");
        editor.copy(None).unwrap();
        // Paste with the anchor on a point: whole grid steps across, exact in height.
        let anchor = editor.clipboard_anchor().unwrap();
        // The footprint spans z 0 to 5.4 (the spawn's box), so its middle snaps to 2.75.
        assert!(
            (anchor - DVec3::new(1.0, 0.0, 2.75)).length() < 1e-9,
            "{anchor}"
        );
        editor.paste_at(DVec3::new(20.1, 0.5, 30.0)).unwrap();
        assert_eq!(editor.selection.len(), 2);
        let map = editor.map.as_ref().unwrap();
        let pasted = editor.selection.items()[0];
        let (min, _) = map.bounds(pasted, &editor.prop_bounds).unwrap();
        assert!(
            (min - DVec3::new(19.0, 0.5, 27.25)).length() < 1e-3,
            "{min}"
        );
        assert_eq!(editor.undo.len(), 1, "a paste is one step");

        // An array of the pasted pair: count copies, k * step.
        editor.array(None, 2, DVec3::new(0.0, 0.0, 4.0)).unwrap();
        assert_eq!(editor.selection.len(), 4);
        assert_eq!(editor.undo.len(), 2);
        assert!(editor.array(None, 0, DVec3::X).is_err());

        // Save the selection as a prefab, place it turned.
        let saved = editor.save_prefab(None, "pair", false).unwrap();
        assert!(saved.contains("pair"), "{saved}");
        assert!(
            editor.save_prefab(None, "pair", false).is_err(),
            "no silent replace"
        );
        assert_eq!(
            editor.prefab_list,
            [PrefabInfo {
                name: "pair".into(),
                private: false
            }]
        );
        assert!(dir.join("prefabs/pair.map").is_file());
        editor
            .place_prefab("pair", DVec3::new(-10.0, 0.0, -10.0), 90.0)
            .unwrap();
        assert_eq!(editor.selection.len(), 4);
        assert!(editor.place_prefab("nope", DVec3::ZERO, 0.0).is_err());
        // The private folder is not there: a private Map cannot save one.
        editor.private = true;
        assert!(editor.save_prefab(None, "secret", false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dragging_a_face_follows_the_pointer_along_the_normal() {
        // The ray from (3, 5, 0) straight down passes the line x-axis through the origin at x = 3.
        let at = along_axis(
            DVec3::ZERO,
            DVec3::X,
            DVec3::new(3.0, 5.0, 0.0),
            DVec3::NEG_Y,
        )
        .unwrap();
        assert!((at - 3.0).abs() < 1e-9);
        // A slanted ray still gives the closest point on the axis, measured from the face.
        let at = along_axis(
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::X,
            DVec3::new(0.0, 4.0, 4.0),
            DVec3::new(1.0, -1.0, -1.0),
        )
        .unwrap();
        assert!((at - 2.0).abs() < 1e-9, "{at}");
        // Looking along the axis: no answer.
        assert!(along_axis(DVec3::ZERO, DVec3::X, DVec3::new(-5.0, 1.0, 1.0), DVec3::X).is_none());
    }

    #[test]
    fn f5_asks_for_a_playtest_and_a_box_is_not_a_click() {
        let mut app = key_app(two_brushes_and_a_spawn());
        tap(&mut app, KeyCode::F5, &[]);
        let sent: Vec<Command> = app
            .world_mut()
            .resource_mut::<Messages<Command>>()
            .drain()
            .collect();
        assert!(matches!(sent[..], [Command::Playtest]));
        // Ctrl+A selects everything, Ctrl+Shift+A everything like the last pick, and J / K / M
        // act on all of it.
        tap(&mut app, KeyCode::KeyA, &[KeyCode::ControlLeft]);
        assert_eq!(app.world().resource::<Editor>().selection.len(), 2);
        tap(&mut app, KeyCode::KeyK, &[KeyCode::ShiftLeft]);
        let editor = app.world().resource::<Editor>();
        assert_eq!(editor.map.as_ref().unwrap().entities[1].angle(), 90.0);
        assert_eq!(editor.undo.len(), 1, "one step for both");
        tap(&mut app, KeyCode::KeyM, &[]);
        let editor = app.world().resource::<Editor>();
        assert_eq!(editor.undo.len(), 2);
        assert_eq!(editor.map.as_ref().unwrap().entities[1].angle(), 270.0);
        // Copy and paste with no pointer in the window: offset 1 m to the right.
        tap(&mut app, KeyCode::KeyC, &[KeyCode::ControlLeft]);
        tap(&mut app, KeyCode::KeyV, &[KeyCode::ControlLeft]);
        let editor = app.world().resource::<Editor>();
        assert_eq!(editor.map.as_ref().unwrap().entities[0].brushes.len(), 2);
        assert_eq!(editor.selection.len(), 2);
        // Ctrl+Shift+D: an array of three.
        tap(
            &mut app,
            KeyCode::KeyD,
            &[KeyCode::ControlLeft, KeyCode::ShiftLeft],
        );
        assert_eq!(app.world().resource::<Editor>().selection.len(), 6);
    }
}
