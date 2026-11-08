//! Positional one-shot sounds (shots, impacts, footsteps, grenades) heard from the eye camera.
//!
//! Bevy's spatial audio (rodio's `Spatial`) pans weakly: the quieter ear never drops below half
//! and nothing tells a sound in front from one behind. So these sounds are placed here instead,
//! once, when they start (they are short): the source is mixed to mono and played to the left
//! and right channel at gains from the sound's bearing (equal power, up to about 14 dB between
//! the ears for a sound at the side), softened by distance (inverse distance beyond
//! `REFERENCE_DISTANCE`), and a sound behind the listener is also low-passed and about 3 dB
//! quieter, since stereo alone cannot tell front from back.
//!
//! The eye camera still carries a `SpatialListener` for the sounds that use Bevy's spatial audio
//! (the bomb beeps and the Flag sounds in `modes/visuals.rs`). Its ear offsets are mirrored on
//! purpose: rodio 0.22's `Spatial` makes the channel of the ear *farther* from the emitter
//! louder, so `SpatialListener::new` puts sounds on the wrong side. Recheck on any bevy/rodio
//! upgrade.

use bevy::audio::{
    AddAudioSource, AudioPlayer, AudioSource, Decodable, PlaybackSettings, Source, SpatialListener,
    Volume,
};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use rodio::source::ChannelVolume;

use crate::player::EyeCamera;

/// Within this many metres a sound plays at full volume; beyond, its volume falls with
/// `REFERENCE_DISTANCE / distance` (-6 dB per doubling: half at 12 m, a quarter at 24 m).
const REFERENCE_DISTANCE: f32 = 6.0;
/// Distance between the listener's ears, in metres, for the Bevy spatial sounds.
const EAR_GAP: f32 = 0.2;
/// How far a sound at the side pans: 1.0 would silence the far ear completely.
const MAX_PAN: f32 = 0.92;
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
        app.add_audio_source::<Placed>().add_observer(add_listener);
    }
}

fn add_listener(add: On<Add<EyeCamera>>, mut commands: Commands) {
    commands.entity(add.entity).insert(SpatialListener {
        left_ear_offset: Vec3::X * EAR_GAP / 2.0,
        right_ear_offset: Vec3::X * EAR_GAP / -2.0,
    });
}

/// An [`AudioSource`] mixed to mono and played at `left` and `right` gains, low-passed when
/// `muffled`.
#[derive(Asset, TypePath, Clone)]
struct Placed {
    source: AudioSource,
    left: f32,
    right: f32,
    muffled: bool,
}

impl Decodable for Placed {
    type Decoder = Box<dyn Source + Send>;

    fn decoder(&self) -> Self::Decoder {
        let stereo = ChannelVolume::new(self.source.decoder(), vec![self.left, self.right]);
        if self.muffled {
            Box::new(stereo.low_pass(MUFFLE_HZ))
        } else {
            Box::new(stereo)
        }
    }
}

/// Where a sound is relative to the listener, as gains.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    left: f32,
    right: f32,
    /// Distance fall-off, 0..1.
    distance_gain: f32,
    behind: bool,
}

impl Placement {
    /// Straight ahead and near: both ears, full volume.
    const CENTRED: Placement = Placement {
        left: 1.0,
        right: 1.0,
        distance_gain: 1.0,
        behind: false,
    };

    /// A sound at `at` heard by a listener at `eye` (its right and forward directions are the
    /// transform's).
    fn of(eye: &GlobalTransform, at: Vec3) -> Placement {
        let offset = at - eye.translation();
        let distance = offset.length();
        if distance <= f32::EPSILON {
            return Placement::CENTRED;
        }
        let direction = offset / distance;
        let pan = direction.dot(*eye.right()).clamp(-1.0, 1.0) * MAX_PAN;
        Placement {
            // Equal power, scaled so a centred sound keeps both channels at 1.
            left: ((1.0 - pan) / 2.0).sqrt() * std::f32::consts::SQRT_2,
            right: ((1.0 + pan) / 2.0).sqrt() * std::f32::consts::SQRT_2,
            distance_gain: (REFERENCE_DISTANCE / distance).min(1.0),
            behind: direction.dot(*eye.forward()) < BEHIND_COS,
        }
    }
}

/// Plays one-shot sounds from a point in the World.
#[derive(SystemParam)]
pub struct RearCue<'w, 's> {
    listener: Query<'w, 's, &'static GlobalTransform, With<SpatialListener>>,
    sources: Res<'w, Assets<AudioSource>>,
    placed: ResMut<'w, Assets<Placed>>,
}

impl RearCue<'_, '_> {
    /// Plays `sound` once from `at`; `speed` is the pitch. A sound that has not loaded yet is
    /// skipped (there is nothing to play).
    pub fn play(
        &mut self,
        commands: &mut Commands,
        sound: &Handle<AudioSource>,
        volume: f32,
        speed: f32,
        at: Vec3,
    ) {
        let Some(source) = self.sources.get(sound) else {
            return;
        };
        let placement = self
            .listener
            .single()
            .map_or(Placement::CENTRED, |eye| Placement::of(eye, at));
        let gain = placement.distance_gain * if placement.behind { BEHIND_GAIN } else { 1.0 };
        let placed = self.placed.add(Placed {
            source: source.clone(),
            left: placement.left,
            right: placement.right,
            muffled: placement.behind,
        });
        commands.spawn((
            Name::new("Sound"),
            AudioPlayer(placed),
            PlaybackSettings::DESPAWN
                .with_speed(speed)
                .with_volume(Volume::Linear(volume * gain)),
        ));
    }
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

    /// A listener at `position` looking along -Z (right is +X).
    fn listener(position: Vec3) -> GlobalTransform {
        GlobalTransform::from_translation(position)
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
    fn a_sound_on_one_side_is_louder_in_that_ear_and_ahead_is_even() {
        let eye = listener(Vec3::new(4.0, 1.6, -2.0));
        let at = |offset: Vec3| Placement::of(&eye, eye.translation() + offset);
        let right = at(Vec3::X * 8.0);
        assert!(right.right > 1.3 && right.left < 0.35, "{right:?}");
        let left = at(Vec3::NEG_X * 8.0);
        assert!(left.left > 1.3 && left.right < 0.35, "{left:?}");
        let ahead = at(Vec3::NEG_Z * 8.0);
        assert!((ahead.left - ahead.right).abs() < 1e-5, "{ahead:?}");
        assert!(
            (ahead.left - 1.0).abs() < 1e-5,
            "centred keeps unit gain: {ahead:?}"
        );
        // Halfway round to the front: a clear but smaller difference.
        let front_right = at(Vec3::new(5.0, 0.0, -5.0));
        assert!(
            front_right.right > front_right.left * 1.5,
            "{front_right:?}"
        );
        assert!(front_right.right < right.right);
    }

    #[test]
    fn volume_falls_with_distance_beyond_the_reference() {
        let eye = listener(Vec3::ZERO);
        let gain = |metres: f32| Placement::of(&eye, Vec3::NEG_Z * metres).distance_gain;
        assert_eq!(gain(1.0), 1.0);
        assert_eq!(gain(REFERENCE_DISTANCE), 1.0);
        assert!((gain(2.0 * REFERENCE_DISTANCE) - 0.5).abs() < 1e-5);
        assert!((gain(4.0 * REFERENCE_DISTANCE) - 0.25).abs() < 1e-5);
        assert!(gain(40.0) < gain(20.0));
    }

    #[test]
    fn behind_is_relative_to_the_view_direction() {
        let eye = listener(Vec3::new(4.0, 1.6, -2.0));
        let behind_of = |eye: &GlobalTransform, at: Vec3| Placement::of(eye, at).behind;
        let ahead = eye.translation() + Vec3::NEG_Z * 10.0;
        let behind = eye.translation() + Vec3::Z * 10.0;
        assert!(!behind_of(&eye, ahead));
        assert!(behind_of(&eye, behind));
        // The same spot flips when the listener turns around.
        let turned = GlobalTransform::from(
            Transform::from_translation(eye.translation()).looking_to(Dir3::Z, Dir3::Y),
        );
        assert!(behind_of(&turned, ahead));
        assert!(!behind_of(&turned, behind));
    }

    #[test]
    fn sounds_at_the_side_or_on_the_listener_are_not_behind() {
        let eye = listener(Vec3::ZERO);
        assert!(!Placement::of(&eye, Vec3::X * 10.0).behind);
        assert_eq!(Placement::of(&eye, Vec3::ZERO), Placement::CENTRED);
        // Just past the side line (about 95 degrees) still counts as the side.
        assert!(!Placement::of(&eye, Vec3::new(10.0, 0.0, 0.9)).behind);
    }

    #[test]
    fn the_muffled_sound_lacks_the_high_frequencies() {
        let placed = |muffled| Placed {
            source: AudioSource {
                bytes: GUNSHOT.into(),
            },
            left: 1.0,
            right: 1.0,
            muffled,
        };
        let clear = roughness(placed(false).decoder());
        let muffled = roughness(placed(true).decoder());
        assert!(
            muffled < clear * 0.6,
            "clear {clear}, muffled {muffled}: the low-pass changed too little"
        );
    }

    #[test]
    fn a_placed_sound_plays_to_the_two_ears_at_the_placement_gains() {
        let placed = Placed {
            source: footstep(),
            left: 0.25,
            right: 1.5,
            muffled: false,
        };
        let samples: Vec<f32> = placed.decoder().collect();
        let (left, right): (Vec<f32>, Vec<f32>) = samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[left, right]| (left, right))
            .unzip();
        let energy = |channel: &[f32]| channel.iter().map(|s| s * s).sum::<f32>();
        let ratio = (energy(&right) / energy(&left)).sqrt();
        assert!((ratio - 6.0).abs() < 0.05, "ear ratio {ratio}, expected 6");
    }

    #[test]
    fn only_sounds_from_behind_play_muffled_and_quieter_and_unloaded_ones_are_skipped() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<AudioSource>()
            .init_asset::<Placed>();
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
                // Behind, at the reference distance: muffled and -3 dB.
                cue.play(
                    &mut commands,
                    &loaded,
                    1.0,
                    1.0,
                    Vec3::new(0.0, 0.0, REFERENCE_DISTANCE),
                );
                // Ahead at twice the reference distance: not muffled, half.
                cue.play(
                    &mut commands,
                    &loaded,
                    1.0,
                    1.0,
                    Vec3::new(0.0, 0.0, -2.0 * REFERENCE_DISTANCE),
                );
                // Not loaded yet: skipped.
                cue.play(&mut commands, &missing, 1.0, 1.0, Vec3::new(0.0, 0.0, 10.0));
            })
            .unwrap();

        let world = app.world_mut();
        let mut played: Vec<(bool, f32)> = world
            .query::<(&AudioPlayer<Placed>, &PlaybackSettings)>()
            .iter(world)
            .map(|(player, settings)| {
                let muffled = world
                    .resource::<Assets<Placed>>()
                    .get(&player.0)
                    .is_some_and(|placed| placed.muffled);
                (muffled, settings.volume.to_linear())
            })
            .collect();
        played.sort_by(|a, b| a.1.total_cmp(&b.1));
        assert_eq!(played.len(), 2);
        assert_eq!(played[0], (false, 0.5));
        assert_eq!(played[1], (true, BEHIND_GAIN));
    }
}
