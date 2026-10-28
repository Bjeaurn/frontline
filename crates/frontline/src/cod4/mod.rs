//! Original CoD4 textures for Homage Maps, bring-your-own (docs/adr/0004): read at runtime from
//! the player's own copy, decoded in memory, applied only to private Homage Maps. Nothing is ever
//! written to disk. Client/Host only; a Dedicated Server never loads textures.
//!
//! - [`source`] indexes the images by name (the `cod4_path` Setting, or `--cod4`), without
//!   decoding anything.
//! - [`iwi`] decodes one image.
//! - `content/private/cod4_textures.ron` maps, per Homage Map, our brush texture names (e.g.
//!   `brick/color`) to CoD4 image names. Names only.
//! - Before a mapped Map loads, [`Cod4::prepare_map`] decodes its images on a background thread
//!   into [`Cod4Overrides`]; the Map's texture hook (`maps`) then builds a material from them
//!   instead of the CC0 one. Anything missing or failing keeps the CC0 material.

pub mod iwi;
mod source;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde::Deserialize;

use self::source::Cod4Index;
use crate::flow::Settings;
use crate::maps::{MapId, private_dir};
use crate::materials::{generate_mips, repeat_sampler};

/// `content/private/cod4_textures.ron`: per Map id, our brush texture name -> CoD4 image names.
const MAPPING_FILE: &str = "cod4_textures.ron";
/// Decoded images are downscaled to at most this many pixels per side.
const MAX_SIDE: u32 = 1024;

/// What one of our brush texture names becomes on a Homage Map.
#[derive(Clone, Debug, Deserialize)]
pub struct TextureEntry {
    /// CoD4 colour image name, e.g. `me_brick_01`.
    pub color: String,
    /// CoD4 normal map image name; the surface is flat shaded without one.
    #[serde(default)]
    pub normal: Option<String>,
    /// How many metres of wall the image's width covers before it repeats.
    pub tile_m: f32,
}

type Mapping = BTreeMap<MapId, BTreeMap<String, TextureEntry>>;

/// A decoded entry of the mapping, ready to become a material.
#[derive(Clone, Debug)]
pub struct PreparedTexture {
    pub color: Image,
    pub normal: Option<Image>,
    pub tile_m: f32,
}

/// The decoded textures of one Map, by our brush texture name.
pub type MapTextures = HashMap<String, PreparedTexture>;

/// Decoded textures per Map file (`private://maps/backlot.map`), shared with the Map texture
/// hook, which runs on an asset-loading thread.
#[derive(Resource, Clone, Default)]
pub struct Cod4Overrides(Arc<Mutex<HashMap<String, Arc<MapTextures>>>>);

impl Cod4Overrides {
    pub fn get(&self, map_file: &str) -> Option<Arc<MapTextures>> {
        self.0
            .lock()
            .expect("overrides lock")
            .get(map_file)
            .cloned()
    }

    fn insert(&self, map_file: &str, textures: MapTextures) {
        self.0
            .lock()
            .expect("overrides lock")
            .insert(map_file.to_owned(), Arc::new(textures));
    }

    fn contains(&self, map_file: &str) -> bool {
        self.0
            .lock()
            .expect("overrides lock")
            .contains_key(map_file)
    }

    fn remove(&self, map_file: &str) {
        self.0.lock().expect("overrides lock").remove(map_file);
    }

    fn clear(&self) {
        self.0.lock().expect("overrides lock").clear();
    }
}

/// A value a background thread hands over once.
type Slot<T> = Arc<Mutex<Option<T>>>;

enum IndexState {
    /// No path configured.
    Off,
    Scanning(Slot<Result<(Cod4Index, Duration), String>>),
    Ready(Arc<Cod4Index>),
    Failed,
}

struct PrepareOutput {
    textures: MapTextures,
    report: Vec<String>,
    took: Duration,
}

/// The configured CoD4 source and the work done on it.
#[derive(Resource)]
pub struct Cod4 {
    overrides: Cod4Overrides,
    mapping: Mapping,
    path: Option<PathBuf>,
    index: IndexState,
    /// Decoding in progress, by Map file.
    jobs: HashMap<String, Slot<PrepareOutput>>,
}

impl Cod4 {
    fn new(overrides: Cod4Overrides, mapping: Mapping) -> Self {
        Self {
            overrides,
            mapping,
            path: None,
            index: IndexState::Off,
            jobs: HashMap::new(),
        }
    }

    /// Points at a new source (or none) and rebuilds the index in the background. Maps that
    /// load afterwards use it; loaded ones keep what they have.
    fn set_path(&mut self, path: Option<PathBuf>) {
        self.overrides.clear();
        self.jobs.clear();
        self.path = path.clone();
        self.index = match path {
            None => IndexState::Off,
            Some(path) => {
                let slot: Slot<_> = default();
                let result = slot.clone();
                let spawned =
                    std::thread::Builder::new()
                        .name("cod4-index".into())
                        .spawn(move || {
                            let started = Instant::now();
                            let scanned =
                                Cod4Index::scan(&path).map(|index| (index, started.elapsed()));
                            *result.lock().expect("index slot") = Some(scanned);
                        });
                match spawned {
                    Ok(_) => IndexState::Scanning(slot),
                    Err(err) => {
                        info!("CoD4 textures: can't start indexing ({err}); using CC0 textures");
                        IndexState::Failed
                    }
                }
            }
        };
    }

    /// One line for the Settings screen.
    pub fn status(&self) -> String {
        match &self.index {
            IndexState::Off => "Not set (CC0 textures)".into(),
            IndexState::Scanning(_) => "Reading...".into(),
            IndexState::Ready(index) => format!(
                "Found {} archive{}, {} images",
                index.archive_count(),
                if index.archive_count() == 1 { "" } else { "s" },
                with_thousands(index.image_count())
            ),
            IndexState::Failed => "Not found".into(),
        }
    }

    /// Whether `map`'s file may load now. A Map with a mapping waits (false) until its CoD4
    /// textures are decoded, starting that when the index is ready; without a source, an
    /// unreadable one or a mapping it falls through to the CC0 materials at once.
    pub fn prepare_map(&mut self, map: &MapId, map_file: &str) -> bool {
        let Some(entries) = self.mapping.get(map) else {
            return true;
        };
        let index = match &self.index {
            IndexState::Off | IndexState::Failed => return true,
            IndexState::Scanning(_) => return false,
            IndexState::Ready(index) => index.clone(),
        };
        if self.overrides.contains(map_file) {
            return true;
        }
        if !self.jobs.contains_key(map_file) {
            let entries = entries.clone();
            let slot: Slot<PrepareOutput> = default();
            let result = slot.clone();
            let spawned = std::thread::Builder::new()
                .name("cod4-decode".into())
                .spawn(move || {
                    *result.lock().expect("decode slot") = Some(prepare(&index, &entries))
                });
            if let Err(err) = spawned {
                info!("CoD4 textures: can't start decoding ({err}); using CC0 textures");
                self.overrides.insert(map_file, MapTextures::new());
                return true;
            }
            self.jobs.insert(map_file.to_owned(), slot);
        }
        false
    }

    /// Frees the decoded textures of a Map that unloaded.
    pub fn release(&self, map_file: &str) {
        self.overrides.remove(map_file);
    }

    fn poll(&mut self) {
        if let IndexState::Scanning(slot) = &self.index {
            let done = slot.lock().expect("index slot").take();
            match done {
                Some(Ok((index, took))) => {
                    info!(
                        "CoD4 textures: indexed {} images in {} archives from {} in {:.0} ms",
                        index.image_count(),
                        index.archive_count(),
                        self.path
                            .as_deref()
                            .map_or_else(String::new, |p| p.display().to_string()),
                        took.as_secs_f64() * 1000.0
                    );
                    self.index = IndexState::Ready(Arc::new(index));
                }
                Some(Err(err)) => {
                    info!("CoD4 textures: {err}; using CC0 textures");
                    self.index = IndexState::Failed;
                }
                None => {}
            }
        }
        let finished: Vec<(String, PrepareOutput)> = self
            .jobs
            .iter()
            .filter_map(|(file, slot)| {
                Some((file.clone(), slot.lock().expect("decode slot").take()?))
            })
            .collect();
        for (file, output) in finished {
            self.jobs.remove(&file);
            info!(
                "CoD4 textures for {file}: {} of the mapped textures decoded in {:.0} ms",
                output.textures.len(),
                output.took.as_secs_f64() * 1000.0
            );
            for line in &output.report {
                info!("  {line}");
            }
            self.overrides.insert(&file, output.textures);
        }
    }
}

/// Reads and decodes every image `entries` names; failures keep that surface on CC0.
fn prepare(index: &Cod4Index, entries: &BTreeMap<String, TextureEntry>) -> PrepareOutput {
    let started = Instant::now();
    let mut report = Vec::new();
    let mut textures = MapTextures::new();
    let wanted: Vec<&str> = entries
        .values()
        .flat_map(|entry| [Some(entry.color.as_str()), entry.normal.as_deref()])
        .flatten()
        .collect();
    let read_started = Instant::now();
    let bytes = match index.read_many(&wanted) {
        Ok(bytes) => bytes,
        Err(err) => {
            report.push(format!("reading images failed: {err}; using CC0 textures"));
            return PrepareOutput {
                textures,
                report,
                took: started.elapsed(),
            };
        }
    };
    report.push(format!(
        "read {} of {} images in {:.0} ms",
        bytes.len(),
        wanted.len(),
        read_started.elapsed().as_secs_f64() * 1000.0
    ));
    for (texture, entry) in entries {
        let decode_started = Instant::now();
        let color = match decode_image(&bytes, &entry.color, false) {
            Ok(image) => image,
            Err(err) => {
                report.push(format!("{texture}: {} {err}; using CC0", entry.color));
                continue;
            }
        };
        let (width, height) = (color.width(), color.height());
        let normal = entry.normal.as_ref().and_then(|name| {
            decode_image(&bytes, name, true)
                .map_err(|err| report.push(format!("{texture}: normal {name} {err}; flat")))
                .ok()
        });
        report.push(format!(
            "{texture} <- {} {width}x{height}{} decoded in {:.1} ms",
            entry.color,
            if normal.is_some() { " + normal" } else { "" },
            decode_started.elapsed().as_secs_f64() * 1000.0
        ));
        textures.insert(
            texture.clone(),
            PreparedTexture {
                color,
                normal,
                tile_m: entry.tile_m,
            },
        );
    }
    PrepareOutput {
        textures,
        report,
        took: started.elapsed(),
    }
}

fn decode_image(
    bytes: &HashMap<String, Vec<u8>>,
    name: &str,
    normal_map: bool,
) -> Result<Image, String> {
    let raw = bytes.get(name).ok_or("isn't in the CoD4 files")?;
    let mut rgba = iwi::decode(raw)
        .map_err(|err| err.to_string())?
        .downscaled_to(MAX_SIDE);
    if normal_map {
        rgba = rgba.into_normal_map();
    }
    let mut image = Image::new(
        Extent3d {
            width: rgba.width,
            height: rgba.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.data,
        if normal_map {
            TextureFormat::Rgba8Unorm
        } else {
            TextureFormat::Rgba8UnormSrgb
        },
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(repeat_sampler());
    generate_mips(&mut image);
    Ok(image)
}

/// 4213 -> "4,213".
fn with_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A path typed or pasted into the Settings field: trimmed, quotes (drag and drop) removed,
/// a leading `~` expanded; empty means none.
pub fn parse_path_input(text: &str) -> Option<PathBuf> {
    let text = text.trim().trim_matches(['"', '\'']).trim();
    if text.is_empty() {
        return None;
    }
    match text.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map(|home| home.join(rest)),
        None => Some(PathBuf::from(text)),
    }
}

/// Loads the mapping and keeps the CoD4 source in step with `Settings::cod4_path`. Windowed
/// builds only; add before `MapsPlugin` (its texture hook uses [`Cod4Overrides`]); the Settings
/// (`FlowPlugin`) are read once the app runs.
pub struct Cod4Plugin;

impl Plugin for Cod4Plugin {
    fn build(&self, app: &mut App) {
        let overrides = Cod4Overrides::default();
        app.insert_resource(overrides.clone())
            .insert_resource(Cod4::new(overrides, load_mapping()))
            .add_systems(Startup, follow_setting)
            .add_systems(
                Update,
                (
                    follow_setting.run_if(resource_changed::<Settings>),
                    poll_cod4,
                ),
            );
    }
}

fn load_mapping() -> Mapping {
    let Some(path) = private_dir().map(|dir| dir.join(MAPPING_FILE)) else {
        return Mapping::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Mapping::new();
    };
    match ron::from_str(&text) {
        Ok(mapping) => mapping,
        Err(err) => {
            warn!(
                "{}: {err}; Homage Maps keep the CC0 textures",
                path.display()
            );
            Mapping::new()
        }
    }
}

fn follow_setting(settings: Res<Settings>, mut cod4: ResMut<Cod4>) {
    if cod4.path != settings.cod4_path {
        cod4.set_path(settings.cod4_path.clone());
    }
}

fn poll_cod4(mut cod4: ResMut<Cod4>) {
    cod4.poll();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separators() {
        assert_eq!(with_thousands(0), "0");
        assert_eq!(with_thousands(999), "999");
        assert_eq!(with_thousands(4213), "4,213");
        assert_eq!(with_thousands(1234567), "1,234,567");
    }

    #[test]
    fn path_input_is_cleaned() {
        assert_eq!(parse_path_input("  "), None);
        assert_eq!(
            parse_path_input(" \"/games/cod4\" "),
            Some(PathBuf::from("/games/cod4"))
        );
        assert_eq!(parse_path_input("'/a b'"), Some(PathBuf::from("/a b")));
    }

    #[test]
    fn mapping_file_format_parses() {
        let mapping: Mapping = ron::from_str(
            r#"{ "backlot": { "brick/color": (color: "a", normal: Some("b"), tile_m: 2.0),
                              "wood/color": (color: "c", tile_m: 1.0) } }"#,
        )
        .unwrap();
        let backlot = &mapping[&MapId::new("backlot")];
        assert_eq!(backlot["brick/color"].normal.as_deref(), Some("b"));
        assert_eq!(backlot["wood/color"].normal, None);
    }
}
