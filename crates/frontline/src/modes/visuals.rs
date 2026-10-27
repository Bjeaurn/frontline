//! Client side of the objective Game Modes (Client and Host): sends the use key (F), freezes the
//! player while planting or defusing, draws the ground rings, poles and billboarded markers over
//! Flags, the HQ and bomb sites, plays the explosion, and lets a dead Search & Destroy player
//! follow a living teammate.

use std::f32::consts::FRAC_PI_2;

use bevy::audio::{SpatialScale, Volume};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::view::{BombView, ModeView};
use crate::assets::EMBEDDED_ROOT;
use crate::data::GameData;
use crate::flow::AppState;
use crate::match_hud::{ENEMY_COLOR, FRIENDLY_COLOR};
use crate::net::protocol::{BombExploded, ObjectiveHold, PlayerOwner};
use crate::player::input::Bindings;
use crate::player::{ControllerModifiers, ControllerSystems, EyeCamera, LocalLife, MouseCaptured};
use crate::shared::{
    FromServer, Health, LocalPlayer, LocalPlayerReplica, PlayerName, PlayerPose, PlayerTeam,
    RemotePlayer, Team, ToServer,
};
use crate::ui::UiFonts;

const NEUTRAL_COLOR: Color = Color::srgb(0.85, 0.85, 0.82);
const SITE_COLOR: Color = Color::srgb(1.0, 0.62, 0.15);
const POLE_HEIGHT: f32 = 3.0;
const MARKER_HEIGHT: f32 = 3.5;
const MARKER_WIDTH: f32 = 140.0;
const MARKER_SIZE: f32 = 22.0;
const BLAST_SECS: f32 = 1.0;
const BLAST_RADIUS: f32 = 25.0;
const BLAST_SOUND: &str = "sounds/impact.ogg";
/// The dry-fire click pitched way up is the bomb's beep.
const BEEP_SOUND: &str = "sounds/dry_fire.ogg";
const BEEP_PITCH: f32 = 2.4;
const BEEP_VOLUME: f32 = 2.0;
/// Metres to sound distance, as for other players' gunshots.
const BEEP_SPATIAL_SCALE: f32 = 0.07;
/// The impact recording slowed right down reads as a deep boom.
const BLAST_PITCH: f32 = 0.3;

pub struct ModeVisualsPlugin;

impl Plugin for ModeVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Spectating>()
            .add_systems(Startup, load_assets)
            .add_systems(OnExit(AppState::InGame), reset)
            .add_systems(
                Update,
                (
                    (send_use_hold, freeze_while_acting)
                        .chain()
                        .before(ControllerSystems::Move),
                    (
                        sync_props,
                        sync_markers,
                        spawn_blasts,
                        beep_bomb,
                        animate_blasts,
                        spectate,
                    )
                        .after(ControllerSystems::View),
                )
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

/// The teammate a dead Search & Destroy player is watching.
#[derive(Resource, Default, Debug)]
pub struct Spectating {
    pub target: Option<Entity>,
}

fn reset(mut spectating: ResMut<Spectating>) {
    spectating.target = None;
}

#[derive(Resource)]
struct Assets3d {
    ring_mesh: Handle<Mesh>,
    pole_mesh: Handle<Mesh>,
    cloth_mesh: Handle<Mesh>,
    sphere: Handle<Mesh>,
    blast_sound: Handle<AudioSource>,
    beep_sound: Handle<AudioSource>,
}

fn load_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, assets: Res<AssetServer>) {
    commands.insert_resource(Assets3d {
        // Unit-radius ring, scaled to each zone.
        ring_mesh: meshes.add(Annulus::new(0.94, 1.0)),
        pole_mesh: meshes.add(Cylinder::new(0.05, POLE_HEIGHT)),
        cloth_mesh: meshes.add(Cuboid::new(0.9, 0.55, 0.03)),
        sphere: meshes.add(Sphere::new(1.0)),
        blast_sound: assets.load(format!("{EMBEDDED_ROOT}{BLAST_SOUND}")),
        beep_sound: assets.load(format!("{EMBEDDED_ROOT}{BEEP_SOUND}")),
    });
}

/// The use key's state, sent to the server on every change.
fn send_use_hold(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    mut sent: Local<bool>,
    mut out: MessageWriter<ToServer<ObjectiveHold>>,
) {
    let held = captured.active() && !life.is_dead() && bindings.use_objective.pressed(&keys);
    if held != *sent {
        *sent = held;
        out.write(ToServer(ObjectiveHold(held)));
    }
}

/// Plants and defuses happen standing still: movement is locked while the server says we're
/// at it.
fn freeze_while_acting(
    modes: Query<&ModeView>,
    me: Query<&PlayerOwner, With<LocalPlayerReplica>>,
    mut modifiers: Query<&mut ControllerModifiers, With<LocalPlayer>>,
) {
    let acting = match (modes.single(), me.single()) {
        (Ok(ModeView::SearchAndDestroy(sd)), Ok(owner)) => {
            sd.action.is_some_and(|action| action.peer == owner.0)
        }
        _ => false,
    };
    if let Ok(mut modifiers) = modifiers.single_mut()
        && modifiers.frozen != acting
    {
        modifiers.frozen = acting;
    }
}

/// One thing the Mode draws in the world: a Flag, the HQ, a bomb site or the bomb.
struct Wanted {
    id: u8,
    position: Vec3,
    label: String,
    color: Color,
    /// Ground ring and pole of this radius (metres); none for the bomb.
    zone_radius: Option<f32>,
}

fn owner_color(owner: Option<Team>, local: Option<Team>) -> Color {
    match owner {
        None => NEUTRAL_COLOR,
        Some(team) if Some(team) == local => FRIENDLY_COLOR,
        Some(_) => ENEMY_COLOR,
    }
}

fn wanted(view: &ModeView, data: &GameData, local: Option<Team>) -> Vec<Wanted> {
    match view {
        ModeView::None => Vec::new(),
        ModeView::Domination(flags) => {
            let radius = data.domination().capture.zone_radius;
            flags
                .iter()
                .enumerate()
                .map(|(index, flag)| Wanted {
                    id: index as u8,
                    position: flag.position,
                    label: flag.label.to_string(),
                    color: owner_color(flag.zone.owner, local),
                    zone_radius: Some(radius),
                })
                .collect()
        }
        ModeView::Headquarters(hq) => hq
            .position
            .map(|position| Wanted {
                id: 0,
                position,
                label: "HQ".to_owned(),
                color: owner_color(hq.zone.owner, local),
                zone_radius: Some(data.headquarters().capture.zone_radius),
            })
            .into_iter()
            .collect(),
        ModeView::SearchAndDestroy(sd) => {
            let radius = data.search_and_destroy().site_radius;
            let mut found: Vec<Wanted> = sd
                .sites
                .iter()
                .enumerate()
                .map(|(index, &(label, position))| Wanted {
                    id: index as u8,
                    position,
                    label: label.to_string(),
                    color: SITE_COLOR,
                    zone_radius: Some(radius),
                })
                .collect();
            // Only attackers see where the bomb is: defenders find a planted one by its beep,
            // and the site markers look the same before and after the plant.
            let attacking = local == Some(sd.attackers);
            match sd.bomb {
                BombView::Dropped(position) if attacking => found.push(Wanted {
                    id: 100,
                    position,
                    label: "BOMB".to_owned(),
                    color: ENEMY_COLOR,
                    zone_radius: None,
                }),
                BombView::Planted { position, .. } if attacking => found.push(Wanted {
                    id: 100,
                    position,
                    label: format!("BOMB {}", clock_text(sd.fuse_left)),
                    color: ENEMY_COLOR,
                    zone_radius: None,
                }),
                BombView::Gone
                | BombView::Carried
                | BombView::Dropped(_)
                | BombView::Planted { .. } => {}
            }
            found
        }
    }
}

fn clock_text(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// A drawn Flag, HQ or site: ring, pole and cloth.
#[derive(Component)]
struct Prop {
    id: u8,
    radius: f32,
    ring: Handle<StandardMaterial>,
    cloth: Handle<StandardMaterial>,
}

fn ring_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color.with_alpha(0.75),
        emissive: color.to_linear() * 1.5,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

fn sync_props(
    modes: Query<&ModeView>,
    data: Res<GameData>,
    life: Res<LocalLife>,
    assets: Res<Assets3d>,
    mut props: Query<(Entity, &Prop, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let wanted = modes
        .single()
        .map(|view| self::wanted(view, &data, life.team()))
        .unwrap_or_default();
    let mut present = Vec::new();
    for (entity, prop, mut transform) in &mut props {
        match wanted
            .iter()
            .find(|zone| zone.id == prop.id && zone.zone_radius == Some(prop.radius))
        {
            Some(zone) => {
                present.push(zone.id);
                for handle in [&prop.ring, &prop.cloth] {
                    if let Some(mut material) = materials.get_mut(handle) {
                        let color = zone.color.with_alpha(0.75);
                        if material.base_color != color {
                            *material = ring_material(zone.color);
                        }
                    }
                }
                if transform.translation != zone.position {
                    transform.translation = zone.position;
                }
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for zone in &wanted {
        let Some(radius) = zone.zone_radius else {
            continue;
        };
        if present.contains(&zone.id) {
            continue;
        }
        let ring = materials.add(ring_material(zone.color));
        let cloth = materials.add(ring_material(zone.color));
        let pole = materials.add(StandardMaterial {
            base_color: Color::srgb(0.25, 0.25, 0.25),
            ..default()
        });
        commands.spawn((
            Name::new(format!("Zone {}", zone.label)),
            DespawnOnExit(AppState::InGame),
            Prop {
                id: zone.id,
                radius,
                ring: ring.clone(),
                cloth: cloth.clone(),
            },
            Transform::from_translation(zone.position),
            Visibility::default(),
            children![
                (
                    Mesh3d(assets.ring_mesh.clone()),
                    MeshMaterial3d(ring),
                    Transform::from_xyz(0.0, 0.06, 0.0)
                        .with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
                        .with_scale(Vec3::splat(radius)),
                    NotShadowCaster,
                ),
                (
                    Mesh3d(assets.pole_mesh.clone()),
                    MeshMaterial3d(pole),
                    Transform::from_xyz(0.0, POLE_HEIGHT / 2.0, 0.0),
                ),
                (
                    Mesh3d(assets.cloth_mesh.clone()),
                    MeshMaterial3d(cloth),
                    Transform::from_xyz(0.47, POLE_HEIGHT - 0.35, 0.0),
                    NotShadowCaster,
                ),
            ],
        ));
    }
}

/// UI text that follows a world point on screen.
#[derive(Component)]
struct Marker {
    id: u8,
    position: Vec3,
}

fn sync_markers(
    modes: Query<&ModeView>,
    data: Res<GameData>,
    life: Res<LocalLife>,
    fonts: Res<UiFonts>,
    camera: Query<(&Camera, &GlobalTransform), With<EyeCamera>>,
    mut markers: Query<(
        Entity,
        &mut Marker,
        &mut Node,
        &mut Text,
        &mut TextColor,
        &mut Visibility,
    )>,
    mut commands: Commands,
) {
    let Ok((camera, eye)) = camera.single() else {
        return;
    };
    let wanted = modes
        .single()
        .map(|view| self::wanted(view, &data, life.team()))
        .unwrap_or_default();
    let mut present = Vec::new();
    for (entity, mut marker, mut node, mut text, mut color, mut visibility) in &mut markers {
        let Some(zone) = wanted.iter().find(|zone| zone.id == marker.id) else {
            commands.entity(entity).despawn();
            continue;
        };
        present.push(zone.id);
        marker.position = zone.position;
        let anchor = zone.position + Vec3::Y * MARKER_HEIGHT;
        let screen = camera.world_to_viewport(eye, anchor).ok();
        let Some(screen) = screen else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        node.left = px(screen.x - MARKER_WIDTH / 2.0);
        node.top = px(screen.y - MARKER_SIZE);
        let shown = format!(
            "{}  {:.0}m",
            zone.label,
            eye.translation().distance(zone.position)
        );
        if text.0 != shown {
            text.0 = shown;
        }
        color.set_if_neq(TextColor(zone.color));
    }
    for zone in &wanted {
        if present.contains(&zone.id) {
            continue;
        }
        commands.spawn((
            Name::new(format!("Marker {}", zone.label)),
            DespawnOnExit(AppState::InGame),
            Marker {
                id: zone.id,
                position: zone.position,
            },
            Node {
                position_type: PositionType::Absolute,
                width: px(MARKER_WIDTH),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Text::default(),
            TextFont {
                font: fonts.bold.clone().into(),
                font_size: FontSize::Px(MARKER_SIZE),
                ..default()
            },
            TextColor(zone.color),
            TextLayout::justify(Justify::Center),
            Visibility::Hidden,
        ));
    }
}

/// Seconds between beeps with `fuse_left` seconds to go: steady, then faster over the last
/// `beep_ramp_secs`.
fn beep_interval(fuse_left: f32, base: f32, fast: f32, ramp: f32) -> f32 {
    let urgency = (1.0 - fuse_left / ramp.max(f32::EPSILON)).clamp(0.0, 1.0);
    base + (fast - base) * urgency
}

/// The planted bomb beeps from where it sits, for everyone, faster as the fuse runs down.
fn beep_bomb(
    time: Res<Time<Real>>,
    modes: Query<&ModeView>,
    data: Res<GameData>,
    assets: Res<Assets3d>,
    mut next_beep: Local<f32>,
    mut commands: Commands,
) {
    let Ok(ModeView::SearchAndDestroy(sd)) = modes.single() else {
        *next_beep = 0.0;
        return;
    };
    let BombView::Planted { position, .. } = sd.bomb else {
        *next_beep = 0.0;
        return;
    };
    *next_beep -= time.delta_secs();
    if *next_beep > 0.0 {
        return;
    }
    let tuning = data.search_and_destroy();
    *next_beep = beep_interval(
        sd.fuse_left as f32,
        tuning.beep_secs,
        tuning.beep_fast_secs,
        tuning.beep_ramp_secs,
    );
    commands.spawn((
        Name::new("Bomb beep"),
        AudioPlayer::new(assets.beep_sound.clone()),
        PlaybackSettings::DESPAWN
            .with_volume(Volume::Linear(BEEP_VOLUME))
            .with_speed(BEEP_PITCH)
            .with_spatial(true)
            .with_spatial_scale(SpatialScale::new(BEEP_SPATIAL_SCALE)),
        Transform::from_translation(position),
    ));
}

#[derive(Component)]
struct Blast {
    age: f32,
    material: Handle<StandardMaterial>,
}

/// The white-out over the whole screen when the bomb goes off.
#[derive(Component)]
struct BlastFlash {
    age: f32,
}

fn spawn_blasts(
    mut explosions: MessageReader<FromServer<BombExploded>>,
    assets: Res<Assets3d>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    for FromServer(blast) in explosions.read() {
        info!("Bomb exploded at {}", blast.position);
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.75, 0.35, 1.0),
            emissive: LinearRgba::rgb(30.0, 15.0, 4.0),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            ..default()
        });
        commands.spawn((
            Name::new("Explosion"),
            DespawnOnExit(AppState::InGame),
            Blast {
                age: 0.0,
                material: material.clone(),
            },
            Mesh3d(assets.sphere.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(blast.position).with_scale(Vec3::splat(0.5)),
            NotShadowCaster,
        ));
        commands.spawn((
            Name::new("Explosion sound"),
            AudioPlayer::new(assets.blast_sound.clone()),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(3.0))
                .with_speed(BLAST_PITCH)
                .with_spatial(true)
                .with_spatial_scale(SpatialScale::new(0.02)),
            Transform::from_translation(blast.position),
        ));
        commands.spawn((
            Name::new("Explosion flash"),
            DespawnOnExit(AppState::InGame),
            BlastFlash { age: 0.0 },
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                ..default()
            },
            GlobalZIndex(20),
            BackgroundColor(Color::srgba(1.0, 0.95, 0.85, 0.9)),
        ));
    }
}

fn animate_blasts(
    time: Res<Time>,
    mut blasts: Query<(Entity, &mut Blast, &mut Transform)>,
    mut flashes: Query<(Entity, &mut BlastFlash, &mut BackgroundColor)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    for (entity, mut blast, mut transform) in &mut blasts {
        blast.age += time.delta_secs();
        let t = (blast.age / BLAST_SECS).min(1.0);
        if t >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        transform.scale = Vec3::splat(0.5 + (BLAST_RADIUS - 0.5) * (1.0 - (1.0 - t).powi(3)));
        if let Some(mut material) = materials.get_mut(&blast.material) {
            material.base_color = Color::srgba(1.0, 0.75, 0.35, 1.0 - t);
        }
    }
    for (entity, mut flash, mut color) in &mut flashes {
        flash.age += time.delta_secs();
        let t = flash.age / (BLAST_SECS * 0.8);
        if t >= 1.0 {
            commands.entity(entity).despawn();
        } else {
            color.0 = Color::srgba(1.0, 0.95, 0.85, 0.9 * (1.0 - t));
        }
    }
}

/// While dead in a live Search & Destroy round, the camera follows a living teammate; a click
/// switches to the next one.
fn spectate(
    modes: Query<&ModeView>,
    life: Res<LocalLife>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut spectating: ResMut<Spectating>,
    local: Query<&PlayerPose, With<LocalPlayer>>,
    mates: Query<(Entity, &PlayerPose, &Health, &PlayerTeam), With<RemotePlayer>>,
    mut camera: Query<&mut Transform, With<EyeCamera>>,
) {
    let wanted = life.is_dead()
        && matches!(
            modes.single(),
            Ok(ModeView::SearchAndDestroy(sd))
                if !matches!(sd.phase, super::view::SdPhaseView::Waiting)
        );
    let (Some(team), true) = (life.team(), wanted) else {
        spectating.target = None;
        return;
    };
    let mut alive: Vec<Entity> = mates
        .iter()
        .filter(|(_, _, health, mate)| !health.is_dead() && mate.0 == Some(team))
        .map(|(entity, ..)| entity)
        .collect();
    alive.sort();
    let current = spectating.target.filter(|target| alive.contains(target));
    let next = match current {
        Some(target) if mouse.just_pressed(MouseButton::Left) => {
            let at = alive.iter().position(|&e| e == target).unwrap_or(0);
            Some(alive[(at + 1) % alive.len()])
        }
        Some(target) => Some(target),
        None => alive.first().copied(),
    };
    spectating.target = next;
    let (Some(target), Ok(local), Ok(mut transform)) = (next, local.single(), camera.single_mut())
    else {
        return;
    };
    let Ok((_, pose, ..)) = mates.get(target) else {
        return;
    };
    let eye = pose.position + Vec3::Y * pose.stance.eye_height();
    let look = Quat::from_rotation_y(pose.yaw) * Quat::from_rotation_x(pose.pitch);
    let body = Quat::from_rotation_y(local.yaw);
    transform.translation = body.inverse() * (eye - local.position);
    transform.rotation = body.inverse() * look;
}

/// Name of the teammate being watched, for the HUD.
pub fn spectated_name(
    spectating: &Spectating,
    names: &Query<&PlayerName, With<RemotePlayer>>,
) -> Option<String> {
    spectating
        .target
        .and_then(|target| names.get(target).ok())
        .map(|name| name.0.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beeps_are_steady_then_speed_up_over_the_last_seconds() {
        assert_eq!(beep_interval(45.0, 1.0, 0.15, 10.0), 1.0);
        assert_eq!(beep_interval(10.0, 1.0, 0.15, 10.0), 1.0);
        assert!((beep_interval(5.0, 1.0, 0.15, 10.0) - 0.575).abs() < 1e-5);
        assert!((beep_interval(0.0, 1.0, 0.15, 10.0) - 0.15).abs() < 1e-5);
    }
}
