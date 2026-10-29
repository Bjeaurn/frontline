//! Footsteps of remote players: a step sound for every stride of a walking or running player,
//! heard from where they are. Crouched and prone players are silent, and so are players with the
//! Ninja Perk (`silent_footsteps`). The steps come at about 2.2 a second walking, rising to
//! about 3 a second sprinting, by the player's replicated speed; each is a random one of the
//! concrete footstep recordings in `assets/sounds/` at a slightly random pitch.

use bevy::audio::AudioSource;
use bevy::prelude::*;

use crate::assets::EMBEDDED_ROOT;
use crate::data::GameData;
use crate::gear::EquippedPerks;
use crate::rear_cue::RearCue;
use crate::shared::{Health, PlayerPose, RemotePlayer, Stance, TELEPORT_DISTANCE};
use crate::weapon::WeaponRng;

const FOOTSTEPS: [&str; 5] = [
    "sounds/footstep_concrete_000.ogg",
    "sounds/footstep_concrete_001.ogg",
    "sounds/footstep_concrete_002.ogg",
    "sounds/footstep_concrete_003.ogg",
    "sounds/footstep_concrete_004.ogg",
];
const FOOTSTEP_VOLUME: f32 = 0.4;
/// Footsteps play between `1 - PITCH_JITTER` and `1 + PITCH_JITTER` times their normal speed.
const PITCH_JITTER: f32 = 0.05;
/// Footsteps a second at walking speed, and at sprinting speed and above.
const WALK_RATE: f32 = 2.2;
const SPRINT_RATE: f32 = 3.0;
/// Metres a second below which a player stands still, as heard.
const MIN_SPEED: f32 = 0.5;
/// Seconds over which a remote player's speed, from the steps its replicated position takes,
/// follows the real one.
const SPEED_SMOOTHING_SECS: f32 = 0.12;
/// Where a stride starts after standing still, so the first step follows soon after setting off.
const FIRST_STEP_PHASE: f32 = 0.6;

#[derive(Resource)]
pub(super) struct FootstepSounds(Vec<Handle<AudioSource>>);

/// How fast a remote player walks and how far into its next footstep it is.
#[derive(Component, Debug, Default)]
pub(super) struct Stride {
    last: Option<Vec3>,
    /// Smoothed horizontal speed in metres a second.
    speed: f32,
    /// `0..1` through the stride; a footstep plays on reaching 1.
    phase: f32,
}

pub(super) fn load_footstep_sounds(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(FootstepSounds(
        FOOTSTEPS
            .iter()
            .map(|file| assets.load(format!("{EMBEDDED_ROOT}{file}")))
            .collect(),
    ));
}

pub(super) fn attach_stride(add: On<Add<RemotePlayer>>, mut commands: Commands) {
    commands.entity(add.entity).insert(Stride::default());
}

/// Footsteps a second at horizontal `speed`: `WALK_RATE` at `walk`, rising to `SPRINT_RATE` at
/// `sprint`.
fn step_rate(speed: f32, walk: f32, sprint: f32) -> f32 {
    let sprinting = ((speed - walk) / (sprint - walk).max(f32::EPSILON)).clamp(0.0, 1.0);
    WALK_RATE + (SPRINT_RATE - WALK_RATE) * sprinting
}

/// Follows each remote player's walking and plays a step per stride.
pub(super) fn play_footsteps(
    time: Res<Time>,
    data: Res<GameData>,
    sounds: Res<FootstepSounds>,
    mut rng: ResMut<WeaponRng>,
    mut cue: RearCue,
    mut players: Query<
        (&PlayerPose, &Health, Option<&EquippedPerks>, &mut Stride),
        With<RemotePlayer>,
    >,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 || sounds.0.is_empty() {
        return;
    }
    let follow = 1.0 - (-dt / SPEED_SMOOTHING_SECS).exp();
    for (pose, health, perks, mut stride) in &mut players {
        let moved = stride
            .last
            .map_or(0.0, |last| (pose.position - last).xz().length());
        let teleported = stride
            .last
            .is_some_and(|last| pose.position.distance(last) > TELEPORT_DISTANCE);
        stride.last = Some(pose.position);
        if teleported || health.is_dead() || pose.stance != Stance::Standing {
            stride.speed = 0.0;
            stride.phase = FIRST_STEP_PHASE;
            continue;
        }
        stride.speed += (moved / dt - stride.speed) * follow;
        if stride.speed < MIN_SPEED {
            stride.phase = FIRST_STEP_PHASE;
            continue;
        }
        stride.phase += step_rate(
            stride.speed,
            data.movement.walk_speed,
            data.movement.sprint_speed,
        ) * dt;
        if stride.phase < 1.0 {
            continue;
        }
        stride.phase -= 1.0;
        if perks.is_some_and(|perks| data.perk_effects(perks.0.iter()).silent_footsteps) {
            continue;
        }
        let pick = ((rng.next() * sounds.0.len() as f32) as usize).min(sounds.0.len() - 1);
        cue.play(
            &mut commands,
            &sounds.0[pick],
            FOOTSTEP_VOLUME,
            1.0 + rng.signed() * PITCH_JITTER,
            pose.position,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_rises_from_walking_to_sprinting_and_stays_within_them() {
        let (walk, sprint) = (5.0, 8.0);
        assert_eq!(step_rate(walk, walk, sprint), WALK_RATE);
        assert_eq!(step_rate(sprint, walk, sprint), SPRINT_RATE);
        let jog = step_rate(6.5, walk, sprint);
        assert!(WALK_RATE < jog && jog < SPRINT_RATE);
        // Slower than a walk (strafing, stepping off) keeps the walking cadence; faster than a
        // sprint (a Perk, a slope) does not outrun the sprinting one.
        assert_eq!(step_rate(2.0, walk, sprint), WALK_RATE);
        assert_eq!(step_rate(12.0, walk, sprint), SPRINT_RATE);
    }
}
