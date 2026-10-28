//! Footsteps of remote players: a quiet thud for every stride of a walking or running player,
//! heard from where they are. Crouched and prone players are silent, and so are players with the
//! Ninja Perk (`silent_footsteps`). There is no footstep recording yet, so the concrete impact
//! from `assets/sounds/` plays at a low pitch.

use bevy::audio::{AudioSource, PlaybackSettings, SpatialScale, Volume};
use bevy::prelude::*;

use crate::assets::EMBEDDED_ROOT;
use crate::data::GameData;
use crate::gear::EquippedPerks;
use crate::shared::{Health, PlayerPose, RemotePlayer, Stance, TELEPORT_DISTANCE};

const FOOTSTEP: &str = "sounds/impact.ogg";
const FOOTSTEP_VOLUME: f32 = 0.4;
/// The two feet play at slightly different pitches.
const FOOT_PITCH: [f32; 2] = [0.55, 0.62];
/// Metres walked between two footsteps.
const STEP_LENGTH: f32 = 0.85;
/// Metres to sound distance, as for gunshots (`weapon::audio`): footsteps carry a fair way less
/// because they start quieter.
const SPATIAL_SCALE: f32 = 0.07;

#[derive(Resource)]
pub(super) struct FootstepSound(Handle<AudioSource>);

/// How far a remote player has walked since its last footstep.
#[derive(Component, Debug, Default)]
pub(super) struct Stride {
    last: Option<Vec3>,
    walked: f32,
    /// Which foot steps next.
    right: bool,
}

pub(super) fn load_footstep_sound(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(FootstepSound(
        assets.load(format!("{EMBEDDED_ROOT}{FOOTSTEP}")),
    ));
}

pub(super) fn attach_stride(add: On<Add<RemotePlayer>>, mut commands: Commands) {
    commands.entity(add.entity).insert(Stride::default());
}

/// Counts each remote player's walking and plays a step per `STEP_LENGTH`.
pub(super) fn play_footsteps(
    data: Res<GameData>,
    sound: Res<FootstepSound>,
    mut players: Query<
        (&PlayerPose, &Health, Option<&EquippedPerks>, &mut Stride),
        With<RemotePlayer>,
    >,
    mut commands: Commands,
) {
    for (pose, health, perks, mut stride) in &mut players {
        let moved = stride
            .last
            .map_or(0.0, |last| (pose.position - last).xz().length());
        let teleported = stride
            .last
            .is_some_and(|last| pose.position.distance(last) > TELEPORT_DISTANCE);
        stride.last = Some(pose.position);
        if teleported || health.is_dead() || pose.stance != Stance::Standing {
            stride.walked = 0.0;
            continue;
        }
        stride.walked += moved;
        if stride.walked < STEP_LENGTH {
            continue;
        }
        stride.walked -= STEP_LENGTH;
        stride.right = !stride.right;
        if perks.is_some_and(|perks| data.perk_effects(perks.0.iter()).silent_footsteps) {
            continue;
        }
        commands.spawn((
            Name::new("Footstep"),
            AudioPlayer::new(sound.0.clone()),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(FOOTSTEP_VOLUME))
                .with_speed(FOOT_PITCH[usize::from(stride.right)])
                .with_spatial(true)
                .with_spatial_scale(SpatialScale::new(SPATIAL_SCALE)),
            Transform::from_translation(pose.position),
        ));
    }
}
