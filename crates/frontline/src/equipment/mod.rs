//! Equipment (CONTEXT.md: Grenade): the lethal Frag, Semtex and Proximity Mine, the tactical
//! Flashbang, Stun Grenade, Smoke and Tear Gas, and the launcher's 40mm round. A Grenade is a
//! physical projectile the server flies (bounce, stick, fuse) and whose effect it applies (blast
//! damage through the damage path, flash, stun); clients draw it and show the effects
//! (`client`), including the clouds of Smoke and Tear Gas.
//!
//! This module holds the data (`data/equipment.ron`) and the pure, unit-tested rules: stepping a
//! Grenade body with bounces, sticking and the fuse, the blast's damage falloff, how a flash
//! blinds and deafens and a stun slows a viewer, how dense a cloud is as it ages and how a
//! mine is tripped. The server system is `combat/grenades.rs`.

pub mod client;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::class::EquipmentId;
use crate::data::GameData;
use crate::gear::{AttachmentId, Loadout};
use crate::net::protocol::ThrowGrenade;
use crate::player::BODY_RADIUS;

/// A thrown or launched body, by what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GrenadeKind {
    Frag,
    /// Sticks to the first surface or player it touches, then goes off after a short fuse.
    Semtex,
    /// Sticks to the first surface it touches and goes off when an enemy comes near.
    Mine,
    Flashbang,
    /// Concussion: slows and shakes everyone near, no damage.
    Stun,
    Smoke,
    /// A damage-free gas cloud that slows and chokes whoever stands in it.
    TearGas,
    /// The 40mm round of the underslung grenade launcher (the M16A4's "noob tube").
    Launcher,
}

impl GrenadeKind {
    pub const ALL: [GrenadeKind; 8] = [
        GrenadeKind::Frag,
        GrenadeKind::Semtex,
        GrenadeKind::Mine,
        GrenadeKind::Flashbang,
        GrenadeKind::Stun,
        GrenadeKind::Smoke,
        GrenadeKind::TearGas,
        GrenadeKind::Launcher,
    ];

    /// The ones a player throws by hand: everything but the launcher's round.
    pub const THROWN: [GrenadeKind; 7] = [
        GrenadeKind::Frag,
        GrenadeKind::Semtex,
        GrenadeKind::Mine,
        GrenadeKind::Flashbang,
        GrenadeKind::Stun,
        GrenadeKind::Smoke,
        GrenadeKind::TearGas,
    ];
}

impl From<EquipmentId> for GrenadeKind {
    fn from(id: EquipmentId) -> Self {
        match id {
            EquipmentId::Frag => GrenadeKind::Frag,
            EquipmentId::Semtex => GrenadeKind::Semtex,
            EquipmentId::Mine => GrenadeKind::Mine,
            EquipmentId::Flashbang => GrenadeKind::Flashbang,
            EquipmentId::Stun => GrenadeKind::Stun,
            EquipmentId::Smoke => GrenadeKind::Smoke,
            EquipmentId::TearGas => GrenadeKind::TearGas,
        }
    }
}

/// A Grenade in the air or on the ground, replicated from the server to everyone (the server
/// owns its `Body`); clients draw it at `position`. `armed` is a mine that has settled in and
/// would now trip (clients blink a light on it).
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grenade {
    pub kind: GrenadeKind,
    pub position: Vec3,
    pub armed: bool,
}

/// When a Grenade goes off.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub enum Fuse {
    /// This long after it left the hand.
    Timed(f32),
    /// On the first surface it meets after flying `arm_distance`, or after `max_secs` in the air.
    Impact { arm_distance: f32, max_secs: f32 },
    /// It sticks to the first surface or player it touches and goes off `secs` after that; one
    /// that touches nothing goes off `max_air_secs` after leaving the hand.
    Stick { secs: f32, max_air_secs: f32 },
    /// It sticks to the first surface it touches (never a player), is armed `arm_secs` later and
    /// goes off when an enemy comes within `trigger_radius` in a clear line, or `max_secs` after
    /// leaving the hand.
    Proximity {
        arm_secs: f32,
        trigger_radius: f32,
        max_secs: f32,
    },
}

impl Fuse {
    /// It stops dead on the first surface instead of bouncing.
    pub fn sticks(self) -> bool {
        matches!(self, Fuse::Stick { .. } | Fuse::Proximity { .. })
    }

    /// It also sticks to a player it touches.
    pub fn sticks_to_players(self) -> bool {
        matches!(self, Fuse::Stick { .. })
    }

    /// It is a mine: placed on a surface, it waits for an enemy.
    pub fn is_mine(self) -> bool {
        matches!(self, Fuse::Proximity { .. })
    }
}

/// What happens where it goes off.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub enum Effect {
    /// `damage` at the centre, falling off linearly to nothing at `radius`.
    Blast { damage: f32, radius: f32 },
    /// Blinds viewers facing it and deafens everyone within `radius`, for up to these times.
    Flash {
        radius: f32,
        blind_secs: f32,
        deaf_secs: f32,
    },
    /// Slows (by up to the share `slow`) and shakes everyone within `radius` for up to `secs`,
    /// fading with distance. No damage.
    Stun { radius: f32, secs: f32, slow: f32 },
    /// A cloud of `radius` that blocks the view for `secs`.
    Smoke { radius: f32, secs: f32 },
    /// A cloud of `radius` that lasts `secs`: whoever stands in it coughs, slowed by up to the
    /// share `slow`, and their view narrows. No damage.
    Gas { radius: f32, secs: f32, slow: f32 },
}

impl Effect {
    /// It leaves a cloud (Smoke, Tear Gas) that lasts a while where it went off.
    pub fn makes_cloud(self) -> bool {
        matches!(self, Effect::Smoke { .. } | Effect::Gas { .. })
    }
}

/// Most Grenades (in the air, stuck or mines) the server keeps at once, and most clouds (live
/// ones plus the cloud-makers still in the air). A throw past either is dropped.
pub const MAX_GRENADES: usize = 48;
pub const MAX_CLOUDS: usize = 6;

/// Whether one more Grenade fits, given `grenades` already out and `clouds` up or on their way,
/// and whether the new one `makes_cloud`.
pub fn room_for(makes_cloud: bool, grenades: usize, clouds: usize) -> bool {
    grenades < MAX_GRENADES && (!makes_cloud || clouds < MAX_CLOUDS)
}

/// How far in front of the eye a thrown Grenade starts, metres.
const THROW_REACH: f32 = 0.45;

/// How a thrown Grenade (every kind but the launcher's round) leaves the hand as the thrower
/// looks up or down (`data/equipment.ron`): it lobs a little above the aim, looking up lobs
/// higher and farther, looking down is a short underhand toss. Angles in degrees, speeds in m/s.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct ThrowArc {
    /// How far above the aim a throw at the horizon leaves the hand.
    pub lob_deg: f32,
    /// The steepest a Grenade ever leaves the hand.
    pub max_launch_deg: f32,
    /// Speed looking straight down, at the horizon, and from `up_pitch_deg` up.
    pub down_speed: f32,
    pub level_speed: f32,
    pub up_speed: f32,
    /// The look angle above the horizon from which the speed is `up_speed` and the lob is spent
    /// (the launch angle is then the look angle itself).
    pub up_pitch_deg: f32,
}

impl ThrowArc {
    /// The launch angle (radians above the horizontal) and speed of a throw while looking
    /// `pitch` radians above the horizon (negative below).
    pub fn launch(&self, pitch: f32) -> (f32, f32) {
        let (speed, lob) = if pitch < 0.0 {
            let down = (-pitch / std::f32::consts::FRAC_PI_2).clamp(0.0, 1.0);
            (self.level_speed.lerp(self.down_speed, down), self.lob_deg)
        } else {
            let up = (pitch / self.up_pitch_deg.to_radians()).clamp(0.0, 1.0);
            (
                self.level_speed.lerp(self.up_speed, up),
                self.lob_deg * (1.0 - up),
            )
        };
        let launch = (pitch + lob.to_radians()).min(self.max_launch_deg.to_radians());
        (launch, speed)
    }
}

/// One Grenade's tuning (`data/equipment.ron`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct GrenadeTuning {
    pub name: String,
    /// Launch speed; for a thrown one the most any throw may have (the server holds it to it).
    pub speed: f32,
    pub gravity: f32,
    pub restitution: f32,
    pub friction: f32,
    pub radius: f32,
    pub fuse: Fuse,
    pub effect: Effect,
    pub count: u32,
    pub cooldown_secs: f32,
    /// How a throw leaves the hand by the look angle; a launched round (always along the aim at
    /// `speed`) has none.
    #[serde(default)]
    pub throw: Option<ThrowArc>,
}

impl GrenadeTuning {
    /// Panics (stopping the game at startup) on numbers that make no sense.
    pub fn check(&self, file: &str, kind: GrenadeKind) {
        assert!(
            self.speed > 0.0
                && self.gravity >= 0.0
                && (0.0..=1.0).contains(&self.restitution)
                && (0.0..=1.0).contains(&self.friction)
                && self.radius > 0.0
                && self.count >= 1
                && self.cooldown_secs >= 0.0,
            "{file}: {kind:?} has a number out of range"
        );
        let throw_ok = match (&self.throw, kind) {
            (None, GrenadeKind::Launcher) => true,
            (None, _) => false,
            (Some(arc), _) => {
                let speeds = [arc.down_speed, arc.level_speed, arc.up_speed];
                speeds
                    .iter()
                    .all(|speed| (0.0..=self.speed).contains(speed))
                    && arc.down_speed > 0.0
                    && (0.0..=45.0).contains(&arc.lob_deg)
                    && arc.max_launch_deg > 0.0
                    && arc.max_launch_deg < 90.0
                    && arc.up_pitch_deg > 0.0
                    && arc.up_pitch_deg < 90.0
            }
        };
        assert!(
            throw_ok,
            "{file}: {kind:?} needs a throw arc (and every speed in it between 0 and its speed)"
        );
        let fuse_ok = match self.fuse {
            Fuse::Timed(secs) => secs > 0.0,
            Fuse::Impact {
                arm_distance,
                max_secs,
            } => arm_distance >= 0.0 && max_secs > 0.0,
            Fuse::Stick { secs, max_air_secs } => secs > 0.0 && max_air_secs > 0.0,
            Fuse::Proximity {
                arm_secs,
                trigger_radius,
                max_secs,
            } => arm_secs >= 0.0 && trigger_radius > 0.0 && max_secs > arm_secs,
        };
        let effect_ok = match self.effect {
            Effect::Blast { damage, radius } => damage > 0.0 && radius > 0.0,
            Effect::Flash {
                radius,
                blind_secs,
                deaf_secs,
            } => radius > 0.0 && blind_secs >= 0.0 && deaf_secs >= 0.0,
            Effect::Smoke { radius, secs } => radius > 0.0 && secs > 0.0,
            Effect::Stun { radius, secs, slow } | Effect::Gas { radius, secs, slow } => {
                radius > 0.0 && secs > 0.0 && (0.0..=1.0).contains(&slow)
            }
        };
        assert!(
            fuse_ok && effect_ok,
            "{file}: {kind:?}'s fuse or effect has a number out of range"
        );
        assert!(
            !matches!(self.fuse, Fuse::Proximity { .. })
                || matches!(self.effect, Effect::Blast { .. }),
            "{file}: {kind:?} is a mine and must blast"
        );
    }
}

/// Speed under which a Grenade that has landed on level-ish ground stops moving.
const REST_SPEED: f32 = 0.6;
/// A surface whose normal points up at least this much is ground a Grenade can rest on.
const REST_NORMAL_Y: f32 = 0.7;
/// How far short of a surface a Grenade stops, so the next cast doesn't start inside it.
const SKIN: f32 = 0.004;

/// Where a sphere (the Grenade's body) swept along a direction first met a surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// How far the centre can travel along the direction before the sphere touches it.
    pub distance: f32,
    /// Unit normal of the surface, facing the ray.
    pub normal: Vec3,
}

/// What a step of a Grenade body did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Flying,
    /// It struck a surface and bounced.
    Bounced,
    /// It struck a surface and stuck to it (a `Fuse::Stick` or `Fuse::Proximity` Grenade).
    Stuck,
    /// The fuse is up: it goes off where it is.
    Detonate,
}

/// A Grenade in flight: the state the server steps every fixed tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub position: Vec3,
    pub velocity: Vec3,
    /// Seconds since it left the hand.
    pub age: f32,
    /// Metres flown, for an Impact fuse's arming distance.
    pub travelled: f32,
    /// It has come to rest on the ground.
    pub resting: bool,
    /// It is held where it is: stuck to a surface, or riding a player.
    pub anchored: bool,
    /// The age at which it first stuck: a Stick fuse counts from there, a mine arms from there.
    pub stuck_at: Option<f32>,
}

impl Body {
    /// A Grenade leaving `origin` along `direction` (a unit vector) at `speed`.
    pub fn thrown(origin: Vec3, direction: Vec3, speed: f32) -> Self {
        Self {
            position: origin,
            velocity: direction * speed,
            age: 0.0,
            travelled: 0.0,
            resting: false,
            anchored: false,
            stuck_at: None,
        }
    }

    /// Holds it where it is from now on (a player was touched, or a surface was): it stops
    /// moving and its Stick fuse starts counting.
    pub fn stick(&mut self) {
        self.velocity = Vec3::ZERO;
        self.anchored = true;
        self.stuck_at.get_or_insert(self.age);
    }

    /// Lets go of what it was stuck to (the player it rode died): it drops, its fuse keeps
    /// counting.
    pub fn release(&mut self) {
        self.anchored = false;
        self.resting = false;
    }

    /// A mine (`Fuse::Proximity`) that has sat still long enough to trip.
    pub fn armed(&self, fuse: Fuse) -> bool {
        match fuse {
            Fuse::Proximity { arm_secs, .. } => {
                self.anchored && self.stuck_at.is_some_and(|at| self.age - at >= arm_secs)
            }
            _ => false,
        }
    }

    /// Advances `dt` seconds: gravity, the move (stopped and bounced, or stuck, by the first
    /// surface `cast` finds on the way: it is given the centre, a unit direction and the length
    /// to sweep a sphere of the Grenade's radius along) and the fuse.
    pub fn step(
        &mut self,
        tuning: &GrenadeTuning,
        dt: f32,
        cast: impl FnOnce(Vec3, Vec3, f32) -> Option<Contact>,
    ) -> Step {
        self.age += dt;
        let mut bounced = false;
        let mut stuck = false;
        if !self.resting && !self.anchored {
            self.velocity.y -= tuning.gravity * dt;
            let movement = self.velocity * dt;
            let length = movement.length();
            if length > f32::EPSILON {
                let direction = movement / length;
                match cast(self.position, direction, length) {
                    Some(contact) => {
                        let reach = (contact.distance - SKIN).clamp(0.0, length);
                        self.position += direction * reach;
                        self.travelled += reach;
                        if tuning.fuse.sticks() {
                            self.stick();
                            stuck = true;
                        } else {
                            let into = self.velocity.dot(contact.normal).min(0.0);
                            let sideways = self.velocity - contact.normal * into;
                            self.velocity = sideways * tuning.friction
                                - contact.normal * into * tuning.restitution;
                            if self.velocity.length() < REST_SPEED
                                && contact.normal.y >= REST_NORMAL_Y
                            {
                                self.velocity = Vec3::ZERO;
                                self.resting = true;
                            }
                            bounced = true;
                        }
                    }
                    None => {
                        self.position += movement;
                        self.travelled += length;
                    }
                }
            }
        }
        let detonate = match tuning.fuse {
            Fuse::Timed(secs) => self.age >= secs,
            Fuse::Impact {
                arm_distance,
                max_secs,
            } => (bounced && self.travelled >= arm_distance) || self.age >= max_secs,
            Fuse::Stick { secs, max_air_secs } => match self.stuck_at {
                Some(at) => self.age - at >= secs,
                None => self.age >= max_air_secs,
            },
            Fuse::Proximity { max_secs, .. } => self.age >= max_secs,
        };
        if detonate {
            Step::Detonate
        } else if stuck {
            Step::Stuck
        } else if bounced {
            Step::Bounced
        } else {
            Step::Flying
        }
    }
}

/// A player's own Semtex may not stick to them in the first moments after the throw (it leaves
/// the hand inside their body's reach).
const OWN_STICK_DELAY_SECS: f32 = 0.35;

/// Whether a Grenade `age` seconds out of the hand may stick to a player, who is `is_owner`
/// when they threw it.
pub fn can_stick_to(is_owner: bool, age: f32) -> bool {
    !is_owner || age >= OWN_STICK_DELAY_SECS
}

/// Where a Grenade of `radius` moving in a straight line from `from` to `to` first touches a
/// player's body (an upright capsule of `BODY_RADIUS` from the feet up `height`), if it does:
/// the point on the path nearest the body's axis. The path is judged in one piece so a fast
/// throw can't skip through a body between two ticks.
pub fn touches_body(from: Vec3, to: Vec3, radius: f32, feet: Vec3, height: f32) -> Option<Vec3> {
    let reach = radius + BODY_RADIUS;
    let along = to - from;
    let flat = along.xz();
    let flat_sq = flat.length_squared();
    let t = if flat_sq > f32::EPSILON {
        ((feet.xz() - from.xz()).dot(flat) / flat_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let at = from + along * t;
    let in_reach = at.xz().distance(feet.xz()) <= reach;
    let in_height = at.y >= feet.y - radius && at.y <= feet.y + height + radius;
    (in_reach && in_height).then_some(at)
}

/// Whether a mine goes off: it is `armed`, a player is within `trigger_radius` of it
/// (`distance`) and nothing of the World is in the way (`in_sight`).
pub fn mine_trips(armed: bool, trigger_radius: f32, distance: f32, in_sight: bool) -> bool {
    armed && in_sight && distance <= trigger_radius
}

/// The oldest of `items` (each an id and its age in seconds) that no longer fit when at most
/// `keep` may stay: sorts them oldest first and returns that leading part. A player's mines
/// are held to `GrenadeTuning::count` this way.
pub fn oldest_beyond_cap<T>(items: &mut [(T, f32)], keep: usize) -> &[(T, f32)] {
    items.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
    &items[..items.len().saturating_sub(keep)]
}

/// Damage of a blast of `damage` and `radius` at `distance` from its centre: all of it at the
/// centre, falling off linearly to nothing at the edge.
pub fn blast_damage(damage: f32, radius: f32, distance: f32) -> f32 {
    if distance >= radius || radius <= 0.0 {
        0.0
    } else {
        damage * (1.0 - distance.max(0.0) / radius)
    }
}

/// How near a point `distance` from a blast centre is, out of `radius`: 1 at the centre, linearly
/// less to 0 at the edge and beyond.
fn nearness(radius: f32, distance: f32) -> f32 {
    if radius <= 0.0 || distance >= radius {
        0.0
    } else {
        1.0 - distance.max(0.0) / radius
    }
}

/// How hard a flash hits one player: `blind` and `deaf` are each 0 (not at all) to 1 (the
/// whole time in the tuning).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashExposure {
    pub blind: f32,
    pub deaf: f32,
}

/// The cosine of the angle between the view and the flash from which a viewer is blinded fully
/// (looking straight at it), and the one below which not at all (looking away).
const BLIND_FACING: (f32, f32) = (-0.2, 0.6);

/// What a flash at `distance` does to a viewer whose view direction makes `view_dot` (the
/// cosine, 1 straight at it) with the direction to the flash, when `radius` is its reach and
/// there is a clear line between them (`in_sight`). Everyone near is deafened, blind or not;
/// only a viewer looking toward it is blinded, and both fade with distance.
pub fn flash_exposure(radius: f32, distance: f32, view_dot: f32, in_sight: bool) -> FlashExposure {
    let near = nearness(radius, distance);
    let (away, towards) = BLIND_FACING;
    let facing = ((view_dot - away) / (towards - away)).clamp(0.0, 1.0);
    FlashExposure {
        blind: if in_sight { near * facing } else { 0.0 },
        deaf: near,
    }
}

/// Share of a stun that gets through a wall: the concussion is felt around corners, only less.
const STUN_THROUGH_WALLS: f32 = 0.4;

/// How hard a stun at `distance` hits one player, 0 (not at all) to 1 (the full time and slow in
/// the tuning), when `radius` is its reach and `in_sight` says whether the World is clear
/// between them. Unlike a flash it doesn't matter where they look; it fades with distance and
/// is weaker behind cover.
pub fn stun_exposure(radius: f32, distance: f32, in_sight: bool) -> f32 {
    nearness(radius, distance) * if in_sight { 1.0 } else { STUN_THROUGH_WALLS }
}

/// Seconds a smoke cloud takes to billow out, and to thin out at the end.
pub const SMOKE_FADE_IN_SECS: f32 = 1.2;
pub const SMOKE_FADE_OUT_SECS: f32 = 3.0;

/// How opaque a smoke cloud lasting `secs` is when it is `age` seconds old: 0 to 1, billowing
/// in, full, then thinning out over the last seconds.
pub fn smoke_density(age: f32, secs: f32) -> f32 {
    if age < 0.0 || age >= secs {
        return 0.0;
    }
    let fade_out = SMOKE_FADE_OUT_SECS.min(secs * 0.5).max(f32::EPSILON);
    let rise = (age / SMOKE_FADE_IN_SECS).clamp(0.0, 1.0);
    let fall = ((secs - age) / fade_out).clamp(0.0, 1.0);
    rise.min(fall)
}

/// How far into a cloud of `radius` around `centre` the eye at `eye` is: 0 outside, 1 at the
/// centre, so the view can fade to grey as it goes in.
pub fn depth_in_cloud(eye: Vec3, centre: Vec3, radius: f32) -> f32 {
    (1.0 - eye.distance(centre) / radius.max(f32::EPSILON)).clamp(0.0, 1.0)
}

/// How much of a cloud (Smoke or Tear Gas) the eye at `eye` is in, 0 to 1: the density of a
/// cloud lasting `secs` that is `age` old, once the eye is a little way in (a third of the
/// radius is as good as the middle).
pub fn cloud_exposure(eye: Vec3, centre: Vec3, radius: f32, age: f32, secs: f32) -> f32 {
    (depth_in_cloud(eye, centre, radius) * 3.0).min(1.0) * smoke_density(age, secs)
}

/// Moves `level` (0 to 1) toward `target`: up at `rise` per second, down at `fall` per second.
/// Gas builds up in the lungs quickly and wears off slowly.
pub fn linger(level: f32, target: f32, rise: f32, fall: f32, dt: f32) -> f32 {
    if target > level {
        (level + rise * dt).min(target)
    } else {
        (level - fall * dt).max(target)
    }
}

/// Seconds between two coughs, and how long one lasts.
const COUGH_PERIOD_SECS: f32 = 1.7;
const COUGH_SECS: f32 = 0.35;
/// The deepest a cough dips the view, radians.
const COUGH_DIP: f32 = 0.07;

/// How far a cough dips the view (radians, 0 between coughs) `time` seconds into the gas, for a
/// player `level` (0 to 1) choked.
pub fn cough_dip(time: f32, level: f32) -> f32 {
    let phase = time.rem_euclid(COUGH_PERIOD_SECS);
    if phase >= COUGH_SECS {
        return 0.0;
    }
    level.clamp(0.0, 1.0) * COUGH_DIP * (phase / COUGH_SECS * std::f32::consts::PI).sin()
}

/// The widest a stun shakes the view at full strength, radians (pitch, yaw, roll).
const STUN_SHAKE: Vec3 = Vec3::new(0.035, 0.05, 0.07);

/// How the view is shaken (pitch, yaw, roll in radians) `time` seconds on, `strength` (0 to 1)
/// into a stun: a slow drunken sway and a faster tremor. Zero at zero strength.
pub fn stun_shake(time: f32, strength: f32) -> Vec3 {
    let strength = strength.clamp(0.0, 1.0);
    let sway = Vec3::new(
        (time * 2.3).sin() * 0.7 + (time * 7.1).sin() * 0.3,
        (time * 1.7).sin() * 0.7 + (time * 6.3).cos() * 0.3,
        (time * 1.3).sin() * 0.6 + (time * 8.9).sin() * 0.4,
    );
    sway * STUN_SHAKE * strength
}

/// The share of its normal speed a player moves at while `strength` (0 to 1) of a `slow` (the
/// share the effect takes off at full strength) is on them.
pub fn slow_scale(slow: f32, strength: f32) -> f32 {
    1.0 - slow.clamp(0.0, 1.0) * strength.clamp(0.0, 1.0)
}

/// The clear middle at full gas, percent of the centre-to-corner distance.
const TUNNEL_NARROWEST_PERCENT: f32 = 22.0;

/// How wide the clear middle of the view is while `level` (0 to 1) choked, as a percentage of
/// the centre-to-corner distance: all of it at 0 (no tunnel), shrinking to a small window.
pub fn tunnel_inner_percent(level: f32) -> f32 {
    100.0 + (TUNNEL_NARROWEST_PERCENT - 100.0) * level.clamp(0.0, 1.0)
}

/// How strongly a flash still blinds (or deafens) `left` seconds before it ends, out of `total`:
/// solid for the first stretch, then fading out.
pub fn flash_alpha(left: f32, total: f32) -> f32 {
    if total <= 0.0 || left <= 0.0 {
        0.0
    } else {
        (left / total * 1.6).min(1.0)
    }
}

/// How many of one kind of Grenade a player still carries, and the wait before the next throw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pouch {
    pub kind: GrenadeKind,
    pub left: u32,
    pub cooldown: f32,
}

impl Pouch {
    fn full(kind: GrenadeKind, data: &GameData) -> Self {
        Self {
            kind,
            left: data.grenade(kind).count,
            cooldown: 0.0,
        }
    }

    /// One is left and the last throw is not too recent.
    pub fn can_take(&self) -> bool {
        self.left > 0 && self.cooldown <= 0.0
    }

    /// Takes one for a throw, starting the wait; false when none is left or the last throw is
    /// too recent.
    pub fn take(&mut self, tuning: &GrenadeTuning) -> bool {
        if !self.can_take() {
            return false;
        }
        self.left -= 1;
        self.cooldown = tuning.cooldown_secs;
        true
    }
}

/// The message that sends a Grenade of `kind` away from the eye at `eye` of a thrower looking
/// along `yaw` and `pitch`: a thrown one by its arc (`ThrowArc::launch`), a launched round
/// straight along the aim.
pub fn throw_request(
    kind: GrenadeKind,
    tuning: &GrenadeTuning,
    yaw: f32,
    pitch: f32,
    eye: Vec3,
) -> ThrowGrenade {
    let (launch, speed) = tuning
        .throw
        .map_or((pitch, tuning.speed), |arc| arc.launch(pitch));
    let direction = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(launch) * Vec3::NEG_Z;
    ThrowGrenade {
        kind,
        origin: eye + direction * THROW_REACH,
        direction,
        speed,
    }
}

/// The state of one key held for a throw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyState {
    /// It went down this frame.
    pub pressed: bool,
    /// It is down now (false for a tap that went up within the frame).
    pub held: bool,
}

/// What the player and the pouches tell `ReadiedThrow` this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct ThrowFeed {
    pub lethal: KeyState,
    pub tactical: KeyState,
    /// The pouch has one to throw (`Pouch::can_take`).
    pub lethal_ready: bool,
    pub tactical_ready: bool,
    /// The player may throw: alive, in control (no menu or overlay), the Weapon not switching.
    pub allowed: bool,
}

/// Which Grenade the local player holds ready: a throw key pressed readies it, letting go
/// throws it. Pure; `equipment::client` feeds it the keys.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadiedThrow(pub Option<PouchSlot>);

impl ReadiedThrow {
    /// Runs a frame: readies the Grenade whose key went down (the lethal first when both did,
    /// and only while the pouch has one) and returns it once its key is up again, a tap
    /// included. The throw is dropped, nothing thrown, when the player may not throw any more
    /// (died, opened a menu, switched Weapon) or the pouch ran empty.
    pub fn step(&mut self, feed: &ThrowFeed) -> Option<PouchSlot> {
        if !feed.allowed {
            self.0 = None;
            return None;
        }
        if self.0.is_none() {
            self.0 = [
                (PouchSlot::Lethal, feed.lethal, feed.lethal_ready),
                (PouchSlot::Tactical, feed.tactical, feed.tactical_ready),
            ]
            .into_iter()
            .find(|(_, key, ready)| key.pressed && *ready)
            .map(|(slot, ..)| slot);
        }
        let slot = self.0?;
        let (key, ready) = match slot {
            PouchSlot::Lethal => (feed.lethal, feed.lethal_ready),
            PouchSlot::Tactical => (feed.tactical, feed.tactical_ready),
        };
        if !ready {
            self.0 = None;
            return None;
        }
        if key.held {
            return None;
        }
        self.0 = None;
        Some(slot)
    }
}

/// Which pouch a throw key draws from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PouchSlot {
    Lethal,
    Tactical,
}

/// The equipment the local player carries (from their Loadout) and what is left of it.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Carried {
    pub lethal: Option<Pouch>,
    pub tactical: Option<Pouch>,
    pub launcher: Option<Pouch>,
}

impl Carried {
    /// A full set for `loadout`: its lethal and tactical equipment, and launcher rounds when
    /// the Primary has the Grenade Launcher fitted.
    pub fn from_loadout(loadout: &Loadout, data: &GameData) -> Self {
        let pouch = |id: Option<EquipmentId>| id.map(|id| Pouch::full(id.into(), data));
        Self {
            lethal: pouch(loadout.lethal),
            tactical: pouch(loadout.tactical),
            launcher: (loadout.attachments.underbarrel == Some(AttachmentId::GrenadeLauncher))
                .then(|| Pouch::full(GrenadeKind::Launcher, data)),
        }
    }

    pub fn pouch_mut(&mut self, slot: PouchSlot) -> Option<&mut Pouch> {
        match slot {
            PouchSlot::Lethal => self.lethal.as_mut(),
            PouchSlot::Tactical => self.tactical.as_mut(),
        }
    }

    /// Runs the waits down by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        for pouch in [&mut self.lethal, &mut self.tactical, &mut self.launcher]
            .into_iter()
            .flatten()
        {
            pouch.cooldown = (pouch.cooldown - dt).max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 64.0;
    const RADIUS: f32 = 0.06;

    fn frag() -> GrenadeTuning {
        GrenadeTuning {
            name: "Frag".into(),
            speed: 17.0,
            gravity: 14.0,
            restitution: 0.35,
            friction: 0.6,
            radius: RADIUS,
            fuse: Fuse::Timed(3.0),
            effect: Effect::Blast {
                damage: 150.0,
                radius: 8.0,
            },
            count: 1,
            cooldown_secs: 1.0,
            throw: None,
        }
    }

    /// A level floor at y = 0: what a sphere of the test tuning's radius swept along `direction`
    /// from `from` meets.
    fn floor(from: Vec3, direction: Vec3, length: f32) -> Option<Contact> {
        if direction.y >= 0.0 {
            return None;
        }
        let distance = ((from.y - RADIUS) / -direction.y).max(0.0);
        (distance <= length).then_some(Contact {
            distance,
            normal: Vec3::Y,
        })
    }

    /// A wall at x = 10 facing back along -X.
    fn wall(from: Vec3, direction: Vec3, length: f32) -> Option<Contact> {
        if direction.x <= 0.0 {
            return None;
        }
        let distance = (10.0 - RADIUS - from.x) / direction.x;
        (distance >= 0.0 && distance <= length).then_some(Contact {
            distance,
            normal: Vec3::NEG_X,
        })
    }

    fn run_until_detonation(
        body: &mut Body,
        tuning: &GrenadeTuning,
        world: fn(Vec3, Vec3, f32) -> Option<Contact>,
    ) -> u32 {
        for tick in 1..=2000 {
            if body.step(tuning, DT, world) == Step::Detonate {
                return tick;
            }
        }
        panic!("never went off");
    }

    #[test]
    fn a_timed_fuse_goes_off_after_its_seconds_whatever_the_tick_length() {
        let tuning = frag();
        let mut body = Body::thrown(Vec3::new(0.0, 1.6, 0.0), Vec3::Y, tuning.speed);
        let ticks = run_until_detonation(&mut body, &tuning, |_, _, _| None);
        assert!((191..=193).contains(&ticks), "3 s of 1/64 s ticks: {ticks}");
        assert!((body.age - 3.0).abs() < DT);

        // A different tick length reaches the same age.
        let mut coarse = Body::thrown(Vec3::new(0.0, 1.6, 0.0), Vec3::Y, tuning.speed);
        let mut steps = 0;
        while coarse.step(&tuning, 0.1, |_, _, _| None) != Step::Detonate {
            steps += 1;
        }
        assert!(
            (coarse.age - 3.0).abs() < 0.1,
            "{steps} steps: {}",
            coarse.age
        );
    }

    #[test]
    fn gravity_pulls_a_throw_down_in_an_arc() {
        let tuning = frag();
        let direction = Vec3::new(1.0, 0.3, 0.0).normalize();
        let mut body = Body::thrown(Vec3::new(0.0, 1.6, 0.0), direction, tuning.speed);
        let mut peak = body.position.y;
        for _ in 0..40 {
            body.step(&tuning, DT, |_, _, _| None);
            peak = peak.max(body.position.y);
        }
        assert!(peak > 1.6, "it rises first");
        assert!(
            body.velocity.y < tuning.speed * direction.y,
            "gravity slowed the climb"
        );
        assert!(body.position.x > 5.0, "and it travels forward");
    }

    #[test]
    fn it_bounces_off_the_floor_losing_energy_and_comes_to_rest() {
        let tuning = frag();
        let mut body = Body::thrown(
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::new(0.3, -0.2, 0.0).normalize(),
            tuning.speed,
        );
        let mut bounces = 0;
        let mut last_peak = f32::MAX;
        let mut peak = 0.0_f32;
        for _ in 0..(2.5 / DT) as u32 {
            let was_going_down = body.velocity.y - tuning.gravity * DT < 0.0;
            let step = body.step(&tuning, DT, floor);
            assert!(
                body.position.y >= tuning.radius - 1e-4,
                "never through the floor: {:?}",
                body.position
            );
            peak = peak.max(body.position.y);
            if step == Step::Bounced {
                assert!(was_going_down);
                bounces += 1;
                assert!(
                    peak < last_peak || peak - tuning.radius < 0.01,
                    "each bounce is lower than the one before (until it is a settling tremble)"
                );
                last_peak = peak;
                peak = 0.0;
            }
        }
        assert!(bounces >= 2, "it bounced {bounces} times");
        assert!(body.resting && body.velocity == Vec3::ZERO, "and settled");
        assert!(
            (body.position.y - tuning.radius).abs() < 1e-3,
            "on the ground"
        );
        // Resting bodies stay put and still count down the fuse.
        let at = body.position;
        body.step(&tuning, DT, floor);
        assert_eq!(body.position, at);
    }

    #[test]
    fn a_wall_reflects_it_back_with_less_speed() {
        let tuning = frag();
        let mut body = Body::thrown(Vec3::new(0.0, 5.0, 0.0), Vec3::X, tuning.speed);
        for _ in 0..200 {
            body.step(&tuning, DT, wall);
            if body.velocity.x < 0.0 {
                break;
            }
        }
        assert!(body.velocity.x < 0.0, "it came back");
        assert!(body.velocity.x.abs() < tuning.speed * tuning.restitution + 0.01);
        assert!(
            body.position.x < 10.0,
            "and stayed on this side of the wall"
        );
    }

    #[test]
    fn an_impact_fuse_waits_for_its_arming_distance_then_goes_off_on_the_first_surface() {
        let tuning = GrenadeTuning {
            fuse: Fuse::Impact {
                arm_distance: 8.0,
                max_secs: 4.0,
            },
            speed: 40.0,
            gravity: 0.0,
            ..frag()
        };
        // Armed: the wall at 10 m is farther than 8 m.
        let mut body = Body::thrown(Vec3::new(0.0, 1.5, 0.0), Vec3::X, tuning.speed);
        let ticks = run_until_detonation(&mut body, &tuning, wall);
        assert!(
            body.position.x > 9.8,
            "it burst at the wall: {:?}",
            body.position
        );
        assert!(ticks < 30, "{ticks}");

        // Too close to arm: it bounces instead and goes off when `max_secs` runs out.
        let mut close = Body::thrown(Vec3::new(5.0, 1.5, 0.0), Vec3::X, tuning.speed);
        let mut result = Step::Flying;
        for _ in 0..10 {
            result = close.step(&tuning, DT, wall);
            if result != Step::Flying {
                break;
            }
        }
        assert_eq!(result, Step::Bounced, "a dud up close");
        let ticks = run_until_detonation(&mut close, &tuning, |_, _, _| None);
        assert!(
            close.age >= 4.0 && ticks > 100,
            "it goes off at max_secs: {ticks}"
        );
    }

    #[test]
    fn blast_damage_falls_off_linearly_and_stops_at_the_radius() {
        assert_eq!(blast_damage(150.0, 8.0, 0.0), 150.0);
        assert!((blast_damage(150.0, 8.0, 4.0) - 75.0).abs() < 1e-4);
        assert!((blast_damage(150.0, 8.0, 7.0) - 18.75).abs() < 1e-4);
        assert_eq!(blast_damage(150.0, 8.0, 8.0), 0.0);
        assert_eq!(blast_damage(150.0, 8.0, 30.0), 0.0);
        assert_eq!(
            blast_damage(150.0, 8.0, -1.0),
            150.0,
            "never more than the centre's"
        );
        // Close enough kills a 100 health player; the edge doesn't.
        assert!(blast_damage(150.0, 8.0, 2.0) >= 100.0);
        assert!(blast_damage(150.0, 8.0, 5.0) < 100.0);
    }

    #[test]
    fn a_flash_blinds_only_those_facing_it_but_deafens_everyone_near() {
        let facing = flash_exposure(16.0, 4.0, 1.0, true);
        let side = flash_exposure(16.0, 4.0, 0.2, true);
        let away = flash_exposure(16.0, 4.0, -0.9, true);
        assert!((facing.blind - 0.75).abs() < 1e-4, "{facing:?}");
        assert!(side.blind > 0.0 && side.blind < facing.blind, "{side:?}");
        assert_eq!(away.blind, 0.0, "looking away is safe");
        assert_eq!(away.deaf, facing.deaf, "but not from the bang");
        // Farther away is weaker, past the radius nothing.
        assert!(flash_exposure(16.0, 12.0, 1.0, true).blind < facing.blind);
        assert_eq!(
            flash_exposure(16.0, 16.0, 1.0, true),
            FlashExposure {
                blind: 0.0,
                deaf: 0.0
            }
        );
        // A wall between: still deaf, not blind.
        let behind = flash_exposure(16.0, 4.0, 1.0, false);
        assert_eq!((behind.blind, behind.deaf), (0.0, facing.deaf));
    }

    #[test]
    fn smoke_billows_in_holds_then_thins_out_and_ends() {
        let secs = 14.0;
        assert_eq!(smoke_density(-1.0, secs), 0.0);
        assert_eq!(smoke_density(0.0, secs), 0.0);
        assert!((smoke_density(0.6, secs) - 0.5).abs() < 1e-4, "half way in");
        assert_eq!(smoke_density(5.0, secs), 1.0);
        assert!(
            (smoke_density(secs - 1.5, secs) - 0.5).abs() < 1e-4,
            "half way out"
        );
        assert_eq!(smoke_density(secs, secs), 0.0);
        assert_eq!(smoke_density(secs + 5.0, secs), 0.0);
        // A very short cloud still rises and falls without going above 1.
        for age in [0.2, 0.5, 1.0, 1.8] {
            assert!((0.0..=1.0).contains(&smoke_density(age, 2.0)));
        }
    }

    #[test]
    fn the_view_greys_out_as_the_eye_goes_into_the_cloud() {
        let centre = Vec3::new(0.0, 1.0, 0.0);
        assert_eq!(depth_in_cloud(Vec3::new(9.0, 1.0, 0.0), centre, 5.5), 0.0);
        assert!((depth_in_cloud(centre, centre, 5.5) - 1.0).abs() < 1e-6);
        assert!(
            depth_in_cloud(Vec3::new(2.0, 1.0, 0.0), centre, 5.5)
                > depth_in_cloud(Vec3::new(4.0, 1.0, 0.0), centre, 5.5)
        );
    }

    /// Metres from the thrower to where the first throw of `kind` at `pitch_deg` (negative
    /// below the horizon) first bounces off a level floor, from a standing eye at 1.65 m.
    fn first_bounce_distance(kind: GrenadeKind, pitch_deg: f32) -> f32 {
        let data = crate::data::GameData::load();
        let tuning = data.grenade(kind);
        let eye = Vec3::new(0.0, 1.65, 0.0);
        let throw = throw_request(kind, tuning, 0.0, pitch_deg.to_radians(), eye);
        assert!(throw.speed <= tuning.speed, "never faster than the cap");
        assert!((throw.direction.length() - 1.0).abs() < 1e-4);
        // The fuse isn't what is measured: a throw that stays up longer than it must still land.
        let tuning = GrenadeTuning {
            fuse: Fuse::Timed(60.0),
            ..tuning.clone()
        };
        let mut body = Body::thrown(throw.origin, throw.direction, throw.speed);
        for _ in 0..2000 {
            if body.step(&tuning, DT, floor) == Step::Bounced {
                return Vec3::new(body.position.x, 0.0, body.position.z).length();
            }
        }
        panic!("{kind:?} at {pitch_deg} degrees never landed");
    }

    #[test]
    fn a_throw_at_the_horizon_lobs_far_before_its_first_bounce() {
        for kind in [
            GrenadeKind::Frag,
            GrenadeKind::Flashbang,
            GrenadeKind::Smoke,
        ] {
            let distance = first_bounce_distance(kind, 0.0);
            assert!(
                (25.0..=30.0).contains(&distance),
                "{kind:?} at the horizon: {distance} m"
            );
        }
    }

    #[test]
    fn looking_up_lobs_higher_and_farther_up_to_the_cap() {
        let horizon = first_bounce_distance(GrenadeKind::Frag, 0.0);
        let mut last = horizon;
        for pitch in [10.0, 20.0, 30.0, 45.0] {
            let distance = first_bounce_distance(GrenadeKind::Frag, pitch);
            // Up to the cap it grows; near it, it levels off (within a metre).
            assert!(distance > last - 1.0, "{pitch} degrees up: {distance} m");
            assert!(distance <= 42.0, "{pitch} degrees up: {distance} m");
            last = distance;
        }
        assert!(
            (35.0..=42.0).contains(&last),
            "about 40 m at 45 degrees up: {last} m"
        );
        for pitch in [60.0, 75.0, 89.0] {
            assert!(
                first_bounce_distance(GrenadeKind::Frag, pitch) <= last + 0.5,
                "straight up never goes farther than the cap throw"
            );
        }
    }

    #[test]
    fn looking_down_is_a_short_toss_and_straight_down_drops_at_the_feet() {
        let mut last = 0.0_f32;
        for pitch in [-89.0, -75.0, -60.0, -45.0, -30.0, -15.0, 0.0] {
            let distance = first_bounce_distance(GrenadeKind::Frag, pitch);
            assert!(distance > last, "{pitch} degrees: {distance} m");
            last = distance;
        }
        assert!(first_bounce_distance(GrenadeKind::Frag, -89.0) < 1.0);
        assert!(first_bounce_distance(GrenadeKind::Frag, -45.0) < 5.0);
        assert!(first_bounce_distance(GrenadeKind::Smoke, -45.0) < 5.0);
        assert!(first_bounce_distance(GrenadeKind::Flashbang, -30.0) < 8.0);
    }

    #[test]
    fn no_look_angle_throws_faster_than_the_cap_or_steeper_than_the_limit() {
        let data = crate::data::GameData::load();
        for kind in [
            GrenadeKind::Frag,
            GrenadeKind::Flashbang,
            GrenadeKind::Smoke,
        ] {
            let tuning = data.grenade(kind);
            let arc = tuning.throw.expect("a thrown kind has an arc");
            for step in -90..=90 {
                let pitch = (step as f32).to_radians();
                let (launch, speed) = arc.launch(pitch);
                assert!(speed > 0.0 && speed <= tuning.speed, "{kind:?} {step}");
                assert!(launch <= arc.max_launch_deg.to_radians() + 1e-6);
            }
        }
    }

    #[test]
    fn a_launched_round_flies_straight_along_the_aim_at_full_speed() {
        let data = crate::data::GameData::load();
        let tuning = data.grenade(GrenadeKind::Launcher);
        let throw = throw_request(GrenadeKind::Launcher, tuning, 0.0, 0.3, Vec3::ZERO);
        assert_eq!(throw.speed, tuning.speed);
        assert!((throw.direction.y - 0.3_f32.sin()).abs() < 1e-5);
    }

    fn feed(lethal: (bool, bool), tactical: (bool, bool)) -> ThrowFeed {
        ThrowFeed {
            lethal: KeyState {
                pressed: lethal.0,
                held: lethal.1,
            },
            tactical: KeyState {
                pressed: tactical.0,
                held: tactical.1,
            },
            lethal_ready: true,
            tactical_ready: true,
            allowed: true,
        }
    }

    #[test]
    fn pressing_readies_and_only_letting_go_throws() {
        let mut ready = ReadiedThrow::default();
        assert_eq!(ready.step(&feed((true, true), (false, false))), None);
        assert_eq!(ready.0, Some(PouchSlot::Lethal), "readied on the press");
        for _ in 0..30 {
            assert_eq!(ready.step(&feed((false, true), (false, false))), None);
        }
        assert_eq!(
            ready.step(&feed((false, false), (false, false))),
            Some(PouchSlot::Lethal)
        );
        assert_eq!(ready.0, None);
        assert_eq!(
            ready.step(&feed((false, false), (false, false))),
            None,
            "one throw per press"
        );
    }

    #[test]
    fn a_tap_within_one_frame_still_throws() {
        let mut ready = ReadiedThrow::default();
        assert_eq!(
            ready.step(&feed((false, false), (true, false))),
            Some(PouchSlot::Tactical)
        );
    }

    #[test]
    fn a_second_key_waits_while_one_is_readied_and_the_lethal_wins_a_tie() {
        let mut ready = ReadiedThrow::default();
        ready.step(&feed((true, true), (true, true)));
        assert_eq!(ready.0, Some(PouchSlot::Lethal));
        // Q goes down later: the readied frag is still the one that throws.
        ready.step(&feed((false, true), (true, true)));
        assert_eq!(
            ready.step(&feed((false, false), (false, true))),
            Some(PouchSlot::Lethal)
        );
        // The held Q was no new press: nothing is readied by it.
        assert_eq!(ready.step(&feed((false, false), (false, true))), None);
        assert_eq!(ready.0, None);
    }

    #[test]
    fn a_throw_is_cancelled_when_the_player_may_no_longer_throw() {
        // Death, a menu or overlay, a weapon switch: `allowed` goes false, the key comes up later.
        let mut ready = ReadiedThrow::default();
        ready.step(&feed((true, true), (false, false)));
        let mut blocked = feed((false, true), (false, false));
        blocked.allowed = false;
        assert_eq!(ready.step(&blocked), None);
        assert_eq!(ready.0, None);
        assert_eq!(
            ready.step(&feed((false, false), (false, false))),
            None,
            "letting go afterwards throws nothing"
        );
        // A press while blocked readies nothing, and nothing is left over after.
        let mut pressed = feed((true, true), (false, false));
        pressed.allowed = false;
        assert_eq!(ready.step(&pressed), None);
        assert_eq!(ready.step(&feed((false, false), (false, false))), None);
    }

    #[test]
    fn nothing_is_readied_without_one_in_the_pouch_and_a_spent_one_cancels() {
        let mut ready = ReadiedThrow::default();
        let mut empty = feed((true, true), (false, false));
        empty.lethal_ready = false;
        assert_eq!(ready.step(&empty), None);
        assert_eq!(ready.0, None, "an empty pouch is not readied");
        // The count went away while held (a new life's pouch, say): cancelled at the release.
        ready.step(&feed((true, true), (false, false)));
        let mut gone = feed((false, false), (false, false));
        gone.lethal_ready = false;
        assert_eq!(ready.step(&gone), None);
        assert_eq!(ready.0, None);
    }

    #[test]
    fn a_pouch_takes_one_per_cooldown_and_reports_it() {
        let tuning = GrenadeTuning { count: 2, ..frag() };
        let mut pouch = Pouch {
            kind: GrenadeKind::Flashbang,
            left: 2,
            cooldown: 0.0,
        };
        assert!(pouch.can_take() && pouch.take(&tuning));
        assert!(!pouch.can_take() && !pouch.take(&tuning), "cooling down");
        pouch.cooldown = 0.0;
        assert!(pouch.take(&tuning));
        pouch.cooldown = 0.0;
        assert!(!pouch.can_take(), "none left");
    }

    #[test]
    fn the_shipped_grenades_are_all_there_and_sane() {
        let data = crate::data::GameData::load();
        for kind in GrenadeKind::ALL {
            let tuning = data.grenade(kind);
            tuning.check("equipment.ron", kind);
        }
        for id in EquipmentId::ALL {
            let kind = GrenadeKind::from(id);
            assert!(data.equipment.contains_key(&kind), "{id:?} has no tuning");
        }
        // The lethal ones kill up close; the launcher's round has a real arming distance.
        let Effect::Blast { damage, .. } = data.grenade(GrenadeKind::Frag).effect else {
            panic!("the frag blasts");
        };
        assert!(damage >= 100.0);
        assert!(matches!(
            data.grenade(GrenadeKind::Launcher).fuse,
            Fuse::Impact { arm_distance, .. } if arm_distance > 0.0
        ));
    }

    fn semtex() -> GrenadeTuning {
        GrenadeTuning {
            fuse: Fuse::Stick {
                secs: 1.5,
                max_air_secs: 5.0,
            },
            restitution: 0.0,
            friction: 0.0,
            ..frag()
        }
    }

    fn mine(max_secs: f32) -> GrenadeTuning {
        GrenadeTuning {
            fuse: Fuse::Proximity {
                arm_secs: 1.5,
                trigger_radius: 2.6,
                max_secs,
            },
            restitution: 0.0,
            friction: 0.0,
            ..frag()
        }
    }

    #[test]
    fn a_stick_fuse_sticks_where_it_lands_and_goes_off_its_seconds_after() {
        let tuning = semtex();
        let direction = Vec3::new(1.0, 0.1, 0.0).normalize();
        let mut body = Body::thrown(Vec3::new(0.0, 1.6, 0.0), direction, 17.0);
        let (mut stuck, mut ticks) = (None, 0u32);
        loop {
            ticks += 1;
            match body.step(&tuning, DT, wall) {
                Step::Stuck => stuck = Some(ticks),
                Step::Detonate => break,
                Step::Bounced => panic!("a Semtex never bounces"),
                Step::Flying => {}
            }
            assert!(ticks < 2000, "never went off");
        }
        let stuck = stuck.expect("it stuck to the wall");
        assert!(body.anchored);
        assert_eq!(body.velocity, Vec3::ZERO);
        assert!(
            body.position.x > 9.8 && body.position.x < 10.0,
            "on the wall: {}",
            body.position
        );
        let waited = (ticks - stuck) as f32 * DT;
        assert!((waited - 1.5).abs() < 2.0 * DT, "1.5 s after: {waited}");
    }

    #[test]
    fn a_stick_fuse_that_touches_nothing_goes_off_in_the_air() {
        let tuning = semtex();
        let mut body = Body::thrown(Vec3::new(0.0, 1.6, 0.0), Vec3::Y, 10.0);
        let ticks = run_until_detonation(&mut body, &tuning, |_, _, _| None);
        assert!(body.stuck_at.is_none());
        assert!((body.age - 5.0).abs() < 2.0 * DT, "{ticks} ticks");
    }

    #[test]
    fn a_held_grenade_stays_put_and_a_released_one_falls_with_its_fuse_running() {
        let tuning = semtex();
        let start = Vec3::new(0.0, 2.0, 0.0);
        let mut body = Body::thrown(start, Vec3::X, 5.0);
        body.stick();
        for _ in 0..32 {
            assert_eq!(body.step(&tuning, DT, |_, _, _| None), Step::Flying);
        }
        assert_eq!(body.position, start, "held where it is");
        body.release();
        body.step(&tuning, DT, |_, _, _| None);
        assert!(body.position.y < start.y, "it drops");
        run_until_detonation(&mut body, &tuning, |_, _, _| None);
        assert!(
            (body.age - 1.5).abs() < 2.0 * DT,
            "the fuse counted on from the first stick: {}",
            body.age
        );
    }

    #[test]
    fn a_mine_arms_after_it_settles_and_goes_off_at_its_limit() {
        let tuning = mine(3.0);
        let mut body = Body::thrown(Vec3::new(0.0, 0.5, 0.0), Vec3::NEG_Y, 5.0);
        assert!(!body.armed(tuning.fuse), "not in the air");
        let (mut armed_at, mut ticks) = (None, 0u32);
        loop {
            ticks += 1;
            let step = body.step(&tuning, DT, floor);
            if armed_at.is_none() && body.armed(tuning.fuse) {
                armed_at = Some(ticks);
            }
            if step == Step::Detonate {
                break;
            }
            assert!(ticks < 2000, "never went off");
        }
        let stuck = body.stuck_at.expect("it stuck to the floor");
        let armed = armed_at.expect("it armed") as f32 * DT;
        assert!((armed - (stuck + 1.5)).abs() < 2.0 * DT, "armed at {armed}");
        assert!((body.age - 3.0).abs() < 2.0 * DT, "gives up at its limit");
        // One that lets go of its surface is not armed until it sticks again.
        body.release();
        assert!(!body.armed(tuning.fuse));
        // Only a mine arms.
        let mut other = Body::thrown(Vec3::ZERO, Vec3::X, 1.0);
        other.stick();
        assert!(!other.armed(semtex().fuse));
    }

    #[test]
    fn a_mine_trips_only_armed_in_reach_and_in_the_open() {
        assert!(mine_trips(true, 2.6, 2.0, true));
        assert!(mine_trips(true, 2.6, 2.6, true), "the edge counts");
        assert!(!mine_trips(true, 2.6, 2.7, true), "out of reach");
        assert!(!mine_trips(false, 2.6, 1.0, true), "still arming");
        assert!(!mine_trips(true, 2.6, 1.0, false), "a wall between");
    }

    #[test]
    fn a_player_keeps_at_most_their_count_of_mines_and_the_oldest_go() {
        let mut down = [(1, 3.0), (2, 9.0), (3, 0.5), (4, 6.0)];
        let gone: Vec<i32> = oldest_beyond_cap(&mut down, 2)
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(gone, [2, 4]);
        let mut few = [(1, 1.0)];
        assert!(oldest_beyond_cap(&mut few, 2).is_empty());
        let mut none: [(i32, f32); 0] = [];
        assert!(oldest_beyond_cap(&mut none, 0).is_empty());
    }

    #[test]
    fn grenades_and_clouds_are_capped() {
        assert!(room_for(false, 0, 0));
        assert!(!room_for(false, MAX_GRENADES, 0), "too many Grenades");
        assert!(
            room_for(false, 1, MAX_CLOUDS),
            "a non-cloud ignores the cloud cap"
        );
        assert!(room_for(true, 1, MAX_CLOUDS - 1));
        assert!(!room_for(true, 1, MAX_CLOUDS), "too many clouds");
    }

    #[test]
    fn a_throw_touches_a_body_it_passes_through_even_within_one_step() {
        let feet = Vec3::new(5.0, 0.0, 0.0);
        let through = touches_body(
            Vec3::new(3.0, 1.2, 0.0),
            Vec3::new(7.0, 1.2, 0.0),
            0.06,
            feet,
            1.8,
        );
        let at = through.expect("it goes through the body");
        assert!(at.distance(Vec3::new(5.0, 1.2, 0.0)) < 1e-4, "{at}");
        let miss = |from: Vec3, to: Vec3| touches_body(from, to, 0.06, feet, 1.8);
        assert!(
            miss(Vec3::new(3.0, 1.2, 1.0), Vec3::new(7.0, 1.2, 1.0)).is_none(),
            "beside"
        );
        assert!(
            miss(Vec3::new(3.0, 2.2, 0.0), Vec3::new(7.0, 2.2, 0.0)).is_none(),
            "over"
        );
        assert!(
            miss(Vec3::new(3.0, -0.5, 0.0), Vec3::new(7.0, -0.5, 0.0)).is_none(),
            "under"
        );
        assert!(
            miss(Vec3::new(3.0, 1.2, 0.0), Vec3::new(4.0, 1.2, 0.0)).is_none(),
            "short"
        );
        // Standing still inside the body counts as touching it.
        assert!(miss(Vec3::new(5.1, 1.0, 0.0), Vec3::new(5.1, 1.0, 0.0)).is_some());
    }

    #[test]
    fn a_semtex_may_not_stick_to_its_thrower_at_first() {
        assert!(!can_stick_to(true, 0.1));
        assert!(can_stick_to(true, 0.5), "later it may");
        assert!(can_stick_to(false, 0.0), "anyone else at once");
    }

    #[test]
    fn a_stun_fades_with_distance_and_is_weaker_behind_cover() {
        assert_eq!(stun_exposure(12.0, 0.0, true), 1.0);
        assert!((stun_exposure(12.0, 6.0, true) - 0.5).abs() < 1e-6);
        assert_eq!(stun_exposure(12.0, 12.0, true), 0.0, "at the edge");
        assert_eq!(stun_exposure(12.0, 30.0, true), 0.0);
        let behind = stun_exposure(12.0, 6.0, false);
        assert!(behind > 0.0 && behind < stun_exposure(12.0, 6.0, true));
        // It doesn't matter where they look: there is no view argument at all.
    }

    #[test]
    fn a_slow_takes_its_share_off_the_speed_in_proportion_to_its_strength() {
        assert_eq!(slow_scale(0.5, 0.0), 1.0);
        assert_eq!(slow_scale(0.5, 1.0), 0.5);
        assert!((slow_scale(0.5, 0.5) - 0.75).abs() < 1e-6);
        assert_eq!(slow_scale(2.0, 1.0), 0.0, "never below standing still");
        assert_eq!(slow_scale(0.5, 3.0), 0.5, "strength is capped");
    }

    #[test]
    fn gas_narrows_the_view_to_a_tunnel() {
        assert_eq!(tunnel_inner_percent(0.0), 100.0);
        assert_eq!(tunnel_inner_percent(1.0), TUNNEL_NARROWEST_PERCENT);
        assert!(tunnel_inner_percent(0.5) < tunnel_inner_percent(0.2));
        assert_eq!(tunnel_inner_percent(-1.0), 100.0);
        assert_eq!(tunnel_inner_percent(9.0), TUNNEL_NARROWEST_PERCENT);
    }

    #[test]
    fn gas_exposure_follows_the_cloud_as_it_billows_and_thins() {
        let centre = Vec3::new(4.0, 1.0, 4.0);
        let secs = 16.0;
        // In the middle, in the held stretch, fully; outside, not at all.
        assert_eq!(cloud_exposure(centre, centre, 5.0, 8.0, secs), 1.0);
        assert_eq!(
            cloud_exposure(centre + Vec3::X * 6.0, centre, 5.0, 8.0, secs),
            0.0
        );
        // It builds up as it billows in, and is gone when the cloud ends.
        assert!(cloud_exposure(centre, centre, 5.0, 0.3, secs) < 0.5);
        assert_eq!(cloud_exposure(centre, centre, 5.0, secs, secs), 0.0);
        // A third of the way in is as good as the middle.
        let third = centre + Vec3::X * (5.0 * 2.0 / 3.0);
        assert!((cloud_exposure(third, centre, 5.0, 8.0, secs) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn gas_builds_up_quickly_and_wears_off_slowly() {
        let up = linger(0.0, 1.0, 2.5, 0.45, 0.1);
        assert!((up - 0.25).abs() < 1e-6);
        let down = linger(1.0, 0.0, 2.5, 0.45, 0.1);
        assert!((down - 0.955).abs() < 1e-5);
        assert_eq!(
            linger(0.9, 1.0, 2.5, 0.45, 1.0),
            1.0,
            "never past the target"
        );
        assert_eq!(linger(0.1, 0.0, 2.5, 0.45, 1.0), 0.0);
        assert_eq!(linger(0.5, 0.5, 2.5, 0.45, 1.0), 0.5);
    }

    #[test]
    fn a_cough_dips_the_view_now_and_then_in_proportion_to_the_gas() {
        assert!(
            (cough_dip(COUGH_SECS / 2.0, 1.0) - COUGH_DIP).abs() < 1e-5,
            "the deepest point"
        );
        assert_eq!(cough_dip(1.0, 1.0), 0.0, "between coughs");
        let again = cough_dip(COUGH_PERIOD_SECS + COUGH_SECS / 2.0, 1.0);
        assert!((again - COUGH_DIP).abs() < 1e-5, "they repeat");
        assert!(cough_dip(COUGH_SECS / 2.0, 0.5) < cough_dip(COUGH_SECS / 2.0, 1.0));
        assert_eq!(cough_dip(COUGH_SECS / 2.0, 0.0), 0.0, "no gas, no cough");
    }

    #[test]
    fn a_stun_shakes_the_view_within_bounds_and_not_at_all_at_zero() {
        assert_eq!(stun_shake(1.234, 0.0), Vec3::ZERO);
        let mut widest = Vec3::ZERO;
        for step in 0..2000 {
            widest = widest.max(stun_shake(step as f32 * 0.01, 1.0).abs());
        }
        assert!(
            widest.x > 0.01 && widest.y > 0.01 && widest.z > 0.01,
            "it does shake"
        );
        assert!(widest.cmple(STUN_SHAKE * 1.0001).all(), "{widest}");
        let half = stun_shake(0.7, 0.5);
        let full = stun_shake(0.7, 1.0);
        assert!((half - full * 0.5).length() < 1e-6, "in proportion");
    }

    #[test]
    fn the_shipped_new_items_have_the_rules_they_are_meant_to() {
        let data = crate::data::GameData::load();
        let frag = data.grenade(GrenadeKind::Frag);
        let semtex = data.grenade(GrenadeKind::Semtex);
        assert!(semtex.fuse.sticks_to_players());
        let (
            Effect::Blast {
                radius: frag_radius,
                ..
            },
            Effect::Blast { damage, radius },
        ) = (frag.effect, semtex.effect)
        else {
            panic!("both blast");
        };
        assert!(radius < frag_radius, "a smaller blast than the Frag");
        assert!(damage >= 100.0, "still lethal");
        let mine = data.grenade(GrenadeKind::Mine);
        assert!(mine.fuse.is_mine() && mine.fuse.sticks());
        assert!(
            !mine.fuse.sticks_to_players(),
            "a mine never rides a player"
        );
        assert!(mine.count >= 2, "a player can lay a few");
        assert!(matches!(mine.effect, Effect::Blast { .. }));
        assert!(matches!(
            data.grenade(GrenadeKind::Stun).effect,
            Effect::Stun { .. }
        ));
        let Effect::Gas { secs, slow, .. } = data.grenade(GrenadeKind::TearGas).effect else {
            panic!("tear gas is a gas");
        };
        assert!(
            secs > SMOKE_FADE_IN_SECS + SMOKE_FADE_OUT_SECS,
            "it holds a while"
        );
        assert!(slow > 0.0 && slow < 1.0, "it slows, it doesn't pin");
        for kind in [GrenadeKind::Stun, GrenadeKind::TearGas, GrenadeKind::Smoke] {
            assert!(
                !matches!(data.grenade(kind).effect, Effect::Blast { .. }),
                "{kind:?} does no damage"
            );
        }
    }
}
