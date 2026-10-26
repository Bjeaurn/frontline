//! First-person kinematic controller for the `LocalPlayer`: mouse look, stances, sprint and
//! move-and-slide against the static arena.

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::hash::{BuildHasher, RandomState};

use avian3d::prelude::*;
use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};

use super::BODY_RADIUS;
use super::input::{Bindings, LookSensitivity};
use crate::cli::Mode;
use crate::shared::{LocalPlayer, PlayerName, PlayerPose, Stance};
use crate::world::SpawnPoints;

// Movement tuning (metres, seconds).
const WALK_SPEED: f32 = 5.0;
const SPRINT_SPEED: f32 = 7.5;
const CROUCH_SPEED: f32 = 3.0;
const PRONE_SPEED: f32 = 1.2;
/// Seconds of continuous sprint on a full tank.
const SPRINT_STAMINA: f32 = 4.0;
/// Stamina regained per second while not sprinting (full tank in 4 s).
const STAMINA_REGEN: f32 = 1.0;
/// After running dry, sprint restarts only once this much stamina is back.
const SPRINT_RESTART_STAMINA: f32 = 1.0;
const GROUND_ACCEL: f32 = 60.0;
const AIR_ACCEL: f32 = 8.0;
const GRAVITY: f32 = 15.0;
const JUMP_HEIGHT: f32 = 0.9;
const TERMINAL_FALL_SPEED: f32 = 40.0;
/// Surfaces whose normal has at least this much `y` count as ground (about 50 degrees).
const MIN_GROUND_NORMAL_Y: f32 = 0.64;
/// Downward probe distance that counts as standing on something.
const GROUND_PROBE: f32 = 0.05;
/// While grounded, the body is pulled down this far to follow ramps and small drops.
const GROUND_SNAP: f32 = 0.35;
/// Falling below this height respawns the player.
const KILL_Y: f32 = -20.0;

// View tuning.
const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.01;
/// Horizontal field of view, in degrees, kept constant across aspect ratios.
const HORIZONTAL_FOV_DEG: f32 = 80.0;
/// How fast the eye height follows stance changes (exponential decay rate, 1/s).
const EYE_HEIGHT_RATE: f32 = 12.0;

/// Clearance tests for stance changes use a slightly shrunken body lifted off the floor, so
/// merely touching the floor or a wall does not count as blocked.
const HEADROOM_SHRINK: f32 = 0.03;
const HEADROOM_LIFT: f32 = 0.02;

/// Movement state of the local player, beyond what `PlayerPose` carries.
#[derive(Component, Debug)]
pub(super) struct Motor {
    velocity: Vec3,
    grounded: bool,
    stamina: f32,
    sprinting: bool,
    jump_requested: bool,
}

impl Default for Motor {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            grounded: false,
            stamina: SPRINT_STAMINA,
            sprinting: false,
            jump_requested: false,
        }
    }
}

impl Motor {
    /// Advances sprint state and stamina by `dt`: sprint drains, rest regenerates, and once the
    /// tank runs dry sprint restarts only from the ground with `SPRINT_RESTART_STAMINA` back.
    fn tick_sprint(&mut self, wants_sprint: bool, dt: f32) {
        if self.sprinting {
            self.sprinting = wants_sprint && self.stamina > 0.0;
        } else {
            self.sprinting =
                wants_sprint && self.grounded && self.stamina >= SPRINT_RESTART_STAMINA;
        }
        self.stamina = if self.sprinting {
            (self.stamina - dt).max(0.0)
        } else {
            (self.stamina + STAMINA_REGEN * dt).min(SPRINT_STAMINA)
        };
    }
}

/// The first-person camera, a child of the local player; `height` is the smoothed eye height.
#[derive(Component, Debug)]
pub(super) struct EyeCamera {
    height: f32,
}

/// Whether the cursor is grabbed for mouse look. Input is ignored while it isn't.
#[derive(Resource, Default, Debug)]
pub(super) struct MouseCaptured(bool);

/// Collision shapes per stance: the body for movement and a shrunken one for headroom tests.
#[derive(Resource)]
pub(super) struct StanceShapes {
    body: [Collider; 3],
    headroom: [Collider; 3],
}

const STANCES: [Stance; 3] = [Stance::Standing, Stance::Crouching, Stance::Prone];

fn stance_index(stance: Stance) -> usize {
    match stance {
        Stance::Standing => 0,
        Stance::Crouching => 1,
        Stance::Prone => 2,
    }
}

/// An upright capsule of the stance's full body height (a sphere when it is too short).
fn body_collider(stance: Stance, shrink: f32) -> Collider {
    let height = stance.body_height();
    let radius = BODY_RADIUS.min(height / 2.0);
    let segment = height - 2.0 * radius;
    if segment > 1e-3 {
        Collider::capsule(radius - shrink, segment)
    } else {
        Collider::sphere(radius - shrink)
    }
}

impl Default for StanceShapes {
    fn default() -> Self {
        Self {
            body: STANCES.map(|stance| body_collider(stance, 0.0)),
            headroom: STANCES.map(|stance| body_collider(stance, HEADROOM_SHRINK)),
        }
    }
}

impl StanceShapes {
    fn body(&self, stance: Stance) -> &Collider {
        &self.body[stance_index(stance)]
    }

    /// Whether a body in `stance` with its feet at `feet` overlaps no static geometry.
    fn fits(&self, spatial: &SpatialQuery, stance: Stance, feet: Vec3) -> bool {
        let center = feet + Vec3::Y * (stance.body_height() / 2.0 + HEADROOM_LIFT);
        let mut blocked = false;
        spatial.shape_intersections_callback(
            &self.headroom[stance_index(stance)],
            center,
            Quat::IDENTITY,
            &SpatialQueryFilter::default(),
            |_| {
                blocked = true;
                false
            },
        );
        !blocked
    }
}

fn random_spawn(spawns: &SpawnPoints) -> Vec3 {
    let index = RandomState::new().hash_one(0_u8) as usize % spawns.0.len().max(1);
    spawns.0.get(index).copied().unwrap_or_default()
}

/// Yaw that faces the arena centre from `position`.
fn yaw_towards_centre(position: Vec3) -> f32 {
    position.x.atan2(position.z)
}

pub(super) fn spawn_local_player(
    mut commands: Commands,
    spawns: Res<SpawnPoints>,
    mode: Res<Mode>,
) {
    let name = match &*mode {
        Mode::Client { name, .. } | Mode::Host { name, .. } => name.clone(),
        Mode::Dedicated { .. } => return,
    };
    let feet = random_spawn(&spawns) + Vec3::Y * HEADROOM_LIFT;
    let yaw = yaw_towards_centre(feet);
    let stance = Stance::Standing;
    commands.spawn((
        Name::new("LocalPlayer"),
        LocalPlayer,
        PlayerName(name),
        PlayerPose {
            position: feet,
            yaw,
            pitch: 0.0,
            stance,
        },
        Motor::default(),
        Transform::from_translation(feet).with_rotation(Quat::from_rotation_y(yaw)),
        Visibility::default(),
        children![(
            Name::new("EyeCamera"),
            EyeCamera {
                height: stance.eye_height(),
            },
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                near: 0.05,
                ..default()
            }),
            Transform::from_xyz(0.0, stance.eye_height(), 0.0),
        )],
    ));
}

/// Grab the cursor on click, release it on the release key or when the window loses focus.
pub(super) fn grab_cursor(
    bindings: Res<Bindings>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut captured: ResMut<MouseCaptured>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let capture = if !window.focused || bindings.release_cursor.just_pressed(&keys) {
        false
    } else if mouse.just_pressed(bindings.grab_cursor) {
        true
    } else {
        return;
    };
    if captured.0 != capture {
        captured.0 = capture;
        cursor.visible = !capture;
        cursor.grab_mode = if capture {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
    }
}

pub(super) fn mouse_look(
    captured: Res<MouseCaptured>,
    motion: Res<AccumulatedMouseMotion>,
    sensitivity: Res<LookSensitivity>,
    mut player: Query<(&mut PlayerPose, &mut Transform), With<LocalPlayer>>,
) {
    if !captured.0 || motion.delta == Vec2::ZERO {
        return;
    }
    let Ok((mut pose, mut transform)) = player.single_mut() else {
        return;
    };
    let delta = motion.delta * sensitivity.0;
    pose.yaw = (pose.yaw - delta.x + PI).rem_euclid(TAU) - PI;
    pose.pitch = (pose.pitch - delta.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    transform.rotation = Quat::from_rotation_y(pose.yaw);
}

/// The stance-relevant key press this frame, in priority order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StanceInput {
    None,
    Jump,
    Crouch,
    Prone,
    /// Sprint pressed while holding forward.
    SprintForward,
}

/// The stance `input` leads to from `current`, or `None` to stay. Taller stances are tried
/// first and the first one `fits` accepts wins.
fn next_stance(
    current: Stance,
    input: StanceInput,
    grounded: bool,
    fits: impl Fn(Stance) -> bool,
) -> Option<Stance> {
    use Stance::*;
    let candidates: &[Stance] = match (input, current) {
        // Jumping from standing is handled by the motor.
        (StanceInput::Jump, Standing) => &[],
        (StanceInput::Jump, Crouching) => &[Standing],
        (StanceInput::Jump, Prone) => &[Standing, Crouching],
        (StanceInput::Crouch, Crouching) => &[Standing],
        (StanceInput::Crouch, Standing | Prone) => &[Crouching],
        (StanceInput::Prone, Prone) => &[Crouching],
        (StanceInput::Prone, Standing | Crouching) if grounded => &[Prone],
        // Sprinting gets you up, like CoD.
        (StanceInput::SprintForward, Crouching | Prone) if grounded => &[Standing],
        _ => &[],
    };
    candidates.iter().copied().find(|&stance| fits(stance))
}

/// Applies stance keys. Taller stances need headroom; Space jumps only when already standing.
pub(super) fn change_stance(
    captured: Res<MouseCaptured>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    shapes: Res<StanceShapes>,
    spatial: SpatialQuery,
    mut player: Query<(&mut PlayerPose, &mut Motor), With<LocalPlayer>>,
) {
    let Ok((mut pose, mut motor)) = player.single_mut() else {
        return;
    };
    motor.jump_requested = false;
    if !captured.0 {
        return;
    }

    let input = if bindings.jump.just_pressed(&keys) {
        StanceInput::Jump
    } else if bindings.crouch.just_pressed(&keys) {
        StanceInput::Crouch
    } else if bindings.prone.just_pressed(&keys) {
        StanceInput::Prone
    } else if bindings.sprint.just_pressed(&keys) && bindings.forward.pressed(&keys) {
        StanceInput::SprintForward
    } else {
        StanceInput::None
    };
    motor.jump_requested = input == StanceInput::Jump && pose.stance == Stance::Standing;

    let position = pose.position;
    if let Some(next) = next_stance(pose.stance, input, motor.grounded, |stance| {
        shapes.fits(&spatial, stance, position)
    }) {
        pose.stance = next;
    }
}

fn axis(negative: bool, positive: bool) -> f32 {
    f32::from(u8::from(positive)) - f32::from(u8::from(negative))
}

pub(super) fn move_player(
    time: Res<Time>,
    captured: Res<MouseCaptured>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    shapes: Res<StanceShapes>,
    spawns: Res<SpawnPoints>,
    move_and_slide: MoveAndSlide,
    mut player: Query<(&mut PlayerPose, &mut Motor, &mut Transform), With<LocalPlayer>>,
) {
    let dt = time.delta_secs();
    let Ok((mut pose, mut motor, mut transform)) = player.single_mut() else {
        return;
    };
    if dt <= 0.0 {
        return;
    }

    let pressed = |binding: &super::input::Binding| captured.0 && binding.pressed(&keys);
    let (forward, back) = (pressed(&bindings.forward), pressed(&bindings.back));
    let input = Vec2::new(
        axis(pressed(&bindings.left), pressed(&bindings.right)),
        axis(back, forward),
    );

    // Sprint: forward only, standing only, limited by stamina.
    let wants_sprint =
        pressed(&bindings.sprint) && forward && !back && pose.stance == Stance::Standing;
    motor.tick_sprint(wants_sprint, dt);

    let speed = match pose.stance {
        Stance::Standing if motor.sprinting => SPRINT_SPEED,
        Stance::Standing => WALK_SPEED,
        Stance::Crouching => CROUCH_SPEED,
        Stance::Prone => PRONE_SPEED,
    };
    let wish = Quat::from_rotation_y(pose.yaw) * Vec3::new(input.x, 0.0, -input.y).normalize_or_zero()
        * speed;

    // Horizontal: snappy on the ground, limited steering in the air, momentum kept when idle.
    let horizontal = Vec3::new(motor.velocity.x, 0.0, motor.velocity.z);
    let horizontal = if motor.grounded {
        horizontal.move_towards(wish, GROUND_ACCEL * dt)
    } else if wish != Vec3::ZERO {
        horizontal.move_towards(wish, AIR_ACCEL * dt)
    } else {
        horizontal
    };

    let jumped = motor.grounded && motor.jump_requested;
    let vertical = if jumped {
        (2.0 * GRAVITY * JUMP_HEIGHT).sqrt()
    } else if motor.grounded {
        0.0
    } else {
        (motor.velocity.y - GRAVITY * dt).max(-TERMINAL_FALL_SPEED)
    };

    let shape = shapes.body(pose.stance);
    let half_height = pose.stance.body_height() / 2.0;
    let config = MoveAndSlideConfig::default();
    let filter = SpatialQueryFilter::default();
    let out = move_and_slide.move_and_slide(
        shape,
        pose.position + Vec3::Y * half_height,
        Quat::IDENTITY,
        horizontal + Vec3::Y * vertical,
        time.delta(),
        &config,
        &filter,
        |_| MoveAndSlideHitResponse::Accept,
    );
    let mut center = out.position;
    let mut velocity = out.projected_velocity;

    // Ground check; while walking, reach further down so ramps and small drops stay grounded.
    let ground = if vertical > 0.0 {
        None
    } else {
        let reach = if motor.grounded { GROUND_SNAP } else { GROUND_PROBE };
        move_and_slide
            .spatial_query
            .cast_shape(
                shape,
                center,
                Quat::IDENTITY,
                Dir3::NEG_Y,
                &ShapeCastConfig::from_max_distance(reach + config.skin_width),
                &filter,
            )
            .filter(|hit| hit.normal1.y >= MIN_GROUND_NORMAL_Y)
    };
    motor.grounded = ground.is_some();
    if let Some(hit) = ground {
        center.y -= (hit.distance - config.skin_width).max(0.0);
        velocity.y = 0.0;
    }

    pose.position = center - Vec3::Y * half_height;
    motor.velocity = velocity;
    if pose.position.y < KILL_Y {
        pose.position = random_spawn(&spawns) + Vec3::Y * HEADROOM_LIFT;
        *motor = Motor::default();
    }
    transform.translation = pose.position;
}

/// Places the camera at the (smoothed) eye height and applies pitch.
pub(super) fn update_eye(
    time: Res<Time>,
    player: Query<&PlayerPose, With<LocalPlayer>>,
    mut camera: Query<(&mut EyeCamera, &mut Transform)>,
) {
    let Ok(pose) = player.single() else {
        return;
    };
    for (mut eye, mut transform) in &mut camera {
        eye.height
            .smooth_nudge(&pose.stance.eye_height(), EYE_HEIGHT_RATE, time.delta_secs());
        transform.translation = Vec3::Y * eye.height;
        transform.rotation = Quat::from_rotation_x(pose.pitch);
    }
}

/// Keeps the horizontal field of view at `HORIZONTAL_FOV_DEG` whatever the window shape.
pub(super) fn apply_horizontal_fov(mut cameras: Query<&mut Projection, With<EyeCamera>>) {
    for mut projection in &mut cameras {
        let Projection::Perspective(perspective) = &*projection else {
            continue;
        };
        let half = HORIZONTAL_FOV_DEG.to_radians() / 2.0;
        let fov = 2.0 * (half.tan() / perspective.aspect_ratio).atan();
        if (perspective.fov - fov).abs() > 1e-4
            && let Projection::Perspective(perspective) = &mut *projection
        {
            perspective.fov = fov;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_up_falls_back_to_the_tallest_stance_with_headroom() {
        use Stance::*;
        let all = |_| true;
        let low_ceiling = |stance| stance != Standing;
        let crawlspace = |stance| stance == Prone;

        assert_eq!(next_stance(Prone, StanceInput::Jump, true, all), Some(Standing));
        assert_eq!(next_stance(Prone, StanceInput::Jump, true, low_ceiling), Some(Crouching));
        assert_eq!(next_stance(Prone, StanceInput::Jump, true, crawlspace), None);
        assert_eq!(next_stance(Crouching, StanceInput::Crouch, true, low_ceiling), None);
        assert_eq!(next_stance(Prone, StanceInput::SprintForward, true, low_ceiling), None);
    }

    #[test]
    fn going_prone_and_sprinting_up_need_ground() {
        use Stance::*;
        let all = |_| true;
        assert_eq!(next_stance(Standing, StanceInput::Prone, false, all), None);
        assert_eq!(next_stance(Crouching, StanceInput::Prone, true, all), Some(Prone));
        assert_eq!(next_stance(Prone, StanceInput::SprintForward, false, all), None);
        assert_eq!(next_stance(Prone, StanceInput::SprintForward, true, all), Some(Standing));
    }

    #[test]
    fn full_stamina_lasts_sprint_stamina_seconds() {
        let mut motor = Motor { grounded: true, ..default() };
        let dt = 0.5;
        for _ in 0..(SPRINT_STAMINA / dt) as usize {
            motor.tick_sprint(true, dt);
            assert!(motor.sprinting);
        }
        assert_eq!(motor.stamina, 0.0);
        motor.tick_sprint(true, dt);
        assert!(!motor.sprinting);
    }

    #[test]
    fn sprint_restarts_only_after_restart_threshold_regained() {
        let mut motor = Motor { grounded: true, stamina: 0.0, ..default() };
        let dt = SPRINT_RESTART_STAMINA / STAMINA_REGEN / 2.0;
        motor.tick_sprint(true, dt);
        assert!(!motor.sprinting);
        motor.tick_sprint(true, dt);
        assert!(!motor.sprinting);
        assert_eq!(motor.stamina, SPRINT_RESTART_STAMINA);
        motor.tick_sprint(true, dt);
        assert!(motor.sprinting);

        // Below the threshold an ongoing sprint continues, but a jump cannot start one.
        let mut sprinting = Motor { grounded: true, sprinting: true, stamina: 0.5, ..default() };
        sprinting.tick_sprint(true, 0.1);
        assert!(sprinting.sprinting);
        let mut airborne = Motor::default();
        airborne.tick_sprint(true, 0.1);
        assert!(!airborne.sprinting);
    }
}
