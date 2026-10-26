//! The local player's Weapon: firing, ammo and reloads, aiming down sights and recoil, plus
//! its viewmodel, effects and HUD. Client and Host only; the server decides what shots hit.

mod effects;
mod hud;
mod viewmodel;

use std::hash::{BuildHasher, RandomState};

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::data::{GameData, WeaponId, WeaponStats};
use crate::net::protocol::{FireShot, Respawn};
use crate::player::input::Bindings;
use crate::player::{
    ControllerModifiers, ControllerSystems, EyeCamera, LocalLife, Motor, MouseCaptured,
    PITCH_LIMIT,
};
use crate::shared::{FromServer, GameLayer, LocalPlayer, PlayerPose, ToServer};

pub struct WeaponClientPlugin;

impl Plugin for WeaponClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WeaponRng::seeded())
            .add_message::<ShotFired>()
            .add_observer(equip_local_player)
            .add_plugins((
                viewmodel::ViewmodelPlugin,
                effects::EffectsPlugin,
                hud::WeaponHudPlugin,
            ))
            .add_systems(
                Update,
                (reset_on_respawn, aim, reload, fire, recover_recoil)
                    .chain()
                    .in_set(WeaponSystems)
                    .after(ControllerSystems::Look)
                    .before(ControllerSystems::Move),
            );
    }
}

/// Weapon logic: trigger, aim, reload and recoil. Presentation runs after it.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct WeaponSystems;

/// The Weapon the local player holds and its moment-to-moment state.
#[derive(Component, Debug)]
struct HeldWeapon {
    id: WeaponId,
    rounds: u32,
    /// Seconds left of the reload in progress.
    reload_left: Option<f32>,
    /// Seconds until the next round may fire; non-positive when ready.
    cooldown: f32,
    /// Trigger held, counting only presses made while the cursor was already grabbed.
    trigger: bool,
    /// Aim-down-sights blend: 0 at the hip, 1 fully aimed.
    ads: f32,
    /// Recoil not yet recovered, as (yaw, pitch) in radians.
    recoil: Vec2,
}

impl HeldWeapon {
    fn new(id: WeaponId, stats: &WeaponStats) -> Self {
        Self {
            id,
            rounds: stats.magazine,
            reload_left: None,
            cooldown: 0.0,
            trigger: false,
            ads: 0.0,
            recoil: Vec2::ZERO,
        }
    }

    fn start_reload(&mut self, stats: &WeaponStats) {
        if self.reload_left.is_none() && self.rounds < stats.magazine {
            info!("Reloading ({} rounds left)", self.rounds);
            self.reload_left = Some(stats.reload_secs);
        }
    }

    /// Reload progress from 0 to 1, if reloading.
    fn reload_progress(&self, stats: &WeaponStats) -> Option<f32> {
        self.reload_left
            .map(|left| 1.0 - left / stats.reload_secs.max(f32::EPSILON))
    }

    /// Advances a reload in progress by `dt` seconds; sprinting cancels it.
    fn tick_reload(&mut self, stats: &WeaponStats, dt: f32, sprinting: bool) {
        let Some(left) = self.reload_left else {
            return;
        };
        if sprinting {
            info!("Reload cancelled by sprinting");
            self.reload_left = None;
        } else if left <= dt {
            self.rounds = stats.magazine;
            self.reload_left = None;
            info!("Reloaded ({} rounds)", self.rounds);
        } else {
            self.reload_left = Some(left - dt);
        }
    }

    /// Runs the trigger for a frame of `dt` seconds and returns how many rounds leave the
    /// barrel: as many as the rpm owes since the last shot, so a long frame fires several.
    /// Running dry starts a reload.
    fn rounds_to_fire(&mut self, stats: &WeaponStats, dt: f32) -> u32 {
        self.cooldown -= dt;
        let mut fired = 0;
        if self.trigger && self.reload_left.is_none() {
            while self.cooldown <= 0.0 {
                if self.rounds == 0 {
                    self.start_reload(stats);
                    break;
                }
                self.rounds -= 1;
                self.cooldown += stats.seconds_per_shot();
                fired += 1;
            }
        }
        self.cooldown = self.cooldown.max(0.0);
        fired
    }

    /// The ADS blend eased in and out.
    fn ads_eased(&self) -> f32 {
        let t = self.ads;
        t * t * (3.0 - 2.0 * t)
    }
}

/// One local round left the barrel: `muzzle` is the viewmodel muzzle in world space and
/// `end` where the client-side ray met the World (or maximum range).
#[derive(Message, Clone, Copy, Debug)]
struct ShotFired {
    muzzle: Vec3,
    end: Vec3,
}

/// Small xorshift generator for spread, recoil and cosmetic jitter.
#[derive(Resource, Debug)]
struct WeaponRng(u64);

impl WeaponRng {
    fn seeded() -> Self {
        Self(RandomState::new().hash_one(0_u8) | 1)
    }

    /// Uniform in `[0, 1)`.
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1_u64 << 24) as f32
    }

    /// Uniform in `[-1, 1)`.
    fn signed(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

/// Camera rotation for a view yaw and pitch.
fn aim_rotation(yaw: f32, pitch: f32) -> Quat {
    Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch)
}

/// Wraps an angle into `[-PI, PI)`, like the controller keeps `PlayerPose::yaw`.
fn wrap_angle(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (angle + PI).rem_euclid(TAU) - PI
}

fn equip_local_player(add: On<Add<LocalPlayer>>, data: Res<GameData>, mut commands: Commands) {
    let id = WeaponId::AssaultRifle;
    commands
        .entity(add.entity)
        .insert(HeldWeapon::new(id, data.weapon(id)));
}

/// A respawn hands out a fresh magazine and a settled aim.
fn reset_on_respawn(
    mut respawns: MessageReader<FromServer<Respawn>>,
    data: Res<GameData>,
    mut weapons: Query<&mut HeldWeapon, With<LocalPlayer>>,
) {
    if respawns.read().count() == 0 {
        return;
    }
    for mut weapon in &mut weapons {
        let id = weapon.id;
        *weapon = HeldWeapon::new(id, data.weapon(id));
    }
}

/// Blends toward the sights while the aim button is held, and feeds the controller hooks.
fn aim(
    time: Res<Time>,
    data: Res<GameData>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<Bindings>,
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    mut player: Query<(&mut HeldWeapon, &mut ControllerModifiers), With<LocalPlayer>>,
) {
    let Ok((mut weapon, mut modifiers)) = player.single_mut() else {
        return;
    };
    let stats = data.weapon(weapon.id);
    let wants = captured.active()
        && mouse.pressed(bindings.aim)
        && !life.is_dead()
        && weapon.reload_left.is_none();
    let step = time.delta_secs() / stats.ads_secs.max(f32::EPSILON);
    weapon.ads = if wants {
        (weapon.ads + step).min(1.0)
    } else {
        (weapon.ads - step).max(0.0)
    };
    let t = weapon.ads_eased();
    modifiers.fov_scale = 1.0_f32.lerp(stats.ads_fov_scale, t);
    modifiers.move_scale = 1.0_f32.lerp(stats.ads_move_scale, t);
    modifiers.sprint_blocked = wants;
}

/// R reloads; sprinting cancels a reload in progress.
fn reload(
    time: Res<Time>,
    data: Res<GameData>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    mut player: Query<(&mut HeldWeapon, &Motor), With<LocalPlayer>>,
) {
    let Ok((mut weapon, motor)) = player.single_mut() else {
        return;
    };
    if life.is_dead() {
        return;
    }
    let stats = data.weapon(weapon.id);
    if captured.active() && bindings.reload.just_pressed(&keys) {
        weapon.start_reload(stats);
    }
    weapon.tick_reload(stats, time.delta_secs(), motor.sprinting());
}

/// Full-auto fire at the Weapon's rpm. Each round goes to the server with spread and the
/// current recoil already applied, then kicks the view.
fn fire(
    time: Res<Time>,
    data: Res<GameData>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<Bindings>,
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    spatial: SpatialQuery,
    mut rng: ResMut<WeaponRng>,
    mut player: Query<(&mut PlayerPose, &mut HeldWeapon), With<LocalPlayer>>,
    eye: Query<&EyeCamera>,
    gun: Query<&Transform, With<viewmodel::ViewmodelGun>>,
    mut shots: MessageWriter<ToServer<FireShot>>,
    mut fired: MessageWriter<ShotFired>,
) {
    let Ok((mut pose, mut weapon)) = player.single_mut() else {
        return;
    };
    let stats = data.weapon(weapon.id);
    let held = captured.active() && mouse.pressed(bindings.fire);
    let pulled = mouse.just_pressed(bindings.fire) && !captured.is_changed();
    weapon.trigger = held && !life.is_dead() && (weapon.trigger || pulled);

    let rounds = weapon.rounds_to_fire(stats, time.delta_secs());
    if rounds > 0 {
        let eye_height = eye.single().map_or(pose.stance.eye_height(), EyeCamera::height);
        let origin = pose.position + Vec3::Y * eye_height;
        let gun = gun.single().copied().unwrap_or_default();
        let filter = SpatialQueryFilter::from_mask(GameLayer::World);
        for _ in 0..rounds {
            let t = weapon.ads_eased();
            let aim = aim_rotation(pose.yaw, pose.pitch);
            let spread = stats.hip_spread_deg.lerp(stats.ads_spread_deg, t).to_radians();
            let (angle, radius) = (rng.next() * std::f32::consts::TAU, spread * rng.next().sqrt());
            let offset = Quat::from_rotation_y(radius * angle.cos())
                * Quat::from_rotation_x(radius * angle.sin());
            let direction = Dir3::new_unchecked((aim * offset * Vec3::NEG_Z).normalize());

            shots.write(ToServer(FireShot {
                weapon: weapon.id,
                origin,
                direction: *direction,
            }));
            let end = spatial
                .cast_ray(origin, direction, stats.range, true, &filter)
                .map_or(stats.range, |hit| hit.distance);
            let muzzle = Transform::from_translation(origin)
                .with_rotation(aim)
                .transform_point(gun.transform_point(viewmodel::MUZZLE));
            fired.write(ShotFired {
                muzzle,
                end: origin + *direction * end,
            });

            let kick = 1.0_f32.lerp(stats.ads_recoil_scale, t);
            let pitch_kick = stats.recoil_pitch_deg.to_radians() * kick;
            let yaw_kick = stats.recoil_yaw_deg.to_radians() * kick * rng.signed();
            let pitch = (pose.pitch + pitch_kick).min(PITCH_LIMIT);
            weapon.recoil += Vec2::new(yaw_kick, pitch - pose.pitch);
            pose.pitch = pitch;
            pose.yaw = wrap_angle(pose.yaw + yaw_kick);
        }
    }
}

/// Returns the view toward where the player was aiming before the kicks.
fn recover_recoil(
    time: Res<Time>,
    data: Res<GameData>,
    mut player: Query<(&mut PlayerPose, &mut HeldWeapon), With<LocalPlayer>>,
) {
    let Ok((mut pose, mut weapon)) = player.single_mut() else {
        return;
    };
    if weapon.recoil == Vec2::ZERO {
        return;
    }
    let rate = data.weapon(weapon.id).recoil_recovery_per_sec;
    let mut step = weapon.recoil * (1.0 - (-rate * time.delta_secs()).exp());
    if weapon.recoil.length_squared() < 1e-10 {
        step = weapon.recoil;
    }
    weapon.recoil -= step;
    pose.yaw = wrap_angle(pose.yaw - step.x);
    pose.pitch = (pose.pitch - step.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame of `reload` then `fire`, as the systems are chained; returns rounds fired.
    fn frame(weapon: &mut HeldWeapon, stats: &WeaponStats, dt: f32) -> u32 {
        weapon.tick_reload(stats, dt, false);
        weapon.rounds_to_fire(stats, dt)
    }

    #[test]
    fn fire_rate_holds_at_any_frame_rate_and_idle_time_banks_nothing() {
        // 480 rpm: a round every 0.125 s, so holding for 1 s fires at 0, 0.125, ..., 1.0.
        let stats = WeaponStats { magazine: 100, ..WeaponStats::fixture() };

        let mut low_fps = HeldWeapon::new(WeaponId::AssaultRifle, &stats);
        low_fps.trigger = true;
        let fired: u32 = (0..4).map(|_| frame(&mut low_fps, &stats, 0.25)).sum();
        assert_eq!(fired, 9);

        let mut hitch = HeldWeapon::new(WeaponId::AssaultRifle, &stats);
        hitch.trigger = true;
        assert_eq!(frame(&mut hitch, &stats, 1.0), 9);

        // A second with the trigger released doesn't let the next pull fire faster.
        let mut idle = HeldWeapon::new(WeaponId::AssaultRifle, &stats);
        assert_eq!(frame(&mut idle, &stats, 1.0), 0);
        idle.trigger = true;
        assert_eq!(frame(&mut idle, &stats, 0.25), 3);
    }

    #[test]
    fn running_dry_reloads_then_fires_again_unless_sprint_cancels() {
        let stats = WeaponStats { magazine: 3, ..WeaponStats::fixture() };
        let mut weapon = HeldWeapon::new(WeaponId::AssaultRifle, &stats);
        weapon.trigger = true;

        assert_eq!(frame(&mut weapon, &stats, 1.0), 3);
        assert_eq!(weapon.rounds, 0);
        assert!(weapon.reload_left.is_some(), "an empty magazine starts a reload");
        assert_eq!(frame(&mut weapon, &stats, 0.5), 0, "no rounds while reloading");
        assert_eq!(frame(&mut weapon, &stats, 0.5), 3, "fires again once reloaded");

        // Empty again and reloading: sprinting cancels without refilling.
        weapon.tick_reload(&stats, 0.1, true);
        assert_eq!(weapon.reload_left, None);
        assert_eq!(weapon.rounds, 0);
    }
}
