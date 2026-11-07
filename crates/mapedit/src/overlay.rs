//! What the Map's entities mean, drawn over the world: spawn points (by Team, with facing),
//! Flags, HQ sites, bomb sites and ammo crates, each with its real zone size, labels over the
//! objectives, and the physics colliders as wireframe. `1`..`5` filter by Game Mode, `X` draws
//! through walls, `L` toggles labels, `C` the collider wireframe.

use std::f32::consts::FRAC_PI_2;

use avian3d::prelude::{PhysicsDebugPlugin, PhysicsGizmos};
use bevy::gizmos::config::{DefaultGizmoConfigGroup, GizmoConfigStore};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use frontline::maps::{AmmoCrate, BombSite, DomFlag, HqSite, SpawnPoint, SpawnTeam};

use crate::menu::InputCapture;
use crate::session::Reload;

/// Game Mode keys, in the order of the `2`..`5` keys.
pub const MODE_KEYS: [&str; 4] = ["tdm", "dom", "hq", "sd"];

// Zone sizes from data/game_modes.ron (capture zones, bomb sites, ammo crates).
const CAPTURE_RADIUS: f32 = 3.5;
const CAPTURE_HEIGHT: f32 = 3.0;
const BOMB_SITE_RADIUS: f32 = 2.5;
const AMMO_CRATE_RADIUS: f32 = 1.6;

/// Which overlays are on.
#[derive(Resource)]
pub struct Overlay {
    /// Only what this Game Mode uses; `None` shows everything.
    pub mode: Option<&'static str>,
    pub through_walls: bool,
    pub labels: bool,
    pub colliders: bool,
    pub fog: bool,
}

impl Overlay {
    pub fn new(mode: Option<&'static str>) -> Self {
        Self {
            mode,
            through_walls: true,
            labels: true,
            colliders: false,
            fog: true,
        }
    }

    fn shows(&self, mode: &str) -> bool {
        self.mode.is_none_or(|filter| filter == mode)
    }
}

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhysicsDebugPlugin)
            .add_systems(Startup, configure_gizmos)
            .add_systems(
                Update,
                (
                    toggles,
                    apply_toggles,
                    apply_fog,
                    draw,
                    add_labels,
                    place_labels,
                )
                    .chain(),
            );
    }
}

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    store.config_mut::<PhysicsGizmos>().0.enabled = false;
}

fn toggles(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<InputCapture>,
    mut overlay: ResMut<Overlay>,
) {
    if capture.blocked() {
        return;
    }
    let modes = [
        (KeyCode::Digit1, None),
        (KeyCode::Digit2, Some(MODE_KEYS[0])),
        (KeyCode::Digit3, Some(MODE_KEYS[1])),
        (KeyCode::Digit4, Some(MODE_KEYS[2])),
        (KeyCode::Digit5, Some(MODE_KEYS[3])),
    ];
    for (key, mode) in modes {
        if keys.just_pressed(key) {
            overlay.mode = mode;
        }
    }
    if keys.just_pressed(KeyCode::KeyX) {
        overlay.through_walls = !overlay.through_walls;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        overlay.labels = !overlay.labels;
    }
    if keys.just_pressed(KeyCode::KeyC) {
        overlay.colliders = !overlay.colliders;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        overlay.fog = !overlay.fog;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        overlay.fog = false;
    }
    if keys.just_pressed(KeyCode::Home) {
        overlay.fog = true;
    }
}

/// Fog hides the far side of a Map from above, so `F` (and the top view) turn it off: the
/// look's fog is overridden every frame, and a reload puts the look's own back.
fn apply_fog(
    overlay: Res<Overlay>,
    mut fogs: Query<&mut DistanceFog>,
    mut reload: MessageWriter<Reload>,
    mut was_off: Local<bool>,
) {
    if overlay.fog {
        if *was_off {
            *was_off = false;
            reload.write(Reload);
        }
        return;
    }
    *was_off = true;
    for mut fog in &mut fogs {
        fog.falloff = FogFalloff::Linear {
            start: 1.0e6,
            end: 2.0e6,
        };
    }
}

fn apply_toggles(overlay: Res<Overlay>, mut store: ResMut<GizmoConfigStore>) {
    if !overlay.is_changed() {
        return;
    }
    store.config_mut::<PhysicsGizmos>().0.enabled = overlay.colliders;
    store.config_mut::<DefaultGizmoConfigGroup>().0.depth_bias =
        if overlay.through_walls { -1.0 } else { 0.0 };
}

/// A horizontal ring.
fn ring(gizmos: &mut Gizmos, centre: Vec3, radius: f32, color: Color) {
    gizmos.circle(
        Isometry3d::new(centre, Quat::from_rotation_x(FRAC_PI_2)),
        radius,
        color,
    );
}

fn draw(
    mut gizmos: Gizmos,
    overlay: Res<Overlay>,
    spawns: Query<(&SpawnPoint, &GlobalTransform)>,
    flags: Query<&GlobalTransform, With<DomFlag>>,
    hqs: Query<&GlobalTransform, With<HqSite>>,
    bombs: Query<&GlobalTransform, With<BombSite>>,
    crates: Query<&GlobalTransform, With<AmmoCrate>>,
) {
    for (spawn, transform) in &spawns {
        let modes = spawn.mode_keys();
        if overlay
            .mode
            .is_some_and(|mode| !modes.is_empty() && !modes.iter().any(|m| m == mode))
        {
            continue;
        }
        let color = match spawn.team {
            SpawnTeam::Alpha => Color::srgb(0.3, 0.6, 1.0),
            SpawnTeam::Bravo => Color::srgb(1.0, 0.35, 0.3),
            SpawnTeam::Any => Color::srgb(0.9, 0.9, 0.9),
        };
        let feet = transform.translation();
        let head = feet + Vec3::Y * 1.8;
        let forward = transform.compute_transform().rotation * Vec3::NEG_Z;
        gizmos.line(feet, head, color);
        gizmos.sphere(Isometry3d::from_translation(head), 0.25, color);
        gizmos.arrow(head, head + forward * 1.4, color);
        ring(&mut gizmos, feet + Vec3::Y * 0.05, 0.4, color);
    }
    if overlay.shows("dom") {
        let color = Color::srgb(1.0, 0.9, 0.2);
        for transform in &flags {
            let at = transform.translation();
            ring(&mut gizmos, at + Vec3::Y * 0.05, CAPTURE_RADIUS, color);
            ring(
                &mut gizmos,
                at + Vec3::Y * CAPTURE_HEIGHT,
                CAPTURE_RADIUS,
                color,
            );
            gizmos.line(at, at + Vec3::Y * CAPTURE_HEIGHT, color);
        }
    }
    if overlay.shows("hq") {
        let color = Color::srgb(0.9, 0.3, 1.0);
        for transform in &hqs {
            let at = transform.translation();
            ring(&mut gizmos, at + Vec3::Y * 0.05, CAPTURE_RADIUS, color);
            ring(
                &mut gizmos,
                at + Vec3::Y * CAPTURE_HEIGHT,
                CAPTURE_RADIUS,
                color,
            );
            gizmos.line(at, at + Vec3::Y * CAPTURE_HEIGHT, color);
        }
    }
    if overlay.shows("sd") {
        let color = Color::srgb(1.0, 0.55, 0.1);
        for transform in &bombs {
            let at = transform.translation();
            ring(&mut gizmos, at + Vec3::Y * 0.05, BOMB_SITE_RADIUS, color);
            gizmos.line(at, at + Vec3::Y * 2.0, color);
        }
    }
    if overlay.shows("tdm") {
        let color = Color::srgb(0.3, 0.9, 0.4);
        for transform in &crates {
            let at = transform.translation();
            ring(&mut gizmos, at + Vec3::Y * 0.05, AMMO_CRATE_RADIUS, color);
            gizmos.line(at, at + Vec3::Y * 1.0, color);
        }
    }
}

/// A screen-space text over a Map entity, shown with the Game Mode `mode` uses it in.
#[derive(Component)]
struct Label {
    target: Entity,
    mode: &'static str,
    height: f32,
}

fn label(
    commands: &mut Commands,
    target: Entity,
    mode: &'static str,
    text: String,
    color: Color,
    height: f32,
) {
    commands.spawn((
        Label {
            target,
            mode,
            height,
        },
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(color),
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        Visibility::Hidden,
    ));
}

fn add_labels(
    mut commands: Commands,
    flags: Query<(Entity, &DomFlag), Added<DomFlag>>,
    hqs: Query<Entity, Added<HqSite>>,
    bombs: Query<(Entity, &BombSite), Added<BombSite>>,
    crates: Query<Entity, Added<AmmoCrate>>,
) {
    for (entity, flag) in &flags {
        let text = format!("Flag {}", flag.label.letter());
        let color = Color::srgb(1.0, 0.9, 0.2);
        label(
            &mut commands,
            entity,
            "dom",
            text,
            color,
            CAPTURE_HEIGHT + 0.6,
        );
    }
    for entity in &hqs {
        let color = Color::srgb(0.9, 0.3, 1.0);
        label(
            &mut commands,
            entity,
            "hq",
            "HQ".into(),
            color,
            CAPTURE_HEIGHT + 0.6,
        );
    }
    for (entity, bomb) in &bombs {
        let text = format!("Bomb {}", bomb.label.letter());
        label(
            &mut commands,
            entity,
            "sd",
            text,
            Color::srgb(1.0, 0.55, 0.1),
            2.6,
        );
    }
    for entity in &crates {
        label(
            &mut commands,
            entity,
            "tdm",
            "Ammo".into(),
            Color::srgb(0.3, 0.9, 0.4),
            1.5,
        );
    }
}

fn place_labels(
    overlay: Res<Overlay>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    targets: Query<&GlobalTransform>,
    mut labels: Query<(Entity, &Label, &mut Node, &mut Visibility)>,
    mut commands: Commands,
) {
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    for (entity, label, mut node, mut visibility) in &mut labels {
        let Ok(target) = targets.get(label.target) else {
            commands.entity(entity).despawn();
            continue;
        };
        let point = target.translation() + Vec3::Y * label.height;
        match camera.world_to_viewport(camera_transform, point) {
            Ok(position)
                if overlay.labels && overlay.mode.is_none_or(|mode| mode == label.mode) =>
            {
                node.left = Val::Px(position.x);
                node.top = Val::Px(position.y);
                *visibility = Visibility::Inherited;
            }
            _ => *visibility = Visibility::Hidden,
        }
    }
}
