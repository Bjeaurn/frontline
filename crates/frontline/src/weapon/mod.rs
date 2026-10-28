//! The local player's Weapon: firing, ammo and reloads, aiming down sights and recoil, plus
//! its viewmodel, effects and HUD. Client and Host only; the server decides what shots hit.

mod aim;
mod audio;
mod bolt;
mod effects;
mod hud;
mod viewmodel;

pub use viewmodel::ViewmodelCamera;

use std::f32::consts::TAU;
use std::hash::{BuildHasher, RandomState};
use std::sync::Arc;

use avian3d::prelude::*;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;

use aim::Breath;
use bolt::{Bolt, BoltEvent};

use crate::data::{FireMode, GameData, WeaponId, WeaponStats};
use crate::gear::{Attachments, Loadout, PerkEffects, Perks};
use crate::loadout::SpawnClock;
use crate::net::protocol::{AmmoRefill, Damaged, FireShot, Respawn};
use crate::player::input::Bindings;
use crate::player::{
    ControllerModifiers, ControllerSystems, EyeCamera, LocalLife, Motor, MouseCaptured, PITCH_LIMIT,
};
use crate::profile::Profile;
use crate::shared::{FromServer, GameLayer, LocalPlayer, PlayerPose, Stance, ToServer};

/// ADS blend from which a scoped Weapon shows the scope instead of the viewmodel, and from
/// which hold breath and the scope glint apply.
const SCOPE_MIN_ADS: f32 = 0.9;
/// Exponential rate (1/s) at which the sway amplitude follows its target, and at which a
/// rack's sway kick dies down.
const SWAY_FOLLOW_PER_SEC: f32 = 8.0;
const SWAY_KICK_DECAY_PER_SEC: f32 = 2.5;

pub struct WeaponClientPlugin;

impl Plugin for WeaponClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WeaponRng::seeded())
            .add_message::<ShotFired>()
            .add_observer(equip_local_player)
            .add_plugins((
                viewmodel::ViewmodelPlugin,
                effects::EffectsPlugin,
                audio::WeaponAudioPlugin,
                hud::WeaponHudPlugin,
            ))
            .add_systems(
                Update,
                (
                    equip_on_respawn,
                    swap_loadout,
                    refill_ammo,
                    switch_weapon,
                    track_body,
                    aim,
                    sway,
                    flinch,
                    reload,
                    fire,
                    recover_recoil,
                )
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
pub struct HeldWeapon {
    id: WeaponId,
    /// Fitted to this Weapon.
    attachments: Attachments,
    /// With the Attachments and the carrier's Perks applied.
    stats: Arc<WeaponStats>,
    rounds: u32,
    /// Seconds left of the reload in progress.
    reload_left: Option<f32>,
    /// Full length of the reload in progress (tactical or empty).
    reload_total: f32,
    /// Seconds until the next round may fire; non-positive when ready.
    cooldown: f32,
    /// Trigger held, counting only presses made while the cursor was already grabbed.
    trigger: bool,
    /// A `Semi` Weapon already fired (or clicked empty) on this trigger pull.
    pull_spent: bool,
    /// The bolt of a bolt-action Weapon.
    bolt: Bolt,
    /// The aim button is held (and nothing, like a reload, takes the sights down).
    aiming: bool,
    /// The breath tank of hold breath.
    breath: Breath,
    /// Seconds along the sway figure-eight.
    sway_time: f32,
    /// Current sway amplitude in degrees, easing toward its target.
    sway_amp: f32,
    /// Extra sway from working the bolt while scoped, as (yaw, pitch) in radians.
    sway_kick: Vec2,
    /// The sway currently added to the view, as (yaw, pitch) in radians.
    sway_applied: Vec2,
    /// Aim-down-sights blend: 0 at the hip, 1 fully aimed.
    ads: f32,
    /// Recoil not yet recovered, as (yaw, pitch) in radians.
    recoil: Vec2,
    /// The body is sprinting this frame.
    sprinting: bool,
    /// The body is mantling or diving this frame.
    mantling: bool,
    /// Seconds left of raising the weapon after a sprint ended.
    sprint_out: f32,
    /// Movement bloom added to hip spread, in degrees; follows the body's speed smoothly.
    bloom: f32,
    /// Spread and recoil multiplier of the current stance.
    accuracy: f32,
    /// Switching to the other Weapon.
    switch: Option<WeaponSwitch>,
    /// Which of the class's two Weapon slots this Weapon came from: a Primary-class Weapon in
    /// the Secondary slot (Overkill) switches like the pistol does.
    slot: Slot,
}

/// One of a class's two Weapon slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Primary,
    Secondary,
}

/// A Primary/Secondary switch under way: the held Weapon lowers, the two exchange half-way
/// and the other raises.
#[derive(Clone, Copy, Debug)]
struct WeaponSwitch {
    left: f32,
    total: f32,
    /// The exchange has happened.
    swapped: bool,
}

impl WeaponSwitch {
    /// Fraction done, 0 to 1.
    fn progress(&self) -> f32 {
        if self.total > 0.0 {
            (1.0 - self.left / self.total).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
}

/// The Weapon on the player's back: the other of Primary and Secondary.
#[derive(Component, Debug)]
pub struct Stowed {
    id: WeaponId,
    slot: Slot,
    attachments: Attachments,
    stats: Arc<WeaponStats>,
    rounds: u32,
}

impl Stowed {
    fn new(id: WeaponId, stats: Arc<WeaponStats>) -> Self {
        Self {
            id,
            slot: Slot::Secondary,
            attachments: Attachments::default(),
            rounds: stats.magazine,
            stats,
        }
    }
}

/// The Loadout in effect on the local player and what its Perks do.
#[derive(Component, Clone, Debug)]
pub struct Kit {
    loadout: Loadout,
    effects: PerkEffects,
}

impl Kit {
    pub fn loadout(&self) -> &Loadout {
        &self.loadout
    }

    /// The Perks in effect, as the server needs to know them.
    pub fn perks(&self) -> Perks {
        self.loadout.perks
    }
}

impl HeldWeapon {
    fn new(id: WeaponId, attachments: Attachments, stats: Arc<WeaponStats>) -> Self {
        Self {
            id,
            attachments,
            rounds: stats.magazine,
            stats,
            reload_left: None,
            reload_total: 0.0,
            cooldown: 0.0,
            trigger: false,
            pull_spent: false,
            bolt: Bolt::Closed,
            aiming: false,
            breath: Breath::default(),
            sway_time: 0.0,
            sway_amp: 0.0,
            sway_kick: Vec2::ZERO,
            sway_applied: Vec2::ZERO,
            ads: 0.0,
            recoil: Vec2::ZERO,
            sprinting: false,
            mantling: false,
            sprint_out: 0.0,
            bloom: 0.0,
            accuracy: 1.0,
            switch: None,
            slot: Slot::Primary,
        }
    }

    fn start_reload(&mut self, stats: &WeaponStats) {
        if self.reload_left.is_none() && self.rounds < stats.magazine {
            let secs = stats.reload_secs_for(self.rounds);
            info!("Reloading ({} rounds left, {:.2} s)", self.rounds, secs);
            self.reload_total = secs;
            self.reload_left = Some(secs);
            self.bolt = Bolt::Closed;
        }
    }

    /// Reload progress from 0 to 1, if reloading.
    fn reload_progress(&self) -> Option<f32> {
        self.reload_left
            .map(|left| 1.0 - left / self.reload_total.max(f32::EPSILON))
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

    /// Whether the weapon is up: not lowered for a sprint, mantle or dive, nor still raising.
    fn ready(&self) -> bool {
        !self.sprinting && !self.mantling && self.sprint_out <= 0.0 && self.switch.is_none()
    }

    /// How far the Weapon is lowered by a switch in progress: 0 up, 1 fully down.
    fn switch_lowering(&self) -> f32 {
        self.switch.map_or(0.0, |switch| {
            (switch.progress() * std::f32::consts::PI).sin()
        })
    }

    /// Exchanges the held Weapon with the stowed one, leaving the new one idle and settled.
    fn exchange(&mut self, stowed: &mut Stowed) {
        std::mem::swap(&mut self.id, &mut stowed.id);
        std::mem::swap(&mut self.slot, &mut stowed.slot);
        std::mem::swap(&mut self.attachments, &mut stowed.attachments);
        std::mem::swap(&mut self.stats, &mut stowed.stats);
        std::mem::swap(&mut self.rounds, &mut stowed.rounds);
        self.reload_left = None;
        self.cooldown = 0.0;
        self.pull_spent = false;
        self.bolt = Bolt::Closed;
        self.ads = 0.0;
        info!("Switched to {}", self.stats.name);
    }

    /// Runs the trigger for a frame of `dt` seconds and returns how many rounds leave the
    /// barrel. `Auto` fires as many as the fire rate owes since the last shot, so a long frame
    /// fires several; `Semi` fires one per pull once the cooldown allows. Pulling on an empty
    /// magazine starts a reload.
    fn rounds_to_fire(&mut self, stats: &WeaponStats, dt: f32) -> u32 {
        self.cooldown -= dt;
        if !self.trigger {
            self.pull_spent = false;
        }
        let mut fired = 0;
        if self.trigger
            && self.ready()
            && self.reload_left.is_none()
            && (!stats.bolt_action || self.bolt.can_fire())
        {
            while self.cooldown <= 0.0 && !self.pull_spent {
                if self.rounds == 0 {
                    self.start_reload(stats);
                    self.pull_spent = stats.fire_mode == FireMode::Semi;
                    break;
                }
                self.rounds -= 1;
                if !stats.bolt_action {
                    self.cooldown += stats.seconds_per_shot(self.ads_eased());
                }
                fired += 1;
                if stats.fire_mode == FireMode::Semi {
                    self.pull_spent = true;
                    if stats.bolt_action && self.rounds > 0 {
                        self.bolt.fire();
                    }
                }
            }
        }
        self.cooldown = self.cooldown.max(0.0);
        fired
    }

    /// Works the bolt of a bolt-action Weapon for `dt` seconds, before the trigger runs: a
    /// click while aimed racks it (and is used up), letting go of the sights racks it by itself.
    fn step_bolt(&mut self, stats: &WeaponStats, dt: f32) -> BoltEvent {
        if !stats.bolt_action {
            return BoltEvent::Idle;
        }
        let click = self.trigger && !self.pull_spent && self.ready() && self.reload_left.is_none();
        let event = self.bolt.step(
            dt,
            self.aiming,
            click,
            stats.rack_secs_scoped,
            stats.rack_secs_unscoped,
        );
        if event == (BoltEvent::RackStarted { scoped: true }) {
            self.pull_spent = true;
        }
        event
    }

    /// How far the bolt is worked, 0 to 1, while it is being racked.
    fn bolt_phase(&self) -> Option<f32> {
        self.bolt.progress()
    }

    /// Looking through the scope: the viewmodel hides and the scope mask shows.
    fn scoped(&self, stats: &WeaponStats) -> bool {
        stats.scope && self.ads > SCOPE_MIN_ADS
    }

    /// The ADS blend eased in and out.
    fn ads_eased(&self) -> f32 {
        ease(self.ads)
    }

    /// How far the weapon still is from ready after a sprint: 1 just after, 0 once up.
    fn sprint_out_fraction(&self, stats: &WeaponStats) -> f32 {
        if stats.sprint_out_secs > 0.0 {
            (self.sprint_out / stats.sprint_out_secs).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Hip spread cone half-angle with movement bloom and stance, in degrees.
    fn hip_spread_deg(&self, stats: &WeaponStats) -> f32 {
        (stats.hip_spread_deg + self.bloom) * self.accuracy
    }

    /// Spread cone half-angle at the current ADS blend, in degrees. Sights ignore movement
    /// bloom but keep the stance bonus.
    fn spread_deg(&self, stats: &WeaponStats) -> f32 {
        self.hip_spread_deg(stats)
            .lerp(stats.ads_spread_deg * self.accuracy, self.ads_eased())
    }

    /// Recoil kick multiplier at the current ADS blend and stance.
    fn recoil_scale(&self, stats: &WeaponStats) -> f32 {
        1.0_f32.lerp(stats.ads_recoil_scale, self.ads_eased()) * self.accuracy
    }
}

/// One local round left the barrel: `muzzle` is the viewmodel muzzle in world space and
/// `end` where the client-side ray met the World (or maximum range).
#[derive(Message, Clone, Copy, Debug)]
struct ShotFired {
    weapon: WeaponId,
    muzzle: Vec3,
    end: Vec3,
    /// A Silencer is fitted: no muzzle flash, and the gunshot's volume and pitch below.
    silenced: bool,
    volume: f32,
    pitch: f32,
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

/// Smoothstep: eases a 0..1 blend in and out.
fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Builds the Weapons and Perk effects of `loadout`: the Primary with its Attachments in hand,
/// the second Weapon (a Secondary, or a Primary with Overkill) stowed if the class has one,
/// both with full magazines. Perk Greed's second Perks stack with the first.
fn equip(loadout: &Loadout, data: &GameData) -> (HeldWeapon, Option<Stowed>, Kit) {
    let effects = data.perk_effects(loadout.perks.iter());
    let primary = Arc::new(data.effective_weapon(loadout.primary, loadout.attachments, &effects));
    let secondary = loadout
        .secondary
        .map(|id| Arc::new(data.effective_weapon(id, Attachments::default(), &effects)));
    let wildcards: Vec<&str> = loadout
        .wildcards
        .iter()
        .map(|id| data.wildcard_name(*id))
        .collect();
    info!(
        "Loadout \"{}\" ({} / {} points): {} with {:?}, second weapon {}; Wildcards {:?}; Perks {:?}; \
         effective: reload {:.2} s (empty {:.2} s), damage x{:.2}, move x{:.2}, \
         sprint stamina {:.1} s, falloff {:.0}-{:.0} m, recoil {:.2} deg",
        loadout.name,
        data.class_cost(loadout),
        data.class_points.budget,
        primary.name,
        loadout.attachments,
        secondary
            .as_ref()
            .map_or("none", |stats| stats.name.as_str()),
        wildcards,
        loadout.perks,
        primary.reload_secs,
        primary.empty_reload_secs,
        effects.damage_mul,
        primary.move_scale,
        data.movement.sprint_stamina_secs * effects.sprint_stamina_mul,
        primary.falloff_start,
        primary.falloff_end,
        primary.recoil_pitch_deg,
    );
    (
        HeldWeapon::new(loadout.primary, loadout.attachments, primary),
        loadout
            .secondary
            .zip(secondary)
            .map(|(id, stats)| Stowed::new(id, stats)),
        Kit {
            loadout: loadout.clone(),
            effects,
        },
    )
}

/// Puts a built kit on the local player: `Stowed` is only there when the class has a second
/// Weapon.
fn insert_kit(
    commands: &mut Commands,
    player: Entity,
    (held, stowed, kit): (HeldWeapon, Option<Stowed>, Kit),
) {
    let mut player = commands.entity(player);
    player.insert((held, kit));
    match stowed {
        Some(stowed) => player.insert(stowed),
        None => player.remove::<Stowed>(),
    };
}

fn equip_local_player(
    add: On<Add<LocalPlayer>>,
    data: Res<GameData>,
    profile: Res<Profile>,
    mut commands: Commands,
) {
    insert_kit(
        &mut commands,
        add.entity,
        equip(profile.loadout(&data), &data),
    );
}

/// A respawn hands out the chosen Loadout with fresh magazines and a settled aim.
fn equip_on_respawn(
    mut respawns: MessageReader<FromServer<Respawn>>,
    data: Res<GameData>,
    profile: Res<Profile>,
    player: Query<Entity, With<LocalPlayer>>,
    mut commands: Commands,
) {
    if respawns.read().count() == 0 {
        return;
    }
    let loadout = profile.loadout(&data);
    for player in &player {
        insert_kit(&mut commands, player, equip(loadout, &data));
        info!("Spawned with Loadout \"{}\"", loadout.name);
    }
}

/// A Loadout chosen (or edited) within the swap window of spawning replaces the carried
/// Weapons and Perks at once; otherwise it waits for the next spawn.
fn swap_loadout(
    time: Res<Time>,
    data: Res<GameData>,
    profile: Res<Profile>,
    clock: Res<SpawnClock>,
    life: Res<LocalLife>,
    player: Query<(Entity, &Kit), With<LocalPlayer>>,
    mut commands: Commands,
) {
    if !profile.is_changed() || profile.is_added() {
        return;
    }
    let loadout = profile.loadout(&data);
    let open = clock.swap_open(time.elapsed_secs_f64(), &life, &data);
    for (player, kit) in &player {
        if kit.loadout == *loadout {
            continue;
        }
        if open {
            insert_kit(&mut commands, player, equip(loadout, &data));
            info!("Loadout \"{}\" applied now", loadout.name);
        } else {
            info!("Loadout \"{}\" applies on next spawn", loadout.name);
        }
    }
}

/// An ammo pack picked up with Scavenger refills both magazines.
fn refill_ammo(
    mut refills: MessageReader<FromServer<AmmoRefill>>,
    mut player: Query<(&mut HeldWeapon, Option<&mut Stowed>), With<LocalPlayer>>,
) {
    if refills.read().count() == 0 {
        return;
    }
    for (mut weapon, stowed) in &mut player {
        weapon.rounds = weapon.stats.magazine;
        if let Some(mut stowed) = stowed {
            stowed.rounds = stowed.stats.magazine;
        }
        info!("Ammo refilled from a pack");
    }
}

/// `1` takes the Primary, `2` the Secondary and the mouse wheel the other Weapon. The switch
/// lowers the held Weapon, exchanges the two half-way and raises the other, taking
/// `weapon_switch_secs` in all; nothing fires or aims meanwhile.
fn switch_weapon(
    time: Res<Time>,
    data: Res<GameData>,
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    bindings: Res<Bindings>,
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    mut player: Query<(&mut HeldWeapon, &mut Stowed), With<LocalPlayer>>,
) {
    let Ok((mut weapon, mut stowed)) = player.single_mut() else {
        return;
    };
    let weapon = &mut *weapon;
    if life.is_dead() {
        weapon.switch = None;
        return;
    }
    if let Some(mut switch) = weapon.switch {
        switch.left -= time.delta_secs();
        if !switch.swapped && switch.progress() >= 0.5 {
            weapon.exchange(&mut stowed);
            switch.swapped = true;
        }
        weapon.switch = (switch.left > 0.0).then_some(switch);
        return;
    }
    if !captured.active() || weapon.mantling {
        return;
    }
    let holding_primary = weapon.slot == Slot::Primary;
    let wheel = scroll.delta.y != 0.0;
    let wants = if holding_primary {
        bindings.secondary_weapon.just_pressed(&keys)
    } else {
        bindings.primary_weapon.just_pressed(&keys)
    };
    if wants || wheel {
        weapon.reload_left = None;
        weapon.trigger = false;
        weapon.bolt = Bolt::Closed;
        weapon.switch = Some(WeaponSwitch {
            left: data.weapon_switch_secs,
            total: data.weapon_switch_secs,
            swapped: false,
        });
    }
}

/// Follows the body: sprint, mantle and dive lower the weapon, a finished sprint starts raising it,
/// speed and air time bloom hip spread, and stance scales accuracy.
fn track_body(
    time: Res<Time>,
    data: Res<GameData>,
    mut player: Query<(&mut HeldWeapon, &Motor, &PlayerPose), With<LocalPlayer>>,
) {
    let Ok((mut weapon, motor, pose)) = player.single_mut() else {
        return;
    };
    let stats = Arc::clone(&weapon.stats);
    let dt = time.delta_secs();

    weapon.sprint_out = if weapon.sprinting && !motor.sprinting() {
        stats.sprint_out_secs
    } else {
        (weapon.sprint_out - dt).max(0.0)
    };
    weapon.sprinting = motor.sprinting();
    weapon.mantling = motor.mantling() || motor.diving();

    let velocity = motor.velocity();
    let speed = Vec2::new(velocity.x, velocity.z).length();
    let walk = (speed / data.movement.walk_speed.max(f32::EPSILON)).clamp(0.0, 1.5);
    let air = if motor.grounded() {
        0.0
    } else {
        stats.air_spread_deg
    };
    let bloom = stats.move_spread_deg * walk + air;
    weapon
        .bloom
        .smooth_nudge(&bloom, stats.bloom_follow_per_sec, dt);

    weapon.accuracy = match pose.stance {
        Stance::Standing => 1.0,
        Stance::Crouching => stats.crouch_accuracy_scale,
        Stance::Prone => stats.prone_accuracy_scale,
    };
}

/// Blends toward the sights while the aim button is held and the weapon is up (a bolt being
/// worked takes it off them), and feeds the controller hooks: aiming zooms and slows, aiming
/// or firing stops a sprint, and the Perks scale the sprint tank.
fn aim(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<Bindings>,
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    mut player: Query<(&mut HeldWeapon, &mut ControllerModifiers, &Kit), With<LocalPlayer>>,
) {
    let Ok((mut weapon, mut modifiers, kit)) = player.single_mut() else {
        return;
    };
    let stats = Arc::clone(&weapon.stats);
    let in_control = captured.active() && !life.is_dead();
    let wants = in_control && mouse.pressed(bindings.aim) && weapon.reload_left.is_none();
    let step = time.delta_secs() / stats.ads_secs.max(f32::EPSILON);
    weapon.aiming = wants;
    weapon.ads = if wants && weapon.ready() {
        (weapon.ads + step).min(1.0)
    } else {
        (weapon.ads - step).max(0.0)
    };
    let t = weapon.ads_eased();
    modifiers.fov_scale = 1.0_f32.lerp(stats.ads_fov_scale(), t);
    modifiers.move_scale = stats.move_scale * 1.0_f32.lerp(stats.ads_move_scale, t);
    modifiers.stamina_scale = kit.effects.sprint_stamina_mul;
    modifiers.sprint_blocked = wants || (in_control && mouse.pressed(bindings.fire));
}

/// R reloads; sprinting cancels a reload in progress.
fn reload(
    time: Res<Time>,
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
    let stats = Arc::clone(&weapon.stats);
    if captured.active() && bindings.reload.just_pressed(&keys) && weapon.switch.is_none() {
        weapon.start_reload(&stats);
    }
    weapon.tick_reload(&stats, time.delta_secs(), motor.sprinting());
}

/// Fires the Weapon at its rpm (blending to its ADS rpm with the sights) once it is up: held
/// for `Auto`, one round per pull for `Semi`. Each round goes to the server with spread and
/// the current recoil already applied, then kicks the view.
fn fire(
    time: Res<Time>,
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
    let stats = Arc::clone(&weapon.stats);
    let held = captured.active() && mouse.pressed(bindings.fire);
    let pulled = mouse.just_pressed(bindings.fire) && !captured.is_changed();
    weapon.trigger = held && !life.is_dead() && (weapon.trigger || pulled);

    let dt = time.delta_secs();
    match weapon.step_bolt(&stats, dt) {
        BoltEvent::RackStarted { scoped } => {
            let secs = if scoped {
                stats.rack_secs_scoped
            } else {
                stats.rack_secs_unscoped
            };
            info!(
                "Racking the bolt ({}, {secs:.2} s)",
                if scoped {
                    "click while scoped"
                } else {
                    "sights down"
                }
            );
            if scoped {
                let kick = stats.rack_kick_deg.to_radians();
                weapon.sway_kick += Vec2::new(rng.signed(), rng.signed() * 0.5) * kick;
            }
        }
        BoltEvent::Closed => info!("Bolt closed"),
        BoltEvent::Idle => {}
    }
    let rounds = weapon.rounds_to_fire(&stats, dt);
    if rounds > 0 {
        let eye_height = eye
            .single()
            .map_or(pose.stance.eye_height(), EyeCamera::height);
        let origin = pose.position + Vec3::Y * eye_height;
        let gun = gun.single().copied().unwrap_or_default();
        let filter = SpatialQueryFilter::from_mask(GameLayer::World);
        let spread = weapon.spread_deg(&stats).to_radians();
        let kick = weapon.recoil_scale(&stats);
        for _ in 0..rounds {
            let aim = aim_rotation(pose.yaw, pose.pitch);
            let (angle, radius) = (
                rng.next() * std::f32::consts::TAU,
                spread * rng.next().sqrt(),
            );
            let offset = Quat::from_rotation_y(radius * angle.cos())
                * Quat::from_rotation_x(radius * angle.sin());
            let direction = Dir3::new_unchecked((aim * offset * Vec3::NEG_Z).normalize());

            shots.write(ToServer(FireShot {
                weapon: weapon.id,
                attachments: weapon.attachments,
                origin,
                direction: *direction,
            }));
            let end = spatial
                .cast_ray(origin, direction, stats.range, true, &filter)
                .map_or(stats.range, |hit| hit.distance);
            let muzzle = Transform::from_translation(origin)
                .with_rotation(aim)
                .transform_point(gun.transform_point(Vec3::from(stats.muzzle_offset)));
            fired.write(ShotFired {
                weapon: weapon.id,
                muzzle,
                end: origin + *direction * end,
                silenced: stats.silenced,
                volume: stats.shot_volume,
                pitch: stats.shot_pitch,
            });

            let pitch_kick = stats.recoil_pitch_deg.to_radians() * kick;
            let yaw_kick = stats.recoil_yaw_deg.to_radians() * kick * rng.signed();
            let pitch = (pose.pitch + pitch_kick).min(PITCH_LIMIT);
            weapon.recoil += Vec2::new(yaw_kick, pitch - pose.pitch);
            pose.pitch = pitch;
            pose.yaw = wrap_angle(pose.yaw + yaw_kick);
        }
    }
}

/// While aimed, drifts the view along a slow figure-eight (the Weapon's sway through the
/// sight's `sway_scale`, stance and breath) and runs hold breath. The drift is added to the
/// pose itself, so the sight points where the shots go. Also publishes the magnification
/// others glint from.
fn sway(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    captured: Res<MouseCaptured>,
    life: Res<LocalLife>,
    mut player: Query<(&mut PlayerPose, &mut HeldWeapon), With<LocalPlayer>>,
) {
    let Ok((mut pose, mut weapon)) = player.single_mut() else {
        return;
    };
    let stats = Arc::clone(&weapon.stats);
    let dt = time.delta_secs();
    let alive = !life.is_dead();
    let aimed = weapon.ads > SCOPE_MIN_ADS;
    let wants_hold = alive
        && captured.active()
        && weapon.aiming
        && aimed
        && stats.steady_sight()
        && bindings.sprint.pressed(&keys);
    weapon.breath.tick(dt, wants_hold);

    weapon.sway_time = (weapon.sway_time + dt).rem_euclid(aim::SWAY_PERIOD_SECS);
    let target = if alive && weapon.switch.is_none() {
        aim::sway_amplitude_deg(
            stats.sway_deg,
            stats.sway_scale,
            pose.stance,
            weapon.breath.sway_mul(),
            weapon.ads_eased(),
        )
    } else {
        0.0
    };
    weapon
        .sway_amp
        .smooth_nudge(&target, SWAY_FOLLOW_PER_SEC, dt);
    weapon.sway_kick *= (-SWAY_KICK_DECAY_PER_SEC * dt).exp();
    let offset =
        aim::sway_curve(weapon.sway_time) * weapon.sway_amp.to_radians() + weapon.sway_kick;
    let delta = offset - weapon.sway_applied;
    if delta != Vec2::ZERO {
        let pitch = (pose.pitch + delta.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        weapon.sway_applied += Vec2::new(delta.x, pitch - pose.pitch);
        pose.yaw = wrap_angle(pose.yaw + delta.x);
        pose.pitch = pitch;
    }

    let glint = if alive && weapon.ads_eased() > SCOPE_MIN_ADS && stats.steady_sight() {
        stats.magnification
    } else {
        0.0
    };
    if pose.aim_magnification != glint {
        pose.aim_magnification = glint;
    }
}

/// A hit from the server kicks the view in a random direction by the Weapon's `flinch_deg`,
/// scaled by the damage and reduced while aimed. It recovers like recoil.
fn flinch(
    mut hits: MessageReader<FromServer<Damaged>>,
    life: Res<LocalLife>,
    mut rng: ResMut<WeaponRng>,
    mut player: Query<(&mut PlayerPose, &mut HeldWeapon), With<LocalPlayer>>,
) {
    let Ok((mut pose, mut weapon)) = player.single_mut() else {
        hits.clear();
        return;
    };
    for FromServer(hit) in hits.read() {
        if life.is_dead() {
            continue;
        }
        let kick =
            aim::flinch_deg(weapon.stats.flinch_deg, hit.amount, weapon.ads_eased()).to_radians();
        let angle = rng.next() * TAU;
        let yaw_kick = kick * angle.cos();
        let pitch = (pose.pitch + kick * angle.sin()).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        weapon.recoil += Vec2::new(yaw_kick, pitch - pose.pitch);
        pose.pitch = pitch;
        pose.yaw = wrap_angle(pose.yaw + yaw_kick);
    }
}

/// Returns the view toward where the player was aiming before the kicks.
fn recover_recoil(
    time: Res<Time>,
    mut player: Query<(&mut PlayerPose, &mut HeldWeapon), With<LocalPlayer>>,
) {
    let Ok((mut pose, mut weapon)) = player.single_mut() else {
        return;
    };
    if weapon.recoil == Vec2::ZERO {
        return;
    }
    let rate = weapon.stats.recoil_recovery_per_sec;
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
    use crate::class::WildcardId;
    use crate::data::WeaponClass;
    use crate::gear::{PerkId, Perks};

    #[test]
    fn overkill_carries_a_primary_as_the_second_weapon_and_switches_like_the_pistol() {
        let data = GameData::load();
        let class = Loadout {
            primary: WeaponId::AssaultRifle,
            secondary: Some(WeaponId::SniperRifle),
            wildcards: vec![WildcardId::Overkill],
            ..data.default_loadouts[0].clone()
        };
        assert!(data.class_errors(&class, None).is_empty());
        let (mut weapon, stowed, _) = equip(&class, &data);
        let mut stowed = stowed.expect("the Overkill Primary is stowed");
        assert_eq!(
            (weapon.id, weapon.slot),
            (WeaponId::AssaultRifle, Slot::Primary)
        );
        assert_eq!(
            (stowed.id, stowed.slot),
            (WeaponId::SniperRifle, Slot::Secondary)
        );
        assert_eq!(stowed.stats.class, WeaponClass::Primary);

        // Switching goes by slot, not by the Weapon's class: the sniper is "the other" Weapon.
        weapon.exchange(&mut stowed);
        assert_eq!(
            (weapon.id, weapon.slot),
            (WeaponId::SniperRifle, Slot::Secondary)
        );
        assert_eq!(weapon.stats.class, WeaponClass::Primary);
        weapon.exchange(&mut stowed);
        assert_eq!(
            (weapon.id, weapon.slot),
            (WeaponId::AssaultRifle, Slot::Primary)
        );
    }

    #[test]
    fn a_class_with_no_second_weapon_stows_nothing() {
        let data = GameData::load();
        let class = Loadout {
            secondary: None,
            ..data.default_loadouts[0].clone()
        };
        assert!(equip(&class, &data).1.is_none());
    }

    #[test]
    fn perk_greed_perks_stack_in_the_kit() {
        let data = GameData::load();
        let class = Loadout {
            perks: Perks {
                picks: [Some(PerkId::SleightOfHand), None, None],
                greed: [Some(PerkId::Scavenger), None, None],
            },
            wildcards: vec![WildcardId::PerkGreed1],
            ..data.default_loadouts[0].clone()
        };
        let (weapon, _, kit) = equip(&class, &data);
        assert!(kit.effects.scavenger, "the Greed Perk is in effect");
        assert!(
            (kit.effects.reload_time_mul - 0.7).abs() < 1e-6,
            "and so is the first"
        );
        assert!(weapon.stats.reload_secs < data.weapon(WeaponId::AssaultRifle).reload_secs);
        assert_eq!(kit.perks().count(), 2);
    }

    fn held(id: WeaponId, stats: &WeaponStats) -> HeldWeapon {
        HeldWeapon::new(id, Attachments::default(), Arc::new(stats.clone()))
    }

    /// One frame of `reload` then `fire`, as the systems are chained; returns rounds fired.
    fn frame(weapon: &mut HeldWeapon, stats: &WeaponStats, dt: f32) -> u32 {
        weapon.tick_reload(stats, dt, false);
        weapon.step_bolt(stats, dt);
        weapon.rounds_to_fire(stats, dt)
    }

    #[test]
    fn exchanging_swaps_the_guns_and_leaves_the_new_one_idle() {
        let rifle = WeaponStats::fixture();
        let pistol = WeaponStats {
            name: "Test Pistol".into(),
            class: WeaponClass::Secondary,
            magazine: 15,
            ..WeaponStats::fixture()
        };
        let mut weapon = held(WeaponId::AssaultRifle, &rifle);
        weapon.rounds = 7;
        weapon.ads = 1.0;
        weapon.reload_left = Some(0.5);
        let mut stowed = Stowed::new(WeaponId::Pistol, Arc::new(pistol));
        stowed.rounds = 11;

        weapon.exchange(&mut stowed);
        assert_eq!((weapon.id, weapon.rounds), (WeaponId::Pistol, 11));
        assert_eq!((stowed.id, stowed.rounds), (WeaponId::AssaultRifle, 7));
        assert_eq!(weapon.stats.class, WeaponClass::Secondary);
        assert_eq!((weapon.ads, weapon.reload_left), (0.0, None));
    }

    #[test]
    fn a_switch_lowers_then_raises_and_blocks_firing() {
        let stats = WeaponStats::fixture();
        let mut weapon = held(WeaponId::AssaultRifle, &stats);
        assert!(weapon.ready() && weapon.switch_lowering() == 0.0);
        weapon.switch = Some(WeaponSwitch {
            left: 0.4,
            total: 0.4,
            swapped: false,
        });
        assert!(!weapon.ready());
        weapon.trigger = true;
        assert_eq!(frame(&mut weapon, &stats, 0.1), 0, "no rounds mid-switch");
        let mut at = |left| {
            weapon.switch = Some(WeaponSwitch {
                left,
                total: 0.4,
                swapped: false,
            });
            weapon.switch_lowering()
        };
        assert!(at(0.4) < 1e-6, "starts up");
        assert!((at(0.2) - 1.0).abs() < 1e-6, "fully down half-way");
        assert!(at(0.0) < 1e-6, "ends up");
    }

    #[test]
    fn fire_rate_holds_at_any_frame_rate_and_semi_fires_once_per_pull() {
        // 480 rpm: a round every 0.125 s, so holding for 1 s fires at 0, 0.125, ..., 1.0.
        // Fully aimed it's 240 rpm: a round every 0.25 s.
        let stats = WeaponStats {
            magazine: 100,
            ads_rpm: 240.0,
            ..WeaponStats::fixture()
        };

        let mut low_fps = held(WeaponId::AssaultRifle, &stats);
        low_fps.trigger = true;
        let fired: u32 = (0..4).map(|_| frame(&mut low_fps, &stats, 0.25)).sum();
        assert_eq!(fired, 9);

        let mut hitch = held(WeaponId::AssaultRifle, &stats);
        hitch.trigger = true;
        assert_eq!(frame(&mut hitch, &stats, 1.0), 9);

        // A second with the trigger released doesn't let the next pull fire faster.
        let mut idle = held(WeaponId::AssaultRifle, &stats);
        assert_eq!(frame(&mut idle, &stats, 1.0), 0);
        idle.trigger = true;
        assert_eq!(frame(&mut idle, &stats, 0.25), 3);

        let mut aimed = held(WeaponId::AssaultRifle, &stats);
        aimed.ads = 1.0;
        aimed.trigger = true;
        let fired: u32 = (0..4).map(|_| frame(&mut aimed, &stats, 0.25)).sum();
        assert_eq!(fired, 5);

        // Semi at 60 rpm: holding fires one round; the next needs a new pull and the 1 s
        // cooldown.
        let semi = WeaponStats {
            fire_mode: FireMode::Semi,
            rpm: 60.0,
            ads_rpm: 60.0,
            ..WeaponStats::fixture()
        };
        let mut pistol = held(WeaponId::Pistol, &semi);
        pistol.trigger = true;
        let held: u32 = (0..30).map(|_| frame(&mut pistol, &semi, 0.1)).sum();
        assert_eq!(held, 1, "holding fires once");
        pistol.trigger = false;
        assert_eq!(frame(&mut pistol, &semi, 0.1), 0);
        pistol.trigger = true;
        assert_eq!(frame(&mut pistol, &semi, 0.1), 1, "a new pull fires");
        frame(&mut pistol, &semi, 0.2);
        pistol.trigger = false;
        frame(&mut pistol, &semi, 0.1);
        pistol.trigger = true;
        let early: u32 = (0..3).map(|_| frame(&mut pistol, &semi, 0.1)).sum();
        assert_eq!(early, 0, "no round before the cooldown ends");
        let late: u32 = (0..4).map(|_| frame(&mut pistol, &semi, 0.1)).sum();
        assert_eq!(
            late, 1,
            "a pull held through the cooldown fires once it ends"
        );
    }

    #[test]
    fn a_bolt_action_needs_a_rack_between_shots() {
        let stats = WeaponStats {
            fire_mode: FireMode::Semi,
            rpm: 50.0,
            ads_rpm: 50.0,
            bolt_action: true,
            rack_secs_scoped: 1.4,
            rack_secs_unscoped: 0.9,
            ..WeaponStats::fixture()
        };

        // From the hip the open bolt racks itself in 0.9 s; holding the trigger never refires.
        let mut gun = held(WeaponId::SniperRifle, &stats);
        gun.trigger = true;
        assert_eq!(frame(&mut gun, &stats, 0.1), 1);
        assert!(!gun.bolt.can_fire());
        let held_fire: u32 = (0..30).map(|_| frame(&mut gun, &stats, 0.1)).sum();
        assert_eq!(held_fire, 0);
        assert!(gun.bolt.can_fire(), "it racked itself");
        gun.trigger = false;
        frame(&mut gun, &stats, 0.1);
        gun.trigger = true;
        assert_eq!(frame(&mut gun, &stats, 0.1), 1, "the next click fires");

        // Scoped: the shot leaves the sights up and the bolt open; a click racks only.
        let mut gun = held(WeaponId::SniperRifle, &stats);
        gun.aiming = true;
        gun.ads = 1.0;
        gun.trigger = true;
        assert_eq!(frame(&mut gun, &stats, 0.1), 1);
        gun.trigger = false;
        assert_eq!(frame(&mut gun, &stats, 2.0), 0);
        assert!(
            gun.bolt.waiting_for_click(),
            "it waits for a click while aimed"
        );
        gun.trigger = true;
        assert_eq!(frame(&mut gun, &stats, 0.1), 0, "the click racks, no shot");
        assert!(gun.bolt_phase().is_some());
        let through_rack: u32 = (0..16).map(|_| frame(&mut gun, &stats, 0.1)).sum();
        assert_eq!(through_rack, 0, "a held click doesn't fire either");
        assert!(gun.bolt.can_fire());
        gun.trigger = false;
        frame(&mut gun, &stats, 0.1);
        gun.trigger = true;
        assert_eq!(frame(&mut gun, &stats, 0.1), 1, "the next click fires");

        // Letting go of the aim mid-rack finishes at the faster rate: 0.7 s into the 1.4 s
        // rack is half done, leaving 0.45 s.
        let mut gun = held(WeaponId::SniperRifle, &stats);
        gun.aiming = true;
        gun.ads = 1.0;
        gun.trigger = true;
        frame(&mut gun, &stats, 0.1);
        gun.trigger = false;
        frame(&mut gun, &stats, 0.1);
        gun.trigger = true;
        frame(&mut gun, &stats, 0.01);
        frame(&mut gun, &stats, 0.69);
        gun.aiming = false;
        frame(&mut gun, &stats, 0.4);
        assert!(!gun.bolt.can_fire());
        frame(&mut gun, &stats, 0.1);
        assert!(gun.bolt.can_fire());
    }

    #[test]
    fn running_dry_reloads_then_fires_again_unless_sprint_cancels() {
        let stats = WeaponStats {
            magazine: 3,
            ..WeaponStats::fixture()
        };
        let mut weapon = held(WeaponId::AssaultRifle, &stats);
        weapon.trigger = true;

        assert_eq!(frame(&mut weapon, &stats, 1.0), 3);
        assert_eq!(weapon.rounds, 0);
        assert!(
            weapon.reload_left.is_some(),
            "an empty magazine starts a reload"
        );
        assert_eq!(
            frame(&mut weapon, &stats, 0.5),
            0,
            "no rounds while reloading"
        );
        assert_eq!(
            frame(&mut weapon, &stats, 0.5),
            3,
            "fires again once reloaded"
        );

        // Empty again and reloading: sprinting cancels without refilling.
        weapon.tick_reload(&stats, 0.1, true);
        assert_eq!(weapon.reload_left, None);
        assert_eq!(weapon.rounds, 0);
    }

    #[test]
    fn an_empty_magazine_reloads_slower_and_the_dip_follows_the_actual_time() {
        let stats = WeaponStats {
            reload_secs: 1.0,
            empty_reload_secs: 2.0,
            ..WeaponStats::fixture()
        };
        let mut tactical = held(WeaponId::AssaultRifle, &stats);
        tactical.rounds = 5;
        tactical.start_reload(&stats);
        assert_eq!(tactical.reload_left, Some(1.0));
        tactical.tick_reload(&stats, 0.5, false);
        assert_eq!(tactical.reload_progress(), Some(0.5));

        let mut empty = held(WeaponId::AssaultRifle, &stats);
        empty.rounds = 0;
        empty.start_reload(&stats);
        assert_eq!(empty.reload_left, Some(2.0));
        empty.tick_reload(&stats, 1.0, false);
        assert_eq!(
            empty.reload_progress(),
            Some(0.5),
            "half way through the longer reload"
        );
        empty.tick_reload(&stats, 1.0, false);
        assert_eq!((empty.rounds, empty.reload_left), (stats.magazine, None));
    }
}
