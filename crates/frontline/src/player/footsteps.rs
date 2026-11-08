//! Footsteps: a step sound for every stride of a walking or running player. Remote players are
//! heard from where they are; you hear your own, quieter. Crouched and prone players are silent,
//! so are players with the Ninja Perk (`silent_footsteps`, to others) and you while airborne.
//!
//! A step falls due by the distance walked, not by a clock: a stride is `step_length` metres
//! (the walking one at walking speed, lengthening to the sprinting one), so steps come exactly as
//! fast as the player moves, none while standing or pushing against a wall, and the next follows
//! as soon as they set off again. Each is a random one of the eight concrete footstep sounds in
//! `assets/sounds/` (`tools/audio/footsteps.py` makes them) at a slightly random pitch.

use bevy::audio::AudioSource;
use bevy::prelude::*;

use super::controller::Motor;
use super::life::LocalLife;
use crate::assets::EMBEDDED_ROOT;
use crate::data::GameData;
use crate::gear::EquippedPerks;
use crate::rear_cue::RearCue;
use crate::shared::{Health, LocalPlayer, PlayerPose, RemotePlayer, Stance, TELEPORT_DISTANCE};
use crate::weapon::WeaponRng;

const FOOTSTEPS: [&str; 8] = [
    "sounds/footstep_concrete_000.ogg",
    "sounds/footstep_concrete_001.ogg",
    "sounds/footstep_concrete_002.ogg",
    "sounds/footstep_concrete_003.ogg",
    "sounds/footstep_concrete_004.ogg",
    "sounds/footstep_concrete_005.ogg",
    "sounds/footstep_concrete_006.ogg",
    "sounds/footstep_concrete_007.ogg",
];
/// Other players' steps: walking is a murmur, sprinting is loud.
const WALK_VOLUME: f32 = 0.3;
const SPRINT_VOLUME: f32 = 0.8;
/// Your own steps: there, but not louder than what you are listening for.
const OWN_WALK_VOLUME: f32 = 0.12;
const OWN_SPRINT_VOLUME: f32 = 0.3;
/// Footsteps play between `1 - PITCH_JITTER` and `1 + PITCH_JITTER` times their normal speed.
const PITCH_JITTER: f32 = 0.06;
/// Footsteps a second at walking speed, and at sprinting speed and above; with the speeds this
/// gives the stride lengths.
const WALK_RATE: f32 = 2.2;
const SPRINT_RATE: f32 = 3.0;
/// Metres a second below which a player stands still, as heard.
const MIN_SPEED: f32 = 0.5;
/// Seconds over which the speed, from the steps the position takes, follows the real one. It only
/// tells a walk from a sprint; when steps fall due is the distance.
const SPEED_SMOOTHING_SECS: f32 = 0.12;
/// Where a stride starts after standing still, so the first step follows soon after setting off.
const FIRST_STEP_PHASE: f32 = 0.6;

#[derive(Resource)]
pub(super) struct FootstepSounds(Vec<Handle<AudioSource>>);

/// How fast a player walks and how far into its next footstep it is.
#[derive(Component, Debug, Default)]
pub(super) struct Stride {
    last: Option<Vec3>,
    /// Smoothed horizontal speed in metres a second.
    speed: f32,
    /// `0..1` through the stride; a footstep falls due on reaching 1.
    phase: f32,
}

impl Stride {
    /// Follows the player to `position` after `dt` seconds. `audible` is false while they make no
    /// steps (crouched, dead, airborne). True when a footstep falls due now.
    fn advance(&mut self, position: Vec3, dt: f32, audible: bool, walk: f32, sprint: f32) -> bool {
        let moved = self
            .last
            .map_or(0.0, |last| (position - last).xz().length());
        let teleported = self
            .last
            .is_some_and(|last| position.distance(last) > TELEPORT_DISTANCE);
        self.last = Some(position);
        if teleported || !audible {
            self.speed = 0.0;
            self.phase = FIRST_STEP_PHASE;
            return false;
        }
        let follow = 1.0 - (-dt / SPEED_SMOOTHING_SECS).exp();
        self.speed += (moved / dt - self.speed) * follow;
        if self.speed < MIN_SPEED {
            self.phase = FIRST_STEP_PHASE;
            return false;
        }
        self.phase += moved / step_length(self.speed, walk, sprint);
        if self.phase < 1.0 {
            return false;
        }
        self.phase %= 1.0;
        true
    }
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

pub(super) fn attach_own_stride(add: On<Add<LocalPlayer>>, mut commands: Commands) {
    commands.entity(add.entity).insert(Stride::default());
}

/// How far into a sprint a player moving at horizontal `speed` is: 0 at `walk` and below, 1 at
/// `sprint` and above.
fn sprint_blend(speed: f32, walk: f32, sprint: f32) -> f32 {
    ((speed - walk) / (sprint - walk).max(f32::EPSILON)).clamp(0.0, 1.0)
}

/// Metres between two footsteps at horizontal `speed`: the walking stride (`walk / WALK_RATE`) at
/// `walk` and below, the sprinting one (`sprint / SPRINT_RATE`) at `sprint` and above.
fn step_length(speed: f32, walk: f32, sprint: f32) -> f32 {
    let walking = walk / WALK_RATE;
    let running = sprint / SPRINT_RATE;
    walking + (running - walking) * sprint_blend(speed, walk, sprint)
}

/// The volume of a step at `speed`: `quiet` walking, `loud` sprinting.
fn gait_volume(speed: f32, walk: f32, sprint: f32, quiet: f32, loud: f32) -> f32 {
    quiet + (loud - quiet) * sprint_blend(speed, walk, sprint)
}

/// Plays one random footstep.
fn play_step(
    commands: &mut Commands,
    cue: &mut RearCue,
    sounds: &FootstepSounds,
    rng: &mut WeaponRng,
    volume: f32,
    at: Vec3,
) {
    let pick = ((rng.next() * sounds.0.len() as f32) as usize).min(sounds.0.len() - 1);
    cue.play(
        commands,
        &sounds.0[pick],
        volume,
        1.0 + rng.signed() * PITCH_JITTER,
        at,
    );
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
    for (pose, health, perks, mut stride) in &mut players {
        let audible = !health.is_dead() && pose.stance == Stance::Standing;
        let (walk, sprint) = (data.movement.walk_speed, data.movement.sprint_speed);
        let due = stride.advance(pose.position, dt, audible, walk, sprint);
        if !due || perks.is_some_and(|perks| data.perk_effects(perks.0.iter()).silent_footsteps) {
            continue;
        }
        play_step(
            &mut commands,
            &mut cue,
            &sounds,
            &mut rng,
            gait_volume(stride.speed, walk, sprint, WALK_VOLUME, SPRINT_VOLUME),
            pose.position,
        );
    }
}

/// The same for the local player: your own steps, quieter, and none in the air.
pub(super) fn play_own_footsteps(
    time: Res<Time>,
    data: Res<GameData>,
    life: Res<LocalLife>,
    sounds: Res<FootstepSounds>,
    mut rng: ResMut<WeaponRng>,
    mut cue: RearCue,
    mut player: Query<(&PlayerPose, &Motor, &mut Stride), With<LocalPlayer>>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 || sounds.0.is_empty() {
        return;
    }
    let Ok((pose, motor, mut stride)) = player.single_mut() else {
        return;
    };
    let (walk, sprint) = (data.movement.walk_speed, data.movement.sprint_speed);
    let audible = !life.is_dead() && motor.grounded() && pose.stance == Stance::Standing;
    let due = stride.advance(pose.position, dt, audible, walk, sprint);
    if due {
        play_step(
            &mut commands,
            &mut cue,
            &sounds,
            &mut rng,
            gait_volume(
                stride.speed,
                walk,
                sprint,
                OWN_WALK_VOLUME,
                OWN_SPRINT_VOLUME,
            ),
            pose.position,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WALK: f32 = 5.0;
    const SPRINT: f32 = 8.0;
    const DT: f32 = 1.0 / 60.0;

    /// Walks `stride` along +x at `speed` for `secs` seconds from `from`, counting steps.
    fn walk(stride: &mut Stride, from: f32, speed: f32, secs: f32) -> (usize, f32) {
        let mut x = from;
        let mut steps = 0;
        for _ in 0..(secs / DT).round() as usize {
            x += speed * DT;
            steps += usize::from(stride.advance(Vec3::new(x, 0.0, 0.0), DT, true, WALK, SPRINT));
        }
        (steps, x)
    }

    #[test]
    fn stride_length_rises_from_walking_to_sprinting_and_stays_within_them() {
        let (walking, running) = (WALK / WALK_RATE, SPRINT / SPRINT_RATE);
        assert_eq!(step_length(WALK, WALK, SPRINT), walking);
        assert_eq!(step_length(SPRINT, WALK, SPRINT), running);
        let jog = step_length(6.5, WALK, SPRINT);
        assert!(jog > walking.min(running) && jog < walking.max(running));
        // Slower than a walk (strafing, stepping off) keeps the walking stride; faster than a
        // sprint (a Perk, a slope) the sprinting one.
        assert_eq!(step_length(2.0, WALK, SPRINT), walking);
        assert_eq!(step_length(12.0, WALK, SPRINT), running);
    }

    #[test]
    fn steps_follow_the_distance_walked_and_stop_when_the_player_does() {
        let mut stride = Stride::default();
        let (steps, x) = walk(&mut stride, 0.0, WALK, 4.0);
        // 20 m at one step per 5 / 2.2 m, the first one early (it starts 60% in).
        let expected = (x / (WALK / WALK_RATE) + FIRST_STEP_PHASE).floor() as usize;
        assert!(
            steps.abs_diff(expected) <= 1,
            "{steps} steps, expected {expected}"
        );
        // Standing still: no more steps, however long.
        let (steps, x) = walk(&mut stride, x, 0.0, 3.0);
        assert_eq!(steps, 0);
        // Setting off again: the first step comes within half a stride.
        let (steps, _) = walk(&mut stride, x, WALK, 0.4);
        assert!(steps >= 1);
    }

    #[test]
    fn nothing_is_heard_while_not_audible_and_a_teleport_does_not_count_as_walking() {
        let mut stride = Stride::default();
        for i in 0..120 {
            let at = Vec3::new(i as f32 * WALK * DT, 0.0, 0.0);
            assert!(!stride.advance(at, DT, false, WALK, SPRINT));
        }
        let mut stride = Stride::default();
        walk(&mut stride, 0.0, WALK, 0.2);
        assert!(!stride.advance(Vec3::new(200.0, 0.0, 0.0), DT, true, WALK, SPRINT));
    }

    #[test]
    fn walking_steps_are_quieter_than_sprinting_ones() {
        let at = |speed| gait_volume(speed, WALK, SPRINT, WALK_VOLUME, SPRINT_VOLUME);
        assert_eq!(at(WALK), WALK_VOLUME);
        assert_eq!(at(SPRINT), SPRINT_VOLUME);
        assert_eq!(at(2.0), WALK_VOLUME);
        assert_eq!(at(12.0), SPRINT_VOLUME);
        assert!(WALK_VOLUME < at(6.5) && at(6.5) < SPRINT_VOLUME);
    }
}
