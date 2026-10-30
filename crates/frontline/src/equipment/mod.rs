//! Equipment (CONTEXT.md: Grenade): the lethal Frag, the tactical Flashbang and Smoke and the
//! launcher's 40mm round. A Grenade is a physical projectile the server flies (bounce, fuse) and
//! whose effect it applies (blast damage through the damage path, flash, smoke); clients draw it
//! and show the effects (`client`).
//!
//! This module holds the data (`data/equipment.ron`) and the pure, unit-tested rules: stepping a
//! Grenade body with bounces and the fuse, the blast's damage falloff, how a flash blinds and
//! deafens a viewer and how dense a smoke cloud is as it ages. The server system is
//! `combat/grenades.rs`.

pub mod client;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::class::EquipmentId;
use crate::data::GameData;
use crate::gear::{AttachmentId, Loadout};

/// A thrown or launched body, by what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GrenadeKind {
    Frag,
    Flashbang,
    Smoke,
    /// The 40mm round of the underslung grenade launcher (the M16A4's "noob tube").
    Launcher,
}

impl GrenadeKind {
    pub const ALL: [GrenadeKind; 4] = [
        GrenadeKind::Frag,
        GrenadeKind::Flashbang,
        GrenadeKind::Smoke,
        GrenadeKind::Launcher,
    ];
}

impl From<EquipmentId> for GrenadeKind {
    fn from(id: EquipmentId) -> Self {
        match id {
            EquipmentId::Frag => GrenadeKind::Frag,
            EquipmentId::Flashbang => GrenadeKind::Flashbang,
            EquipmentId::Smoke => GrenadeKind::Smoke,
        }
    }
}

/// A Grenade in the air or on the ground, replicated from the server to everyone (the server
/// owns its `Body`); clients draw it at `position`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grenade {
    pub kind: GrenadeKind,
    pub position: Vec3,
}

/// When a Grenade goes off.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub enum Fuse {
    /// This long after it left the hand.
    Timed(f32),
    /// On the first surface it meets after flying `arm_distance`, or after `max_secs` in the air.
    Impact { arm_distance: f32, max_secs: f32 },
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
    /// A cloud of `radius` that blocks the view for `secs`.
    Smoke { radius: f32, secs: f32 },
}

/// One Grenade's tuning (`data/equipment.ron`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct GrenadeTuning {
    pub name: String,
    pub speed: f32,
    pub gravity: f32,
    pub restitution: f32,
    pub friction: f32,
    pub radius: f32,
    pub fuse: Fuse,
    pub effect: Effect,
    pub count: u32,
    pub cooldown_secs: f32,
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
        let fuse_ok = match self.fuse {
            Fuse::Timed(secs) => secs > 0.0,
            Fuse::Impact {
                arm_distance,
                max_secs,
            } => arm_distance >= 0.0 && max_secs > 0.0,
        };
        let effect_ok = match self.effect {
            Effect::Blast { damage, radius } => damage > 0.0 && radius > 0.0,
            Effect::Flash {
                radius,
                blind_secs,
                deaf_secs,
            } => radius > 0.0 && blind_secs >= 0.0 && deaf_secs >= 0.0,
            Effect::Smoke { radius, secs } => radius > 0.0 && secs > 0.0,
        };
        assert!(
            fuse_ok && effect_ok,
            "{file}: {kind:?}'s fuse or effect has a number out of range"
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
}

impl Body {
    /// A Grenade leaving `origin` along `direction` (a unit vector) at the tuning's speed.
    pub fn thrown(origin: Vec3, direction: Vec3, tuning: &GrenadeTuning) -> Self {
        Self {
            position: origin,
            velocity: direction * tuning.speed,
            age: 0.0,
            travelled: 0.0,
            resting: false,
        }
    }

    /// Advances `dt` seconds: gravity, the move (stopped and bounced by the first surface
    /// `cast` finds on the way: it is given the centre, a unit direction and the length to
    /// sweep a sphere of the Grenade's radius along) and the fuse.
    pub fn step(
        &mut self,
        tuning: &GrenadeTuning,
        dt: f32,
        cast: impl FnOnce(Vec3, Vec3, f32) -> Option<Contact>,
    ) -> Step {
        self.age += dt;
        let mut bounced = false;
        if !self.resting {
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
                        let into = self.velocity.dot(contact.normal).min(0.0);
                        let sideways = self.velocity - contact.normal * into;
                        self.velocity =
                            sideways * tuning.friction - contact.normal * into * tuning.restitution;
                        if self.velocity.length() < REST_SPEED && contact.normal.y >= REST_NORMAL_Y
                        {
                            self.velocity = Vec3::ZERO;
                            self.resting = true;
                        }
                        bounced = true;
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
        };
        if detonate {
            Step::Detonate
        } else if bounced {
            Step::Bounced
        } else {
            Step::Flying
        }
    }
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
    if radius <= 0.0 || distance >= radius {
        return FlashExposure {
            blind: 0.0,
            deaf: 0.0,
        };
    }
    let near = 1.0 - distance.max(0.0) / radius;
    let (away, towards) = BLIND_FACING;
    let facing = ((view_dot - away) / (towards - away)).clamp(0.0, 1.0);
    FlashExposure {
        blind: if in_sight { near * facing } else { 0.0 },
        deaf: near,
    }
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

    /// Takes one for a throw, starting the wait; false when none is left or the last throw is
    /// too recent.
    pub fn take(&mut self, tuning: &GrenadeTuning) -> bool {
        if self.left == 0 || self.cooldown > 0.0 {
            return false;
        }
        self.left -= 1;
        self.cooldown = tuning.cooldown_secs;
        true
    }
}

/// Which pouch a throw key draws from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PouchSlot {
    Lethal,
    Tactical,
    /// The underslung grenade launcher's rounds.
    Launcher,
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
            PouchSlot::Launcher => self.launcher.as_mut(),
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
        let mut body = Body::thrown(Vec3::new(0.0, 1.6, 0.0), Vec3::Y, &tuning);
        let ticks = run_until_detonation(&mut body, &tuning, |_, _, _| None);
        assert!((191..=193).contains(&ticks), "3 s of 1/64 s ticks: {ticks}");
        assert!((body.age - 3.0).abs() < DT);

        // A different tick length reaches the same age.
        let mut coarse = Body::thrown(Vec3::new(0.0, 1.6, 0.0), Vec3::Y, &tuning);
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
        let mut body = Body::thrown(Vec3::new(0.0, 1.6, 0.0), direction, &tuning);
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
            &tuning,
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
        let mut body = Body::thrown(Vec3::new(0.0, 5.0, 0.0), Vec3::X, &tuning);
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
        let mut body = Body::thrown(Vec3::new(0.0, 1.5, 0.0), Vec3::X, &tuning);
        let ticks = run_until_detonation(&mut body, &tuning, wall);
        assert!(
            body.position.x > 9.8,
            "it burst at the wall: {:?}",
            body.position
        );
        assert!(ticks < 30, "{ticks}");

        // Too close to arm: it bounces instead and goes off when `max_secs` runs out.
        let mut close = Body::thrown(Vec3::new(5.0, 1.5, 0.0), Vec3::X, &tuning);
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
}
