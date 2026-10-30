//! Soldier visuals for `RemotePlayer`s, smoothed toward their replicated `PlayerPose`: the
//! Faction of their Team (`data/factions.ron`) decides what they wear, an armband and chest and
//! back panels show friendly or enemy relative to the local player's Team, and they are hidden
//! while dead or teamless. The stance and aim come from the pose, the walk cycle from how fast
//! the visual moves. A player aimed through a 3x or stronger sight shows a glint toward anyone it
//! points at.
//!
//! Respawns never glide: a pose jump over `TELEPORT_DISTANCE` snaps the visual, and a revived
//! player stays hidden until its (interpolated, so later-arriving) pose has reached the spawn.

use bevy::asset::{AssetPath, RenderAssetUsages};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use lightyear::prelude::ConfirmedHistory;

use super::controller::EyeCamera;
use super::life::LocalLife;
use super::soldier::{self, Gait, Rig, SoldierMeshes};
use crate::assets::EMBEDDED_ROOT;
use crate::data::{GameData, GunModel, STEADY_MAGNIFICATION};
use crate::maps::MapRegistry;
use crate::match_hud::is_friendly;
use crate::shared::{
    Health, MatchState, PlayerPose, PlayerTeam, RemotePlayer, Stance, TELEPORT_DISTANCE, Team,
};
use crate::weapon::rig::gun_rig;

/// Exponential decay rate (1/s) of the transform toward the pose; higher follows tighter.
const FOLLOW_RATE: f32 = 20.0;
/// The same for the soldier's limbs toward the stance, walk cycle and aim.
const LIMB_RATE: f32 = 24.0;
/// The same for the speed that drives the walk cycle, so jitter in the pose doesn't flail legs.
const SPEED_RATE: f32 = 10.0;
/// A revived player shows once its pose is this close to where the server respawned it.
const SPAWN_REACHED: f32 = 1.0;
const FRIENDLY_BODY: Color = Color::srgb(0.2, 0.42, 0.85);
const ENEMY_BODY: Color = Color::srgb(0.8, 0.2, 0.16);
/// The glint shows to a viewer within this angle of where the shooter aims, in degrees.
const GLINT_CONE_DEG: f32 = 25.0;
/// How far in front of the shooter's eye the scope sits, in metres.
const GLINT_FORWARD: f32 = 0.55;
/// Glint size: at least this wide, in metres, or this fraction of the distance to the viewer.
const GLINT_MIN_SIZE: f32 = 0.4;
const GLINT_ANGULAR_SIZE: f32 = 0.03;
/// Side of the generated glint texture, in pixels.
const GLINT_TEXTURE_PX: u32 = 64;

#[derive(Resource)]
pub(super) struct RemoteAssets {
    /// One soldier per Faction, indexed like `GameData::factions`.
    soldiers: Vec<SoldierMeshes>,
    /// White: the soldier meshes carry their colours in their vertices.
    body_material: Handle<StandardMaterial>,
    friendly_material: Handle<StandardMaterial>,
    enemy_material: Handle<StandardMaterial>,
    glint: Handle<Mesh>,
    glint_material: Handle<StandardMaterial>,
}

impl RemoteAssets {
    fn team_material(&self, friendly: bool) -> Handle<StandardMaterial> {
        if friendly {
            self.friendly_material.clone()
        } else {
            self.enemy_material.clone()
        }
    }

    pub(super) fn soldier(&self, faction: usize) -> &SoldierMeshes {
        &self.soldiers[faction.min(self.soldiers.len() - 1)]
    }
}

/// Index into `GameData::factions` of the Faction `team` plays as on the current Map (`state`,
/// absent before the Match state arrives).
pub(super) fn faction_index(
    data: &GameData,
    maps: &MapRegistry,
    state: Option<&MatchState>,
    team: Option<Team>,
) -> usize {
    let team = team.unwrap_or(Team::Alpha);
    let faction = match state {
        Some(state) => data.faction(maps, &state.map, team),
        None => &data.factions[team.index()],
    };
    data.factions
        .iter()
        .position(|candidate| candidate.id == faction.id)
        .unwrap_or(team.index())
}

/// Visual state attached to a remote player entity.
#[derive(Component, Debug)]
pub(super) struct RemoteVisuals {
    rig: Rig,
    /// Index of the Faction the soldier is dressed as.
    faction: usize,
    stance: Stance,
    eye_height: f32,
    /// Whether the team marks currently wear the friendly colour.
    friendly: bool,
    /// Alive and on a Team, per the latest replicated `Health` and `PlayerTeam`.
    in_play: bool,
    /// Where the server put the player on revival, until the pose has caught up with it.
    awaiting_spawn: Option<Vec3>,
    /// Whether the visuals were drawn last frame.
    shown: bool,
    /// The glint billboard at the scope.
    glint: Entity,
    /// Smoothed horizontal speed of the visual, in metres a second.
    speed: f32,
    gait: Gait,
}

/// A remote player just died where its visual stood; `corpse` leaves a body there.
#[derive(Message, Clone, Copy, Debug)]
pub(super) struct RemoteDeath {
    /// The player's visual transform (feet, yaw) at death.
    pub transform: Transform,
    pub stance: Stance,
    pub friendly: bool,
    /// Index of the Faction the body is dressed as.
    pub faction: usize,
}

fn pose_transform(pose: &PlayerPose) -> Transform {
    Transform::from_translation(pose.position).with_rotation(Quat::from_rotation_y(pose.yaw))
}

/// Team colour of a friendly or enemy player's armbands and panels.
pub(super) fn team_color(friendly: bool) -> Color {
    if friendly { FRIENDLY_BODY } else { ENEMY_BODY }
}

pub(super) fn team_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::from(color.to_linear() * 0.7),
        emissive: color.to_linear() * 0.12,
        perceptual_roughness: 0.9,
        reflectance: 0.0,
        ..default()
    }
}

/// The material of soldier meshes: their vertex colours show through unchanged.
pub(super) fn soldier_material() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        reflectance: 0.15,
        ..default()
    }
}

/// A bright star: a hot core, a soft halo and four rays, white on transparent for additive
/// blending.
fn glint_texture() -> Image {
    let size = GLINT_TEXTURE_PX;
    let data = (0..size * size)
        .flat_map(|i| {
            let at =
                (Vec2::new((i % size) as f32, (i / size) as f32) + 0.5) / size as f32 * 2.0 - 1.0;
            let core = (-at.length_squared() * 40.0).exp();
            let halo = (-at.length() * 4.0).exp() * 0.35;
            let ray = |along: f32, across: f32| (-across.abs() * 22.0 - along.abs() * 2.2).exp();
            let value = (core + halo + 0.6 * (ray(at.x, at.y) + ray(at.y, at.x))).clamp(0.0, 1.0);
            [255, 255, 255, (value * 255.0) as u8]
        })
        .collect();
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

pub(super) fn load_remote_assets(
    mut commands: Commands,
    data: Res<GameData>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    asset_server: Res<AssetServer>,
) {
    let gun = asset_server.load(
        AssetPath::from(format!("{EMBEDDED_ROOT}{}", gun_rig(GunModel::Rifle).file))
            .with_label("Scene0"),
    );
    commands.insert_resource(RemoteAssets {
        soldiers: data
            .factions
            .iter()
            .map(|faction| SoldierMeshes::new(&faction.look, &mut meshes, gun.clone()))
            .collect(),
        body_material: materials.add(soldier_material()),
        friendly_material: materials.add(team_material(FRIENDLY_BODY)),
        enemy_material: materials.add(team_material(ENEMY_BODY)),
        glint: meshes.add(Rectangle::from_size(Vec2::ONE)),
        glint_material: materials.add(StandardMaterial {
            base_color: Color::linear_rgb(5.0, 5.0, 4.4),
            base_color_texture: Some(images.add(glint_texture())),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            cull_mode: None,
            ..default()
        }),
    });
}

/// Starts hidden; `follow_remote_poses` shows it from the next frame if it is in play.
pub(super) fn attach_remote_visuals(
    add: On<Add<RemotePlayer>>,
    mut commands: Commands,
    assets: Res<RemoteAssets>,
    data: Res<GameData>,
    maps: Res<MapRegistry>,
    life: Res<LocalLife>,
    state: Query<&MatchState>,
    players: Query<(&PlayerPose, Option<&Health>, Option<&PlayerTeam>)>,
) {
    let entity = add.entity;
    let Ok((pose, health, team)) = players.get(entity) else {
        warn!("RemotePlayer {entity} has no PlayerPose; not drawing it");
        return;
    };
    let team = team.and_then(|team| team.0);
    let friendly = is_friendly(life.team(), team);
    let faction = faction_index(&data, &maps, state.single().ok(), team);
    let rig = assets.soldier(faction).spawn(
        &mut commands,
        entity,
        &assets.body_material,
        &assets.team_material(friendly),
        &soldier::pose(pose.stance, Gait::default(), pose.pitch),
    );
    let eye_height = pose.stance.eye_height();
    let glint = commands
        .spawn((
            Name::new("RemoteGlint"),
            Mesh3d(assets.glint.clone()),
            MeshMaterial3d(assets.glint_material.clone()),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            ChildOf(entity),
        ))
        .id();
    commands.entity(entity).insert((
        RemoteVisuals {
            rig,
            faction,
            stance: pose.stance,
            eye_height,
            friendly,
            in_play: health.is_some_and(|health| !health.is_dead()) && team.is_some(),
            awaiting_spawn: None,
            shown: false,
            glint,
            speed: 0.0,
            gait: Gait::default(),
        },
        pose_transform(pose),
        Visibility::Hidden,
    ));
}

/// Moves each remote player's visuals to its pose and decides whether they are drawn.
///
/// Shown players are smoothed toward the pose, except across a teleport, which snaps.
/// Hidden ones (dead, teamless, or revived but not yet at their spawn) snap every frame, so
/// they reappear exactly where they are. A revived player's spawn is the newest replicated pose:
/// the server moves it in the same tick that restores `Health`, but `Health` applies on
/// arrival while the pose only shows once interpolation reaches it. On a Host there is no
/// interpolation and the pose is already there. Dying sends a `RemoteDeath`. Also poses the
/// soldier (stance, walk cycle, aim), re-dresses it when its Team's Faction changes and recolours
/// its team marks when either side's Team changes.
pub(super) fn follow_remote_poses(
    time: Res<Time>,
    assets: Res<RemoteAssets>,
    data: Res<GameData>,
    maps: Res<MapRegistry>,
    life: Res<LocalLife>,
    state: Query<&MatchState>,
    mut players: Query<
        (
            &PlayerPose,
            &Health,
            Option<&PlayerTeam>,
            Option<&ConfirmedHistory<PlayerPose>>,
            &mut Transform,
            &mut Visibility,
            &mut RemoteVisuals,
        ),
        With<RemotePlayer>,
    >,
    mut parts: Query<
        (
            &mut Transform,
            &mut Mesh3d,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<RemotePlayer>,
    >,
    mut deaths: MessageWriter<RemoteDeath>,
) {
    let dt = time.delta_secs();
    let state = state.single().ok();
    for (pose, health, team, history, mut transform, mut visibility, mut visuals) in &mut players {
        let team = team.and_then(|team| team.0);
        let friendly = is_friendly(life.team(), team);
        let faction = faction_index(&data, &maps, state, team);
        let in_play = !health.is_dead() && team.is_some();
        if in_play != visuals.in_play {
            visuals.in_play = in_play;
            if in_play {
                let spawn = history
                    .and_then(ConfirmedHistory::newest_present)
                    .map_or(pose.position, |(_, newest)| newest.position);
                visuals.awaiting_spawn = Some(spawn);
            } else {
                visuals.awaiting_spawn = None;
                if health.is_dead() && visuals.shown {
                    deaths.write(RemoteDeath {
                        transform: *transform,
                        stance: visuals.stance,
                        friendly: visuals.friendly,
                        faction: visuals.faction,
                    });
                }
            }
        }
        if visuals
            .awaiting_spawn
            .is_some_and(|spawn| pose.position.distance(spawn) <= SPAWN_REACHED)
        {
            visuals.awaiting_spawn = None;
        }
        let shown = in_play && visuals.awaiting_spawn.is_none();

        let target = pose_transform(pose);
        let snap = !shown
            || !visuals.shown
            || transform.translation.distance(target.translation) > TELEPORT_DISTANCE;
        let before = transform.translation;
        if snap {
            *transform = target;
            visuals.eye_height = pose.stance.eye_height();
            visuals.speed = 0.0;
        } else {
            transform
                .translation
                .smooth_nudge(&target.translation, FOLLOW_RATE, dt);
            transform
                .rotation
                .smooth_nudge(&target.rotation, FOLLOW_RATE, dt);
            visuals
                .eye_height
                .smooth_nudge(&pose.stance.eye_height(), FOLLOW_RATE, dt);
            if dt > 0.0 {
                let moved = (transform.translation - before).xz().length() / dt;
                visuals.speed.smooth_nudge(&moved, SPEED_RATE, dt);
            }
        }
        visuals.shown = shown;
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        visuals.stance = pose.stance;

        let (walk, sprint) = match pose.stance {
            Stance::Standing => (data.movement.walk_speed, data.movement.sprint_speed),
            Stance::Crouching => (data.movement.crouch_speed, f32::MAX),
            Stance::Prone => (data.movement.prone_speed, f32::MAX),
        };
        let speed = visuals.speed;
        visuals.gait.advance(speed, walk, sprint, dt);
        let poses = soldier::pose(pose.stance, visuals.gait, pose.pitch);
        for (entity, target) in visuals.rig.parts.into_iter().zip(&poses) {
            let Ok((mut part, ..)) = parts.get_mut(entity) else {
                continue;
            };
            if snap {
                *part = *target;
            } else {
                part.translation
                    .smooth_nudge(&target.translation, LIMB_RATE, dt);
                part.rotation.smooth_nudge(&target.rotation, LIMB_RATE, dt);
            }
        }

        if visuals.faction != faction {
            visuals.faction = faction;
            let soldier = assets.soldier(faction);
            for (i, entity) in visuals.rig.parts.into_iter().enumerate() {
                if let Ok((_, mut mesh, _)) = parts.get_mut(entity) {
                    mesh.0 = soldier.part(i).clone();
                }
            }
            for (i, entity) in visuals.rig.marks.into_iter().enumerate() {
                if let Ok((_, mut mesh, _)) = parts.get_mut(entity) {
                    mesh.0 = soldier.mark(i).clone();
                }
            }
        }
        if visuals.friendly != friendly {
            visuals.friendly = friendly;
            for entity in visuals.rig.marks {
                if let Ok((_, _, mut material)) = parts.get_mut(entity) {
                    material.0 = assets.team_material(friendly);
                }
            }
        }
    }
}

/// Shows the glint of every remote player aimed through a strong sight (`aim_magnification`)
/// when the local player is within `GLINT_CONE_DEG` of where it points: a bright billboard at
/// the scope, brighter the closer to dead centre.
pub(super) fn update_glints(
    time: Res<Time>,
    eye: Query<&GlobalTransform, With<EyeCamera>>,
    remotes: Query<(&PlayerPose, &Transform, &RemoteVisuals), With<RemotePlayer>>,
    mut glints: Query<(&mut Transform, &mut Visibility), Without<RemotePlayer>>,
) {
    let viewer = eye.single().ok().map(GlobalTransform::translation);
    let flicker = 1.0 + 0.12 * (time.elapsed_secs() * 19.0).sin();
    for (pose, transform, visuals) in &remotes {
        let Ok((mut glint, mut visibility)) = glints.get_mut(visuals.glint) else {
            continue;
        };
        let shooter = transform.translation + Vec3::Y * visuals.eye_height;
        let aim = Quat::from_rotation_y(pose.yaw) * Quat::from_rotation_x(pose.pitch);
        let strength = viewer
            .filter(|_| visuals.shown && pose.aim_magnification >= STEADY_MAGNIFICATION)
            .map_or(0.0, |viewer| {
                glint_strength(shooter, aim * Vec3::NEG_Z, viewer)
            });
        if strength <= 0.0 {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        let viewer = viewer.unwrap_or_default();
        let scope = shooter + aim * Vec3::NEG_Z * GLINT_FORWARD;
        let size = GLINT_MIN_SIZE.max(scope.distance(viewer) * GLINT_ANGULAR_SIZE)
            * (0.35 + 0.65 * strength)
            * flicker;
        let facing = Transform::from_translation(scope).looking_at(viewer, Vec3::Y);
        *glint = Transform {
            translation: transform.rotation.inverse() * (scope - transform.translation),
            rotation: transform.rotation.inverse() * facing.rotation,
            scale: Vec3::splat(size),
        };
        visibility.set_if_neq(Visibility::Inherited);
    }
}

/// How strongly a viewer at `viewer` sees the glare of a shooter at `shooter` aiming along
/// `direction` (unit): 1 dead centre of the aim down to 0 at `GLINT_CONE_DEG` and beyond.
fn glint_strength(shooter: Vec3, direction: Vec3, viewer: Vec3) -> f32 {
    let Some(to_viewer) = (viewer - shooter).try_normalize() else {
        return 0.0;
    };
    let angle = direction
        .dot(to_viewer)
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees();
    (1.0 - angle / GLINT_CONE_DEG).clamp(0.0, 1.0)
}
