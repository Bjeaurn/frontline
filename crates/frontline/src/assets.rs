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
    "icons/factions/brigade.png",
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
