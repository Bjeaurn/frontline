//! The ammo crates in the world (Client and Host): one beside each Domination Flag, lit with its
//! lid open while a player of the holding Team stands there, and the fixed crates of Team
//! Deathmatch, open and glowing while ready and shut dark on their respawn timer. Drawn from the
//! replicated `ModeView`, procedurally (a body, a hinged lid, a glow inside and a small light).

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::view::ModeView;
use crate::flow::AppState;

const BODY_SIZE: Vec3 = Vec3::new(0.9, 0.45, 0.6);
const LID_THICKNESS: f32 = 0.06;
/// Where the crate at a Domination Flag stands, beside its pole.
const FLAG_OFFSET: Vec3 = Vec3::new(1.4, 0.0, 1.0);
/// The lid is open this far (radians) when the crate is usable.
const LID_OPEN: f32 = 1.9;
/// Exponential rate (1/s) at which the lid and glow follow the crate's state.
const FOLLOW_PER_SEC: f32 = 6.0;
const GLOW_LUMENS: f32 = 90_000.0;
const GLOW_RANGE: f32 = 5.0;

pub struct AmmoCrateVisualsPlugin;

impl Plugin for AmmoCrateVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_assets).add_systems(
            Update,
            (sync_crates, animate_crates)
                .chain()
                .run_if(in_state(AppState::InGame)),
        );
    }
}

#[derive(Resource)]
struct CrateAssets {
    body: Handle<Mesh>,
    lid: Handle<Mesh>,
    glow: Handle<Mesh>,
    shell: Handle<StandardMaterial>,
    glow_material: Handle<StandardMaterial>,
}

fn load_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(CrateAssets {
        body: meshes.add(Cuboid::from_size(BODY_SIZE)),
        lid: meshes.add(Cuboid::new(
            BODY_SIZE.x + 0.04,
            LID_THICKNESS,
            BODY_SIZE.z + 0.04,
        )),
        glow: meshes.add(Cuboid::new(BODY_SIZE.x - 0.12, 0.02, BODY_SIZE.z - 0.12)),
        shell: materials.add(StandardMaterial {
            base_color: Color::srgb(0.30, 0.36, 0.17),
            perceptual_roughness: 0.75,
            ..default()
        }),
        glow_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.5, 1.0, 0.45),
            emissive: LinearRgba::rgb(0.9, 3.2, 0.7),
            unlit: true,
            ..default()
        }),
    });
}

/// Which crate of the Mode this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Flag,
    Fixed,
}

/// A crate the Mode wants drawn.
struct Wanted {
    kind: Kind,
    index: usize,
    position: Vec3,
    /// Usable now: lit with the lid open.
    usable: bool,
}

fn wanted(view: &ModeView) -> Vec<Wanted> {
    match view {
        ModeView::Domination(flags) => flags
            .iter()
            .enumerate()
            .map(|(index, flag)| Wanted {
                kind: Kind::Flag,
                index,
                position: flag.position + FLAG_OFFSET,
                usable: flag.resupplying,
            })
            .collect(),
        ModeView::TeamDeathmatch(crates) => crates
            .iter()
            .enumerate()
            .map(|(index, slot)| Wanted {
                kind: Kind::Fixed,
                index,
                position: slot.position,
                usable: slot.ready,
            })
            .collect(),
        ModeView::None | ModeView::Headquarters(_) | ModeView::SearchAndDestroy(_) => Vec::new(),
    }
}

/// A drawn crate and the parts that follow its state.
#[derive(Component)]
struct AmmoCrateProp {
    kind: Kind,
    index: usize,
    usable: bool,
    /// 0 shut and dark, 1 open and lit; eases toward `usable`.
    open: f32,
    lid: Entity,
    glow: Entity,
    light: Entity,
}

/// Spawns, moves and despawns the drawn crates to match the Mode's view.
fn sync_crates(
    modes: Query<&ModeView>,
    assets: Res<CrateAssets>,
    mut props: Query<(Entity, &mut AmmoCrateProp, &mut Transform)>,
    mut commands: Commands,
) {
    let wanted = modes.single().map(wanted).unwrap_or_default();
    let mut present = Vec::new();
    for (entity, mut prop, mut transform) in &mut props {
        match wanted
            .iter()
            .find(|crate_| crate_.kind == prop.kind && crate_.index == prop.index)
        {
            Some(crate_) => {
                present.push((crate_.kind, crate_.index));
                if prop.usable != crate_.usable {
                    prop.usable = crate_.usable;
                }
                let at = crate_.position + Vec3::Y * BODY_SIZE.y / 2.0;
                if transform.translation != at {
                    transform.translation = at;
                }
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for crate_ in &wanted {
        if present.contains(&(crate_.kind, crate_.index)) {
            continue;
        }
        let root = commands.spawn_empty().id();
        // The lid hinges on the back top edge and swings up and over.
        let lid = commands
            .spawn((
                Name::new("Ammo crate lid"),
                Transform::from_xyz(0.0, BODY_SIZE.y / 2.0, -BODY_SIZE.z / 2.0),
                Visibility::default(),
                ChildOf(root),
                children![(
                    Mesh3d(assets.lid.clone()),
                    MeshMaterial3d(assets.shell.clone()),
                    Transform::from_xyz(0.0, LID_THICKNESS / 2.0, BODY_SIZE.z / 2.0),
                    NotShadowCaster,
                )],
            ))
            .id();
        let glow = commands
            .spawn((
                Name::new("Ammo crate glow"),
                Mesh3d(assets.glow.clone()),
                MeshMaterial3d(assets.glow_material.clone()),
                Transform::from_xyz(0.0, BODY_SIZE.y / 2.0 - 0.02, 0.0),
                Visibility::Hidden,
                NotShadowCaster,
                ChildOf(root),
            ))
            .id();
        let light = commands
            .spawn((
                Name::new("Ammo crate light"),
                PointLight {
                    color: Color::srgb(0.6, 1.0, 0.5),
                    intensity: GLOW_LUMENS,
                    range: GLOW_RANGE,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.7, 0.0),
                Visibility::Hidden,
                ChildOf(root),
            ))
            .id();
        commands.entity(root).insert((
            Name::new(format!("Ammo crate {:?} {}", crate_.kind, crate_.index + 1)),
            DespawnOnExit(AppState::InGame),
            AmmoCrateProp {
                kind: crate_.kind,
                index: crate_.index,
                usable: crate_.usable,
                open: 0.0,
                lid,
                glow,
                light,
            },
            Transform::from_translation(crate_.position + Vec3::Y * BODY_SIZE.y / 2.0),
            Visibility::default(),
            Mesh3d(assets.body.clone()),
            MeshMaterial3d(assets.shell.clone()),
        ));
    }
}

/// Eases each crate's lid, glow and light toward open (usable) or shut (not).
fn animate_crates(
    time: Res<Time>,
    mut props: Query<&mut AmmoCrateProp>,
    mut parts: Query<(&mut Transform, &mut Visibility), Without<AmmoCrateProp>>,
) {
    let follow = 1.0 - (-FOLLOW_PER_SEC * time.delta_secs()).exp();
    for mut prop in &mut props {
        let target = if prop.usable { 1.0 } else { 0.0 };
        if prop.open == target {
            continue;
        }
        let open = prop.open + (target - prop.open) * follow;
        prop.open = if (open - target).abs() < 0.01 {
            target
        } else {
            open
        };
        let (lid, glow, light, open) = (prop.lid, prop.glow, prop.light, prop.open);
        if let Ok((mut transform, _)) = parts.get_mut(lid) {
            transform.rotation = Quat::from_rotation_x(-open * LID_OPEN);
        }
        let shown = if open > 0.05 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for part in [glow, light] {
            if let Ok((_, mut visibility)) = parts.get_mut(part) {
                visibility.set_if_neq(shown);
            }
        }
    }
}
