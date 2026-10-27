//! Visuals of the ammo packs dead players drop (`gear::AmmoPack`, spawned by the server): a
//! small olive crate slowly turning where the player fell.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::gear::AmmoPack;

const PACK_SIZE: Vec3 = Vec3::new(0.34, 0.2, 0.24);
/// Lift above the floor, so the crate sits on it.
const PACK_LIFT: f32 = 0.1;
/// Turn rate, in radians per second.
const SPIN: f32 = 1.2;

#[derive(Resource)]
pub(super) struct PackAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

pub(super) fn load_pack_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(PackAssets {
        mesh: meshes.add(Cuboid::from_size(PACK_SIZE)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.5, 0.22),
            emissive: LinearRgba::rgb(0.05, 0.1, 0.02),
            perceptual_roughness: 0.7,
            ..default()
        }),
    });
}

/// Dresses a new pack (replicated from the server, or the server's own on a Host).
pub(super) fn show_pack(
    add: On<Add<AmmoPack>>,
    packs: Query<&AmmoPack>,
    assets: Res<PackAssets>,
    mut commands: Commands,
) {
    let Ok(pack) = packs.get(add.entity) else {
        return;
    };
    commands.entity(add.entity).insert((
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.material.clone()),
        Transform::from_translation(pack.position + Vec3::Y * PACK_LIFT),
        Visibility::default(),
        NotShadowCaster,
    ));
}

pub(super) fn spin_packs(time: Res<Time>, mut packs: Query<&mut Transform, With<AmmoPack>>) {
    for mut transform in &mut packs {
        transform.rotate_y(SPIN * time.delta_secs());
    }
}
