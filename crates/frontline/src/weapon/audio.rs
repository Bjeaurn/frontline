//! Weapon sounds from the recordings in `assets/sounds/` (credits there): each Weapon's gunshot
//! (`shot_sound` in `data/weapons.ron`), the dry-fire click of a trigger pulled on an empty
//! magazine, the higher click of a fire-mode switch and the magazine on reloads, and the odd
//! bullet impact or ricochet where a shot ends.
//!
//! The local player's own sounds play flat (non-spatial). Other players' shots are spatial: they
//! come from the shooter's muzzle and fade with distance, heard from the eye camera (`rear_cue`,
//! which also muffles sounds from behind). Every gunshot gets a slight random pitch.

use std::collections::HashMap;

use bevy::asset::LoadState;
use bevy::audio::{AudioSource, Volume};
use bevy::prelude::*;
use lightyear::prelude::LocalId;
use lightyear::prelude::client::{Client, Connected};

use super::{DryFire, HeldWeapon, ModeSwitched, ShotFired, WeaponRng, WeaponSystems};
use crate::assets::EMBEDDED_ROOT;
use crate::data::{GameData, WeaponId};
use crate::net::protocol::{ProjectileFired, ProjectileLanded, ShotTracer};
use crate::rear_cue::RearCue;
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
/// The fire-mode switch is the dry-fire click, higher and a little quieter.
const MODE_CLICK_SPEED: f32 = 1.7;
const MODE_CLICK_VOLUME: f32 = 0.4;
const IMPACT_VOLUME: f32 = 0.9;
/// Gunshots play between `1 - PITCH_JITTER` and `1 + PITCH_JITTER` times their normal speed.
const PITCH_JITTER: f32 = 0.05;
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
        app.add_systems(Startup, load_sounds).add_systems(
            Update,
            (
                play_local_shots,
                play_remote_shots,
                play_projectile_sounds,
                play_reload_sounds,
                play_trigger_clicks,
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

/// Plays `sound` once, flat (not from a point in the World). `speed` is the pitch.
fn play_flat(commands: &mut Commands, sound: &Handle<AudioSource>, volume: f32, speed: f32) {
    commands.spawn((
        Name::new("Sound"),
        AudioPlayer::new(sound.clone()),
        PlaybackSettings::DESPAWN
            .with_volume(Volume::Linear(volume))
            .with_speed(speed),
    ));
}

fn pitch(rng: &mut WeaponRng) -> f32 {
    1.0 + (rng.next() * 2.0 - 1.0) * PITCH_JITTER
}

/// A shot that ended `end` against the World is now and then heard hitting it.
fn play_impact(
    commands: &mut Commands,
    cue: &mut RearCue,
    sounds: &WeaponSounds,
    rng: &mut WeaponRng,
    end: Vec3,
) {
    let roll = rng.next();
    let sound = if roll < RICOCHET_CHANCE {
        &sounds.ricochet
    } else if roll < IMPACT_CHANCE {
        &sounds.impact
    } else {
        return;
    };
    let speed = 0.9 + rng.next() * 0.2;
    cue.play(commands, sound, IMPACT_VOLUME, speed, end);
}

fn play_local_shots(
    mut fired: MessageReader<ShotFired>,
    sounds: Res<WeaponSounds>,
    data: Res<GameData>,
    mut rng: ResMut<WeaponRng>,
    mut cue: RearCue,
    mut commands: Commands,
) {
    // Only a round's first pellet makes its sound; the others are the same shot.
    for shot in fired.read().filter(|shot| shot.pellet == 0) {
        let speed = pitch(&mut rng) * shot.pitch;
        play_flat(
            &mut commands,
            &sounds.shots[&shot.weapon],
            LOCAL_SHOT_VOLUME * shot.volume,
            speed,
        );
        let range = data.weapon(shot.weapon).range;
        if let Some(end) = shot.end
            && shot.muzzle.distance(end) < range - RANGE_MARGIN
        {
            play_impact(&mut commands, &mut cue, &sounds, &mut rng, end);
        }
    }
}

fn play_remote_shots(
    mut tracers: MessageReader<FromServer<ShotTracer>>,
    sounds: Res<WeaponSounds>,
    data: Res<GameData>,
    mut rng: ResMut<WeaponRng>,
    mut cue: RearCue,
    mut commands: Commands,
) {
    for FromServer(tracer) in tracers.read().filter(|tracer| tracer.0.pellet == 0) {
        let (volume, shot_pitch) = data.shot_sound(tracer.weapon, tracer.attachments);
        let speed = pitch(&mut rng) * shot_pitch;
        cue.play(
            &mut commands,
            &sounds.shots[&tracer.weapon],
            REMOTE_SHOT_VOLUME * volume,
            speed,
            tracer.origin,
        );
        let range = data.weapon(tracer.weapon).range;
        if tracer.origin.distance(tracer.end) < range - RANGE_MARGIN {
            play_impact(&mut commands, &mut cue, &sounds, &mut rng, tracer.end);
        }
    }
}

/// Server-simulated bullets: other players' shots are heard as the bullet leaves the muzzle
/// (the shooter's own gunshot already played), and the odd impact where any bullet lands.
fn play_projectile_sounds(
    mut fired: MessageReader<FromServer<ProjectileFired>>,
    mut landed: MessageReader<FromServer<ProjectileLanded>>,
    local: Query<&LocalId, (With<Client>, With<Connected>)>,
    sounds: Res<WeaponSounds>,
    data: Res<GameData>,
    mut rng: ResMut<WeaponRng>,
    mut cue: RearCue,
    mut commands: Commands,
) {
    let own = local.single().ok().map(|id| id.0);
    for FromServer(shot) in fired.read() {
        if Some(shot.shooter) == own {
            continue;
        }
        let (volume, shot_pitch) = data.shot_sound(shot.weapon, shot.attachments);
        let speed = pitch(&mut rng) * shot_pitch;
        cue.play(
            &mut commands,
            &sounds.shots[&shot.weapon],
            REMOTE_SHOT_VOLUME * volume,
            speed,
            shot.origin,
        );
    }
    for FromServer(landing) in landed.read() {
        if landing.struck {
            play_impact(&mut commands, &mut cue, &sounds, &mut rng, landing.position);
        }
    }
}

/// The magazine going in part-way through a reload, once per reload (`played` remembers).
fn play_reload_sounds(
    player: Query<&HeldWeapon, With<LocalPlayer>>,
    sounds: Res<WeaponSounds>,
    mut played: Local<bool>,
    mut commands: Commands,
) {
    let progress = player.single().ok().and_then(HeldWeapon::reload_progress);
    let Some(progress) = progress else {
        *played = false;
        return;
    };
    if progress >= MAG_INSERT_AT && !*played {
        *played = true;
        play_flat(&mut commands, &sounds.mag_insert, MAG_VOLUME, 1.0);
    }
}

/// The click of a trigger pulled on an empty magazine, and the higher one of a fire-mode switch.
fn play_trigger_clicks(
    mut dry: MessageReader<DryFire>,
    mut switched: MessageReader<ModeSwitched>,
    sounds: Res<WeaponSounds>,
    mut commands: Commands,
) {
    for _ in dry.read() {
        play_flat(&mut commands, &sounds.dry_fire, CLICK_VOLUME, 1.0);
    }
    for _ in switched.read() {
        play_flat(
            &mut commands,
            &sounds.dry_fire,
            MODE_CLICK_VOLUME,
            MODE_CLICK_SPEED,
        );
    }
}
