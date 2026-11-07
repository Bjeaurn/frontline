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
    "textures/concrete/color.jpg",
    "textures/concrete/normal.jpg",
    "textures/concrete/roughness.jpg",
    "textures/plaster/color.jpg",
    "textures/plaster/normal.jpg",
    "textures/plaster/roughness.jpg",
    "textures/metal/color.jpg",
    "textures/metal/normal.jpg",
    "textures/metal/roughness.jpg",
    "textures/corrugated/color.jpg",
    "textures/corrugated/normal.jpg",
    "textures/corrugated/roughness.jpg",
    "textures/wood/color.jpg",
    "textures/wood/normal.jpg",
    "textures/wood/roughness.jpg",
    "textures/ground/color.jpg",
    "textures/ground/normal.jpg",
    "textures/ground/roughness.jpg",
    "textures/brick/color.jpg",
    "textures/brick/normal.jpg",
    "textures/brick/roughness.jpg",
    "textures/grass/color.jpg",
    "textures/grass/normal.jpg",
    "textures/grass/roughness.jpg",
    "models/weapons/m4.glb",
    "models/weapons/m16.glb",
    "models/weapons/awp.glb",
    "models/weapons/mac10.glb",
    "models/weapons/pew.glb",
    "models/weapons/nade_low.glb",
    "models/weapons/flashbang_low.glb",
    "models/weapons/smoke_low.glb",
];

/// Registers [`EMBEDDED`] with the asset server. Add it before plugins that load those paths.
pub struct EmbeddedAssetsPlugin;

impl Plugin for EmbeddedAssetsPlugin {
    fn build(&self, app: &mut App) {
        let registry = app.world().resource::<EmbeddedAssetRegistry>();
        for (path, bytes) in EMBEDDED {
            registry.insert_asset(PathBuf::new(), &Path::new("frontline").join(path), *bytes);
        }
    }
}
