//! Positional sounds heard from the eye camera, with a rear cue: Bevy's spatial audio is plain
//! stereo panning, so a sound behind the listener would sound like one in front. A sound that
//! starts behind the listener plays low-passed and about 3 dB quieter instead.
//!
//! The listener is the eye camera. Its ear offsets are mirrored on purpose: rodio 0.22's `Spatial`
//! (behind Bevy's spatial audio) makes the channel of the ear *farther* from the emitter louder,
//! so `SpatialListener::new` puts sounds on the wrong side. Recheck on any bevy/rodio upgrade.
//! Panning is bounded by rodio (the quieter ear never drops below half), whatever `EAR_GAP` is.

use std::collections::HashMap;

use bevy::audio::{
    AddAudioSource, AudioPlayer, AudioSource, Decodable, PlaybackSettings, Source, SpatialListener,
    SpatialScale, Volume,
};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::player::EyeCamera;

/// Metres to sound distance: full volume within 1 / SPATIAL_SCALE metres, then inverse-square
/// (about -20 dB at 45 m).
const SPATIAL_SCALE: f32 = 0.07;
/// Distance between the listener's ears, in metres.
const EAR_GAP: f32 = 0.2;
/// A sound is behind the listener when the cosine of its angle to the view direction is below
/// this (about 100 degrees), so sounds at the side stay clear.
const BEHIND_COS: f32 = -0.2;
/// Cutoff of the low-pass on sounds behind the listener, in hertz.
const MUFFLE_HZ: u32 = 2500;
/// About -3 dB.
const BEHIND_GAIN: f32 = 0.708;

pub struct RearCuePlugin;

impl Plugin for RearCuePlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<Muffled>()
            .init_resource::<MuffledSounds>()
            .add_observer(add_listener);
    }
}

fn add_listener(add: On<Add<EyeCamera>>, mut commands: Commands) {
    commands.entity(add.entity).insert(SpatialListener {
        left_ear_offset: Vec3::X * EAR_GAP / 2.0,
        right_ear_offset: Vec3::X * EAR_GAP / -2.0,
    });
}

/// An [`AudioSource`] played through a low-pass filter.
#[derive(Asset, TypePath, Clone)]
struct Muffled(AudioSource);

impl Decodable for Muffled {
    type Decoder = Box<dyn Source + Send>;

    fn decoder(&self) -> Self::Decoder {
        Box::new(self.0.decoder().low_pass(MUFFLE_HZ))
    }
}

/// The filtered twin of each sound, made the first time it plays from behind.
#[derive(Resource, Default)]
struct MuffledSounds(HashMap<AssetId<AudioSource>, Handle<Muffled>>);

/// Plays one-shot sounds from a point in the World.
#[derive(SystemParam)]
pub struct RearCue<'w, 's> {
    listener: Query<'w, 's, &'static GlobalTransform, With<SpatialListener>>,
    sources: Res<'w, Assets<AudioSource>>,
    muffled: ResMut<'w, Assets<Muffled>>,
    twins: ResMut<'w, MuffledSounds>,
}

impl RearCue<'_, '_> {
    /// Plays `sound` once from `at`; `speed` is the pitch.
    pub fn play(
        &mut self,
        commands: &mut Commands,
        sound: &Handle<AudioSource>,
        volume: f32,
        speed: f32,
        at: Vec3,
    ) {
        let settings = PlaybackSettings::DESPAWN
            .with_speed(speed)
            .with_spatial(true)
            .with_spatial_scale(SpatialScale::new(SPATIAL_SCALE));
        let behind = self
            .listener
            .single()
            .is_ok_and(|listener| is_behind(listener.translation(), listener.forward(), at));
        match behind.then(|| self.muffled(sound)).flatten() {
            Some(twin) => commands.spawn((
                Name::new("Sound"),
                AudioPlayer(twin),
                settings.with_volume(Volume::Linear(volume * BEHIND_GAIN)),
                Transform::from_translation(at),
            )),
            None => commands.spawn((
                Name::new("Sound"),
                AudioPlayer::new(sound.clone()),
                settings.with_volume(Volume::Linear(volume)),
                Transform::from_translation(at),
            )),
        };
    }

    /// The filtered twin of `sound`, or `None` while it has not loaded.
    fn muffled(&mut self, sound: &Handle<AudioSource>) -> Option<Handle<Muffled>> {
        if let Some(twin) = self.twins.0.get(&sound.id()) {
            return Some(twin.clone());
        }
        let twin = self.muffled.add(Muffled(self.sources.get(sound)?.clone()));
        self.twins.0.insert(sound.id(), twin.clone());
        Some(twin)
    }
}

/// Whether `at` is behind a listener standing at `position` and looking along `forward`.
fn is_behind(position: Vec3, forward: Dir3, at: Vec3) -> bool {
    let offset = at - position;
    offset.length_squared() > f32::EPSILON && offset.normalize().dot(*forward) < BEHIND_COS
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    const FOOTSTEP: &[u8] = include_bytes!("../assets/sounds/footstep_concrete_000.ogg");
    const GUNSHOT: &[u8] = include_bytes!("../assets/sounds/shot_ar.ogg");

    fn footstep() -> AudioSource {
        AudioSource {
            bytes: FOOTSTEP.into(),
        }
    }

    /// How jagged the waveform is: summed sample-to-sample change over summed level, which
    /// falls as the high frequencies go.
    fn roughness(samples: impl Iterator<Item = f32>) -> f32 {
        let (mut change, mut level, mut last) = (0.0, 0.0, 0.0);
        for sample in samples {
            change += (sample - last).abs();
            level += sample.abs();
            last = sample;
        }
        change / level
    }

    #[test]
    fn behind_is_relative_to_the_view_direction() {
        let listener = Vec3::new(4.0, 1.6, -2.0);
        let ahead = listener + Vec3::NEG_Z * 10.0;
        let behind = listener + Vec3::Z * 10.0;
        assert!(!is_behind(listener, Dir3::NEG_Z, ahead));
        assert!(is_behind(listener, Dir3::NEG_Z, behind));
        // The same spot flips when the listener turns around.
        assert!(is_behind(listener, Dir3::Z, ahead));
        assert!(!is_behind(listener, Dir3::Z, behind));
    }

    #[test]
    fn sounds_at_the_side_or_on_the_listener_are_not_behind() {
        let listener = Vec3::ZERO;
        assert!(!is_behind(listener, Dir3::NEG_Z, Vec3::X * 10.0));
        assert!(!is_behind(listener, Dir3::NEG_Z, Vec3::ZERO));
        // Just past the side line (about 95 degrees) still counts as the side.
        let slightly_back = Vec3::new(10.0, 0.0, 0.9);
        assert!(!is_behind(listener, Dir3::NEG_Z, slightly_back));
    }

    #[test]
    fn the_muffled_twin_lacks_the_high_frequencies() {
        let shot = || AudioSource {
            bytes: GUNSHOT.into(),
        };
        let clear = roughness(shot().decoder());
        let muffled = roughness(Muffled(shot()).decoder());
        assert!(
            muffled < clear * 0.6,
            "clear {clear}, muffled {muffled}: the low-pass changed too little"
        );
    }

    #[test]
    fn only_sounds_from_behind_play_muffled_and_quieter() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<AudioSource>()
            .init_asset::<Muffled>()
            .init_resource::<MuffledSounds>();
        // The listener looks along -Z from the origin.
        app.world_mut()
            .spawn((SpatialListener::new(EAR_GAP), GlobalTransform::IDENTITY));
        let loaded = app
            .world_mut()
            .resource_mut::<Assets<AudioSource>>()
            .add(footstep());
        let missing = Handle::<AudioSource>::default();

        app.world_mut()
            .run_system_once(move |mut cue: RearCue, mut commands: Commands| {
                cue.play(&mut commands, &loaded, 1.0, 1.0, Vec3::new(0.0, 0.0, 10.0));
                cue.play(&mut commands, &loaded, 1.0, 1.0, Vec3::new(0.0, 0.0, -10.0));
                // Not loaded yet: nothing to filter, so it plays as it is.
                cue.play(&mut commands, &missing, 1.0, 1.0, Vec3::new(0.0, 0.0, 10.0));
            })
            .unwrap();

        let world = app.world_mut();
        let muffled: Vec<f32> = world
            .query::<(&AudioPlayer<Muffled>, &PlaybackSettings)>()
            .iter(world)
            .map(|(_, settings)| settings.volume.to_linear())
            .collect();
        let plain: Vec<f32> = world
            .query::<(&AudioPlayer<AudioSource>, &PlaybackSettings)>()
            .iter(world)
            .map(|(_, settings)| settings.volume.to_linear())
            .collect();
        assert_eq!(muffled, [BEHIND_GAIN]);
        assert_eq!(plain, [1.0, 1.0]);
    }
}
