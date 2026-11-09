//! Files compiled into the binary, so a copied executable runs without an `assets/` folder.
//! Client/Host only: a Dedicated Server never registers (or loads) them.

use std::path::{Path, PathBuf};

use bevy::asset::io::embedded::EmbeddedAssetRegistry;
use bevy::prelude::*;

/// Asset path prefix of everything in [`EMBEDDED`]: `embedded://frontline/<path>`.
pub const EMBEDDED_ROOT: &str = "embedded://frontline/";

/// `(path under assets/, bytes)` for every embedded file.
macro_rules! embed {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_bytes!(concat!("../assets/", $path)))),*]
    };
}

const EMBEDDED: &[(&str, &[u8])] = embed![
    "icons/factions/ranger.png",
    "icons/factions/harmattan.png",
    "icons/factions/northern_sabre.png",
    "icons/factions/volkov.png",
    "icons/factions/baekho.png",
    "icons/factions/tianlang.png",
    "icons/factions/simba.png",
    "icons/factions/steppe_wolves.png",
    "icons/factions/crimson_vanguard.png",
    "icons/ranks/chevrons_01.png",
    "icons/ranks/chevrons_02.png",
    "icons/ranks/chevrons_03.png",
    "icons/ranks/chevrons_04.png",
    "icons/ranks/chevrons_05.png",
    "icons/ranks/chevrons_06.png",
    "icons/ranks/chevrons_07.png",
    "icons/ranks/chevrons_08.png",
    "icons/ranks/chevrons_09.png",
    "icons/ranks/chevrons_10.png",
    "icons/ranks/bars_01.png",
    "icons/ranks/bars_02.png",
    "icons/ranks/bars_03.png",
    "icons/ranks/bars_04.png",
    "icons/ranks/bars_05.png",
    "icons/ranks/bars_06.png",
    "icons/ranks/bars_07.png",
    "icons/ranks/bars_08.png",
    "icons/ranks/bars_09.png",
    "icons/ranks/bars_10.png",
    "icons/ranks/stars_01.png",
    "icons/ranks/stars_02.png",
    "icons/ranks/stars_03.png",
    "icons/ranks/stars_04.png",
    "icons/ranks/stars_05.png",
    "icons/ranks/stars_06.png",
    "icons/ranks/stars_07.png",
    "icons/ranks/stars_08.png",
    "icons/ranks/stars_09.png",
    "icons/ranks/stars_10.png",
    "camos/woodland.png",
    "camos/desert.png",
    "camos/urban.png",
    "camos/tiger.png",
    "camos/arctic.png",
    "camos/gold.png",
    "sounds/shot_ar.ogg",
    "sounds/shot_smg.ogg",
    "sounds/shot_sniper.ogg",
    "sounds/dry_fire.ogg",
    "sounds/mag_insert.ogg",
    "sounds/impact.ogg",
    "sounds/ring.ogg",
    "sounds/ricochet.ogg",
    "sounds/footstep_concrete_000.ogg",
    "sounds/footstep_concrete_001.ogg",
    "sounds/footstep_concrete_002.ogg",
    "sounds/footstep_concrete_003.ogg",
    "sounds/footstep_concrete_004.ogg",
    "sounds/footstep_concrete_005.ogg",
    "sounds/footstep_concrete_006.ogg",
    "sounds/footstep_concrete_007.ogg",
    "models/weapons/m4.glb",
    "models/weapons/m16.glb",
    "models/weapons/ak47.glb",
    "models/weapons/ak47variant.glb",
    "models/weapons/shotgun.glb",
    "models/weapons/awp.glb",
    "models/weapons/mac10.glb",
    "models/weapons/pew.glb",
    "models/weapons/nade_low.glb",
    "models/weapons/flashbang_low.glb",
    "models/weapons/smoke_low.glb",
];

/// The colour, normal and roughness maps of each surface folder under `assets/textures/`
/// (`materials::SurfaceKind`; a test there checks that every surface is listed here).
macro_rules! embed_surfaces {
    ($($name:literal),* $(,)?) => {
        &[$(
            (
                concat!("textures/", $name, "/color.jpg"),
                include_bytes!(concat!("../assets/textures/", $name, "/color.jpg")),
            ),
            (
                concat!("textures/", $name, "/normal.jpg"),
                include_bytes!(concat!("../assets/textures/", $name, "/normal.jpg")),
            ),
            (
                concat!("textures/", $name, "/roughness.jpg"),
                include_bytes!(concat!("../assets/textures/", $name, "/roughness.jpg")),
            ),
        )*]
    };
}

const EMBEDDED_SURFACES: &[(&str, &[u8])] = embed_surfaces![
    "concrete",
    "plaster",
    "metal",
    "corrugated",
    "wood",
    "ground",
    "brick",
    "grass",
    "plaster_yellow",
    "plaster_ochre",
    "plaster_blue",
    "plaster_white",
    "plaster_green",
    "brick_old",
    "brick_hollow",
    "stone",
    "concrete_damaged",
    "wood_painted",
    "wood_siding",
    "asphalt",
    "sand",
    "gravel",
    "paving",
    "paving_brick",
    "tiles",
    "wood_floor",
    "roof_tiles",
    "roof_slate",
    "metal_rust",
    "metal_green",
    "metal_white",
    "corrugated_rust",
    "corrugated_blue",
];

/// Whether `path` (under `assets/`) is compiled into the binary.
pub fn is_embedded(path: &str) -> bool {
    EMBEDDED
        .iter()
        .chain(EMBEDDED_SURFACES)
        .any(|(embedded, _)| *embedded == path)
}

/// Registers [`EMBEDDED`] and the surface textures with the asset server. Add it before plugins
/// that load those paths.
pub struct EmbeddedAssetsPlugin;

impl Plugin for EmbeddedAssetsPlugin {
    fn build(&self, app: &mut App) {
        let registry = app.world().resource::<EmbeddedAssetRegistry>();
        for (path, bytes) in EMBEDDED.iter().chain(EMBEDDED_SURFACES) {
            registry.insert_asset(PathBuf::new(), &Path::new("frontline").join(path), *bytes);
        }
    }
}
