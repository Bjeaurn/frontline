//! Physics, in every mode. The geometry players move and shoot against comes from the loaded
//! Map (`maps`).

use avian3d::prelude::*;
use bevy::{mesh::MeshPlugin, prelude::*};

/// Physics and the asset support it needs. Used in every mode; add before `MapsPlugin`.
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        // Avian's collider-from-mesh systems need `Assets<Mesh>`, which `MinimalPlugins`
        // (Dedicated Server) lacks; windowed modes already have both from `DefaultPlugins`.
        if !app.is_plugin_added::<AssetPlugin>() {
            app.add_plugins(AssetPlugin::default());
        }
        if !app.is_plugin_added::<MeshPlugin>() {
            app.add_plugins(MeshPlugin);
        }
        app.add_plugins(PhysicsPlugins::default());
    }
}
