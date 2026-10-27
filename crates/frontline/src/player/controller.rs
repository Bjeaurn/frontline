//! First-person kinematic controller for the `LocalPlayer`: mouse look, stances, sprint,
//! move-and-slide against the loaded Map, landing slowdown, mantling, and server-ordered
//! respawns. Movement tuning comes from `GameData::movement` (`data/movement.ron`).

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::hash::{BuildHasher, RandomState};

use avian3d::prelude::*;
use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};

use super::BODY_RADIUS;
use super::input::{Binding, Bindings};
use super::life::LocalLife;
use crate::data::{GameData, MovementTuning};
use crate::flow::{AppState, Settings};
use crate::maps::MapSpawns;
use crate::net::protocol::Respawn;
use crate::shared::{FromServer, GameLayer, LocalPlayer, PlayerName, PlayerPose, Stance};

// Collision geometry (metres).
/// Surfaces whose normal has at least this much `y` count as ground (about 50 degrees).
const MIN_GROUND_NORMAL_Y: f32 = 0.64;
/// Downward probe distance that counts as standing on something.
const GROUND_PROBE: f32 = 0.05;
/// While grounded, the body is pulled down this far to follow ramps and small drops.
const GROUND_SNAP: f32 = 0.35;
/// Falling below this height respawns the player.
const KILL_Y: f32 = -20.0;
/// A mantle ends with the body centre this far past the wall face, so thin walls hold it too.
const MANTLE_LEDGE_INSET: f32 = 0.2;
/// Ledges less than this above the feet are not mantled.
const MANTLE_MIN_RISE: f32 = 0.05;
/// The ledge probe starts this far above the highest mantleable height; starting inside
/// geometry there means the wall is too tall.
const LEDGE_PROBE_MARGIN: f32 = 0.05;
/// Mantle path shape, as fractions of its duration: the rise ends at `MANTLE_RISE_END` and the
/// move over the ledge starts at `MANTLE_OVER_START`.
const MANTLE_RISE_END: f32 = 0.6;
const MANTLE_OVER_START: f32 = 0.35;

// View tuning.
/// Furthest the view may pitch up or down, in radians.
pub const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.01;
/// How fast the eye height follows stance changes (exponential decay rate, 1/s).
const EYE_HEIGHT_RATE: f32 = 12.0;
/// Camera dip (metres) and roll (radians) at the middle of a mantle.
const MANTLE_VIEW_DIP: f32 = 0.08;
const MANTLE_VIEW_ROLL: f32 = 0.06;

/// Clearance tests for stance changes use a slightly shrunken body lifted off the floor, so
/// merely touching the floor or a wall does not count as blocked.
const HEADROOM_SHRINK: f32 = 0.03;
const HEADROOM_LIFT: f32 = 0.02;

/// Movement state of the local player, beyond what `PlayerPose` carries.
#[derive(Component, Debug)]
pub struct Motor {
    velocity: Vec3,
    grounded: bool,
    /// Feet height when last grounded (infinite before the first touchdown). Mid-jump, mantle
    /// height is measured from here.
    ground_y: f32,
    stamina: f32,
    sprinting: bool,
    jump_requested: bool,
    /// Seconds since the latest touchdown.
    since_landing: f32,
    mantle: Option<Mantle>,
}

impl Motor {
    fn new(tuning: &MovementTuning) -> Self {
        Self {
            velocity: Vec3::ZERO,
            grounded: false,
            ground_y: f32::INFINITY,
            stamina: tuning.sprint_stamina_secs,
            sprinting: false,
            jump_requested: false,
            since_landing: f32::INFINITY,
            mantle: None,
        }
    }

    /// Advances sprint state and stamina by `dt`: sprint drains, rest regenerates, and once the
    /// tank runs dry sprint restarts only from the ground with `sprint_restart_stamina` back.
    fn tick_sprint(&mut self, wants_sprint: bool, dt: f32, tuning: &MovementTuning) {
        if self.sprinting {
            self.sprinting = wants_sprint && self.stamina > 0.0;
        } else {
            self.sprinting =
                wants_sprint && self.grounded && self.stamina >= tuning.sprint_restart_stamina;
        }
        self.stamina = if self.sprinting {
            (self.stamina - dt).max(0.0)
        } else {
            (self.stamina + tuning.stamina_regen_per_sec * dt).min(tuning.sprint_stamina_secs)
        };
    }

    /// Speed multiplier of the post-landing slowdown: `landing_slow_scale` at touchdown, back
    /// to 1 linearly over `landing_slow_secs`.
    fn landing_scale(&self, tuning: &MovementTuning) -> f32 {
        if self.since_landing >= tuning.landing_slow_secs {
            return 1.0;
        }
        let recovered = self.since_landing / tuning.landing_slow_secs;
        tuning.landing_slow_scale + (1.0 - tuning.landing_slow_scale) * recovered
    }

    pub fn velocity(&self) -> Vec3 {
        self.velocity
    }

    pub fn grounded(&self) -> bool {
        self.grounded
    }

    pub fn sprinting(&self) -> bool {
        self.sprinting
    }

    /// A mantle is in progress: input is ignored, the stance is standing, and the weapon
    /// should be lowered.
    pub fn mantling(&self) -> bool {
        self.mantle.is_some()
    }
}

/// An ongoing mantle: the feet travel from `from` up to the ledge height, then over it to `to`.
#[derive(Clone, Copy, Debug)]
struct Mantle {
    from: Vec3,
    to: Vec3,
    elapsed: f32,
    secs: f32,
}

impl Mantle {
    /// Fraction done, 0 to 1.
    fn progress(&self) -> f32 {
        if self.secs > 0.0 {
            (self.elapsed / self.secs).min(1.0)
        } else {
            1.0
        }
    }

    /// Feet position at `progress`: a quick rise that eases out, overlapped by a smooth move
    /// over the ledge once the body is mostly up.
    fn feet(&self, progress: f32) -> Vec3 {
        let rise = 1.0 - (1.0 - (progress / MANTLE_RISE_END).min(1.0)).powi(3);
        let over = ((progress - MANTLE_OVER_START) / (1.0 - MANTLE_OVER_START)).clamp(0.0, 1.0);
        let over = over * over * (3.0 - 2.0 * over);
        let horizontal = self.from.lerp(self.to, over);
        Vec3::new(
            horizontal.x,
            self.from.y + (self.to.y - self.from.y) * rise,
            horizontal.z,
        )
    }
}

/// Hooks the weapon uses to adjust the controller, e.g. while aiming down sights.
/// Lives on the `LocalPlayer`.
#[derive(Component, Debug)]
pub struct ControllerModifiers {
    /// Multiplies movement speed.
    pub move_scale: f32,
    /// Multiplies the horizontal field of view.
    pub fov_scale: f32,
    /// Sprinting is not allowed (and an ongoing sprint stops).
    pub sprint_blocked: bool,
}

impl Default for ControllerModifiers {
    fn default() -> Self {
        Self {
            move_scale: 1.0,
            fov_scale: 1.0,
            sprint_blocked: false,
        }
    }
}

/// The first-person camera, a child of the local player; `height` is the smoothed eye height.
#[derive(Component, Debug)]
pub struct EyeCamera {
    height: f32,
}

impl EyeCamera {
    /// Current (smoothed) eye height above the feet, in metres.
    pub fn height(&self) -> f32 {
        self.height
    }
}

/// Whether the cursor is grabbed for mouse look. Input is ignored while it isn't.
#[derive(Resource, Default, Debug)]
pub struct MouseCaptured(bool);

impl MouseCaptured {
    pub fn active(&self) -> bool {
        self.0
    }
}

/// Whether a menu or overlay owns the cursor. While set, the cursor stays free and clicks
/// don't grab it; when it clears, the cursor is grabbed again.
#[derive(Resource, Default, Debug)]
pub struct MenuOpen(pub bool);

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

/// An upright cylinder of the stance's full body height. Its flat base rests level on ledges
/// and box edges; a capsule's rounded base meets an edge at a slant, which move-and-slide turns
/// into a pop upwards or a slide off the edge.
fn body_collider(stance: Stance, shrink: f32) -> Collider {
    Collider::cylinder(BODY_RADIUS - shrink, stance.body_height() - 2.0 * shrink)
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

    /// Centre of the headroom-test body in `stance` with its feet at `feet`.
    fn headroom_center(stance: Stance, feet: Vec3) -> Vec3 {
        feet + Vec3::Y * (stance.body_height() / 2.0 + HEADROOM_LIFT)
    }

    /// Whether a body in `stance` with its feet at `feet` overlaps no static geometry.
    fn fits(&self, spatial: &SpatialQuery, stance: Stance, feet: Vec3) -> bool {
        let mut blocked = false;
        spatial.shape_intersections_callback(
            &self.headroom[stance_index(stance)],
            Self::headroom_center(stance, feet),
            Quat::IDENTITY,
            &SpatialQueryFilter::from_mask(GameLayer::World),
            |_| {
                blocked = true;
                false
            },
        );
        !blocked
    }

    /// First static geometry hit by the headroom-test body in `stance`, feet at `feet`, swept
    /// `distance` along `direction`.
    fn sweep(
        &self,
        spatial: &SpatialQuery,
        stance: Stance,
        feet: Vec3,
        direction: Dir3,
        distance: f32,
    ) -> Option<ShapeHitData> {
        spatial.cast_shape(
            &self.headroom[stance_index(stance)],
            Self::headroom_center(stance, feet),
            Quat::IDENTITY,
            direction,
            &ShapeCastConfig::from_max_distance(distance),
            &SpatialQueryFilter::from_mask(GameLayer::World),
        )
    }
}

/// Where a mantle from `feet` towards `facing` ends, if a ledge is within reach: a walkable top
/// at most `mantle_max_height` above `base_y`, with standing room up there and nothing in the
/// way from straight above the feet across to it.
fn find_ledge(
    shapes: &StanceShapes,
    spatial: &SpatialQuery,
    feet: Vec3,
    facing: Dir3,
    base_y: f32,
    tuning: &MovementTuning,
) -> Option<Vec3> {
    let stance = Stance::Standing;
    let wall = shapes.sweep(spatial, stance, feet, facing, tuning.mantle_reach)?;
    if wall.distance <= 0.0 || wall.normal1.y >= MIN_GROUND_NORMAL_Y {
        return None;
    }
    let into_wall = Dir3::new(Vec3::new(-wall.normal1.x, 0.0, -wall.normal1.z)).ok()?;
    let over = feet + facing * wall.distance + into_wall * (BODY_RADIUS + MANTLE_LEDGE_INSET);

    let highest = base_y + tuning.mantle_max_height;
    let probe = Vec3::new(over.x, highest + LEDGE_PROBE_MARGIN, over.z);
    let lowest = feet.y + MANTLE_MIN_RISE;
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let top = spatial.cast_ray(probe, Dir3::NEG_Y, probe.y - lowest, true, &filter)?;
    let top_y = probe.y - top.distance;
    // A zero distance means the probe started inside the wall: it is too tall.
    if top.distance <= 0.0 || top.normal.y < MIN_GROUND_NORMAL_Y || top_y > highest {
        return None;
    }

    let ledge = Vec3::new(over.x, top_y, over.z);
    let raised = Vec3::new(feet.x, top_y, feet.z);
    if !shapes.fits(spatial, stance, raised) || !shapes.fits(spatial, stance, ledge) {
        return None;
    }
    if let Ok((across, distance)) = Dir3::new_and_length(ledge - raised)
        && shapes.sweep(spatial, stance, raised, across, distance).is_some()
    {
        return None;
    }
    Some(ledge + Vec3::Y * HEADROOM_LIFT)
}

/// Feet position and yaw of a random spawn point of the loaded Map (any Team's); the origin
/// before a Map is ready.
fn random_spawn(spawns: Option<&MapSpawns>) -> (Vec3, f32) {
    let spawns = spawns.map_or(&[][..], |spawns| &spawns.0[..]);
    let index = RandomState::new().hash_one(0_u8) as usize % spawns.len().max(1);
    spawns
        .get(index)
        .map_or((Vec3::ZERO, 0.0), |spawn| (spawn.position, spawn.yaw))
}

pub(super) fn spawn_local_player(
    mut commands: Commands,
    data: Res<GameData>,
    spawns: Option<Res<MapSpawns>>,
    settings: Res<Settings>,
) {
    let name = settings.name.clone();
    let (feet, yaw) = random_spawn(spawns.as_deref());
    let feet = feet + Vec3::Y * HEADROOM_LIFT;
    let stance = Stance::Standing;
    commands.spawn((
        Name::new("LocalPlayer"),
        DespawnOnExit(AppState::InGame),
        LocalPlayer,
        PlayerName(name),
        PlayerPose {
            position: feet,
            yaw,
            pitch: 0.0,
            stance,
        },
        Motor::new(&data.movement),
        ControllerModifiers::default(),
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

/// Grabs the cursor when a menu closes or on click; frees it while a menu is open or the window
/// is unfocused.
pub(super) fn grab_cursor(
    bindings: Res<Bindings>,
    menu: Res<MenuOpen>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut captured: ResMut<MouseCaptured>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let capture = if menu.0 || !window.focused {
        false
    } else if menu.is_changed() || mouse.just_pressed(bindings.grab_cursor) {
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

/// Applies the server's `Respawn`: teleport, face its yaw, level the view, stop moving.
pub(super) fn apply_respawn(
    data: Res<GameData>,
    mut respawns: MessageReader<FromServer<Respawn>>,
    mut player: Query<(&mut PlayerPose, &mut Motor, &mut Transform), With<LocalPlayer>>,
) {
    let Some(FromServer(respawn)) = respawns.read().last() else {
        return;
    };
    let Ok((mut pose, mut motor, mut transform)) = player.single_mut() else {
        return;
    };
    info!("Respawning at {}", respawn.position);
    *pose = PlayerPose {
        position: respawn.position + Vec3::Y * HEADROOM_LIFT,
        yaw: respawn.yaw,
        pitch: 0.0,
        stance: Stance::Standing,
    };
    *motor = Motor::new(&data.movement);
    *transform = Transform::from_translation(pose.position)
        .with_rotation(Quat::from_rotation_y(pose.yaw));
}

pub(super) fn mouse_look(
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    motion: Res<AccumulatedMouseMotion>,
    settings: Res<Settings>,
    mut player: Query<&mut PlayerPose, With<LocalPlayer>>,
) {
    if !captured.0 || life.is_dead() || motion.delta == Vec2::ZERO {
        return;
    }
    let Ok(mut pose) = player.single_mut() else {
        return;
    };
    let delta = motion.delta * settings.mouse_sensitivity;
    pose.yaw = (pose.yaw - delta.x + PI).rem_euclid(TAU) - PI;
    pose.pitch = (pose.pitch - delta.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
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
/// Ignored while mantling.
pub(super) fn change_stance(
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
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
    if !captured.0 || life.is_dead() || motor.mantling() {
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

/// Walks, jumps and falls, or advances a mantle. Holding forward into a ledge mantles it: from
/// the air on contact, from the ground with a jump.
#[allow(clippy::too_many_arguments)]
pub(super) fn move_player(
    time: Res<Time>,
    data: Res<GameData>,
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    shapes: Res<StanceShapes>,
    spawns: Option<Res<MapSpawns>>,
    move_and_slide: MoveAndSlide,
    mut player: Query<
        (
            &mut PlayerPose,
            &mut Motor,
            &mut Transform,
            &ControllerModifiers,
        ),
        With<LocalPlayer>,
    >,
) {
    let dt = time.delta_secs();
    let Ok((mut pose, mut motor, mut transform, modifiers)) = player.single_mut() else {
        return;
    };
    if dt <= 0.0 {
        return;
    }
    let tuning = &data.movement;

    let active = captured.0 && !life.is_dead() && !motor.mantling();
    let pressed = |binding: &Binding| active && binding.pressed(&keys);
    let (forward, back) = (pressed(&bindings.forward), pressed(&bindings.back));
    let input = Vec2::new(
        axis(pressed(&bindings.left), pressed(&bindings.right)),
        axis(back, forward),
    );
    let facing = Quat::from_rotation_y(pose.yaw);

    if forward
        && !back
        && (!motor.grounded || motor.jump_requested)
        && let Some(to) = find_ledge(
            &shapes,
            &move_and_slide.spatial_query,
            pose.position,
            Dir3::new_unchecked(facing * Vec3::NEG_Z),
            motor.ground_y.min(pose.position.y),
            tuning,
        )
    {
        motor.mantle = Some(Mantle {
            from: pose.position,
            to,
            elapsed: 0.0,
            secs: tuning.mantle_secs,
        });
        motor.velocity = Vec3::ZERO;
        motor.sprinting = false;
    }

    if let Some(mantle) = &mut motor.mantle {
        mantle.elapsed += dt;
        let progress = mantle.progress();
        pose.position = mantle.feet(progress);
        pose.stance = Stance::Standing;
        if progress >= 1.0 {
            motor.mantle = None;
            motor.grounded = true;
            motor.ground_y = pose.position.y;
        }
        motor.tick_sprint(false, dt, tuning);
    } else {
        // Sprint: forward only, standing only, limited by stamina.
        let wants_sprint = pressed(&bindings.sprint)
            && forward
            && !back
            && pose.stance == Stance::Standing
            && !modifiers.sprint_blocked;
        motor.tick_sprint(wants_sprint, dt, tuning);

        let speed = match pose.stance {
            Stance::Standing if motor.sprinting => tuning.sprint_speed,
            Stance::Standing => tuning.walk_speed,
            Stance::Crouching => tuning.crouch_speed,
            Stance::Prone => tuning.prone_speed,
        } * modifiers.move_scale
            * motor.landing_scale(tuning);
        let wish = facing * Vec3::new(input.x, 0.0, -input.y).normalize_or_zero() * speed;
        slide(
            &mut pose,
            &mut motor,
            wish,
            &time,
            &shapes,
            &move_and_slide,
            tuning,
        );
    }

    if pose.position.y < KILL_Y {
        pose.position = random_spawn(spawns.as_deref()).0 + Vec3::Y * HEADROOM_LIFT;
        *motor = Motor::new(tuning);
    }
    *transform = Transform::from_translation(pose.position)
        .with_rotation(Quat::from_rotation_y(pose.yaw));
}

/// One frame of walking, jumping or falling towards horizontal velocity `wish`, sliding along
/// the Map.
fn slide(
    pose: &mut PlayerPose,
    motor: &mut Motor,
    wish: Vec3,
    time: &Time,
    shapes: &StanceShapes,
    move_and_slide: &MoveAndSlide,
    tuning: &MovementTuning,
) {
    let dt = time.delta_secs();
    // Horizontal: snappy on the ground, limited steering in the air, momentum kept when idle.
    let horizontal = Vec3::new(motor.velocity.x, 0.0, motor.velocity.z);
    let horizontal = if motor.grounded {
        horizontal.move_towards(wish, tuning.ground_accel * dt)
    } else if wish != Vec3::ZERO {
        horizontal.move_towards(wish, tuning.air_accel * dt)
    } else {
        horizontal
    };

    let jumped = motor.grounded && motor.jump_requested;
    let vertical = if jumped {
        (2.0 * tuning.gravity * tuning.jump_height).sqrt()
    } else if motor.grounded {
        0.0
    } else {
        (motor.velocity.y - tuning.gravity * dt).max(-tuning.terminal_fall_speed)
    };

    let shape = shapes.body(pose.stance);
    let half_height = pose.stance.body_height() / 2.0;
    let config = MoveAndSlideConfig::default();
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
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
    let landed = !motor.grounded && ground.is_some();
    motor.grounded = ground.is_some();
    if let Some(hit) = ground {
        center.y -= (hit.distance - config.skin_width).max(0.0);
        velocity.y = 0.0;
    }
    // Touchdown bleeds the speed carried through the air, then `landing_scale` eases back.
    if landed {
        motor.since_landing = 0.0;
        velocity.x *= tuning.landing_slow_scale;
        velocity.z *= tuning.landing_slow_scale;
    } else {
        motor.since_landing += dt;
    }

    pose.position = center - Vec3::Y * half_height;
    if motor.grounded {
        motor.ground_y = pose.position.y;
    }
    motor.velocity = velocity;
}

/// Places the camera at the (smoothed) eye height and applies pitch, plus a dip and roll while
/// mantling.
pub(super) fn update_eye(
    time: Res<Time>,
    player: Query<(&PlayerPose, &Motor), With<LocalPlayer>>,
    mut camera: Query<(&mut EyeCamera, &mut Transform)>,
) {
    let Ok((pose, motor)) = player.single() else {
        return;
    };
    let mantle = motor
        .mantle
        .map_or(0.0, |mantle| (PI * mantle.progress()).sin());
    for (mut eye, mut transform) in &mut camera {
        eye.height
            .smooth_nudge(&pose.stance.eye_height(), EYE_HEIGHT_RATE, time.delta_secs());
        transform.translation = Vec3::Y * (eye.height - MANTLE_VIEW_DIP * mantle);
        transform.rotation =
            Quat::from_rotation_x(pose.pitch) * Quat::from_rotation_z(MANTLE_VIEW_ROLL * mantle);
    }
}

/// Keeps the horizontal field of view at the Settings FOV (times the weapon's `fov_scale`)
/// whatever the window shape.
pub(super) fn apply_horizontal_fov(
    settings: Res<Settings>,
    player: Query<&ControllerModifiers, With<LocalPlayer>>,
    mut cameras: Query<&mut Projection, With<EyeCamera>>,
) {
    let scale = player.single().map_or(1.0, |modifiers| modifiers.fov_scale);
    for mut projection in &mut cameras {
        let Projection::Perspective(perspective) = &*projection else {
            continue;
        };
        let half = (settings.fov_deg * scale).to_radians() / 2.0;
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
        let tuning = MovementTuning::fixture();
        let mut motor = Motor { grounded: true, ..Motor::new(&tuning) };
        let dt = 0.5;
        for _ in 0..(tuning.sprint_stamina_secs / dt) as usize {
            motor.tick_sprint(true, dt, &tuning);
            assert!(motor.sprinting);
        }
        assert_eq!(motor.stamina, 0.0);
        motor.tick_sprint(true, dt, &tuning);
        assert!(!motor.sprinting);
    }

    #[test]
    fn sprint_restarts_only_after_restart_threshold_regained() {
        let tuning = MovementTuning::fixture();
        let mut motor = Motor { grounded: true, stamina: 0.0, ..Motor::new(&tuning) };
        let dt = tuning.sprint_restart_stamina / tuning.stamina_regen_per_sec / 2.0;
        motor.tick_sprint(true, dt, &tuning);
        assert!(!motor.sprinting);
        motor.tick_sprint(true, dt, &tuning);
        assert!(!motor.sprinting);
        assert_eq!(motor.stamina, tuning.sprint_restart_stamina);
        motor.tick_sprint(true, dt, &tuning);
        assert!(motor.sprinting);

        // Below the threshold an ongoing sprint continues, but a jump cannot start one.
        let mut sprinting =
            Motor { grounded: true, sprinting: true, stamina: 0.5, ..Motor::new(&tuning) };
        sprinting.tick_sprint(true, 0.1, &tuning);
        assert!(sprinting.sprinting);
        let mut airborne = Motor::new(&tuning);
        airborne.tick_sprint(true, 0.1, &tuning);
        assert!(!airborne.sprinting);
    }
}
