//! First-person kinematic controller for the `LocalPlayer`: mouse look, stances, sprint,
//! move-and-slide against the loaded Map, landing slowdown, mantling and vaulting, dive to prone, and
//! server-ordered respawns. Movement tuning comes from `GameData::movement`
//! (`data/movement.ron`).

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::hash::{BuildHasher, RandomState};
use std::time::Duration;

use avian3d::prelude::*;
use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};

use super::BODY_RADIUS;
use super::input::{Binding, Bindings};
use super::life::LocalLife;
use crate::data::{ActiveRules, GameData, MovementTuning};
use crate::flow::{AppState, Settings};
use crate::maps::MapSpawns;
use crate::net::protocol::{FallDamage, Respawn};
use crate::profile::Profile;
use crate::shared::{FromServer, GameLayer, LocalPlayer, PlayerName, PlayerPose, Stance, ToServer};

// Collision geometry (metres).
/// Surfaces whose normal has at least this much `y` count as ground (about 50 degrees).
const MIN_GROUND_NORMAL_Y: f32 = 0.64;
/// Downward probe distance that counts as standing on something.
const GROUND_PROBE: f32 = 0.05;
/// While grounded, the body is pulled down this far to follow ramps and small drops.
const GROUND_SNAP: f32 = 0.35;
/// Slopes whose normal has at least this much `y` (about 37 degrees) are walked at full
/// horizontal speed: stairs are ramps, and sliding along one would cost a quarter of the speed
/// every frame.
const STAIR_MIN_NORMAL_Y: f32 = 0.8;
/// The highest step the body climbs by walking into it. Anything taller is a jump or a mantle,
/// so waist-high cover stays cover (the map generators' `STEP` assumes 0.4 m).
const STEP_HEIGHT: f32 = 0.45;
/// A move that covered less than this share of the wanted distance counts as blocked, and
/// a step is tried.
const STEP_BLOCKED_FRACTION: f32 = 0.9;
/// A step is taken only if it gains at least this share of the wanted distance over the plain
/// slide, so walls and glancing slides never count.
const STEP_MIN_GAIN: f32 = 0.3;
/// How fast the camera catches up with a step (exponential decay rate, 1/s).
const STEP_VIEW_RATE: f32 = 14.0;
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
/// A vault probes the sill top and the wall's thickness this far in from its face and down from
/// its top, and lands this far clear of the wall's far face (body radius on top).
const VAULT_PROBE_DEPTH: f32 = 0.05;
const VAULT_LANDING_GAP: f32 = 0.1;
/// Vault path shape, as fractions of its duration: the feet reach the sill top at
/// `VAULT_APEX` and the move across starts at `VAULT_OVER_START`.
const VAULT_APEX: f32 = 0.45;
const VAULT_OVER_START: f32 = 0.2;

// View tuning.
/// Furthest the view may pitch up or down, in radians.
pub const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.01;
/// How fast the eye height follows stance changes (exponential decay rate, 1/s).
const EYE_HEIGHT_RATE: f32 = 12.0;
/// Eye height follows the stance change of a dive at this slower rate (1/s), so the view drops
/// with the body instead of snapping.
const DIVE_EYE_HEIGHT_RATE: f32 = 7.0;
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
    /// Seconds of sprint a full tank holds.
    tank: f32,
    sprinting: bool,
    jump_requested: bool,
    /// Seconds since the latest touchdown.
    since_landing: f32,
    mantle: Option<Mantle>,
    dive: Option<Dive>,
    /// Seconds left after a dive's recovery during which stance changes stay blocked.
    getup: f32,
    /// Normal of the ground the body stands on (up when airborne).
    ground_normal: Vec3,
    /// Highest feet height since the body last stood on something: a fall is measured from it.
    peak_y: f32,
    /// How far below the body the camera still sits after a step up (metres); it eases to 0.
    view_lag: f32,
}

impl Motor {
    fn new(tuning: &MovementTuning) -> Self {
        Self {
            velocity: Vec3::ZERO,
            grounded: false,
            ground_y: f32::INFINITY,
            stamina: tuning.sprint_stamina_secs,
            tank: tuning.sprint_stamina_secs,
            sprinting: false,
            jump_requested: false,
            since_landing: f32::INFINITY,
            mantle: None,
            dive: None,
            getup: 0.0,
            ground_normal: Vec3::Y,
            peak_y: f32::NEG_INFINITY,
            view_lag: 0.0,
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
            (self.stamina + tuning.stamina_regen_per_sec * dt).min(self.tank)
        };
    }

    /// Resizes the stamina tank to `tank` seconds (a Perk can change it), keeping it as full as
    /// it was.
    fn set_tank(&mut self, tank: f32) {
        if tank != self.tank {
            self.stamina = self.stamina / self.tank.max(f32::EPSILON) * tank;
            self.tank = tank;
        }
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

    /// A dive is in progress (lunge or recovery): movement, firing and stance changes are
    /// locked, the stance is prone, and the weapon should be lowered.
    pub fn diving(&self) -> bool {
        self.dive.is_some()
    }

    /// Starts a dive along the horizontal velocity (or `facing` when standing still): a lunge
    /// at `dive_speed` with a small hop.
    fn start_dive(&mut self, facing: Vec3, tuning: &MovementTuning) {
        let horizontal = Vec3::new(self.velocity.x, 0.0, self.velocity.z);
        let direction = horizontal.try_normalize().unwrap_or(facing);
        self.dive = Some(Dive {
            direction,
            elapsed: 0.0,
        });
        self.sprinting = false;
        self.grounded = false;
        let hop = (2.0 * tuning.gravity * tuning.dive_hop_height).sqrt();
        self.velocity = direction * tuning.dive_speed + Vec3::Y * hop;
    }
}

/// An ongoing mantle or vault: the feet travel from `from` up to the ledge height, then over it
/// to `to`. A vault (`crest` is the feet height over the sill) rises to the sill top and drops
/// again on the far side instead.
#[derive(Clone, Copy, Debug)]
struct Mantle {
    from: Vec3,
    to: Vec3,
    crest: Option<f32>,
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
        if let Some(crest) = self.crest {
            return self.vault_feet(crest, progress);
        }
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

    /// A vault's feet at `progress`: up to `crest` (ease out), then down to the landing (ease
    /// in), with the move across starting once the feet are mostly up.
    fn vault_feet(&self, crest: f32, progress: f32) -> Vec3 {
        let y = if progress < VAULT_APEX {
            let up = 1.0 - (1.0 - progress / VAULT_APEX).powi(3);
            self.from.y + (crest - self.from.y) * up
        } else {
            let down = (progress - VAULT_APEX) / (1.0 - VAULT_APEX);
            crest + (self.to.y - crest) * down * down
        };
        let over = ((progress - VAULT_OVER_START) / (1.0 - VAULT_OVER_START)).clamp(0.0, 1.0);
        let over = over * over * (3.0 - 2.0 * over);
        let horizontal = self.from.lerp(self.to, over);
        Vec3::new(horizontal.x, y, horizontal.z)
    }
}

/// An ongoing dive to prone: a lunge along `direction` (unit, horizontal), then a recovery.
#[derive(Clone, Copy, Debug)]
struct Dive {
    direction: Vec3,
    elapsed: f32,
}

impl Dive {
    /// Advances by `dt`: the horizontal velocity to move at (zero in the recovery) and whether
    /// the recovery is over.
    fn advance(&mut self, dt: f32, tuning: &MovementTuning) -> (Vec3, bool) {
        self.elapsed += dt;
        let wish = if self.elapsed < tuning.dive_secs {
            self.direction * tuning.dive_speed
        } else {
            Vec3::ZERO
        };
        (
            wish,
            self.elapsed >= tuning.dive_secs + tuning.dive_recover_secs,
        )
    }
}

/// Hooks the weapon uses to adjust the controller, e.g. while aiming down sights.
/// Lives on the `LocalPlayer`.
#[derive(Component, Debug)]
pub struct ControllerModifiers {
    /// Multiplies movement speed.
    pub move_scale: f32,
    /// How far into aiming down sights the weapon is, 0..1 (eased); blends the ADS mouse
    /// sensitivity multiplier in.
    pub ads: f32,
    /// `ads` while the sight in use is magnified (a scope), else 0: blends the scope mouse
    /// sensitivity multiplier in.
    pub scope: f32,
    /// Multiplies the horizontal field of view.
    pub fov_scale: f32,
    /// Multiplies the sprint stamina tank.
    pub stamina_scale: f32,
    /// Sprinting is not allowed (and an ongoing sprint stops).
    pub sprint_blocked: bool,
    /// Movement is locked (planting or defusing the bomb).
    pub frozen: bool,
    /// Multiplies movement speed on top of `move_scale`: what a stun or gas takes off
    /// (`equipment::client`).
    pub status_move: f32,
    /// How far the view is shaken (pitch, yaw, roll in radians; a stun, a cough). The camera
    /// shows it; where shots go is not touched.
    pub view_shake: Vec3,
}

impl Default for ControllerModifiers {
    fn default() -> Self {
        Self {
            move_scale: 1.0,
            ads: 0.0,
            scope: 0.0,
            fov_scale: 1.0,
            stamina_scale: 1.0,
            sprint_blocked: false,
            frozen: false,
            status_move: 1.0,
            view_shake: Vec3::ZERO,
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
        && shapes
            .sweep(spatial, stance, raised, across, distance)
            .is_some()
    {
        return None;
    }
    Some(ledge + Vec3::Y * HEADROOM_LIFT)
}

/// Where a vault from `feet` towards `facing` ends, if a thin wall or window sill is within
/// reach: its top `vault_min_height` to `vault_max_height` above `base_y`, at most
/// `vault_max_thickness` thick, with room to cross it crouched and a floor to land on behind
/// it (no more than a step higher than the feet, at most `vault_max_drop` lower). Gives the
/// landing feet position and the feet height over the sill.
fn find_vault(
    shapes: &StanceShapes,
    spatial: &SpatialQuery,
    feet: Vec3,
    facing: Dir3,
    base_y: f32,
    tuning: &MovementTuning,
) -> Option<(Vec3, f32)> {
    let wall = shapes.sweep(spatial, Stance::Standing, feet, facing, tuning.mantle_reach)?;
    if wall.distance <= 0.0 || wall.normal1.y >= MIN_GROUND_NORMAL_Y {
        return None;
    }
    let into_wall = Dir3::new(Vec3::new(-wall.normal1.x, 0.0, -wall.normal1.z)).ok()?;
    let face = feet + facing * wall.distance + into_wall * (BODY_RADIUS - HEADROOM_SHRINK);
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);

    // The sill top, just inside the wall face. A zero distance means the probe started inside
    // the wall: it is too tall.
    let highest = base_y + tuning.vault_max_height;
    let probe = face + into_wall * VAULT_PROBE_DEPTH;
    let probe = Vec3::new(probe.x, highest + LEDGE_PROBE_MARGIN, probe.z);
    let reach = probe.y - (feet.y + MANTLE_MIN_RISE);
    if reach <= 0.0 {
        return None;
    }
    let top = spatial.cast_ray(probe, Dir3::NEG_Y, reach, true, &filter)?;
    let crest = probe.y - top.distance;
    if top.distance <= 0.0
        || top.normal.y < MIN_GROUND_NORMAL_Y
        || crest > highest
        || crest < base_y + tuning.vault_min_height
    {
        return None;
    }

    // Thickness: from beyond the thickest vaultable wall, back towards it. It hits the far
    // face, or starts inside the wall when the wall is too thick.
    let beyond = face + into_wall * tuning.vault_max_thickness;
    let beyond = Vec3::new(beyond.x, crest - VAULT_PROBE_DEPTH, beyond.z);
    let far_face = spatial.cast_ray(
        beyond,
        -into_wall,
        tuning.vault_max_thickness,
        true,
        &filter,
    )?;
    if far_face.distance <= 0.0 {
        return None;
    }
    let thickness = tuning.vault_max_thickness - far_face.distance;

    // The floor behind: no higher than a step, no deeper than the drop allowed.
    let over = face + into_wall * (thickness + BODY_RADIUS + VAULT_LANDING_GAP);
    let drop = Vec3::new(over.x, crest + HEADROOM_LIFT, over.z);
    let floor = spatial.cast_ray(
        drop,
        Dir3::NEG_Y,
        drop.y - (feet.y - tuning.vault_max_drop),
        true,
        &filter,
    )?;
    let floor_y = drop.y - floor.distance;
    if floor.distance <= 0.0
        || floor.normal.y < MIN_GROUND_NORMAL_Y
        || floor_y > feet.y + STEP_HEIGHT
    {
        return None;
    }

    // Crouched across the sill, then standing room to land in.
    let crouch = Stance::Crouching;
    let raised = Vec3::new(feet.x, crest, feet.z);
    let across = Vec3::new(over.x, crest, over.z);
    let landing = Vec3::new(over.x, floor_y, over.z);
    if !shapes.fits(spatial, crouch, raised)
        || !shapes.fits(spatial, crouch, across)
        || !shapes.fits(spatial, Stance::Standing, landing)
    {
        return None;
    }
    if let Ok((direction, distance)) = Dir3::new_and_length(across - raised)
        && shapes
            .sweep(spatial, crouch, raised, direction, distance)
            .is_some()
    {
        return None;
    }
    Some((landing + Vec3::Y * HEADROOM_LIFT, crest + HEADROOM_LIFT))
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
    profile: Res<Profile>,
) {
    let name = profile.name.clone();
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
            aim_magnification: 0.0,
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
        aim_magnification: 0.0,
    };
    *motor = Motor::new(&data.movement);
    *transform =
        Transform::from_translation(pose.position).with_rotation(Quat::from_rotation_y(pose.yaw));
}

/// Radians of view rotation `motion` (mouse counts) turns the view by: yaw then pitch, positive
/// to the right and down. `ads` (0..1) blends from the hip sensitivity to `ads_multiplier`
/// times it; `scoped` (0..1, the ADS blend through a magnified sight, else 0) blends in the
/// extra `scope_multiplier` on top; `invert_y` flips the pitch.
fn look_delta(
    motion: Vec2,
    sensitivity: f32,
    ads_multiplier: f32,
    ads: f32,
    scope_multiplier: f32,
    scoped: f32,
    invert_y: bool,
) -> Vec2 {
    let scale =
        sensitivity * 1.0_f32.lerp(ads_multiplier, ads) * 1.0_f32.lerp(scope_multiplier, scoped);
    let delta = motion * scale;
    if invert_y {
        Vec2::new(delta.x, -delta.y)
    } else {
        delta
    }
}

pub(super) fn mouse_look(
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    motion: Res<AccumulatedMouseMotion>,
    settings: Res<Settings>,
    mut player: Query<(&mut PlayerPose, &ControllerModifiers), With<LocalPlayer>>,
) {
    if !captured.0 || life.is_dead() || motion.delta == Vec2::ZERO {
        return;
    }
    let Ok((mut pose, modifiers)) = player.single_mut() else {
        return;
    };
    let delta = look_delta(
        motion.delta,
        settings.mouse_sensitivity,
        settings.ads_sensitivity,
        modifiers.ads,
        settings.scope_sensitivity,
        modifiers.scope,
        settings.invert_y,
    );
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
/// Prone while sprinting dives instead. Ignored while mantling, diving and getting up after a
/// dive.
pub(super) fn change_stance(
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    data: Res<GameData>,
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
    if !captured.0 || life.is_dead() || motor.mantling() || motor.diving() || motor.getup > 0.0 {
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

    if input == StanceInput::Prone
        && pose.stance == Stance::Standing
        && motor.sprinting
        && motor.grounded
    {
        let facing = Quat::from_rotation_y(pose.yaw) * Vec3::NEG_Z;
        motor.start_dive(facing, &data.movement);
        pose.stance = Stance::Prone;
        return;
    }

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

/// Walks, jumps and falls, or advances a mantle or vault. Holding forward into a thin wall
/// vaults it, into a ledge mantles it: from the air on contact, from the ground with a jump.
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
    rules: ActiveRules,
    mut falls: MessageWriter<ToServer<FallDamage>>,
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

    motor.getup = (motor.getup - dt).max(0.0);

    motor.set_tank(tuning.sprint_stamina_secs * modifiers.stamina_scale);

    let active =
        captured.0 && !life.is_dead() && !motor.mantling() && !motor.diving() && !modifiers.frozen;
    let pressed = |binding: &Binding| active && binding.pressed(&keys);
    let (forward, back) = (pressed(&bindings.forward), pressed(&bindings.back));
    let input = Vec2::new(
        axis(pressed(&bindings.left), pressed(&bindings.right)),
        axis(back, forward),
    );
    let facing = Quat::from_rotation_y(pose.yaw);

    let climb = if forward && !back && (!motor.grounded || motor.jump_requested) {
        let ahead = Dir3::new_unchecked(facing * Vec3::NEG_Z);
        let base_y = motor.ground_y.min(pose.position.y);
        let spatial = &move_and_slide.spatial_query;
        // A thin wall is vaulted; anything else, or a thin wall with no room behind, mantled.
        find_vault(&shapes, spatial, pose.position, ahead, base_y, tuning)
            .map(|(to, crest)| (to, Some(crest), tuning.vault_secs))
            .or_else(|| {
                find_ledge(&shapes, spatial, pose.position, ahead, base_y, tuning)
                    .map(|to| (to, None, tuning.mantle_secs))
            })
    } else {
        None
    };
    if let Some((to, crest, secs)) = climb {
        motor.mantle = Some(Mantle {
            from: pose.position,
            to,
            crest,
            elapsed: 0.0,
            secs,
        });
        motor.velocity = Vec3::ZERO;
        motor.sprinting = false;
    }

    let mut fall = None;
    if let Some(mantle) = &mut motor.mantle {
        mantle.elapsed += dt;
        let progress = mantle.progress();
        pose.position = mantle.feet(progress);
        pose.stance = Stance::Standing;
        if progress >= 1.0 {
            motor.mantle = None;
            motor.grounded = true;
            motor.ground_y = pose.position.y;
            motor.peak_y = pose.position.y;
        }
        motor.tick_sprint(false, dt, tuning);
    } else if let Some(dive) = motor.dive.as_mut() {
        // Dive: the lunge keeps its momentum, the recovery stops dead; no input either way.
        let (wish, finished) = dive.advance(dt, tuning);
        fall = slide(
            &mut pose,
            &mut motor,
            wish,
            &time,
            &shapes,
            &move_and_slide,
            tuning,
        );
        if finished {
            motor.dive = None;
            motor.getup = tuning.dive_getup_secs;
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
            * modifiers.status_move
            * motor.landing_scale(tuning);
        let wish = facing * Vec3::new(input.x, 0.0, -input.y).normalize_or_zero() * speed;
        fall = slide(
            &mut pose,
            &mut motor,
            wish,
            &time,
            &shapes,
            &move_and_slide,
            tuning,
        );
    }

    // The server applies the Match Rules' fall damage to the height we report; only falls
    // that can hurt are worth a message.
    if let Some(height) = fall
        && !life.is_dead()
        && height > rules.get().fall_damage.min_height
    {
        info!("Landed after falling {height:.1} m");
        falls.write(ToServer(FallDamage { height }));
    }

    if pose.position.y < KILL_Y {
        pose.position = random_spawn(spawns.as_deref()).0 + Vec3::Y * HEADROOM_LIFT;
        *motor = Motor::new(tuning);
    }
    *transform =
        Transform::from_translation(pose.position).with_rotation(Quat::from_rotation_y(pose.yaw));
}

/// Vertical speed that keeps `horizontal` parallel to ground with `normal`, so a stair-like slope
/// is climbed (or descended) at the full horizontal speed. Steeper ground is left to the slide.
fn along_slope(horizontal: Vec3, normal: Vec3) -> f32 {
    if normal.y < STAIR_MIN_NORMAL_Y {
        return 0.0;
    }
    -(normal.x * horizontal.x + normal.z * horizontal.z) / normal.y
}

/// Tries to climb a step of up to `STEP_HEIGHT` in the way of a body at `start` (shape centre)
/// walking `horizontal` for `dt`, which the plain slide ended at `slid`. Lifts the body, walks
/// it forward and sets it down again; returns the new centre and the slide's remaining velocity
/// if that gets clearly further than the plain slide did and ends on walkable ground.
#[allow(clippy::too_many_arguments)]
fn step_up(
    move_and_slide: &MoveAndSlide,
    shape: &Collider,
    start: Vec3,
    slid: Vec3,
    horizontal: Vec3,
    dt: f32,
    config: &MoveAndSlideConfig,
    filter: &SpatialQueryFilter,
) -> Option<(Vec3, Vec3)> {
    let wanted = horizontal.length() * dt;
    let flat = |from: Vec3, to: Vec3| (to - from).xz().length();
    let done = flat(start, slid);
    if wanted < 1e-4 || done >= wanted * STEP_BLOCKED_FRACTION {
        return None;
    }
    let reach = STEP_HEIGHT + config.skin_width;
    let up = move_and_slide
        .spatial_query
        .cast_shape(
            shape,
            start,
            Quat::IDENTITY,
            Dir3::Y,
            &ShapeCastConfig::from_max_distance(reach),
            filter,
        )
        .map_or(STEP_HEIGHT, |hit| {
            (hit.distance - config.skin_width).max(0.0)
        });
    if up <= config.skin_width {
        return None;
    }
    let raised = start + Vec3::Y * up;
    let over = move_and_slide.move_and_slide(
        shape,
        raised,
        Quat::IDENTITY,
        Vec3::new(horizontal.x, 0.0, horizontal.z),
        Duration::from_secs_f32(dt),
        config,
        filter,
        |_| MoveAndSlideHitResponse::Accept,
    );
    if flat(raised, over.position) < done + STEP_MIN_GAIN * wanted {
        return None;
    }
    let down = move_and_slide
        .spatial_query
        .cast_shape(
            shape,
            over.position,
            Quat::IDENTITY,
            Dir3::NEG_Y,
            &ShapeCastConfig::from_max_distance(up + config.skin_width),
            filter,
        )
        .filter(|hit| hit.normal1.y >= MIN_GROUND_NORMAL_Y)?;
    let centre = over.position - Vec3::Y * hit_distance(&down, config);
    (centre.y > start.y + config.skin_width).then_some((centre, over.projected_velocity))
}

/// How far a ground cast travelled before touching, less the skin the body keeps off it.
fn hit_distance(hit: &ShapeHitData, config: &MoveAndSlideConfig) -> f32 {
    (hit.distance - config.skin_width).max(0.0)
}

/// One frame of walking, jumping or falling towards horizontal velocity `wish`, sliding along
/// the Map. Returns the height of the fall if the body touched down this frame, measured from
/// the peak of the fall.
fn slide(
    pose: &mut PlayerPose,
    motor: &mut Motor,
    wish: Vec3,
    time: &Time,
    shapes: &StanceShapes,
    move_and_slide: &MoveAndSlide,
    tuning: &MovementTuning,
) -> Option<f32> {
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
        along_slope(horizontal, motor.ground_normal)
    } else {
        (motor.velocity.y - tuning.gravity * dt).max(-tuning.terminal_fall_speed)
    };

    let shape = shapes.body(pose.stance);
    let half_height = pose.stance.body_height() / 2.0;
    let config = MoveAndSlideConfig::default();
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let start = pose.position + Vec3::Y * half_height;
    let out = move_and_slide.move_and_slide(
        shape,
        start,
        Quat::IDENTITY,
        horizontal + Vec3::Y * vertical,
        time.delta(),
        &config,
        &filter,
        |_| MoveAndSlideHitResponse::Accept,
    );
    let mut center = out.position;
    let mut velocity = out.projected_velocity;

    // Walking into a low step climbs it.
    if motor.grounded
        && !jumped
        && let Some((stepped, remaining)) = step_up(
            move_and_slide,
            shape,
            start,
            center,
            horizontal,
            dt,
            &config,
            &filter,
        )
    {
        motor.view_lag += stepped.y - start.y;
        center = stepped;
        velocity = remaining;
    }

    // Ground check; while walking, reach further down so ramps and small drops stay grounded.
    let ground = if (vertical > 0.0 && !motor.grounded) || jumped {
        None
    } else {
        let reach = if motor.grounded {
            GROUND_SNAP
        } else {
            GROUND_PROBE
        };
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
    motor.ground_normal = ground.map_or(Vec3::Y, |hit| hit.normal1);
    if let Some(hit) = ground {
        center.y -= hit_distance(&hit, &config);
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
    let fall = landed.then_some(motor.peak_y - pose.position.y);
    if motor.grounded {
        motor.ground_y = pose.position.y;
        motor.peak_y = pose.position.y;
    } else {
        motor.peak_y = motor.peak_y.max(pose.position.y);
    }
    motor.velocity = velocity;
    fall
}

/// Places the camera at the (smoothed) eye height and applies pitch, plus a dip and roll while
/// mantling.
pub(super) fn update_eye(
    time: Res<Time>,
    mut player: Query<(&PlayerPose, &mut Motor, &ControllerModifiers), With<LocalPlayer>>,
    mut camera: Query<(&mut EyeCamera, &mut Transform)>,
) {
    let Ok((pose, mut motor, modifiers)) = player.single_mut() else {
        return;
    };
    let mantle = motor
        .mantle
        .map_or(0.0, |mantle| (PI * mantle.progress()).sin());
    let rate = if motor.diving() {
        DIVE_EYE_HEIGHT_RATE
    } else {
        EYE_HEIGHT_RATE
    };
    motor
        .view_lag
        .smooth_nudge(&0.0, STEP_VIEW_RATE, time.delta_secs());
    let shake = modifiers.view_shake;
    for (mut eye, mut transform) in &mut camera {
        eye.height
            .smooth_nudge(&pose.stance.eye_height(), rate, time.delta_secs());
        transform.translation = Vec3::Y * (eye.height - MANTLE_VIEW_DIP * mantle - motor.view_lag);
        transform.rotation = Quat::from_rotation_y(shake.y)
            * Quat::from_rotation_x(pose.pitch + shake.x)
            * Quat::from_rotation_z(MANTLE_VIEW_ROLL * mantle + shake.z);
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
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn ads_blends_the_sensitivity_multiplier_in() {
        let motion = Vec2::new(10.0, 4.0);
        let close = |a: Vec2, b: Vec2| (a - b).abs().max_element() < 1e-6;
        assert!(close(
            look_delta(motion, 0.002, 0.5, 0.0, 1.0, 0.0, false),
            motion * 0.002
        ));
        assert!(close(
            look_delta(motion, 0.002, 0.5, 1.0, 1.0, 0.0, false),
            motion * 0.001
        ));
        assert!(close(
            look_delta(motion, 0.002, 0.5, 0.5, 1.0, 0.0, false),
            motion * 0.0015
        ));
    }

    #[test]
    fn invert_y_flips_only_the_pitch() {
        let delta = look_delta(Vec2::new(10.0, 4.0), 0.002, 1.0, 0.0, 1.0, 0.0, true);
        assert!((delta - Vec2::new(0.02, -0.008)).abs().max_element() < 1e-6);
    }

    #[test]
    fn the_scope_multiplier_stacks_on_the_ads_one_only_through_a_magnified_sight() {
        let motion = Vec2::new(10.0, 4.0);
        let close = |a: Vec2, b: Vec2| (a - b).abs().max_element() < 1e-6;
        // Aimed through a scope: 0.5 (ADS) * 0.4 (scope) of the hip.
        assert!(close(
            look_delta(motion, 0.002, 0.5, 1.0, 0.4, 1.0, false),
            motion * 0.002 * 0.5 * 0.4
        ));
        // Aimed through plain sights (`scoped` stays 0): the scope multiplier is not used.
        assert!(close(
            look_delta(motion, 0.002, 0.5, 1.0, 0.4, 0.0, false),
            motion * 0.001
        ));
        // Half into the scope: the blend is halfway.
        assert!(close(
            look_delta(motion, 0.002, 1.0, 0.5, 0.4, 0.5, false),
            motion * 0.002 * 0.7
        ));
    }

    #[test]
    fn standing_up_falls_back_to_the_tallest_stance_with_headroom() {
        use Stance::*;
        let all = |_| true;
        let low_ceiling = |stance| stance != Standing;
        let crawlspace = |stance| stance == Prone;

        assert_eq!(
            next_stance(Prone, StanceInput::Jump, true, all),
            Some(Standing)
        );
        assert_eq!(
            next_stance(Prone, StanceInput::Jump, true, low_ceiling),
            Some(Crouching)
        );
        assert_eq!(
            next_stance(Prone, StanceInput::Jump, true, crawlspace),
            None
        );
        assert_eq!(
            next_stance(Crouching, StanceInput::Crouch, true, low_ceiling),
            None
        );
        assert_eq!(
            next_stance(Prone, StanceInput::SprintForward, true, low_ceiling),
            None
        );
    }

    #[test]
    fn going_prone_and_sprinting_up_need_ground() {
        use Stance::*;
        let all = |_| true;
        assert_eq!(next_stance(Standing, StanceInput::Prone, false, all), None);
        assert_eq!(
            next_stance(Crouching, StanceInput::Prone, true, all),
            Some(Prone)
        );
        assert_eq!(
            next_stance(Prone, StanceInput::SprintForward, false, all),
            None
        );
        assert_eq!(
            next_stance(Prone, StanceInput::SprintForward, true, all),
            Some(Standing)
        );
    }

    #[test]
    fn full_stamina_lasts_sprint_stamina_seconds() {
        let tuning = MovementTuning::fixture();
        let mut motor = Motor {
            grounded: true,
            ..Motor::new(&tuning)
        };
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
    fn a_bigger_tank_lasts_proportionally_longer() {
        let tuning = MovementTuning::fixture();
        let mut motor = Motor {
            grounded: true,
            ..Motor::new(&tuning)
        };
        // Marathon-style: 2.5x the tank, and the fill fraction carries over.
        motor.set_tank(tuning.sprint_stamina_secs * 2.5);
        assert_eq!(motor.stamina, tuning.sprint_stamina_secs * 2.5);
        let dt = 0.5;
        let mut lasted = 0.0;
        loop {
            motor.tick_sprint(true, dt, &tuning);
            if !motor.sprinting {
                break;
            }
            lasted += dt;
        }
        assert!((lasted - tuning.sprint_stamina_secs * 2.5).abs() <= dt);
    }

    #[test]
    fn sprint_restarts_only_after_restart_threshold_regained() {
        let tuning = MovementTuning::fixture();
        let mut motor = Motor {
            grounded: true,
            stamina: 0.0,
            ..Motor::new(&tuning)
        };
        let dt = tuning.sprint_restart_stamina / tuning.stamina_regen_per_sec / 2.0;
        motor.tick_sprint(true, dt, &tuning);
        assert!(!motor.sprinting);
        motor.tick_sprint(true, dt, &tuning);
        assert!(!motor.sprinting);
        assert_eq!(motor.stamina, tuning.sprint_restart_stamina);
        motor.tick_sprint(true, dt, &tuning);
        assert!(motor.sprinting);

        // Below the threshold an ongoing sprint continues, but a jump cannot start one.
        let mut sprinting = Motor {
            grounded: true,
            sprinting: true,
            stamina: 0.5,
            ..Motor::new(&tuning)
        };
        sprinting.tick_sprint(true, 0.1, &tuning);
        assert!(sprinting.sprinting);
        let mut airborne = Motor::new(&tuning);
        airborne.tick_sprint(true, 0.1, &tuning);
        assert!(!airborne.sprinting);
    }

    #[test]
    fn dive_lunges_then_recovers_without_moving() {
        let tuning = MovementTuning::fixture();
        let mut dive = Dive {
            direction: Vec3::X,
            elapsed: 0.0,
        };
        let dt = 0.1;
        let mut lunge_secs = 0.0;
        loop {
            let (wish, finished) = dive.advance(dt, &tuning);
            if wish == Vec3::ZERO {
                break;
            }
            assert_eq!(wish, Vec3::X * tuning.dive_speed);
            assert!(!finished);
            lunge_secs += dt;
        }
        assert!((lunge_secs - tuning.dive_secs).abs() <= dt);
        let mut recover_secs = 0.0;
        loop {
            let (wish, finished) = dive.advance(dt, &tuning);
            assert_eq!(wish, Vec3::ZERO);
            recover_secs += dt;
            if finished {
                break;
            }
        }
        assert!((recover_secs - tuning.dive_recover_secs).abs() <= dt * 2.0);
    }

    #[test]
    fn dive_spam_is_slower_than_plain_sprinting() {
        let tuning = MovementTuning::fixture();
        let locked = tuning.dive_secs + tuning.dive_recover_secs + tuning.dive_getup_secs;
        // Over the time a dive locks you out, sprinting covers more ground than the lunge.
        assert!(tuning.sprint_speed * locked > tuning.dive_speed * tuning.dive_secs);
    }

    // ---- vault ----------------------------------------------------------------------------

    /// Static boxes as (min, max) corners on the World layer, in an app with physics.
    fn world(boxes: &[(Vec3, Vec3)]) -> App {
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, crate::world::WorldPlugin));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            20,
        )));
        // Plugins register part of their resources in `finish`, which `App::run` would call.
        app.finish();
        app.cleanup();
        for (min, max) in boxes {
            let size = max - min;
            app.world_mut().spawn((
                RigidBody::Static,
                Collider::cuboid(size.x, size.y, size.z),
                Transform::from_translation((min + max) / 2.0),
                CollisionLayers::new(GameLayer::World, LayerMask::ALL),
            ));
        }
        for _ in 0..4 {
            app.update();
        }
        app
    }

    /// A vault from the origin towards -z, where the floor is the top of the box under y = 0.
    fn vault(app: &mut App) -> Option<(Vec3, f32)> {
        app.world_mut()
            .run_system_once(|spatial: SpatialQuery| {
                find_vault(
                    &StanceShapes::default(),
                    &spatial,
                    Vec3::ZERO,
                    Dir3::NEG_Z,
                    0.0,
                    &MovementTuning::fixture(),
                )
            })
            .unwrap()
    }

    /// A wall across -z (its near face 0.4 m ahead of the player, `thick` deep, `sill` high)
    /// with a window above the sill up to `head` when given, over a floor that runs to
    /// `far_floor` (its top height) behind the wall.
    fn window_world(
        sill: f32,
        thick: f32,
        head: Option<f32>,
        far_floor: f32,
        extra: &[(Vec3, Vec3)],
    ) -> App {
        let near = -0.4;
        let far = near - thick;
        let mut boxes = vec![
            (Vec3::new(-5.0, -1.0, -0.5), Vec3::new(5.0, 0.0, 0.0)),
            (
                Vec3::new(-5.0, far_floor - 3.0, far - 5.0),
                Vec3::new(5.0, far_floor, far),
            ),
            (Vec3::new(-2.0, 0.0, far), Vec3::new(2.0, sill, near)),
        ];
        if let Some(head) = head {
            boxes.push((Vec3::new(-2.0, head, far), Vec3::new(2.0, head + 1.0, near)));
        }
        boxes.extend_from_slice(extra);
        world(&boxes)
    }

    #[test]
    fn a_window_sill_is_vaulted_to_the_floor_behind() {
        let mut app = window_world(0.9, 0.4, Some(2.3), 0.0, &[]);
        let (landing, crest) = vault(&mut app).expect("a 0.9 m sill in a 0.4 m wall");
        // Over the sill top, landing past the far face (-0.8) by the body radius and a gap.
        assert!((crest - (0.9 + HEADROOM_LIFT)).abs() < 1e-3, "{crest}");
        assert!(
            (landing.z - (-0.8 - BODY_RADIUS - VAULT_LANDING_GAP)).abs() < 0.02,
            "{landing}"
        );
        assert!((landing.y - HEADROOM_LIFT).abs() < 1e-3, "{landing}");
    }

    #[test]
    fn a_low_thin_wall_needs_no_window() {
        let mut app = window_world(1.0, 0.3, None, 0.0, &[]);
        assert!(vault(&mut app).is_some());
    }

    #[test]
    fn only_sills_between_the_min_and_max_height_are_vaulted() {
        let tuning = MovementTuning::fixture();
        // A step-high wall is walked, a chest-high one mantled; neither is vaulted.
        for (sill, vaulted) in [
            (0.4, false),
            (tuning.vault_min_height - 0.05, false),
            (tuning.vault_min_height + 0.05, true),
            (tuning.vault_max_height - 0.05, true),
            (tuning.vault_max_height + 0.1, false),
            (1.6, false),
        ] {
            let mut app = window_world(sill, 0.4, None, 0.0, &[]);
            assert_eq!(vault(&mut app).is_some(), vaulted, "sill {sill}");
        }
    }

    #[test]
    fn only_thin_walls_are_vaulted() {
        let tuning = MovementTuning::fixture();
        for (thick, vaulted) in [
            (0.1, true),
            (tuning.vault_max_thickness - 0.05, true),
            (tuning.vault_max_thickness + 0.1, false),
            (2.0, false),
        ] {
            let mut app = window_world(0.9, thick, None, 0.0, &[]);
            assert_eq!(vault(&mut app).is_some(), vaulted, "thickness {thick}");
        }
    }

    #[test]
    fn a_window_must_leave_room_to_cross_crouched() {
        let crouch = Stance::Crouching.body_height();
        let mut tight = window_world(0.9, 0.4, Some(0.9 + crouch - 0.1), 0.0, &[]);
        assert!(vault(&mut tight).is_none());
        let mut roomy = window_world(0.9, 0.4, Some(0.9 + crouch + 0.2), 0.0, &[]);
        assert!(vault(&mut roomy).is_some());
    }

    #[test]
    fn a_blocked_landing_is_not_vaulted() {
        // A crate right behind the wall, where the body would land.
        let landing_zone = (Vec3::new(-1.0, 0.0, -1.6), Vec3::new(1.0, 1.0, -0.9));
        let mut app = window_world(0.9, 0.4, Some(2.3), 0.0, &[landing_zone]);
        assert!(vault(&mut app).is_none());
        // The same crate well clear of it leaves the vault open.
        let clear = (Vec3::new(-1.0, 0.0, -3.0), Vec3::new(1.0, 1.0, -2.5));
        let mut app = window_world(0.9, 0.4, Some(2.3), 0.0, &[clear]);
        assert!(vault(&mut app).is_some());
    }

    #[test]
    fn the_floor_behind_must_be_within_a_step_up_and_the_max_drop_down() {
        let tuning = MovementTuning::fixture();
        for (far_floor, vaulted) in [
            (0.3, true),
            (STEP_HEIGHT + 0.2, false),
            (1.0, false),
            (-0.5, true),
            (-(tuning.vault_max_drop - 0.1), true),
            (-(tuning.vault_max_drop + 0.5), false),
        ] {
            let mut app = window_world(0.9, 0.4, Some(2.3), far_floor, &[]);
            assert_eq!(vault(&mut app).is_some(), vaulted, "far floor {far_floor}");
        }
    }

    #[test]
    fn a_parapet_over_a_deep_drop_is_not_vaulted() {
        // From the roof, the street behind the parapet is 3 m down: not a landing.
        let mut roof = window_world(0.9, 0.4, None, -3.0, &[]);
        assert!(vault(&mut roof).is_none());
    }

    #[test]
    fn a_wall_met_end_on_is_not_vaulted() {
        // Facing along the 4 m wall from its end: "thickness" far beyond a sill's.
        let mut app = window_world(0.9, 0.4, Some(2.3), 0.0, &[]);
        let found = app
            .world_mut()
            .run_system_once(|spatial: SpatialQuery| {
                find_vault(
                    &StanceShapes::default(),
                    &spatial,
                    Vec3::new(2.5, 0.0, -0.6),
                    Dir3::NEG_X,
                    0.0,
                    &MovementTuning::fixture(),
                )
            })
            .unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn a_vault_rises_to_the_sill_and_lands_on_the_far_side() {
        let vault = Mantle {
            from: Vec3::ZERO,
            to: Vec3::new(0.0, 0.1, -1.2),
            crest: Some(0.92),
            elapsed: 0.0,
            secs: 0.5,
        };
        assert_eq!(vault.feet(0.0), Vec3::ZERO);
        assert!((vault.feet(VAULT_APEX).y - 0.92).abs() < 1e-5);
        assert!((vault.feet(1.0) - vault.to).length() < 1e-5);
        // Never above the sill top on the way, and the move across waits until it is nearly up.
        let samples = (0..=100).map(|i| vault.feet(i as f32 / 100.0));
        assert!(
            samples
                .clone()
                .all(|feet| feet.y <= 0.92 + 1e-5 && feet.y >= -1e-5)
        );
        assert!(samples.take(10).all(|feet| feet.z > -0.05));
    }
}
