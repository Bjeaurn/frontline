//! The Map registry: every playable Map by id, from `data/maps.ron` plus the Homage Maps in the
//! private content folder when it is there (docs/adr/0003).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use bevy::asset::io::AssetSourceBuilder;
use bevy::asset::io::file::FileAssetReader;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::data::{GameData, GameModeId};

const MAPS_FILE: &str = "maps.ron";
const BUILTIN_MAPS: &str = include_str!("../../../../data/maps.ron");

/// The private content checkout (`frontline-private`), relative to the working directory like
/// `data/`. Optional: when it is missing there are simply no Homage Maps.
const PRIVATE_DIR: &str = "content/private";
/// Asset source serving [`PRIVATE_DIR`], e.g. `private://maps/dust.map`.
pub const PRIVATE_SOURCE: &str = "private";

/// The Map a server plays when `--map` isn't given, and the main menu's backdrop.
pub const DEFAULT_MAP: &str = "arena";

/// Stable key of a Map, e.g. `arena`: what `--map`, `maps.ron` and `MatchState` name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Reflect)]
#[serde(transparent)]
pub struct MapId(pub String);

impl MapId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl fmt::Display for MapId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One entry of `maps.ron`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct MapInfo {
    /// Display name.
    pub name: String,
    /// Asset path of the `.map` file, e.g. `maps/arena.map`. In `content/private/maps.ron` it is
    /// relative to `content/private/` and gets the `private://` source.
    pub file: String,
    /// Look preset (`data/looks.ron`). When absent, the map's worldspawn `look` applies.
    #[serde(default)]
    pub look: Option<String>,
    /// Game Mode keys ([`GameModeId::key`]) the Map has the entities for; empty means every mode.
    #[serde(default)]
    pub modes: Vec<String>,
    /// Faction ids (`data/factions.ron`) Alpha and Bravo play as on this Map: its theater.
    pub factions: [String; 2],
    /// A Homage Map from the private content folder: never shipped (docs/adr/0003).
    #[serde(default)]
    pub private: bool,
}

impl MapInfo {
    pub fn supports(&self, mode: GameModeId) -> bool {
        mode_listed(&self.modes, mode)
    }
}

/// Whether `mode` is among `keys`, where an empty list means every mode.
pub fn mode_listed<S: AsRef<str>>(keys: &[S], mode: GameModeId) -> bool {
    keys.is_empty() || keys.iter().any(|key| key.as_ref() == mode.key())
}

/// Every Map this process can load, by id.
#[derive(Resource, Debug, Default)]
pub struct MapRegistry {
    maps: BTreeMap<MapId, MapInfo>,
}

impl MapRegistry {
    pub fn get(&self, id: &MapId) -> Option<&MapInfo> {
        self.maps.get(id)
    }

    /// Edits an entry at run time (MapEdit points a Map at its live preview file, and changes
    /// its name, look, Game Modes and Factions).
    pub fn get_mut(&mut self, id: &MapId) -> Option<&mut MapInfo> {
        self.maps.get_mut(id)
    }

    /// Adds or replaces an entry at run time (MapEdit registers Maps it creates).
    pub fn insert(&mut self, id: MapId, info: MapInfo) {
        self.maps.insert(id, info);
    }

    /// In id order.
    pub fn iter(&self) -> impl Iterator<Item = (&MapId, &MapInfo)> {
        self.maps.iter()
    }

    /// Ids, comma-separated, for error messages.
    pub fn known_ids(&self) -> String {
        let ids: Vec<&str> = self.maps.keys().map(|id| id.0.as_str()).collect();
        ids.join(", ")
    }

    /// The built-in Maps, then the private ones: those declared in the private `maps.ron`
    /// (files relative to the private source) and any other `.map` in `private_files` (stems,
    /// files under `maps/`) with defaults, playing `default_factions`. A private Map never
    /// replaces a built-in one.
    fn build(
        builtin: BTreeMap<MapId, MapInfo>,
        private_declared: BTreeMap<MapId, MapInfo>,
        private_files: &[String],
        default_factions: &[String; 2],
    ) -> Self {
        let mut maps = builtin;
        let mut private = private_declared;
        for info in private.values_mut() {
            info.private = true;
            if !info.file.contains("://") {
                info.file = format!("{PRIVATE_SOURCE}://{}", info.file);
            }
        }
        for stem in private_files {
            let file = format!("{PRIVATE_SOURCE}://maps/{stem}.map");
            if private.values().any(|info| info.file == file) {
                continue;
            }
            private.entry(MapId::new(stem.clone())).or_insert(MapInfo {
                name: stem.clone(),
                file,
                look: None,
                modes: Vec::new(),
                factions: default_factions.clone(),
                private: true,
            });
        }
        for (id, info) in private {
            if maps.contains_key(&id) {
                warn!("Ignoring private Map \"{id}\": a built-in Map has that id");
                continue;
            }
            maps.insert(id, info);
        }
        Self { maps }
    }
}

/// `content/private` as an absolute path, if that folder exists.
pub fn private_dir() -> Option<PathBuf> {
    let dir = std::env::current_dir().ok()?.join(PRIVATE_DIR);
    dir.is_dir().then_some(dir)
}

/// Registers the `private://` asset source when the private content folder exists. Call before
/// any plugin adds `AssetPlugin`.
pub fn register_private_source(app: &mut App) {
    let Some(dir) = private_dir() else {
        return;
    };
    // An absolute root, so it doesn't move with Bevy's asset base path (the crate directory
    // under cargo, the executable's directory otherwise).
    app.register_asset_source(
        PRIVATE_SOURCE,
        AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(dir.clone()))),
    );
}

/// Asset source serving the folder of the Map file given with `--map-file`, e.g.
/// `mapfile://level.map`.
pub const MAP_FILE_SOURCE: &str = "mapfile";

/// Registers the `mapfile://` asset source on the folder of `path` (a `.map` file). Call before
/// any plugin adds `AssetPlugin`.
pub fn register_map_file_source(app: &mut App, path: &Path) {
    let Some(dir) = std::path::absolute(path)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    else {
        return;
    };
    app.register_asset_source(
        MAP_FILE_SOURCE,
        AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(dir.clone()))),
    );
}

/// The id a Map file given by path plays under: its file name without the extension (and
/// without a leading dot, as MapEdit's playtest copies have).
pub fn map_file_id(path: &Path) -> MapId {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    MapId::new(stem.trim_start_matches('.'))
}

/// The registry entry of a Map file given by path: shown under its id, the Map's own (worldspawn)
/// look, every Game Mode, the `default_factions`.
fn map_file_info(path: &Path, default_factions: &[String; 2]) -> Result<MapInfo, String> {
    if path.extension().is_none_or(|ext| ext != "map") {
        return Err(format!("{} is not a .map file", path.display()));
    }
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(MapInfo {
        name: map_file_id(path).0,
        file: format!("{MAP_FILE_SOURCE}://{name}"),
        look: None,
        modes: Vec::new(),
        factions: default_factions.clone(),
        private: false,
    })
}

/// Adds the Map file `path` (`--map-file`) to the registry and returns its id. The folder must
/// have been registered with [`register_map_file_source`].
pub fn add_map_file(app: &mut App, path: &Path) -> Result<MapId, String> {
    let data = app.world().resource::<GameData>();
    let factions = [data.factions[0].id.clone(), data.factions[1].id.clone()];
    let info = map_file_info(path, &factions)?;
    let id = map_file_id(path);
    app.world_mut()
        .resource_mut::<MapRegistry>()
        .insert(id.clone(), info);
    Ok(id)
}

/// Loads `data/maps.ron` (or the built-in copy) and the private Maps, and checks that every
/// Map's Factions exist. Maps without an entry play the first two Factions of `factions.ron`.
pub(super) fn load_registry(data: &GameData) -> MapRegistry {
    let builtin = crate::data::load(MAPS_FILE, BUILTIN_MAPS);
    let (declared, files) = match private_dir() {
        Some(dir) => {
            let declared = read_private_declared(&dir.join(MAPS_FILE));
            let files = private_map_stems(&dir.join("maps"));
            (declared, files)
        }
        None => (BTreeMap::new(), Vec::new()),
    };
    let default_factions = [data.factions[0].id.clone(), data.factions[1].id.clone()];
    let registry = MapRegistry::build(builtin, declared, &files, &default_factions);
    for (id, info) in registry.iter() {
        for faction in &info.factions {
            assert!(
                data.faction_by_id(faction).is_some(),
                "Map \"{id}\" uses unknown faction \"{faction}\" (not in factions.ron)"
            );
        }
    }
    let private = registry.iter().filter(|(_, info)| info.private).count();
    info!(
        "Maps: {} ({private} private)",
        registry
            .maps
            .keys()
            .map(|id| id.0.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    registry
}

/// `content/private/maps.ron`, if present. Like `data/`, a present-but-broken file is fatal.
fn read_private_declared(path: &Path) -> BTreeMap<MapId, MapInfo> {
    match std::fs::read_to_string(path) {
        Ok(text) => ron::from_str(&text).unwrap_or_else(|err| panic!("{}: {err}", path.display())),
        Err(_) => BTreeMap::new(),
    }
}

/// File stems of the `.map` files directly in `dir`, sorted; none when it is missing.
fn private_map_stems(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut stems: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "map"))
        .filter_map(|path| Some(path.file_stem()?.to_str()?.to_owned()))
        .collect();
    stems.sort();
    stems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(file: &str) -> MapInfo {
        MapInfo {
            name: file.to_owned(),
            file: file.to_owned(),
            look: None,
            modes: vec!["tdm".into()],
            factions: ["alpha_side".into(), "bravo_side".into()],
            private: false,
        }
    }

    #[test]
    fn builtin_maps_parse_and_include_the_default() {
        let maps: BTreeMap<MapId, MapInfo> = ron::from_str(BUILTIN_MAPS).unwrap();
        let arena = &maps[&MapId::new(DEFAULT_MAP)];
        assert_eq!(arena.file, "maps/arena.map");
        assert!(!arena.private);
        assert!(arena.supports(GameModeId::TeamDeathmatch));
    }

    #[test]
    fn private_maps_are_declared_or_discovered_and_never_shadow_builtins() {
        let builtin = BTreeMap::from([(MapId::new("arena"), info("maps/arena.map"))]);
        let declared = BTreeMap::from([
            (MapId::new("crash"), info("maps/crash_v2.map")),
            (MapId::new("arena"), info("maps/arena.map")),
        ]);
        let files = ["crash_v2".to_owned(), "vacant".to_owned()];
        let defaults = ["first".to_owned(), "second".to_owned()];
        let registry = MapRegistry::build(builtin, declared, &files, &defaults);

        let ids: Vec<&str> = registry.iter().map(|(id, _)| id.0.as_str()).collect();
        assert_eq!(ids, ["arena", "crash", "vacant"]);
        assert_eq!(
            registry.get(&MapId::new("arena")).unwrap().file,
            "maps/arena.map"
        );
        let crash = registry.get(&MapId::new("crash")).unwrap();
        assert_eq!(crash.file, "private://maps/crash_v2.map");
        assert!(crash.private);
        let vacant = registry.get(&MapId::new("vacant")).unwrap();
        assert_eq!(vacant.file, "private://maps/vacant.map");
        assert!(vacant.private && vacant.supports(GameModeId::TeamDeathmatch));
        assert_eq!(vacant.factions, defaults);
        assert_eq!(crash.factions, ["alpha_side", "bravo_side"]);
    }

    #[test]
    fn missing_private_folder_contents_are_empty() {
        let dir = Path::new("definitely/not/here");
        assert!(private_map_stems(dir).is_empty());
        assert!(read_private_declared(&dir.join(MAPS_FILE)).is_empty());
    }

    #[test]
    fn a_map_file_by_path_is_a_map_with_the_generic_entry() {
        let dir = std::env::temp_dir().join(format!("frontline-map-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(".mapedit-play-z_town.map");
        std::fs::write(&file, "").unwrap();
        let factions = ["first".to_owned(), "second".to_owned()];
        let info = map_file_info(&file, &factions).unwrap();
        assert_eq!(map_file_id(&file), MapId::new("mapedit-play-z_town"));
        assert_eq!(info.file, "mapfile://.mapedit-play-z_town.map");
        assert_eq!(info.name, "mapedit-play-z_town");
        assert!(info.look.is_none() && info.modes.is_empty() && !info.private);
        assert!(info.supports(GameModeId::Headquarters), "every Game Mode");
        assert_eq!(info.factions, factions);
        assert!(map_file_info(&dir.join("missing.map"), &factions).is_err());
        let text = dir.join("notes.txt");
        std::fs::write(&text, "").unwrap();
        assert!(map_file_info(&text, &factions).is_err(), "not a .map");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
