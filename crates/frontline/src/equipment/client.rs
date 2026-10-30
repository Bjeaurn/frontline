//! Equipment on the client: G throws the lethal, Q the tactical and V fires the underslung
//! launcher (each a `ThrowGrenade` to the server), the carried count on the HUD, Grenades drawn
//! where the server says they are, and what a detonation shows: a fireball and bang, a flashbang
//! that whites out a viewer facing it and deafens, a smoke cloud that greys the view.

use bevy::audio::{AudioPlayer, AudioSource, GlobalVolume, PlaybackSettings, Volume};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::{
    Carried, Effect, Grenade, GrenadeKind, PouchSlot, depth_in_cloud, flash_alpha, smoke_density,
};
use crate::assets::EMBEDDED_ROOT;
use crate::data::GameData;
use crate::flow::{AppState, Settings};
use crate::net::protocol::{Flashed, GrenadeDetonated, ThrowGrenade};
use crate::player::input::{Action, Bindings};
use crate::player::{EyeCamera, LocalLife, MouseCaptured};
use crate::rear_cue::RearCue;
use crate::shared::{FromServer, LocalPlayer, PlayerPose, ToServer};
use crate::weapon::HeldWeapon;

/// A thrown Grenade leaves the hand a little above the aim, so the throw lobs.
const LOB_PITCH: f32 = 0.14;
/// How far in front of the eye a Grenade starts, metres.
const THROW_REACH: f32 = 0.45;
const BURST_SECS: f32 = 0.55;
const BURST_SOUND: &str = "sounds/impact.ogg";
const BANG_SOUND: &str = "sounds/shot_sniper.ogg";
const RING_SOUND: &str = "sounds/ring.ogg";
/// Master volume left while deafened at full strength.
const DEAF_VOLUME: f32 = 0.12;

pub struct EquipmentClientPlugin;

impl Plugin for EquipmentClientPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlashState>()
            .add_systems(Startup, load_assets)
            .add_observer(show_grenade)
            .add_systems(OnEnter(AppState::InGame), spawn_overlays)
            .add_systems(
                Update,
                (
                    throw,
                    follow_grenades,
                    on_detonations,
                    animate_bursts,
                    animate_clouds,
                    on_flashed,
                    update_screen_effects,
                    update_hud,
                )
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[derive(Resource)]
struct GrenadeAssets {
    sphere: Handle<Mesh>,
    bodies: [Handle<StandardMaterial>; 4],
    boom: Handle<AudioSource>,
    bang: Handle<AudioSource>,
    ring: Handle<AudioSource>,
}

fn kind_index(kind: GrenadeKind) -> usize {
    GrenadeKind::ALL
        .iter()
        .position(|k| *k == kind)
        .unwrap_or(0)
}

fn load_assets(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut body = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.6,
            ..default()
        })
    };
    commands.insert_resource(GrenadeAssets {
        sphere: meshes.add(Sphere::new(1.0).mesh().ico(3).unwrap()),
        bodies: [
            body(Color::srgb(0.25, 0.32, 0.15)),
            body(Color::srgb(0.85, 0.85, 0.8)),
            body(Color::srgb(0.5, 0.55, 0.58)),
            body(Color::srgb(0.7, 0.55, 0.15)),
        ],
        boom: assets.load(format!("{EMBEDDED_ROOT}{BURST_SOUND}")),
        bang: assets.load(format!("{EMBEDDED_ROOT}{BANG_SOUND}")),
        ring: assets.load(format!("{EMBEDDED_ROOT}{RING_SOUND}")),
    });
}

/// G / Q / V: throws or launches a Grenade along the aim.
#[allow(clippy::too_many_arguments)]
fn throw(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    data: Res<GameData>,
    eye: Query<&EyeCamera>,
    mut player: Query<(&PlayerPose, &HeldWeapon, &mut Carried), With<LocalPlayer>>,
    mut throws: MessageWriter<ToServer<ThrowGrenade>>,
) {
    let Ok((pose, weapon, mut carried)) = player.single_mut() else {
        return;
    };
    carried.tick(time.delta_secs());
    if life.is_dead() || !captured.active() || !weapon.can_throw() {
        return;
    }
    let wanted = [
        (PouchSlot::Lethal, &bindings.lethal, LOB_PITCH),
        (PouchSlot::Tactical, &bindings.tactical, LOB_PITCH),
        (PouchSlot::Launcher, &bindings.launcher, 0.0),
    ];
    for (slot, binding, lob) in wanted {
        if !binding.just_pressed(&keys) || (slot == PouchSlot::Launcher && !weapon.is_primary()) {
            continue;
        }
        let Some(pouch) = carried.pouch_mut(slot) else {
            continue;
        };
        let kind = pouch.kind;
        if !pouch.take(data.grenade(kind)) {
            continue;
        }
        let aim = Quat::from_rotation_y(pose.yaw) * Quat::from_rotation_x(pose.pitch + lob);
        let direction = aim * Vec3::NEG_Z;
        let eye_height = eye
            .single()
            .map_or(pose.stance.eye_height(), EyeCamera::height);
        let origin = pose.position + Vec3::Y * eye_height + direction * THROW_REACH;
        info!("Throwing {kind:?}");
        throws.write(ToServer(ThrowGrenade {
            kind,
            origin,
            direction,
        }));
    }
}

/// Dresses a Grenade the server made (or replicated to this client).
fn show_grenade(
    add: On<Add<Grenade>>,
    grenades: Query<&Grenade>,
    assets: Res<GrenadeAssets>,
    mut commands: Commands,
) {
    let Ok(grenade) = grenades.get(add.entity) else {
        return;
    };
    let size = if grenade.kind == GrenadeKind::Launcher {
        0.08
    } else {
        0.07
    };
    commands.entity(add.entity).insert((
        Mesh3d(assets.sphere.clone()),
        MeshMaterial3d(assets.bodies[kind_index(grenade.kind)].clone()),
        Transform::from_translation(grenade.position).with_scale(Vec3::splat(size)),
        Visibility::default(),
    ));
}

/// Eases each Grenade toward its latest replicated position.
fn follow_grenades(time: Res<Time>, mut grenades: Query<(&Grenade, &mut Transform)>) {
    let blend = 1.0 - (-40.0 * time.delta_secs()).exp();
    for (grenade, mut transform) in &mut grenades {
        transform.translation = transform.translation.lerp(grenade.position, blend);
    }
}

#[derive(Component)]
struct Burst {
    age: f32,
    radius: f32,
    material: Handle<StandardMaterial>,
    color: (f32, f32, f32),
}

#[derive(Component)]
struct Cloud {
    age: f32,
    secs: f32,
    radius: f32,
    material: Handle<StandardMaterial>,
}

/// A detonation: fireball and bang, flash pop or smoke cloud, where it went off.
fn on_detonations(
    mut detonations: MessageReader<FromServer<GrenadeDetonated>>,
    assets: Res<GrenadeAssets>,
    data: Res<GameData>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cue: RearCue,
    mut commands: Commands,
) {
    for FromServer(blast) in detonations.read() {
        let at = blast.position;
        match data.grenade(blast.kind).effect {
            Effect::Blast { .. } | Effect::Flash { .. } => {
                let flash = matches!(data.grenade(blast.kind).effect, Effect::Flash { .. });
                let color = if flash {
                    (1.0, 1.0, 1.0)
                } else {
                    (1.0, 0.7, 0.3)
                };
                let radius = if flash { 1.6 } else { blast.radius * 0.4 };
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgba(color.0, color.1, color.2, 1.0),
                    emissive: LinearRgba::rgb(30.0 * color.0, 18.0 * color.1, 6.0 * color.2),
                    unlit: true,
                    alpha_mode: AlphaMode::Add,
                    ..default()
                });
                commands.spawn((
                    Name::new("Grenade burst"),
                    DespawnOnExit(AppState::InGame),
                    Burst {
                        age: 0.0,
                        radius,
                        material: material.clone(),
                        color,
                    },
                    Mesh3d(assets.sphere.clone()),
                    MeshMaterial3d(material),
                    Transform::from_translation(at).with_scale(Vec3::splat(0.3)),
                    NotShadowCaster,
                    PointLight {
                        intensity: 4_000_000.0,
                        range: radius * 4.0,
                        color: Color::srgb(color.0, color.1, color.2),
                        ..default()
                    },
                ));
                if flash {
                    cue.play(&mut commands, &assets.bang, 2.0, 1.9, at);
                } else {
                    cue.play(&mut commands, &assets.boom, 3.0, 0.45, at);
                    cue.play(&mut commands, &assets.bang, 1.8, 0.5, at);
                }
            }
            Effect::Smoke { .. } => {
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgba(0.66, 0.67, 0.69, 0.0),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                });
                commands.spawn((
                    Name::new("Smoke cloud"),
                    DespawnOnExit(AppState::InGame),
                    Cloud {
                        age: 0.0,
                        secs: blast.secs,
                        radius: blast.radius,
                        material: material.clone(),
                    },
                    Mesh3d(assets.sphere.clone()),
                    MeshMaterial3d(material),
                    Transform::from_translation(at).with_scale(Vec3::splat(0.1)),
                    NotShadowCaster,
                ));
                cue.play(&mut commands, &assets.boom, 0.7, 0.9, at);
            }
        }
    }
}

fn animate_bursts(
    time: Res<Time>,
    mut bursts: Query<(Entity, &mut Burst, &mut Transform, &mut PointLight)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    for (entity, mut burst, mut transform, mut light) in &mut bursts {
        burst.age += time.delta_secs();
        let t = (burst.age / BURST_SECS).min(1.0);
        if t >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        transform.scale = Vec3::splat(0.3 + (burst.radius - 0.3) * (1.0 - (1.0 - t).powi(3)));
        light.intensity = 4_000_000.0 * (1.0 - t);
        if let Some(mut material) = materials.get_mut(&burst.material) {
            let (r, g, b) = burst.color;
            material.base_color = Color::srgba(r, g, b, 1.0 - t);
        }
    }
}

fn animate_clouds(
    time: Res<Time>,
    mut clouds: Query<(Entity, &mut Cloud, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    for (entity, mut cloud, mut transform) in &mut clouds {
        cloud.age += time.delta_secs();
        if cloud.age >= cloud.secs {
            commands.entity(entity).despawn();
            continue;
        }
        let billow = (cloud.age / super::SMOKE_FADE_IN_SECS).clamp(0.0, 1.0);
        transform.scale = Vec3::splat(cloud.radius * (0.25 + 0.75 * billow.sqrt()));
        if let Some(mut material) = materials.get_mut(&cloud.material) {
            material.base_color = Color::srgba(
                0.66,
                0.67,
                0.69,
                0.97 * smoke_density(cloud.age, cloud.secs),
            );
        }
    }
}

/// What is left of the blinding and the deafness of the latest flashbangs.
#[derive(Resource, Default)]
struct FlashState {
    blind_left: f32,
    blind_total: f32,
    deaf_left: f32,
    deaf_total: f32,
}

fn on_flashed(
    mut flashes: MessageReader<FromServer<Flashed>>,
    mut state: ResMut<FlashState>,
    assets: Res<GrenadeAssets>,
    mut commands: Commands,
) {
    for FromServer(flashed) in flashes.read() {
        if flashed.blind_secs > state.blind_left {
            state.blind_left = flashed.blind_secs;
            state.blind_total = flashed.blind_secs;
        }
        if flashed.deaf_secs > state.deaf_left {
            state.deaf_left = flashed.deaf_secs;
            state.deaf_total = flashed.deaf_secs;
            commands.spawn((
                Name::new("Ringing"),
                AudioPlayer::new(assets.ring.clone()),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.5)),
            ));
        }
        info!(
            "Flashed: blind {:.1} s, deaf {:.1} s",
            flashed.blind_secs, flashed.deaf_secs
        );
    }
}

#[derive(Component)]
struct FlashOverlay;

#[derive(Component)]
struct SmokeOverlay;

#[derive(Component)]
struct EquipmentHud;

fn spawn_overlays(mut commands: Commands) {
    let full = Node {
        position_type: PositionType::Absolute,
        width: percent(100.0),
        height: percent(100.0),
        ..default()
    };
    commands.spawn((
        Name::new("Flash overlay"),
        DespawnOnExit(AppState::InGame),
        FlashOverlay,
        full.clone(),
        GlobalZIndex(18),
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.0)),
    ));
    commands.spawn((
        Name::new("Smoke overlay"),
        DespawnOnExit(AppState::InGame),
        SmokeOverlay,
        full,
        GlobalZIndex(17),
        BackgroundColor(Color::srgba(0.66, 0.67, 0.69, 0.0)),
    ));
    commands.spawn((
        Name::new("Equipment"),
        DespawnOnExit(AppState::InGame),
        EquipmentHud,
        Text::default(),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
        Node {
            position_type: PositionType::Absolute,
            right: px(24.0),
            bottom: px(84.0),
            ..default()
        },
    ));
}

/// Runs the flash down, whites the screen out and ducks the sound while it lasts, and greys the
/// view as the eye sits in a smoke cloud.
#[allow(clippy::too_many_arguments)]
fn update_screen_effects(
    time: Res<Time>,
    life: Res<LocalLife>,
    settings: Res<Settings>,
    mut state: ResMut<FlashState>,
    mut volume: ResMut<GlobalVolume>,
    eye: Query<&GlobalTransform, With<EyeCamera>>,
    clouds: Query<(&Cloud, &Transform)>,
    mut flash: Single<&mut BackgroundColor, (With<FlashOverlay>, Without<SmokeOverlay>)>,
    mut smoke: Single<&mut BackgroundColor, (With<SmokeOverlay>, Without<FlashOverlay>)>,
) {
    if life.is_dead() {
        *state = FlashState::default();
    }
    let dt = time.delta_secs();
    state.blind_left = (state.blind_left - dt).max(0.0);
    state.deaf_left = (state.deaf_left - dt).max(0.0);
    flash.0 = Color::srgba(
        1.0,
        1.0,
        1.0,
        flash_alpha(state.blind_left, state.blind_total),
    );

    let deaf = flash_alpha(state.deaf_left, state.deaf_total);
    let master = settings.master_volume * (1.0 - deaf * (1.0 - DEAF_VOLUME));
    if (volume.volume != Volume::Linear(master)) && (deaf > 0.0 || state.deaf_total > 0.0) {
        volume.volume = Volume::Linear(master);
        if state.deaf_left <= 0.0 {
            state.deaf_total = 0.0;
        }
    }

    let grey = eye.single().map_or(0.0, |eye| {
        clouds
            .iter()
            .map(|(cloud, transform)| {
                let depth = depth_in_cloud(eye.translation(), transform.translation, cloud.radius);
                (depth * 3.0).min(1.0) * smoke_density(cloud.age, cloud.secs)
            })
            .fold(0.0, f32::max)
    });
    smoke.0 = Color::srgba(0.66, 0.67, 0.69, 0.97 * grey);
}

/// "[G] FRAG GRENADE x1   [Q] FLASHBANG x2   [V] 40MM GRENADE x3".
fn update_hud(
    data: Res<GameData>,
    bindings: Res<Bindings>,
    player: Query<&Carried, With<LocalPlayer>>,
    mut hud: Single<&mut Text, With<EquipmentHud>>,
) {
    let Ok(carried) = player.single() else {
        return;
    };
    let entry = |action: Action, pouch: Option<super::Pouch>| {
        pouch.map(|pouch| {
            format!(
                "[{}] {} x{}",
                bindings.describe(action).to_uppercase(),
                data.grenade(pouch.kind).name.to_uppercase(),
                pouch.left
            )
        })
    };
    let shown = [
        entry(Action::Lethal, carried.lethal),
        entry(Action::Tactical, carried.tactical),
        entry(Action::Launcher, carried.launcher),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("   ");
    if hud.0 != shown {
        hud.0 = shown;
    }
}
