//! The greybox arena: static colliders in every mode, meshes and lights only when rendering.

use avian3d::prelude::*;
use bevy::{mesh::MeshPlugin, prelude::*};

use crate::cli::Mode;
use crate::shared::GameLayer;

/// Arena floor is `ARENA_SIZE` x `ARENA_SIZE` metres, centred on the origin, top face at y = 0.
const ARENA_SIZE: f32 = 60.0;
const PERIMETER_WALL_HEIGHT: f32 = 4.0;

/// Physics, arena geometry and spawn points. Used in every mode.
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
        app.add_plugins(PhysicsPlugins::default())
            .insert_resource(SpawnPoints(spawn_points()))
            .add_systems(
                Startup,
                (
                    spawn_arena,
                    (add_arena_visuals, spawn_lighting).run_if(is_rendering),
                )
                    .chain(),
            );
    }
}

/// Feet positions where players may (re)spawn, spread around the arena.
#[derive(Resource, Clone, Debug)]
pub struct SpawnPoints(pub Vec<Vec3>);

/// Run condition: this process draws the world (Client or Host).
pub fn is_rendering(mode: Res<Mode>) -> bool {
    !matches!(*mode, Mode::Dedicated { .. })
}

/// Yaw that faces the arena centre from `position`.
pub fn yaw_towards_centre(position: Vec3) -> f32 {
    position.x.atan2(position.z)
}

/// Surface category of an arena block; picks its greybox colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Surface {
    Floor,
    Perimeter,
    Building,
    Ramp,
    Cover,
    Crate,
    Slab,
}

impl Surface {
    /// In declaration order, so `surface as usize` indexes it.
    const ALL: [Surface; 7] = [
        Surface::Floor,
        Surface::Perimeter,
        Surface::Building,
        Surface::Ramp,
        Surface::Cover,
        Surface::Crate,
        Surface::Slab,
    ];

    fn color(self) -> Color {
        match self {
            Surface::Floor => Color::srgb(0.36, 0.37, 0.38),
            Surface::Perimeter => Color::srgb(0.55, 0.55, 0.57),
            Surface::Building => Color::srgb(0.68, 0.64, 0.56),
            Surface::Ramp => Color::srgb(0.72, 0.50, 0.28),
            Surface::Cover => Color::srgb(0.47, 0.52, 0.45),
            Surface::Crate => Color::srgb(0.56, 0.44, 0.30),
            Surface::Slab => Color::srgb(0.42, 0.46, 0.55),
        }
    }
}

/// One static box of arena geometry. `size` is the full extent along each local axis.
#[derive(Component, Clone, Copy, Debug)]
struct ArenaBlock {
    size: Vec3,
    surface: Surface,
}

fn spawn_points() -> Vec<Vec3> {
    [
        (-24.0, -24.0),
        (24.0, -24.0),
        (-24.0, 24.0),
        (24.0, 24.0),
        (0.0, -25.0),
        (0.0, 25.0),
        (-25.0, 0.0),
        (25.0, 0.0),
    ]
    .into_iter()
    .map(|(x, z)| Vec3::new(x, 0.0, z))
    .collect()
}

fn spawn_arena(mut commands: Commands) {
    for (block, transform) in arena_layout() {
        commands.spawn((
            Name::new(format!("{:?}", block.surface)),
            block,
            transform,
            RigidBody::Static,
            Collider::cuboid(block.size.x, block.size.y, block.size.z),
            CollisionLayers::new(GameLayer::World, LayerMask::ALL),
        ));
    }
}

fn add_arena_visuals(
    mut commands: Commands,
    blocks: Query<(Entity, &ArenaBlock)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let palette = Surface::ALL.map(|surface| {
        materials.add(StandardMaterial {
            base_color: surface.color(),
            perceptual_roughness: 0.9,
            ..default()
        })
    });
    for (entity, block) in &blocks {
        commands.entity(entity).insert((
            Mesh3d(meshes.add(Cuboid::from_size(block.size))),
            MeshMaterial3d(palette[block.surface as usize].clone()),
        ));
    }
}

fn spawn_lighting(mut commands: Commands) {
    commands.insert_resource(ClearColor(Color::srgb(0.62, 0.70, 0.78)));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.85, 0.9, 1.0),
        brightness: 250.0,
        ..default()
    });
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.6), Vec3::Y),
    ));
}

/// Every static block of the arena with its world transform.
fn arena_layout() -> Vec<(ArenaBlock, Transform)> {
    let mut blocks = Vec::new();
    let mut add = |surface: Surface, size: [f32; 3], center: [f32; 3]| {
        blocks.push((
            ArenaBlock {
                size: Vec3::from(size),
                surface,
            },
            Transform::from_translation(Vec3::from(center)),
        ));
    };

    // Floor (top face at y = 0) and perimeter walls.
    let half = ARENA_SIZE / 2.0;
    let wall_y = PERIMETER_WALL_HEIGHT / 2.0;
    add(Surface::Floor, [ARENA_SIZE, 1.0, ARENA_SIZE], [0.0, -0.5, 0.0]);
    for sign in [-1.0, 1.0] {
        add(
            Surface::Perimeter,
            [ARENA_SIZE + 2.0, PERIMETER_WALL_HEIGHT, 1.0],
            [0.0, wall_y, sign * (half + 0.5)],
        );
        add(
            Surface::Perimeter,
            [1.0, PERIMETER_WALL_HEIGHT, ARENA_SIZE],
            [sign * (half + 0.5), wall_y, 0.0],
        );
    }

    // Central building: 12 x 12 m, 3 m ground storey with a walkable roof.
    // North/south: walk-in doorways. West: a 0.6 m prone-only crawl gap. East: solid, ramp to roof.
    for sign in [-1.0, 1.0] {
        let z = sign * 5.8;
        add(Surface::Building, [5.2, 3.0, 0.4], [-3.4, 1.5, z]);
        add(Surface::Building, [5.2, 3.0, 0.4], [3.4, 1.5, z]);
        add(Surface::Building, [1.6, 0.8, 0.4], [0.0, 2.6, z]);
        add(Surface::Building, [0.4, 3.0, 4.8], [-5.8, 1.5, sign * 3.2]);
        // Roof parapet, crouch height; the east side leaves a gap where the ramp arrives.
        add(Surface::Building, [12.0, 1.0, 0.3], [0.0, 3.8, sign * 5.85]);
        add(Surface::Building, [0.3, 1.0, 4.3], [5.85, 3.8, sign * 3.55]);
    }
    add(Surface::Building, [0.4, 2.4, 1.6], [-5.8, 1.8, 0.0]);
    add(Surface::Building, [0.4, 3.0, 11.2], [5.8, 1.5, 0.0]);
    add(Surface::Building, [12.0, 0.3, 12.0], [0.0, 3.15, 0.0]);
    add(Surface::Building, [0.3, 1.0, 11.4], [-5.85, 3.8, 0.0]);
    add(Surface::Cover, [4.0, 1.0, 0.4], [0.0, 0.5, 0.0]);

    // West crawl barrier: a slab on two pillars leaves a 0.6 m gap, flanked by tall walls.
    add(Surface::Slab, [1.0, 1.6, 1.0], [-16.0, 0.8, -2.0]);
    add(Surface::Slab, [1.0, 1.6, 1.0], [-16.0, 0.8, 2.0]);
    add(Surface::Slab, [1.0, 1.0, 5.0], [-16.0, 1.1, 0.0]);
    add(Surface::Perimeter, [0.5, 2.5, 8.0], [-16.0, 1.25, -6.5]);
    add(Surface::Perimeter, [0.5, 2.5, 8.0], [-16.0, 1.25, 6.5]);

    // Crouch-height (1 m) cover walls.
    add(Surface::Cover, [4.0, 1.0, 0.4], [0.0, 0.5, -14.0]);
    add(Surface::Cover, [4.0, 1.0, 0.4], [0.0, 0.5, 14.0]);
    add(Surface::Cover, [0.4, 1.0, 4.0], [20.0, 0.5, -8.0]);
    add(Surface::Cover, [0.4, 1.0, 4.0], [20.0, 0.5, 8.0]);
    add(Surface::Cover, [3.0, 1.0, 0.6], [-22.0, 0.5, -10.0]);
    add(Surface::Cover, [3.0, 1.0, 0.6], [-22.0, 0.5, 10.0]);

    // Crates, some stacked.
    add(Surface::Crate, [1.0, 1.0, 1.0], [10.0, 0.5, -10.0]);
    add(Surface::Crate, [1.0, 1.0, 1.0], [11.2, 0.5, -10.3]);
    add(Surface::Crate, [1.0, 1.0, 1.0], [10.6, 1.5, -10.15]);
    add(Surface::Crate, [1.0, 1.0, 1.0], [-10.0, 0.5, 10.0]);
    add(Surface::Crate, [1.0, 1.0, 1.0], [-11.2, 0.5, 10.4]);
    add(Surface::Crate, [1.0, 1.0, 1.0], [10.0, 0.5, 12.0]);
    add(Surface::Crate, [1.0, 1.0, 1.0], [-9.5, 0.5, -12.0]);

    // Medium and full-height cover.
    add(Surface::Cover, [2.0, 1.5, 2.0], [-8.0, 0.75, -20.0]);
    add(Surface::Cover, [2.0, 1.5, 2.0], [8.0, 0.75, 20.0]);
    add(Surface::Cover, [2.0, 2.2, 2.0], [16.0, 1.1, 16.0]);
    add(Surface::Cover, [2.0, 2.2, 2.0], [-16.0, 1.1, -16.0]);
    add(Surface::Perimeter, [6.0, 2.5, 0.5], [16.0, 1.25, -22.0]);
    add(Surface::Perimeter, [6.0, 2.5, 0.5], [-16.0, 1.25, 22.0]);
    add(Surface::Slab, [2.5, 2.6, 6.0], [-22.0, 1.3, 18.0]);
    add(Surface::Slab, [2.5, 2.6, 6.0], [22.0, 1.3, -14.0]);

    // Ramp from the east floor (x = 13) up to the roof edge (x = 6, y = 3.3).
    let (run, rise, thickness) = (7.0_f32, 3.3_f32, 0.3_f32);
    let angle = rise.atan2(run);
    let rotation = Quat::from_rotation_z(-angle);
    let top_mid = Vec3::new(6.0 + run / 2.0, rise / 2.0, 0.0);
    blocks.push((
        ArenaBlock {
            size: Vec3::new(run.hypot(rise), thickness, 2.6),
            surface: Surface::Ramp,
        },
        Transform::from_translation(top_mid - rotation * Vec3::Y * (thickness / 2.0))
            .with_rotation(rotation),
    ));

    blocks
}
