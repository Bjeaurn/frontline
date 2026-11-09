//! Equipment on the client: G throws the lethal and Q the tactical when the key is let go (a
//! `ThrowGrenade` to the server; the press only readies it), the carried count on the HUD,
//! Grenades drawn where the server says they are (a placed mine with a blinking light), and what
//! a detonation shows: a fireball and bang, a flashbang that whites out a viewer facing it and
//! deafens, a stun grenade that slows and shakes the view for seconds, a smoke cloud that greys
//! the view, a Tear Gas cloud that slows, coughs and narrows the view of whoever stands in it
//! (the client works that out from the clouds, as movement is the client's, docs/adr/0002).
//! The underslung launcher is a mode of the Primary (`weapon`).

use bevy::asset::AssetPath;
use bevy::audio::{AudioPlayer, AudioSource, GlobalVolume, PlaybackSettings, Volume};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::{
    Carried, Effect, Grenade, KeyState, PouchSlot, ReadiedThrow, ThrowFeed, cloud_exposure,
    cough_dip, flash_alpha, linger, slow_scale, stun_shake, throw_request, tunnel_inner_percent,
};
use crate::assets::EMBEDDED_ROOT;
use crate::atmosphere::ActiveLook;
use crate::data::GameData;
use crate::flow::{AppState, Settings};
use crate::net::protocol::{Flashed, GrenadeDetonated, Stunned, ThrowGrenade};
use crate::particles::{
    Detail, Fade, ParticleAssets, PuffSpec, Shape, random_unit, spawn_puff, spawn_spark,
};
use crate::player::input::{Action, Binding, Bindings};
use crate::player::{ControllerModifiers, EyeCamera, LocalLife, MouseCaptured};
use crate::rear_cue::RearCue;
use crate::shared::{FromServer, LocalPlayer, PlayerPose, ToServer};
use crate::ui::{AMBER, TEXT, UiFonts, hud_text};
use crate::video::ActiveVideo;
use crate::weapon::WeaponRng;
use crate::weapon::{EquipmentRows, HeldWeapon};

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
            .init_resource::<StatusState>()
            .init_resource::<ReadiedThrow>()
            .add_systems(Startup, load_assets)
            .add_observer(show_grenade)
            .add_systems(OnEnter(AppState::InGame), spawn_overlays)
            .add_systems(
                Update,
                (
                    throw,
                    follow_grenades,
                    blink_mine_lights,
                    on_detonations,
                    animate_bursts,
                    animate_clouds,
                    on_flashed,
                    on_stunned,
                    update_screen_effects,
                    update_status,
                    update_hud,
                )
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[derive(Resource)]
struct GrenadeAssets {
    boom: Handle<AudioSource>,
    bang: Handle<AudioSource>,
    ring: Handle<AudioSource>,
    /// The sphere and glowing red material of a mine's light.
    light_mesh: Handle<Mesh>,
    light_material: Handle<StandardMaterial>,
}

fn load_assets(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(GrenadeAssets {
        boom: assets.load(format!("{EMBEDDED_ROOT}{BURST_SOUND}")),
        bang: assets.load(format!("{EMBEDDED_ROOT}{BANG_SOUND}")),
        ring: assets.load(format!("{EMBEDDED_ROOT}{RING_SOUND}")),
        light_mesh: meshes.add(Sphere::new(MINE_LIGHT_RADIUS)),
        light_material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.1, 0.05),
            emissive: LinearRgba::rgb(12.0, 0.4, 0.2),
            unlit: true,
            ..default()
        }),
    });
}

/// G / Q: pressing readies the lethal / tactical Grenade, letting go throws it along the aim
/// (`ReadiedThrow`). Dying, opening a menu, switching Weapon or running out cancels it.
#[allow(clippy::too_many_arguments)]
fn throw(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    data: Res<GameData>,
    mut readied: ResMut<ReadiedThrow>,
    eye: Query<&EyeCamera>,
    mut player: Query<(&PlayerPose, &HeldWeapon, &mut Carried), With<LocalPlayer>>,
    mut throws: MessageWriter<ToServer<ThrowGrenade>>,
) {
    let Ok((pose, weapon, mut carried)) = player.single_mut() else {
        readied.set_if_neq(ReadiedThrow::default());
        return;
    };
    carried.tick(time.delta_secs());
    let key = |binding: &Binding| KeyState {
        pressed: binding.just_pressed(&keys),
        held: binding.pressed(&keys),
    };
    let feed = ThrowFeed {
        lethal: key(&bindings.lethal),
        tactical: key(&bindings.tactical),
        lethal_ready: carried.lethal.is_some_and(|pouch| pouch.can_take()),
        tactical_ready: carried.tactical.is_some_and(|pouch| pouch.can_take()),
        allowed: !life.is_dead() && captured.active() && weapon.can_throw(),
    };
    let Some(slot) = readied.step(&feed) else {
        return;
    };
    let Some(pouch) = carried.pouch_mut(slot) else {
        return;
    };
    let kind = pouch.kind;
    let tuning = data.grenade(kind);
    if !pouch.take(tuning) {
        return;
    }
    let eye_height = eye
        .single()
        .map_or(pose.stance.eye_height(), EyeCamera::height);
    let request = throw_request(
        kind,
        tuning,
        pose.yaw,
        pose.pitch,
        pose.position + Vec3::Y * eye_height,
    );
    info!(
        "Throwing {kind:?} at {:.1} m/s, pitch {:.0} deg",
        request.speed,
        pose.pitch.to_degrees()
    );
    throws.write(ToServer(request));
}

/// The little red light on a placed mine; lit while it blinks (`blink_mine_lights`).
#[derive(Component)]
struct MineLight;

/// Dresses a Grenade the server made (or replicated to this client) with its model, and a mine
/// with its light.
fn show_grenade(
    add: On<Add<Grenade>>,
    grenades: Query<&Grenade>,
    data: Res<GameData>,
    assets: Res<GrenadeAssets>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    let Ok(grenade) = grenades.get(add.entity) else {
        return;
    };
    let (file, scale, centre) = crate::weapon::rig::grenade_model(grenade.kind);
    let scene =
        asset_server.load(AssetPath::from(format!("{EMBEDDED_ROOT}{file}")).with_label("Scene0"));
    commands.entity(add.entity).insert((
        Transform::from_translation(grenade.position),
        Visibility::default(),
        children![(
            WorldAssetRoot(scene),
            // The model's middle on the Grenade's position.
            Transform::from_xyz(0.0, -centre * scale, 0.0).with_scale(Vec3::splat(scale)),
        )],
    ));
    if data.grenade(grenade.kind).fuse.is_mine() {
        commands.spawn((
            Name::new("Mine light"),
            MineLight,
            Mesh3d(assets.light_mesh.clone()),
            MeshMaterial3d(assets.light_material.clone()),
            Transform::from_xyz(0.0, 0.09, 0.0),
            Visibility::Hidden,
            NotShadowCaster,
            ChildOf(add.entity),
        ));
    }
}

/// Blinks the light of every armed mine; one still arming stays dark.
fn blink_mine_lights(
    time: Res<Time>,
    grenades: Query<&Grenade>,
    mut lights: Query<(&ChildOf, &mut Visibility), With<MineLight>>,
) {
    let lit = time.elapsed_secs().rem_euclid(MINE_BLINK_SECS) < MINE_BLINK_LIT_SECS;
    for (parent, mut visibility) in &mut lights {
        let armed = grenades
            .get(parent.parent())
            .is_ok_and(|grenade| grenade.armed);
        visibility.set_if_neq(if armed && lit {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Eases each Grenade toward its latest replicated position.
fn follow_grenades(time: Res<Time>, mut grenades: Query<(&Grenade, &mut Transform)>) {
    let blend = 1.0 - (-40.0 * time.delta_secs()).exp();
    for (grenade, mut transform) in &mut grenades {
        transform.translation = transform.translation.lerp(grenade.position, blend);
    }
}

/// The light a blast throws on its surroundings for its first moments.
#[derive(Component)]
struct Burst {
    age: f32,
    intensity: f32,
}

/// The volume of a smoke or gas cloud: smoke greys the view of anyone inside it, gas (with a
/// `slow`) slows and chokes them (`update_screen_effects`, `update_status`). Its puffs are the
/// particles `on_detonations` spawned.
#[derive(Component)]
struct Cloud {
    age: f32,
    secs: f32,
    radius: f32,
    /// The share of speed a Tear Gas cloud takes at full strength; `None` for Smoke.
    slow: Option<f32>,
}

/// Peak of a frag's light, lumens; a stun's is lower, it makes a thud, not a fireball.
const BLAST_LIGHT: f32 = 4_000_000.0;
const STUN_LIGHT: f32 = 1_500_000.0;
/// Linear colour of smoke puffs (about sRGB 0.66).
const SMOKE_PUFF: LinearRgba = LinearRgba::rgb(0.40, 0.41, 0.43);
/// Linear colour of Tear Gas puffs (a sickly yellow-green) and how opaque they are: you see
/// through them, unlike smoke.
const GAS_PUFF: LinearRgba = LinearRgba::rgb(0.42, 0.50, 0.08);
const GAS_PUFF_ALPHA: f32 = 0.5;
/// The dark olive of the tunnel around a choking player's view, and its opacity at the edge.
const GAS_TINT: Color = Color::srgb(0.30, 0.36, 0.05);
const GAS_EDGE_ALPHA: f32 = 0.95;
/// Gas builds up in the lungs this fast (per second, 1 is full) and wears off slower.
const GAS_RISE_PER_SEC: f32 = 2.5;
const GAS_FALL_PER_SEC: f32 = 0.45;
/// Opacity of the stun's haze at full strength; volume of the ringing it leaves.
const DAZE_ALPHA: f32 = 0.3;
const STUN_RING_VOLUME: f32 = 0.3;
/// Smoke billows out over this long, a bit slower than its opacity rises.
const SMOKE_GROW_SECS: f32 = super::SMOKE_FADE_IN_SECS * 1.6;
/// Smoke puffs push apart at this rate (fraction of speed lost per second).
const SMOKE_DRAG: f32 = 1.6;
/// Puffs in a smoke cloud at Medium detail (half that at Low, more at High).
const SMOKE_PUFFS: u32 = 34;
/// A mine's light blinks every this many seconds, lit for the first part of each.
const MINE_BLINK_SECS: f32 = 1.2;
const MINE_BLINK_LIT_SECS: f32 = 0.18;
/// Radius of the light sphere, metres.
const MINE_LIGHT_RADIUS: f32 = 0.025;

fn random_in_ball(rng: &mut WeaponRng) -> Vec3 {
    random_unit(rng) * rng.next().cbrt()
}

/// A detonation: fireball and bang, flash pop or smoke cloud, where it went off.
#[allow(clippy::too_many_arguments)]
fn on_detonations(
    mut detonations: MessageReader<FromServer<GrenadeDetonated>>,
    assets: Res<GrenadeAssets>,
    particles: Res<ParticleAssets>,
    look: Res<ActiveLook>,
    video: Res<ActiveVideo>,
    data: Res<GameData>,
    mut rng: ResMut<WeaponRng>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cue: RearCue,
    mut commands: Commands,
) {
    let detail = Detail::of(&video.0.quality);
    for FromServer(blast) in detonations.read() {
        let at = blast.position;
        match data.grenade(blast.kind).effect {
            Effect::Blast { .. } => {
                let radius = blast.radius * 0.4;
                spawn_fireball(
                    &mut commands,
                    &mut materials,
                    &particles,
                    &mut rng,
                    detail,
                    look.dust(),
                    at,
                    radius,
                );
                commands.spawn((
                    Name::new("Grenade burst"),
                    DespawnOnExit(AppState::InGame),
                    Burst {
                        age: 0.0,
                        intensity: BLAST_LIGHT,
                    },
                    Transform::from_translation(at + Vec3::Y * 0.5),
                    PointLight {
                        intensity: BLAST_LIGHT,
                        range: radius * 4.0,
                        color: Color::srgb(1.0, 0.7, 0.3),
                        ..default()
                    },
                ));
                cue.play(&mut commands, &assets.boom, 3.0, 0.45, at);
                cue.play(&mut commands, &assets.bang, 1.8, 0.5, at);
            }
            Effect::Flash { .. } => {
                spawn_flash_pop(&mut commands, &mut materials, &particles, &mut rng, at, 1.0);
                commands.spawn((
                    Name::new("Grenade burst"),
                    DespawnOnExit(AppState::InGame),
                    Burst {
                        age: 0.0,
                        intensity: BLAST_LIGHT,
                    },
                    Transform::from_translation(at + Vec3::Y * 0.3),
                    PointLight {
                        intensity: BLAST_LIGHT,
                        range: 6.4,
                        color: Color::WHITE,
                        ..default()
                    },
                ));
                cue.play(&mut commands, &assets.bang, 2.0, 1.9, at);
            }
            Effect::Stun { .. } => {
                // A heavy concussive thud: a bigger, duller pop than the flashbang's.
                spawn_flash_pop(&mut commands, &mut materials, &particles, &mut rng, at, 1.8);
                commands.spawn((
                    Name::new("Grenade burst"),
                    DespawnOnExit(AppState::InGame),
                    Burst {
                        age: 0.0,
                        intensity: STUN_LIGHT,
                    },
                    Transform::from_translation(at + Vec3::Y * 0.3),
                    PointLight {
                        intensity: STUN_LIGHT,
                        range: 6.4,
                        color: Color::srgb(0.85, 0.9, 1.0),
                        ..default()
                    },
                ));
                cue.play(&mut commands, &assets.boom, 3.0, 0.7, at);
                cue.play(&mut commands, &assets.bang, 1.4, 0.8, at);
            }
            Effect::Smoke { .. } => {
                spawn_smoke_puffs(
                    &mut commands,
                    &mut materials,
                    &particles,
                    &mut rng,
                    detail,
                    at,
                    blast.radius,
                    blast.secs,
                    SMOKE_PUFF,
                    1.0,
                );
                commands.spawn((
                    Name::new("Smoke cloud"),
                    DespawnOnExit(AppState::InGame),
                    Cloud {
                        age: 0.0,
                        secs: blast.secs,
                        radius: blast.radius,
                        slow: None,
                    },
                    Transform::from_translation(at),
                ));
                cue.play(&mut commands, &assets.boom, 0.7, 0.9, at);
            }
            Effect::Gas { slow, .. } => {
                spawn_smoke_puffs(
                    &mut commands,
                    &mut materials,
                    &particles,
                    &mut rng,
                    detail,
                    at,
                    blast.radius,
                    blast.secs,
                    GAS_PUFF,
                    GAS_PUFF_ALPHA,
                );
                commands.spawn((
                    Name::new("Gas cloud"),
                    DespawnOnExit(AppState::InGame),
                    Cloud {
                        age: 0.0,
                        secs: blast.secs,
                        radius: blast.radius,
                        slow: Some(slow),
                    },
                    Transform::from_translation(at),
                ));
                // A canister's pop and a long hiss (the smoke's boom, pitched up).
                cue.play(&mut commands, &assets.boom, 0.6, 1.5, at);
            }
        }
    }
}

/// A frag's explosion: a white-hot flash, an orange fireball, a shock ring, a dust ring along the
/// ground, rising black smoke and sparks.
#[allow(clippy::too_many_arguments)]
fn spawn_fireball(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    particles: &ParticleAssets,
    rng: &mut WeaponRng,
    detail: Detail,
    dust: LinearRgba,
    at: Vec3,
    radius: f32,
) {
    let mut puff = |commands: &mut Commands, spec: PuffSpec| {
        spawn_puff(commands, materials, particles, spec);
    };
    puff(
        commands,
        PuffSpec {
            at: at + Vec3::Y * 0.4,
            start_size: radius * 0.8,
            end_size: radius * 2.2,
            grow_secs: 0.14,
            life: 0.14,
            shape: Shape::Star,
            color: LinearRgba::rgb(9.0, 6.5, 3.0),
            alpha: 1.0,
            additive: true,
            drag: 0.0,
            fade: Fade::Out { in_secs: 0.01 },
            ..default()
        },
    );
    puff(
        commands,
        PuffSpec {
            at: at + Vec3::Y * 0.4,
            start_size: 0.4,
            end_size: radius * 2.2,
            grow_secs: 0.28,
            life: 0.28,
            shape: Shape::Ring,
            color: LinearRgba::rgb(1.4, 1.2, 1.0),
            alpha: 0.3,
            additive: true,
            drag: 0.0,
            fade: Fade::Out { in_secs: 0.02 },
            ..default()
        },
    );
    // The fire itself: orange blended puffs (additive ones wash out to white), a hotter core
    // inside them.
    for _ in 0..detail.count(5) {
        let offset = random_in_ball(rng) * radius * 0.3;
        puff(
            commands,
            PuffSpec {
                at: at + Vec3::Y * 0.5 + offset,
                velocity: (offset.normalize_or_zero() + Vec3::Y * 0.7) * 2.5,
                drag: 3.5,
                start_size: radius * 0.45,
                end_size: radius * (1.0 + rng.next() * 0.7),
                grow_secs: 0.3,
                life: 0.4 + rng.next() * 0.2,
                color: LinearRgba::rgb(1.0, 0.33, 0.04),
                alpha: 0.9,
                spin: rng.signed(),
                angle: rng.next() * std::f32::consts::TAU,
                ..default()
            },
        );
    }
    for _ in 0..detail.count(3) {
        let offset = random_in_ball(rng) * radius * 0.2;
        puff(
            commands,
            PuffSpec {
                at: at + Vec3::Y * 0.5 + offset,
                velocity: Vec3::Y * 1.5,
                drag: 3.0,
                start_size: radius * 0.35,
                end_size: radius * 0.8,
                grow_secs: 0.25,
                life: 0.3,
                color: LinearRgba::rgb(2.6, 1.1, 0.2),
                alpha: 0.8,
                additive: true,
                angle: rng.next() * std::f32::consts::TAU,
                ..default()
            },
        );
    }
    for _ in 0..detail.count(5) {
        let drift = random_in_ball(rng) * 1.6 + Vec3::Y * (1.6 + rng.next() * 1.8);
        puff(
            commands,
            PuffSpec {
                at: at + Vec3::Y * 0.5 + random_in_ball(rng) * radius * 0.25,
                velocity: drift,
                drag: 1.3,
                gravity: -0.3,
                start_size: radius * 0.4,
                end_size: radius * (1.2 + rng.next() * 0.8),
                grow_secs: 1.8,
                life: 2.6 + rng.next() * 1.6,
                color: LinearRgba::rgb(0.05, 0.048, 0.045),
                alpha: 0.62,
                fade: Fade::Out { in_secs: 0.25 },
                spin: rng.signed() * 0.4,
                angle: rng.next() * std::f32::consts::TAU,
                ..default()
            },
        );
    }
    for i in 0..detail.count(6) {
        let angle = i as f32 / detail.count(6) as f32 * std::f32::consts::TAU + rng.next();
        let outward = Vec3::new(angle.cos(), 0.0, angle.sin());
        puff(
            commands,
            PuffSpec {
                at: at + outward * 0.3 + Vec3::Y * 0.25,
                velocity: outward * (radius * 2.2) + Vec3::Y * 0.6,
                drag: 2.6,
                start_size: radius * 0.3,
                end_size: radius * 1.1,
                grow_secs: 0.6,
                life: 0.9 + rng.next() * 0.4,
                color: dust,
                alpha: 0.5,
                angle: rng.next() * std::f32::consts::TAU,
                ..default()
            },
        );
    }
    if detail > Detail::Low {
        for _ in 0..detail.count(12) {
            let mut direction = random_unit(rng);
            direction.y = direction.y.abs() * 0.8 + 0.2;
            spawn_spark(
                commands,
                particles,
                at + Vec3::Y * 0.3,
                direction * (6.0 + rng.next() * 9.0),
                0.5 + rng.next() * 0.5,
            );
        }
    }
}

/// A flashbang's pop: a blinding white star, a ring and a few soft white puffs, `scale` times
/// the size (1 for a flashbang, more for the heavier stun grenade).
fn spawn_flash_pop(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    particles: &ParticleAssets,
    rng: &mut WeaponRng,
    at: Vec3,
    scale: f32,
) {
    let mut puff = |commands: &mut Commands, spec: PuffSpec| {
        spawn_puff(commands, materials, particles, spec);
    };
    puff(
        commands,
        PuffSpec {
            at: at + Vec3::Y * 0.3,
            start_size: 1.6 * scale,
            end_size: 4.5 * scale,
            grow_secs: 0.25,
            life: 0.25,
            shape: Shape::Star,
            color: LinearRgba::rgb(14.0, 14.0, 14.0),
            alpha: 1.0,
            additive: true,
            drag: 0.0,
            fade: Fade::Out { in_secs: 0.01 },
            ..default()
        },
    );
    puff(
        commands,
        PuffSpec {
            at: at + Vec3::Y * 0.3,
            start_size: 0.4 * scale,
            end_size: 5.5 * scale,
            grow_secs: 0.35,
            life: 0.35,
            shape: Shape::Ring,
            color: LinearRgba::rgb(2.0, 2.0, 2.0),
            alpha: 0.6,
            additive: true,
            drag: 0.0,
            fade: Fade::Out { in_secs: 0.02 },
            ..default()
        },
    );
    for _ in 0..3 {
        puff(
            commands,
            PuffSpec {
                at: at + Vec3::Y * 0.3,
                velocity: random_unit(rng) * 1.5,
                drag: 3.0,
                start_size: 0.8 * scale,
                end_size: 2.4 * scale,
                grow_secs: 0.4,
                life: 0.5,
                color: LinearRgba::rgb(3.0, 3.0, 3.0),
                alpha: 0.7,
                additive: true,
                angle: rng.next() * std::f32::consts::TAU,
                ..default()
            },
        );
    }
}

/// A cloud of puffs (Smoke or Tear Gas): puffs of `tint` that billow out to fill a ball of
/// `radius`, hold for `secs` (as `smoke_density` says) and thin out again. Smoke's, at `alpha`
/// 1, are so many, wide and opaque in the middle that you cannot see a target through the
/// cloud; the puffs rest on the ground and reach about a puff's height up, over a standing
/// head.
#[allow(clippy::too_many_arguments)]
fn spawn_smoke_puffs(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    particles: &ParticleAssets,
    rng: &mut WeaponRng,
    detail: Detail,
    at: Vec3,
    radius: f32,
    secs: f32,
    tint: LinearRgba,
    alpha: f32,
) {
    for _ in 0..detail.count(SMOKE_PUFFS) {
        let end_size = radius * (1.0 + rng.next() * 0.5);
        let mut target = random_in_ball(rng) * radius * 0.6;
        // A puff is a flat quad: rest it on the ground (half its size up) so it doesn't cut a
        // hard line through the floor.
        target.y = end_size * (0.5 + rng.next() * 0.25);
        let shade = 0.85 + rng.next() * 0.3;
        spawn_puff(
            commands,
            materials,
            particles,
            PuffSpec {
                at: at + Vec3::Y * 0.3,
                velocity: target * SMOKE_DRAG,
                drag: SMOKE_DRAG,
                start_size: radius * 0.25,
                end_size,
                grow_secs: SMOKE_GROW_SECS,
                life: secs,
                shape: Shape::Dense,
                color: LinearRgba::rgb(tint.red * shade, tint.green * shade, tint.blue * shade),
                alpha,
                fade: Fade::Hold {
                    in_secs: super::SMOKE_FADE_IN_SECS,
                    out_secs: super::SMOKE_FADE_OUT_SECS.min(secs * 0.5),
                },
                spin: rng.signed() * 0.12,
                angle: rng.next() * std::f32::consts::TAU,
                ..default()
            },
        );
    }
}

fn animate_bursts(
    time: Res<Time>,
    mut bursts: Query<(Entity, &mut Burst, &mut PointLight)>,
    mut commands: Commands,
) {
    for (entity, mut burst, mut light) in &mut bursts {
        burst.age += time.delta_secs();
        let t = (burst.age / BURST_SECS).min(1.0);
        if t >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        light.intensity = burst.intensity * (1.0 - t);
    }
}

fn animate_clouds(
    time: Res<Time>,
    mut clouds: Query<(Entity, &mut Cloud)>,
    mut commands: Commands,
) {
    for (entity, mut cloud) in &mut clouds {
        cloud.age += time.delta_secs();
        if cloud.age >= cloud.secs {
            commands.entity(entity).despawn();
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

/// The tunnel of a Tear Gas cloud around the view.
#[derive(Component)]
struct GasOverlay;

/// The haze of a stun over the view.
#[derive(Component)]
struct DazeOverlay;

/// The screen-wide flash and smoke tints. They sit above every menu (`GlobalZIndex` 10), and a UI
/// node blocks the pointer from everything below it unless it is `Pickable::IGNORE`: without it
/// the menus in a game (pause, loadout, class editor) got no hover or clicks at all.
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
        Pickable::IGNORE,
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.0)),
    ));
    commands.spawn((
        Name::new("Smoke overlay"),
        DespawnOnExit(AppState::InGame),
        SmokeOverlay,
        full.clone(),
        GlobalZIndex(17),
        Pickable::IGNORE,
        BackgroundColor(Color::srgba(0.66, 0.67, 0.69, 0.0)),
    ));
    commands.spawn((
        Name::new("Gas overlay"),
        DespawnOnExit(AppState::InGame),
        GasOverlay,
        full.clone(),
        GlobalZIndex(16),
        Pickable::IGNORE,
        tunnel(0.0),
        Visibility::Hidden,
    ));
    commands.spawn((
        Name::new("Stun overlay"),
        DespawnOnExit(AppState::InGame),
        DazeOverlay,
        full,
        GlobalZIndex(15),
        Pickable::IGNORE,
        BackgroundColor(Color::srgba(0.86, 0.88, 0.92, 0.0)),
    ));
    // The equipment HUD lives in the ammo cluster (`weapon::EquipmentRows`, rebuilt by `update_hud`).
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

    // Only Smoke greys the view; Tear Gas is felt (`update_status`).
    let grey = eye.single().map_or(0.0, |eye| {
        clouds
            .iter()
            .filter(|(cloud, _)| cloud.slow.is_none())
            .map(|(cloud, transform)| {
                cloud_exposure(
                    eye.translation(),
                    transform.translation,
                    cloud.radius,
                    cloud.age,
                    cloud.secs,
                )
            })
            .fold(0.0, f32::max)
    });
    smoke.0 = Color::srgba(0.66, 0.67, 0.69, 0.97 * grey);
}

/// What a stun and a Tear Gas cloud still do to the local player.
#[derive(Resource, Default)]
struct StatusState {
    /// Seconds of stun left, the seconds it began with, how hard it shakes the view and the
    /// share it takes off the speed at the start.
    stun_left: f32,
    stun_total: f32,
    stun_strength: f32,
    stun_slow: f32,
    /// How choked the player is (0 to 1), the share of speed the thickest cloud around them
    /// takes at full strength, and the seconds spent choking (the cough clock).
    gas: f32,
    gas_slow: f32,
    gas_clock: f32,
}

fn on_stunned(
    mut stuns: MessageReader<FromServer<Stunned>>,
    mut state: ResMut<StatusState>,
    assets: Res<GrenadeAssets>,
    mut commands: Commands,
) {
    for FromServer(stunned) in stuns.read() {
        if stunned.secs > state.stun_left {
            state.stun_left = stunned.secs;
            state.stun_total = stunned.secs;
            state.stun_strength = stunned.strength;
            state.stun_slow = stunned.slow;
            commands.spawn((
                Name::new("Concussion"),
                AudioPlayer::new(assets.ring.clone()),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(STUN_RING_VOLUME)),
            ));
        }
        info!(
            "Stunned: {:.1} s at {:.2}, slow {:.2}",
            stunned.secs, stunned.strength, stunned.slow
        );
    }
}

/// The clear middle of the view shrinking to a tunnel as `level` (0 to 1) choked.
fn tunnel(level: f32) -> BackgroundGradient {
    BackgroundGradient::from(RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::FarthestCorner,
        vec![
            ColorStop::percent(GAS_TINT.with_alpha(0.0), tunnel_inner_percent(level)),
            ColorStop::percent(GAS_TINT.with_alpha(GAS_EDGE_ALPHA * level), 100.0),
        ],
    ))
}

/// Runs the stun down and the gas up and down, and hands the controller what they do: the
/// speed they take off, and the view shaken by the stun and the coughs. Whites the view over
/// while stunned (a haze, there is no blur) and narrows it to a tunnel while choking.
#[allow(clippy::too_many_arguments)]
fn update_status(
    time: Res<Time>,
    life: Res<LocalLife>,
    mut state: ResMut<StatusState>,
    mut applied: Local<f32>,
    eye: Query<&GlobalTransform, With<EyeCamera>>,
    clouds: Query<(&Cloud, &Transform)>,
    mut modifiers: Query<&mut ControllerModifiers, With<LocalPlayer>>,
    mut daze: Single<&mut BackgroundColor, (With<DazeOverlay>, Without<GasOverlay>)>,
    mut gas: Single<(&mut BackgroundGradient, &mut Visibility), With<GasOverlay>>,
) {
    if life.is_dead() {
        *state = StatusState::default();
    }
    let dt = time.delta_secs();
    state.stun_left = (state.stun_left - dt).max(0.0);
    let stun = flash_alpha(state.stun_left, state.stun_total);

    // The thickest cloud the eye is in sets how much gas there is to breathe.
    let (exposure, slow) = eye.single().map_or((0.0, 0.0), |eye| {
        clouds
            .iter()
            .filter_map(|(cloud, transform)| {
                cloud.slow.map(|slow| {
                    let exposure = cloud_exposure(
                        eye.translation(),
                        transform.translation,
                        cloud.radius,
                        cloud.age,
                        cloud.secs,
                    );
                    (exposure, slow)
                })
            })
            .fold(
                (0.0, 0.0),
                |best, now| if now.0 > best.0 { now } else { best },
            )
    });
    if exposure > 0.0 {
        state.gas_slow = slow;
    }
    state.gas = linger(state.gas, exposure, GAS_RISE_PER_SEC, GAS_FALL_PER_SEC, dt);
    if state.gas > 0.0 {
        state.gas_clock += dt;
    } else {
        state.gas_clock = 0.0;
    }

    if let Ok(mut modifiers) = modifiers.single_mut() {
        modifiers.status_move =
            slow_scale(state.stun_slow, stun) * slow_scale(state.gas_slow, state.gas);
        modifiers.view_shake = stun_shake(time.elapsed_secs(), stun * state.stun_strength)
            + Vec3::X * cough_dip(state.gas_clock, state.gas);
    }
    let sway = 0.85 + 0.15 * (time.elapsed_secs() * 4.0).sin();
    daze.0 = Color::srgba(
        0.86,
        0.88,
        0.92,
        DAZE_ALPHA * stun * state.stun_strength * sway,
    );
    let (gradient, visibility) = &mut *gas;
    if (state.gas - *applied).abs() > 0.004 {
        *applied = state.gas;
        **gradient = tunnel(state.gas);
    }
    visibility.set_if_neq(if state.gas > 0.004 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

/// The carried equipment as rows above the ammo: a key plate, the name and the count, e.g.
/// `[G] FRAG GRENADE  x1`. The Grenade being held ready, or the launcher while it is the mode
/// the Primary is in, shows its plate lit.
fn update_hud(
    data: Res<GameData>,
    bindings: Res<Bindings>,
    fonts: Res<UiFonts>,
    readied: Res<ReadiedThrow>,
    player: Query<(&Carried, &HeldWeapon), With<LocalPlayer>>,
    rows: Single<(Entity, Option<&Children>), With<EquipmentRows>>,
    mut shown: Local<Vec<(String, String, u32, bool)>>,
    mut commands: Commands,
) {
    let Ok((carried, weapon)) = player.single() else {
        return;
    };
    let entry = |action: Action, pouch: Option<super::Pouch>, lit: bool| {
        pouch.map(|pouch| {
            (
                bindings.describe(action).to_uppercase(),
                data.grenade(pouch.kind).name.to_uppercase(),
                pouch.left,
                lit,
            )
        })
    };
    let now: Vec<(String, String, u32, bool)> = [
        entry(
            Action::Lethal,
            carried.lethal,
            readied.0 == Some(PouchSlot::Lethal),
        ),
        entry(
            Action::Tactical,
            carried.tactical,
            readied.0 == Some(PouchSlot::Tactical),
        ),
        entry(Action::Launcher, carried.launcher, weapon.launcher_mode()),
    ]
    .into_iter()
    .flatten()
    .collect();
    // `shown` outlives a game; a fresh (childless) column must be filled again.
    if *shown == now && rows.1.is_some() == !now.is_empty() {
        return;
    }
    let (root, children) = *rows;
    for child in children.into_iter().flatten() {
        commands.entity(*child).despawn();
    }
    for (key, name, left, lit) in &now {
        let (plate, key_color, name_color) = if *lit {
            (AMBER, Color::BLACK, AMBER)
        } else {
            (Color::srgba(0.0, 0.0, 0.0, 0.4), AMBER, TEXT)
        };
        commands.spawn((
            ChildOf(root),
            Node {
                align_items: AlignItems::Center,
                column_gap: px(8.0),
                ..default()
            },
            children![
                (
                    Node {
                        min_width: px(22.0),
                        padding: UiRect::axes(px(5.0), px(0.0)),
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1.0)),
                        ..default()
                    },
                    BorderColor::all(AMBER),
                    BackgroundColor(plate),
                    children![hud_text(&fonts.bold, key, 14.0, key_color)],
                ),
                hud_text(&fonts.bold, name, 17.0, name_color),
                hud_text(&fonts.display, &format!("{left}"), 20.0, Color::WHITE),
            ],
        ));
    }
    *shown = now;
}
