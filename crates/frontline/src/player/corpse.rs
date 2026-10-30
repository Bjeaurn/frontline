//! Bodies of dead remote players (client-side, cosmetic): a copy of the player's soldier that
//! tips over from where it died, lies there `corpse_secs`, then sinks into the ground fading
//! out. No colliders, no ragdoll; at most `max_corpses` at once, oldest removed first.

use std::f32::consts::FRAC_PI_2;
use std::hash::{BuildHasher, RandomState};

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;

use super::remote::{RemoteAssets, RemoteDeath, soldier_material, team_color, team_material};
use super::soldier::{self, Gait};
use crate::data::GameData;
use crate::flow::AppState;
use crate::shared::{GameLayer, Stance};

/// How long the body takes to tip over onto the ground.
const FALL_SECS: f32 = 0.5;
/// How far either side of straight backwards the body may fall, in radians.
const FALL_SPREAD: f32 = 0.5;
/// How long the body takes to sink away once its time is up.
const SINK_SECS: f32 = 1.5;
/// How deep it sinks: more than a lying soldier is thick, so this takes it fully under.
const SINK_DEPTH: f32 = 0.6;
/// The ground is looked for this far above and below the death position (dying mid-jump).
const GROUND_PROBE: f32 = 0.5;
const GROUND_MAX_DROP: f32 = 20.0;
/// Where the rifle points on a body: down, dropped from the hands.
const DROPPED_PITCH: f32 = -0.9;

/// A body on the ground (or on its way down).
#[derive(Component, Debug)]
pub(super) struct Corpse {
    /// Seconds since death.
    age: f32,
    /// Feet position and yaw at death, on the ground.
    feet: Transform,
    /// Height above the feet that the body tips about (its back surface).
    pivot_height: f32,
    /// Horizontal unit direction it tips toward.
    fall_direction: Vec3,
    /// How far it tips: a quarter turn upright, none when already prone.
    tip_angle: f32,
    team_color: Color,
    /// The soldier's and the team marks' materials, faded together while sinking.
    materials: [Handle<StandardMaterial>; 2],
}

impl Corpse {
    /// The body's transform `age` seconds after death: rotated about its back surface, so a
    /// quarter turn lays it on the ground, then lowered while sinking.
    fn transform(&self, corpse_secs: f32) -> Transform {
        let fall = (self.age / FALL_SECS).min(1.0);
        // Accelerates like a fall rather than a hinge at constant speed.
        let angle = self.tip_angle * fall * fall;
        let axis = Vec3::Y.cross(self.fall_direction);
        let pivot = self.feet.translation + Vec3::Y * self.pivot_height;
        let mut body = self.feet;
        body.rotate_around(pivot, Quat::from_axis_angle(axis, angle));
        body.translation.y -= SINK_DEPTH * self.sinking(corpse_secs);
        body
    }

    /// 0 while lying, rising to 1 as it sinks away.
    fn sinking(&self, corpse_secs: f32) -> f32 {
        ((self.age - corpse_secs) / SINK_SECS).clamp(0.0, 1.0)
    }
}

/// Lays a body down for each `RemoteDeath`, removing the oldest beyond `max_corpses`.
pub(super) fn spawn_corpses(
    mut deaths: MessageReader<RemoteDeath>,
    data: Res<GameData>,
    assets: Res<RemoteAssets>,
    spatial: SpatialQuery,
    corpses: Query<(Entity, &Corpse)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let max = data.presentation.max_corpses;
    if deaths.is_empty() {
        return;
    }
    // Oldest last, so the next to go is popped.
    let mut lying: Vec<(Entity, f32)> = corpses
        .iter()
        .map(|(entity, corpse)| (entity, corpse.age))
        .collect();
    lying.sort_by(|a, b| a.1.total_cmp(&b.1));
    let ground_filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let random = RandomState::new();

    for death in deaths.read() {
        if max == 0 {
            continue;
        }
        while lying.len() >= max {
            let (oldest, _) = lying.pop().expect("non-empty when at the cap");
            commands.entity(oldest).try_despawn();
        }

        let mut feet = death.transform;
        if let Some(hit) = spatial.cast_ray(
            feet.translation + Vec3::Y * GROUND_PROBE,
            Dir3::NEG_Y,
            GROUND_PROBE + GROUND_MAX_DROP,
            true,
            &ground_filter,
        ) {
            feet.translation.y += GROUND_PROBE - hit.distance;
        }
        // Backwards (yaw faces -Z), turned a little either way.
        let spread = (random.hash_one(lying.len()) as f64 / u64::MAX as f64) as f32;
        let turn = Quat::from_rotation_y((spread * 2.0 - 1.0) * FALL_SPREAD);
        let fall_direction = (turn * feet.rotation * Vec3::Z).with_y(0.0).normalize();
        let tip_angle = match death.stance {
            Stance::Standing | Stance::Crouching => FRAC_PI_2,
            Stance::Prone => 0.0,
        };
        let team_look = team_material(team_color(death.friendly));
        let team_color = team_look.base_color;
        let body = materials.add(soldier_material());
        let team = materials.add(team_look);
        let corpse = Corpse {
            age: 0.0,
            feet,
            pivot_height: soldier::back_height(death.stance),
            fall_direction,
            tip_angle,
            team_color,
            materials: [body.clone(), team.clone()],
        };
        let entity = commands
            .spawn((
                Name::new("Corpse"),
                DespawnOnExit(AppState::InGame),
                corpse.transform(data.presentation.corpse_secs),
                Visibility::default(),
                corpse,
            ))
            .id();
        assets.soldier(death.faction).spawn(
            &mut commands,
            entity,
            &body,
            &team,
            &soldier::pose(death.stance, Gait::default(), DROPPED_PITCH),
        );
        lying.insert(0, (entity, 0.0));
    }
}

/// Tips bodies over, then after `corpse_secs` sinks and fades them out and removes them.
pub(super) fn animate_corpses(
    time: Res<Time>,
    data: Res<GameData>,
    mut corpses: Query<(Entity, &mut Corpse, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let corpse_secs = data.presentation.corpse_secs;
    for (entity, mut corpse, mut transform) in &mut corpses {
        corpse.age += time.delta_secs();
        if corpse.age >= corpse_secs + SINK_SECS {
            commands.entity(entity).despawn();
            continue;
        }
        *transform = corpse.transform(corpse_secs);
        let sinking = corpse.sinking(corpse_secs);
        if sinking > 0.0 {
            let alpha = 1.0 - sinking;
            for (handle, color) in corpse
                .materials
                .iter()
                .zip([Color::WHITE, corpse.team_color])
            {
                if let Some(mut material) = materials.get_mut(handle) {
                    material.alpha_mode = AlphaMode::Blend;
                    material.base_color = color.with_alpha(alpha);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIVOT: f32 = 0.25;

    fn standing_corpse(age: f32) -> Corpse {
        Corpse {
            age,
            feet: Transform::IDENTITY,
            pivot_height: PIVOT,
            fall_direction: Vec3::Z,
            tip_angle: FRAC_PI_2,
            team_color: Color::WHITE,
            materials: [Handle::default(), Handle::default()],
        }
    }

    #[test]
    fn tips_onto_its_back_then_sinks() {
        let corpse_secs = 10.0;
        let upright = standing_corpse(0.0).transform(corpse_secs);
        assert!(upright.translation.length() < 1e-5);

        // Lying: the feet one pivot height up and as far back toward the head's old side, the
        // body's up pointing along the fall, so its back surface is on the ground.
        let lying = standing_corpse(FALL_SECS).transform(corpse_secs);
        assert!((lying.translation.y - PIVOT).abs() < 1e-5);
        assert!((lying.translation.z + PIVOT).abs() < 1e-5);
        assert!((lying.rotation * Vec3::Y).dot(Vec3::Z) > 0.999);
        let back = lying.transform_point(Vec3::new(0.0, 1.0, PIVOT));
        assert!(back.y.abs() < 1e-5, "back surface at {}", back.y);

        let sunk = standing_corpse(corpse_secs + SINK_SECS).transform(corpse_secs);
        assert!((sunk.translation.y - (PIVOT - SINK_DEPTH)).abs() < 1e-5);
    }
}
