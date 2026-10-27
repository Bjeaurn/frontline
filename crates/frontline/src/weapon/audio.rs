//! Weapon sounds from the recordings in `assets/sounds/` (credits there): each Weapon's gunshot
//! (`shot_sound` in `data/weapons.ron`), the dry-fire click and the magazine on reloads, and the
//! odd bullet impact or ricochet where a shot ends.
//!
//! The local player's own sounds play flat (non-spatial). Other players' shots are spatial: they
//! come from the shooter's muzzle and fade with distance, heard from the eye camera. Every
//! gunshot gets a slight random pitch.

use std::collections::HashMap;

use bevy::asset::LoadState;
use bevy::audio::{AudioSource, SpatialListener, SpatialScale, Volume};
use bevy::prelude::*;

use super::{HeldWeapon, ShotFired, WeaponRng, WeaponSystems};
use crate::assets::EMBEDDED_ROOT;
use crate::data::{GameData, WeaponId};
use crate::net::protocol::ShotTracer;
use crate::player::EyeCamera;
use crate::shared::{FromServer, LocalPlayer};

const DRY_FIRE: &str = "sounds/dry_fire.ogg";
const MAG_INSERT: &str = "sounds/mag_insert.ogg";
const IMPACT: &str = "sounds/impact.ogg";
const RICOCHET: &str = "sounds/ricochet.ogg";

const LOCAL_SHOT_VOLUME: f32 = 0.8;
/// Remote shots start louder because the spatial falloff is steep (inverse square).
const REMOTE_SHOT_VOLUME: f32 = 1.8;
const CLICK_VOLUME: f32 = 0.5;
const MAG_VOLUME: f32 = 0.45;
const IMPACT_VOLUME: f32 = 0.9;
/// Gunshots play between `1 - PITCH_JITTER` and `1 + PITCH_JITTER` times their normal speed.
const PITCH_JITTER: f32 = 0.05;
/// Metres to sound distance: full volume within 1 / SPATIAL_SCALE metres, then inverse-square
/// (about -20 dB at 45 m).
const SPATIAL_SCALE: f32 = 0.07;
/// Distance between the listener's ears, in metres.
const EAR_GAP: f32 = 0.2;
/// How far into a reload the magazine goes in.
const MAG_INSERT_AT: f32 = 0.6;
/// Chance a shot that ends against the World is heard hitting it, and (further) glancing off.
const IMPACT_CHANCE: f32 = 0.25;
const RICOCHET_CHANCE: f32 = 0.08;
/// Shots ending this much short of the Weapon's range hit something.
const RANGE_MARGIN: f32 = 0.5;

pub(super) struct WeaponAudioPlugin;

impl Plugin for WeaponAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(add_listener)
            .add_systems(Startup, load_sounds)
            .add_systems(
                Update,
                (
                    play_local_shots,
                    play_remote_shots,
                    play_reload_sounds,
                    log_sounds_loaded,
                )
                    .after(WeaponSystems),
            );
    }
}

#[derive(Resource)]
struct WeaponSounds {
    shots: HashMap<WeaponId, Handle<AudioSource>>,
    dry_fire: Handle<AudioSource>,
    mag_insert: Handle<AudioSource>,
    impact: Handle<AudioSource>,
    ricochet: Handle<AudioSource>,
}

impl WeaponSounds {
    fn handles(&self) -> impl Iterator<Item = &Handle<AudioSource>> {
        self.shots.values().chain([
            &self.dry_fire,
            &self.mag_insert,
            &self.impact,
            &self.ricochet,
        ])
    }
}

fn load_sounds(mut commands: Commands, data: Res<GameData>, assets: Res<AssetServer>) {
    let effect = |file: &str| assets.load(format!("{EMBEDDED_ROOT}{file}"));
    commands.insert_resource(WeaponSounds {
        shots: data
            .weapons
            .iter()
            .map(|(id, stats)| (*id, assets.load(stats.shot_sound.clone())))
            .collect(),
        dry_fire: effect(DRY_FIRE),
        mag_insert: effect(MAG_INSERT),
        impact: effect(IMPACT),
        ricochet: effect(RICOCHET),
    });
}

/// Logs once when every weapon sound has loaded (failures are logged by the asset server).
fn log_sounds_loaded(
    sounds: Res<WeaponSounds>,
    assets: Res<AssetServer>,
    mut reported: Local<bool>,
) {
    if *reported {
        return;
    }
    let states: Vec<LoadState> = sounds
        .handles()
        .map(|handle| assets.load_state(handle))
        .collect();
    if states
        .iter()
        .all(|state| matches!(state, LoadState::Loaded))
    {
        info!("Weapon sounds loaded ({} files)", states.len());
        *reported = true;
    } else if states
        .iter()
        .any(|state| matches!(state, LoadState::Failed(_)))
    {
        *reported = true;
    }
}

/// The eye camera is the ears.
fn add_listener(add: On<Add<EyeCamera>>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert(SpatialListener::new(EAR_GAP));
}

/// Plays `sound` once; spatial at `at`, else flat. `speed` is the pitch.
fn play(
    commands: &mut Commands,
    sound: &Handle<AudioSource>,
    volume: f32,
    speed: f32,
    at: Option<Vec3>,
) {
    let settings = PlaybackSettings::DESPAWN
        .with_volume(Volume::Linear(volume))
        .with_speed(speed);
    match at {
        Some(position) => commands.spawn((
            Name::new("Sound"),
            AudioPlayer::new(sound.clone()),
            settings
                .with_spatial(true)
                .with_spatial_scale(SpatialScale::new(SPATIAL_SCALE)),
            Transform::from_translation(position),
        )),
        None => commands.spawn((
            Name::new("Sound"),
            AudioPlayer::new(sound.clone()),
            settings,
        )),
    };
}

fn pitch(rng: &mut WeaponRng) -> f32 {
    1.0 + (rng.next() * 2.0 - 1.0) * PITCH_JITTER
}

/// A shot that ended `end` against the World is now and then heard hitting it.
fn play_impact(commands: &mut Commands, sounds: &WeaponSounds, rng: &mut WeaponRng, end: Vec3) {
    let roll = rng.next();
    let sound = if roll < RICOCHET_CHANCE {
        &sounds.ricochet
    } else if roll < IMPACT_CHANCE {
        &sounds.impact
    } else {
        return;
    };
    play(
        commands,
        sound,
        IMPACT_VOLUME,
        0.9 + rng.next() * 0.2,
        Some(end),
    );
}

fn play_local_shots(
    mut fired: MessageReader<ShotFired>,
    sounds: Res<WeaponSounds>,
    data: Res<GameData>,
    mut rng: ResMut<WeaponRng>,
    mut commands: Commands,
) {
    for shot in fired.read() {
        let speed = pitch(&mut rng) * shot.pitch;
        play(
            &mut commands,
            &sounds.shots[&shot.weapon],
            LOCAL_SHOT_VOLUME * shot.volume,
            speed,
            None,
        );
        let range = data.weapon(shot.weapon).range;
        if shot.muzzle.distance(shot.end) < range - RANGE_MARGIN {
            play_impact(&mut commands, &sounds, &mut rng, shot.end);
        }
    }
}

fn play_remote_shots(
    mut tracers: MessageReader<FromServer<ShotTracer>>,
    sounds: Res<WeaponSounds>,
    data: Res<GameData>,
    mut rng: ResMut<WeaponRng>,
    mut commands: Commands,
) {
    for FromServer(tracer) in tracers.read() {
        let (volume, shot_pitch) = data.shot_sound(tracer.attachments);
        let speed = pitch(&mut rng) * shot_pitch;
        play(
            &mut commands,
            &sounds.shots[&tracer.weapon],
            REMOTE_SHOT_VOLUME * volume,
            speed,
            Some(tracer.origin),
        );
        let range = data.weapon(tracer.weapon).range;
        if tracer.origin.distance(tracer.end) < range - RANGE_MARGIN {
            play_impact(&mut commands, &sounds, &mut rng, tracer.end);
        }
    }
}

/// Where the local Weapon's reload stands, so each sound plays once per reload.
#[derive(Default)]
struct ReloadWatch {
    reloading: bool,
    mag_played: bool,
}

/// The click of a trigger pulled on an empty magazine as the reload starts, then the magazine
/// going in part-way through.
fn play_reload_sounds(
    player: Query<&HeldWeapon, With<LocalPlayer>>,
    sounds: Res<WeaponSounds>,
    mut watch: Local<ReloadWatch>,
    mut commands: Commands,
) {
    let Ok(weapon) = player.single() else {
        *watch = ReloadWatch::default();
        return;
    };
    let Some(progress) = weapon.reload_progress(&weapon.stats) else {
        *watch = ReloadWatch::default();
        return;
    };
    if !watch.reloading {
        watch.reloading = true;
        if weapon.rounds == 0 {
            play(&mut commands, &sounds.dry_fire, CLICK_VOLUME, 1.0, None);
        }
    }
    if progress >= MAG_INSERT_AT && !watch.mag_played {
        watch.mag_played = true;
        play(&mut commands, &sounds.mag_insert, MAG_VOLUME, 1.0, None);
    }
}
