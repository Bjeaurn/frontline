//! What a player carries: Attachments, Perks and the Loadout (class) that picks them (CONTEXT.md:
//! Loadout, Attachment, Perk), plus the pure rules that turn them into effective Weapon stats and
//! Perk effects. Shared by the client (viewmodel, HUD, editor) and the server (damage, range).
//! The point budget and slot rules of a class are in `class`.
//!
//! The numbers live in `data/attachments.ron` and `data/perks.ron`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::class::{EquipmentId, EquipmentSlot, WildcardId};
use crate::data::{GameData, SightKind, WeaponClass, WeaponId, WeaponStats};

/// Longest Loadout name the editor accepts.
pub const MAX_LOADOUT_NAME_CHARS: usize = 16;

/// The camo of a Loadout that has none.
pub const NO_CAMO: &str = "none";

fn one() -> f32 {
    1.0
}

/// How wide a muzzle device is drawn, relative to the barrel, when its data doesn't say.
fn default_muzzle_girth() -> f32 {
    1.9
}

fn no_camo() -> String {
    NO_CAMO.to_owned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AttachmentId {
    RedDot,
    Holographic,
    Prism,
    Acog,
    Silencer,
    Foregrip,
    /// The M16A4's underslung grenade launcher (the "noob tube"): fires `GrenadeKind::Launcher`.
    GrenadeLauncher,
    /// A 1x open reflex sight with a crosshair reticle; quick to aim.
    Reflex,
    /// A 6x scope: the strongest Optic.
    Scope6x,
    /// Muzzle brake: steadier, with a bigger flash.
    Compensator,
    /// Muzzle device that hides the flash from everyone else and shrinks it in the shooter's view.
    FlashHider,
    /// A grip angled forward: quicker to aim and a little steadier.
    AngledGrip,
}

impl AttachmentId {
    /// Every Attachment, in the order menus list them.
    pub const ALL: [AttachmentId; 12] = [
        AttachmentId::RedDot,
        AttachmentId::Reflex,
        AttachmentId::Holographic,
        AttachmentId::Prism,
        AttachmentId::Acog,
        AttachmentId::Scope6x,
        AttachmentId::Silencer,
        AttachmentId::Compensator,
        AttachmentId::FlashHider,
        AttachmentId::Foregrip,
        AttachmentId::AngledGrip,
        AttachmentId::GrenadeLauncher,
    ];
}

/// A position on a Weapon that holds at most one Attachment (CONTEXT.md: Attachment Slot).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub enum AttachmentSlot {
    Optic,
    Muzzle,
    Underbarrel,
}

impl AttachmentSlot {
    /// Every Slot, in the order the editor lists them.
    pub const ALL: [AttachmentSlot; 3] = [
        AttachmentSlot::Optic,
        AttachmentSlot::Muzzle,
        AttachmentSlot::Underbarrel,
    ];
}

/// Multipliers an Attachment applies to a Weapon's stats; 1 changes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct StatMods {
    /// Scales both falloff distances: below 1 the damage drops off sooner.
    pub falloff_mul: f32,
    /// Scales the time to aim down sights: above 1 is slower.
    pub ads_secs_mul: f32,
    /// Scales the recoil kick: below 1 is steadier.
    pub recoil_mul: f32,
    /// Scales the idle sway while aimed: below 1 is steadier.
    pub sway_mul: f32,
}

impl Default for StatMods {
    fn default() -> Self {
        Self {
            falloff_mul: 1.0,
            ads_secs_mul: 1.0,
            recoil_mul: 1.0,
            sway_mul: 1.0,
        }
    }
}

impl StatMods {
    /// Both sets of modifiers at once.
    pub fn stack(self, other: StatMods) -> StatMods {
        StatMods {
            falloff_mul: self.falloff_mul * other.falloff_mul,
            ads_secs_mul: self.ads_secs_mul * other.ads_secs_mul,
            recoil_mul: self.recoil_mul * other.recoil_mul,
            sway_mul: self.sway_mul * other.sway_mul,
        }
    }

    pub fn apply(&self, stats: &mut WeaponStats) {
        stats.falloff_start *= self.falloff_mul;
        stats.falloff_end *= self.falloff_mul;
        stats.ads_secs *= self.ads_secs_mul;
        stats.recoil_pitch_deg *= self.recoil_mul;
        stats.recoil_yaw_deg *= self.recoil_mul;
        stats.sway_deg *= self.sway_mul;
    }
}

/// One fitting for an Attachment Slot.
#[derive(Clone, Debug, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub description: String,
    pub slot: AttachmentSlot,
    /// Class points it costs, when that isn't the default for its kind (`class_points.ron`).
    #[serde(default)]
    pub cost: Option<u32>,
    #[serde(default)]
    pub mods: StatMods,
    /// The sight drawn and aimed through (Optics only).
    #[serde(default)]
    pub sight: SightKind,
    /// Magnification of an Optic: drives the ADS field of view, hold breath and glint.
    #[serde(default = "one")]
    pub magnification: f32,
    /// Multiplies the idle sway and gun motion while aimed through an Optic.
    #[serde(default = "one")]
    pub sway_scale: f32,
    /// No muzzle flash.
    #[serde(default)]
    pub silenced: bool,
    /// Multiplies the gunshot's volume.
    #[serde(default = "one")]
    pub shot_volume: f32,
    /// Multiplies the gunshot's playback speed: below 1 is lower and duller.
    #[serde(default = "one")]
    pub shot_pitch: f32,
    /// How far it extends the muzzle in front of the barrel, in metres.
    #[serde(default)]
    pub muzzle_length: f32,
    /// How wide the muzzle device is drawn, as a multiple of the barrel's radius.
    #[serde(default = "default_muzzle_girth")]
    pub muzzle_girth: f32,
    /// Multiplies the size of the muzzle flash in the shooter's own view.
    #[serde(default = "one")]
    pub flash_scale: f32,
    /// Other players see no muzzle flash where this is fitted (the shooter's own view is
    /// `flash_scale` and `silenced`).
    #[serde(default)]
    pub hides_flash_from_others: bool,
}

/// What is fitted to a Weapon. Copy, so a shot can carry it to the server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachments {
    #[serde(default)]
    pub optic: Option<AttachmentId>,
    #[serde(default)]
    pub muzzle: Option<AttachmentId>,
    #[serde(default)]
    pub underbarrel: Option<AttachmentId>,
}

impl Attachments {
    pub fn iter(self) -> impl Iterator<Item = AttachmentId> {
        [self.optic, self.muzzle, self.underbarrel]
            .into_iter()
            .flatten()
    }

    /// How many are fitted.
    pub fn count(self) -> usize {
        self.iter().count()
    }

    pub fn get(self, slot: AttachmentSlot) -> Option<AttachmentId> {
        match slot {
            AttachmentSlot::Optic => self.optic,
            AttachmentSlot::Muzzle => self.muzzle,
            AttachmentSlot::Underbarrel => self.underbarrel,
        }
    }

    pub fn set(&mut self, slot: AttachmentSlot, id: Option<AttachmentId>) {
        match slot {
            AttachmentSlot::Optic => self.optic = id,
            AttachmentSlot::Muzzle => self.muzzle = id,
            AttachmentSlot::Underbarrel => self.underbarrel = id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PerkId {
    SleightOfHand,
    Scavenger,
    Bandolier,
    SteadyAim,
    Quickdraw,
    StoppingPower,
    Lightweight,
    QuickRecovery,
    Juggernaut,
    DangerClose,
    Marathon,
    Ninja,
    Hardened,
    TacticalMask,
    FallGuard,
}

impl PerkId {
    /// Every Perk, in the order menus list them (each Perk Slot's, in turn).
    pub const ALL: [PerkId; 15] = [
        PerkId::SleightOfHand,
        PerkId::Scavenger,
        PerkId::Bandolier,
        PerkId::SteadyAim,
        PerkId::Quickdraw,
        PerkId::StoppingPower,
        PerkId::Lightweight,
        PerkId::QuickRecovery,
        PerkId::Juggernaut,
        PerkId::DangerClose,
        PerkId::Marathon,
        PerkId::Ninja,
        PerkId::Hardened,
        PerkId::TacticalMask,
        PerkId::FallGuard,
    ];
}

/// Number of Perk Slots.
pub const PERK_SLOTS: usize = 3;

/// The Perks of a class: at most one per Perk Slot, and a second one in a Slot whose Perk Greed
/// Wildcard is equipped. Copy, so the client can tell the server and the server can replicate it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Perks {
    /// The Perk of each Perk Slot, in slot order.
    #[serde(default)]
    pub picks: [Option<PerkId>; PERK_SLOTS],
    /// The second Perk of each Perk Slot (Perk Greed).
    #[serde(default)]
    pub greed: [Option<PerkId>; PERK_SLOTS],
}

impl Perks {
    /// Every Perk in effect.
    pub fn iter(self) -> impl Iterator<Item = PerkId> {
        self.picks.into_iter().chain(self.greed).flatten()
    }

    /// How many Perks there are.
    pub fn count(self) -> usize {
        self.iter().count()
    }

    /// The Perks of Perk Slot `slot` (0 to 2): its own and its Greed one.
    pub fn in_slot(self, slot: usize) -> impl Iterator<Item = PerkId> {
        [self.picks[slot], self.greed[slot]].into_iter().flatten()
    }
}

/// What a Perk changes; 1 and `false` change nothing.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct PerkEffects {
    /// Scales every Weapon's reload time.
    pub reload_time_mul: f32,
    /// Scales bullet damage (applied by the server).
    pub damage_mul: f32,
    /// Scales movement speed.
    pub move_speed_mul: f32,
    /// Scales how long a sprint lasts.
    pub sprint_stamina_mul: f32,
    /// Other players don't hear this player's footsteps.
    pub silent_footsteps: bool,
    /// A dead player's ammo pack gives the full refill instead of a share of the Reserve.
    pub scavenger: bool,
    /// Spare magazines added to the Reserve of every Weapon.
    pub extra_magazines: u32,
    /// Scales the hip-fire spread (standing, moving and airborne): below 1 is steadier.
    pub hip_spread_mul: f32,
    /// Scales the time to aim down sights: below 1 is faster.
    pub ads_secs_mul: f32,
    /// Scales how long health regeneration waits after damage (applied by the server).
    pub regen_delay_mul: f32,
    /// Scales how fast health regenerates (applied by the server).
    pub regen_rate_mul: f32,
    /// Scales maximum health (applied by the server, shown by the client).
    pub max_health_mul: f32,
    /// Scales the damage this player's Grenades and launcher rounds deal (applied by the server).
    pub explosive_damage_mul: f32,
    /// Scales how far a hit kicks the view.
    pub flinch_mul: f32,
    /// Scales how long a Flashbang blinds and deafens this player (applied by the server).
    pub flash_time_mul: f32,
    /// Falls hurt this player not at all (applied by the server).
    pub fall_guard: bool,
}

impl Default for PerkEffects {
    fn default() -> Self {
        Self {
            reload_time_mul: 1.0,
            damage_mul: 1.0,
            move_speed_mul: 1.0,
            sprint_stamina_mul: 1.0,
            silent_footsteps: false,
            scavenger: false,
            extra_magazines: 0,
            hip_spread_mul: 1.0,
            ads_secs_mul: 1.0,
            regen_delay_mul: 1.0,
            regen_rate_mul: 1.0,
            max_health_mul: 1.0,
            explosive_damage_mul: 1.0,
            flinch_mul: 1.0,
            flash_time_mul: 1.0,
            fall_guard: false,
        }
    }
}

impl PerkEffects {
    /// Both sets of effects at once: multipliers multiply, magazines add, flags are either's.
    pub fn combine(self, other: PerkEffects) -> PerkEffects {
        PerkEffects {
            reload_time_mul: self.reload_time_mul * other.reload_time_mul,
            damage_mul: self.damage_mul * other.damage_mul,
            move_speed_mul: self.move_speed_mul * other.move_speed_mul,
            sprint_stamina_mul: self.sprint_stamina_mul * other.sprint_stamina_mul,
            silent_footsteps: self.silent_footsteps || other.silent_footsteps,
            scavenger: self.scavenger || other.scavenger,
            extra_magazines: self.extra_magazines + other.extra_magazines,
            hip_spread_mul: self.hip_spread_mul * other.hip_spread_mul,
            ads_secs_mul: self.ads_secs_mul * other.ads_secs_mul,
            regen_delay_mul: self.regen_delay_mul * other.regen_delay_mul,
            regen_rate_mul: self.regen_rate_mul * other.regen_rate_mul,
            max_health_mul: self.max_health_mul * other.max_health_mul,
            explosive_damage_mul: self.explosive_damage_mul * other.explosive_damage_mul,
            flinch_mul: self.flinch_mul * other.flinch_mul,
            flash_time_mul: self.flash_time_mul * other.flash_time_mul,
            fall_guard: self.fall_guard || other.fall_guard,
        }
    }

    /// Puts the effects that change a Weapon (reload, handling, Reserve, spread, flinch) on its
    /// stats, which already carry its Attachments.
    pub fn apply_to_weapon(&self, stats: &mut WeaponStats) {
        stats.reload_secs *= self.reload_time_mul;
        stats.empty_reload_secs *= self.reload_time_mul;
        stats.move_scale *= self.move_speed_mul;
        stats.reserve_mags += self.extra_magazines;
        stats.hip_spread_deg *= self.hip_spread_mul;
        stats.move_spread_deg *= self.hip_spread_mul;
        stats.air_spread_deg *= self.hip_spread_mul;
        stats.ads_secs *= self.ads_secs_mul;
        stats.flinch_deg *= self.flinch_mul;
    }

    /// A player's maximum health, given the Match Rules' `base`.
    pub fn max_health(&self, base: f32) -> f32 {
        base * self.max_health_mul
    }

    /// Seconds without damage before regeneration starts, given the Match Rules' `base`.
    pub fn regen_delay_secs(&self, base: f32) -> f32 {
        base * self.regen_delay_mul
    }

    /// Health regenerated per second, given the Match Rules' `base`.
    pub fn regen_per_sec(&self, base: f32) -> f32 {
        base * self.regen_rate_mul
    }

    /// The damage a fall of `damage` really does.
    pub fn fall_damage(&self, damage: f32) -> f32 {
        if self.fall_guard { 0.0 } else { damage }
    }

    /// The damage a Grenade blast or launcher round of `damage` does when this player fired it.
    pub fn blast_damage(&self, damage: f32) -> f32 {
        damage * self.explosive_damage_mul
    }

    /// How long a Flashbang effect of `secs` lasts on this player.
    pub fn flash_secs(&self, secs: f32) -> f32 {
        secs * self.flash_time_mul
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Perk {
    pub name: String,
    pub description: String,
    /// The Perk Slot it belongs to, 1 to 3.
    pub slot: usize,
    #[serde(default)]
    pub effects: PerkEffects,
}

/// Dropped ammo packs: anyone walking over one takes a share of their Reserve, a Scavenger the lot.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct AmmoPackTuning {
    /// A player within this many metres picks a pack up.
    pub pickup_radius: f32,
    /// Seconds before an untouched pack disappears.
    pub lifetime_secs: f32,
    /// Fraction of each Weapon's maximum Reserve a pack gives, 0 to 1.
    pub share: f32,
}

impl AmmoPackTuning {
    /// What taking a pack gives (an `AmmoRefill::share`): `share` of every Weapon's maximum Reserve,
    /// or with Scavenger the full refill (1).
    pub fn share_for(&self, scavenger: bool) -> f32 {
        if scavenger { 1.0 } else { self.share }
    }
}

/// The Perks a player has equipped. Sent by the client with its Loadout, kept and replicated by
/// the server so it can apply them (damage) and every client can too (footsteps).
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EquippedPerks(pub Perks);

/// An ammo pack a dead player dropped at `position`, replicated to everyone. Server-owned.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmmoPack {
    pub position: Vec3,
}

/// A Loadout (a class): the Weapons, Attachments, Perks, equipment and Wildcards a player
/// spawns with. Default and custom Loadouts share this shape. Whether it is a valid Pick 10
/// class is `GameData::class_errors` (`class`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loadout {
    pub name: String,
    pub primary: WeaponId,
    /// Fitted to the Primary.
    #[serde(default)]
    pub attachments: Attachments,
    /// The second Weapon: a Secondary, or a Primary with the Overkill Wildcard. Optional.
    #[serde(default)]
    pub secondary: Option<WeaponId>,
    #[serde(default)]
    pub perks: Perks,
    #[serde(default)]
    pub lethal: Option<EquipmentId>,
    #[serde(default)]
    pub tactical: Option<EquipmentId>,
    /// At most `class_points.max_wildcards`, each once.
    #[serde(default)]
    pub wildcards: Vec<WildcardId>,
    /// The Primary's camo: an id from `data/progression.ron`, or `none`.
    #[serde(default = "no_camo")]
    pub camo: String,
}

impl Loadout {
    /// Whether the class carries `wildcard`.
    pub fn has(&self, wildcard: WildcardId) -> bool {
        self.wildcards.contains(&wildcard)
    }

    /// The equipment in `slot`.
    pub fn equipment(&self, slot: EquipmentSlot) -> Option<EquipmentId> {
        match slot {
            EquipmentSlot::Lethal => self.lethal,
            EquipmentSlot::Tactical => self.tactical,
        }
    }

    pub fn set_equipment(&mut self, slot: EquipmentSlot, id: Option<EquipmentId>) {
        match slot {
            EquipmentSlot::Lethal => self.lethal = id,
            EquipmentSlot::Tactical => self.tactical = id,
        }
    }

    /// The Loadout with everything that is the wrong kind of thing for its place replaced or
    /// emptied (a hand-edited or stale profile): a non-Primary Primary, an Attachment in the
    /// wrong Slot or one the Primary doesn't offer, a Perk from another Perk Slot, equipment in
    /// the wrong slot, a repeated Wildcard. How many things a class may hold and what they cost
    /// is not repaired here but reported by `GameData::class_errors` (and trimmed by
    /// `fitted`).
    pub fn validated(mut self, data: &GameData) -> Loadout {
        if data.weapon(self.primary).class != WeaponClass::Primary {
            self.primary = WeaponId::ALL
                .into_iter()
                .find(|id| data.weapon(*id).class == WeaponClass::Primary)
                .expect("checked at startup");
        }
        let stats = data.weapon(self.primary);
        for slot in AttachmentSlot::ALL {
            let fits = |id: AttachmentId| {
                data.attachment(id).slot == slot
                    && stats.accepts(slot)
                    && data
                        .progression
                        .attachment_level(self.primary, id)
                        .is_some()
            };
            let fitted = self.attachments.get(slot).filter(|id| fits(*id));
            self.attachments.set(slot, fitted);
        }
        for slot in 0..PERK_SLOTS {
            let in_slot = |perk: &PerkId| data.perk(*perk).slot == slot + 1;
            self.perks.picks[slot] = self.perks.picks[slot].filter(in_slot);
            self.perks.greed[slot] = self.perks.greed[slot]
                .filter(in_slot)
                .filter(|perk| Some(*perk) != self.perks.picks[slot]);
        }
        for slot in [EquipmentSlot::Lethal, EquipmentSlot::Tactical] {
            let fitted = self.equipment(slot).filter(|id| {
                data.class_points
                    .equipment
                    .get(id)
                    .is_some_and(|equipment| equipment.slot == slot)
            });
            self.set_equipment(slot, fitted);
        }
        let mut seen = Vec::new();
        self.wildcards.retain(|wildcard| {
            let new = !seen.contains(wildcard);
            seen.push(*wildcard);
            new
        });
        self.name = self
            .name
            .trim()
            .chars()
            .take(MAX_LOADOUT_NAME_CHARS)
            .collect();
        if self.camo.is_empty() {
            self.camo = no_camo();
        }
        self
    }
}

impl WeaponStats {
    /// The short name used in toasts and lock notes: "M4" for "M4 (AR)".
    pub fn short_name(&self) -> &str {
        self.name.split(" (").next().unwrap_or(&self.name)
    }

    /// Whether a Weapon has a place for an Attachment in `slot`: Secondaries have none, and a
    /// built-in scope leaves no room for an Optic.
    pub fn accepts(&self, slot: AttachmentSlot) -> bool {
        match slot {
            AttachmentSlot::Optic => self.class == WeaponClass::Primary && !self.scope,
            AttachmentSlot::Muzzle | AttachmentSlot::Underbarrel => {
                self.class == WeaponClass::Primary
            }
        }
    }
}

impl GameData {
    pub fn attachment(&self, id: AttachmentId) -> &Attachment {
        &self.attachments[&id]
    }

    pub fn perk(&self, id: PerkId) -> &Perk {
        &self.perks[&id]
    }

    /// The Perks of Perk Slot `slot` (1 to 3), in menu order.
    pub fn perks_in_slot(&self, slot: usize) -> impl Iterator<Item = PerkId> + '_ {
        PerkId::ALL
            .into_iter()
            .filter(move |id| self.perk(*id).slot == slot)
    }

    /// The Muzzle Attachment really fitted to `weapon`: none on a Weapon without a Muzzle slot.
    fn fitted_muzzle(&self, weapon: WeaponId, attachments: Attachments) -> Option<&Attachment> {
        attachments
            .muzzle
            .filter(|_| self.weapon(weapon).accepts(AttachmentSlot::Muzzle))
            .map(|id| self.attachment(id))
    }

    /// The volume and playback speed of `weapon`'s gunshot with `attachments` fitted: its own
    /// (`shot_volume`, `shot_pitch` in weapons.ron) times its Muzzle Attachment's.
    pub fn shot_sound(&self, weapon: WeaponId, attachments: Attachments) -> (f32, f32) {
        let own = self.weapon(weapon);
        self.fitted_muzzle(weapon, attachments).map_or(
            (own.shot_volume, own.shot_pitch),
            |muzzle| {
                (
                    own.shot_volume * muzzle.shot_volume,
                    own.shot_pitch * muzzle.shot_pitch,
                )
            },
        )
    }

    /// How big a muzzle flash other players see where `weapon` fires with `attachments` fitted,
    /// against a plain muzzle's (1): none (0) with a Muzzle Attachment that hides it (the Silencer,
    /// the Flash Hider), more with a Compensator.
    pub fn flash_scale_seen_by_others(&self, weapon: WeaponId, attachments: Attachments) -> f32 {
        self.fitted_muzzle(weapon, attachments)
            .map_or(1.0, |muzzle| {
                if muzzle.hides_flash_from_others {
                    0.0
                } else {
                    muzzle.flash_scale
                }
            })
    }

    /// The Attachments fitted at once, combined.
    pub fn attachment_mods(&self, attachments: Attachments) -> StatMods {
        attachments.iter().fold(StatMods::default(), |mods, id| {
            mods.stack(self.attachment(id).mods)
        })
    }

    /// What `perks` do together: every Perk's effects stack, a Perk Greed pair included.
    pub fn perk_effects(&self, perks: impl IntoIterator<Item = PerkId>) -> PerkEffects {
        perks
            .into_iter()
            .fold(PerkEffects::default(), |effects, id| {
                effects.combine(self.perk(id).effects)
            })
    }

    /// A Weapon's stats with its Attachments fitted and the carrier's Perks in effect.
    pub fn effective_weapon(
        &self,
        id: WeaponId,
        attachments: Attachments,
        perks: &PerkEffects,
    ) -> WeaponStats {
        let mut stats = self.weapon(id).clone();
        self.attachment_mods(attachments).apply(&mut stats);
        match attachments
            .optic
            .filter(|_| stats.accepts(AttachmentSlot::Optic))
        {
            Some(optic) => {
                let optic = self.attachment(optic);
                stats.sight = optic.sight;
                stats.magnification = optic.magnification;
                stats.sway_scale = optic.sway_scale;
            }
            None => {
                stats.magnification = stats.iron_magnification;
                stats.sway_scale = stats.iron_sway_scale;
                if stats.scope {
                    stats.sight = SightKind::Scope;
                } else {
                    stats.sight = SightKind::Irons;
                    stats.sight_point = stats.iron_point;
                    stats.sight_eye_relief = stats.iron_eye_relief;
                }
            }
        }
        if let Some(muzzle) = self.fitted_muzzle(id, attachments) {
            stats.silenced = muzzle.silenced;
            stats.shot_volume *= muzzle.shot_volume;
            stats.shot_pitch *= muzzle.shot_pitch;
            stats.muzzle_length = muzzle.muzzle_length;
            stats.muzzle_girth = muzzle.muzzle_girth;
            stats.flash_scale = muzzle.flash_scale;
            stats.muzzle_offset.2 -= muzzle.muzzle_length;
        }
        perks.apply_to_weapon(&mut stats);
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> GameData {
        GameData::load()
    }

    fn picks(a: PerkId, b: PerkId, c: PerkId) -> Perks {
        Perks {
            picks: [Some(a), Some(b), Some(c)],
            greed: [None; PERK_SLOTS],
        }
    }

    #[test]
    fn silencer_shortens_falloff_and_quiets_the_shot() {
        let data = data();
        let base = data.weapon(WeaponId::Smg);
        let silenced = data.effective_weapon(
            WeaponId::Smg,
            Attachments {
                muzzle: Some(AttachmentId::Silencer),
                ..Attachments::default()
            },
            &PerkEffects::default(),
        );
        assert!((silenced.falloff_end - base.falloff_end * 0.75).abs() < 1e-4);
        assert!((silenced.falloff_start - base.falloff_start * 0.75).abs() < 1e-4);
        assert!(silenced.silenced && silenced.shot_volume < 1.0 && silenced.shot_pitch < 1.0);
        assert!(!base.silenced);
    }

    #[test]
    fn a_foregrip_steadies_the_recoil_and_slows_the_aim_a_little() {
        let data = data();
        let base = data.weapon(WeaponId::AssaultRifle);
        let gripped = data.effective_weapon(
            WeaponId::AssaultRifle,
            Attachments {
                underbarrel: Some(AttachmentId::Foregrip),
                ..Attachments::default()
            },
            &PerkEffects::default(),
        );
        assert!((gripped.recoil_pitch_deg - base.recoil_pitch_deg * 0.75).abs() < 1e-4);
        assert!((gripped.recoil_yaw_deg - base.recoil_yaw_deg * 0.75).abs() < 1e-4);
        assert!(gripped.ads_secs > base.ads_secs);
    }

    #[test]
    fn grips_steady_the_sway_or_quicken_the_aim() {
        let data = data();
        let rifle = WeaponId::AssaultRifle;
        let with = |id| {
            data.effective_weapon(
                rifle,
                Attachments {
                    underbarrel: Some(id),
                    ..Attachments::default()
                },
                &PerkEffects::default(),
            )
        };
        let base = data.weapon(rifle);
        let (foregrip, angled) = (with(AttachmentId::Foregrip), with(AttachmentId::AngledGrip));
        // The Foregrip steadies recoil and sway; the Angled Grip steadies less but aims faster.
        assert!(foregrip.sway_deg < base.sway_deg);
        assert!((angled.sway_deg - base.sway_deg).abs() < 1e-6);
        assert!(angled.recoil_pitch_deg < base.recoil_pitch_deg);
        assert!(angled.recoil_pitch_deg > foregrip.recoil_pitch_deg);
        assert!(angled.ads_secs < base.ads_secs && foregrip.ads_secs > base.ads_secs);
    }

    #[test]
    fn muzzle_devices_trade_recoil_flash_and_what_others_see() {
        let data = data();
        let rifle = WeaponId::AssaultRifle;
        let with = |muzzle| Attachments {
            muzzle,
            ..Attachments::default()
        };
        let stats =
            |attachments| data.effective_weapon(rifle, attachments, &PerkEffects::default());
        let plain = stats(Attachments::default());
        let compensator = stats(with(Some(AttachmentId::Compensator)));
        assert!(compensator.recoil_pitch_deg < plain.recoil_pitch_deg);
        assert!(compensator.flash_scale > 1.0 && !compensator.silenced);
        assert!(compensator.shot_volume > plain.shot_volume);
        let hider = stats(with(Some(AttachmentId::FlashHider)));
        assert!(hider.flash_scale > 0.0 && hider.flash_scale < 1.0 && !hider.silenced);
        assert_eq!(hider.shot_volume, plain.shot_volume);
        assert!((hider.recoil_pitch_deg - plain.recoil_pitch_deg).abs() < 1e-6);

        // What others see: a plain flash, a bigger one, or none.
        let seen = |weapon, attachments| data.flash_scale_seen_by_others(weapon, attachments);
        assert_eq!(seen(rifle, Attachments::default()), 1.0);
        assert!(seen(rifle, with(Some(AttachmentId::Compensator))) > 1.0);
        assert_eq!(seen(rifle, with(Some(AttachmentId::FlashHider))), 0.0);
        assert_eq!(seen(rifle, with(Some(AttachmentId::Silencer))), 0.0);
        // A Weapon with no Muzzle slot ignores whatever the message says.
        assert_eq!(
            seen(WeaponId::Pistol, with(Some(AttachmentId::FlashHider))),
            1.0
        );
    }

    #[test]
    fn a_gunshot_is_the_weapons_own_loudness_times_its_muzzle_devices() {
        let data = data();
        let ak = data.weapon(WeaponId::Ak47);
        let bare = Attachments::default();
        assert_eq!(
            data.shot_sound(WeaponId::Ak47, bare),
            (ak.shot_volume, ak.shot_pitch)
        );
        let silenced = Attachments {
            muzzle: Some(AttachmentId::Silencer),
            ..bare
        };
        let (volume, pitch) = data.shot_sound(WeaponId::Ak47, silenced);
        let silencer = data.attachment(AttachmentId::Silencer);
        assert!((volume - ak.shot_volume * silencer.shot_volume).abs() < 1e-6);
        assert!((pitch - ak.shot_pitch * silencer.shot_pitch).abs() < 1e-6);
        // An Optic or grip does not change the sound.
        let sighted = Attachments {
            optic: Some(AttachmentId::Scope6x),
            underbarrel: Some(AttachmentId::Foregrip),
            ..bare
        };
        assert_eq!(
            data.shot_sound(WeaponId::Ak47, sighted),
            (ak.shot_volume, ak.shot_pitch)
        );
        // The shot the Weapon's effective stats carry is the same one the others hear.
        let effective = data.effective_weapon(WeaponId::Ak47, silenced, &PerkEffects::default());
        assert_eq!(
            data.shot_sound(WeaponId::Ak47, silenced),
            (effective.shot_volume, effective.shot_pitch)
        );
    }

    #[test]
    fn the_reflex_and_the_6x_scope_are_the_ends_of_the_optic_range() {
        let data = data();
        let rifle = |optic| {
            data.effective_weapon(
                WeaponId::AssaultRifle,
                Attachments {
                    optic: Some(optic),
                    ..Attachments::default()
                },
                &PerkEffects::default(),
            )
        };
        let base = data.weapon(WeaponId::AssaultRifle);
        let reflex = rifle(AttachmentId::Reflex);
        assert_eq!(
            (reflex.sight, reflex.magnification),
            (SightKind::Reflex, 1.0)
        );
        assert!(reflex.ads_secs < base.ads_secs && !reflex.steady_sight());
        let scope = rifle(AttachmentId::Scope6x);
        assert_eq!(scope.sight, SightKind::LongScope);
        assert!(scope.magnification > rifle(AttachmentId::Acog).magnification);
        assert!(scope.steady_sight() && scope.ads_secs > rifle(AttachmentId::Acog).ads_secs);
        // Heavy optics cost more class points than the 1x ones.
        assert!(
            data.attachment_cost(AttachmentId::Scope6x)
                > data.attachment_cost(AttachmentId::Reflex)
        );
    }

    #[test]
    fn optics_set_magnification_sway_and_the_field_of_view() {
        let data = data();
        let rifle = |optic: Option<AttachmentId>| {
            data.effective_weapon(
                WeaponId::AssaultRifle,
                Attachments {
                    optic,
                    ..Attachments::default()
                },
                &PerkEffects::default(),
            )
        };
        let irons = rifle(None);
        let lineup = [
            (None, SightKind::Irons, 1.0, 0.8),
            (Some(AttachmentId::RedDot), SightKind::RedDot, 1.0, 0.9),
            (
                Some(AttachmentId::Holographic),
                SightKind::Holographic,
                1.5,
                0.95,
            ),
            (Some(AttachmentId::Prism), SightKind::Prism, 2.0, 1.0),
            (Some(AttachmentId::Acog), SightKind::Acog, 4.0, 1.15),
        ];
        for (optic, sight, magnification, sway_scale) in lineup {
            let stats = rifle(optic);
            assert_eq!(stats.sight, sight);
            assert_eq!(stats.magnification, magnification, "{sight:?}");
            assert_eq!(stats.sway_scale, sway_scale, "{sight:?}");
            assert_eq!(
                stats.steady_sight(),
                magnification >= 3.0,
                "hold breath and glint from 3x: {sight:?}"
            );
        }
        let acog = rifle(Some(AttachmentId::Acog));
        assert!(acog.ads_fov_scale() < irons.ads_fov_scale());
        assert!(acog.ads_secs > irons.ads_secs);
        let red_dot = rifle(Some(AttachmentId::RedDot));
        assert!((red_dot.ads_fov_scale() - irons.ads_fov_scale()).abs() < 1e-6);
    }

    #[test]
    fn no_optic_means_iron_sights_except_on_a_scoped_weapon() {
        let data = data();
        let none = Attachments::default();
        let smg = data.effective_weapon(WeaponId::Smg, none, &PerkEffects::default());
        assert_eq!(smg.sight, SightKind::Irons);
        let sniper = data.effective_weapon(WeaponId::SniperRifle, none, &PerkEffects::default());
        assert_eq!(sniper.sight, SightKind::Scope);
        // A scope leaves no room for an Optic.
        let red_dot = Attachments {
            optic: Some(AttachmentId::RedDot),
            ..Attachments::default()
        };
        let sniper = data.effective_weapon(WeaponId::SniperRifle, red_dot, &PerkEffects::default());
        assert_eq!(sniper.sight, SightKind::Scope);
        assert_eq!((sniper.magnification, sniper.sway_scale), (8.0, 1.3));
        assert!(sniper.steady_sight());
    }

    #[test]
    fn perks_stack_and_change_reload_and_speed() {
        let data = data();
        let effects = data.perk_effects([
            PerkId::SleightOfHand,
            PerkId::StoppingPower,
            PerkId::Marathon,
        ]);
        assert!((effects.reload_time_mul - 0.7).abs() < 1e-6);
        assert!((effects.damage_mul - 1.25).abs() < 1e-6);
        assert!((effects.sprint_stamina_mul - 2.5).abs() < 1e-6);
        assert!(!effects.silent_footsteps && !effects.scavenger);

        let base = data.weapon(WeaponId::Smg);
        let stats = data.effective_weapon(WeaponId::Smg, Attachments::default(), &effects);
        assert!((stats.reload_secs - base.reload_secs * 0.7).abs() < 1e-4);
        assert!((stats.empty_reload_secs - base.empty_reload_secs * 0.7).abs() < 1e-4);

        let light = data.perk_effects([PerkId::Lightweight, PerkId::Ninja, PerkId::Scavenger]);
        assert!((light.move_speed_mul - 1.1).abs() < 1e-6);
        assert!(light.silent_footsteps && light.scavenger);
    }

    #[test]
    fn a_perk_greed_pair_in_one_slot_stacks() {
        let data = data();
        let perks = Perks {
            greed: [Some(PerkId::Scavenger), None, None],
            ..picks(
                PerkId::SleightOfHand,
                PerkId::StoppingPower,
                PerkId::Marathon,
            )
        };
        assert_eq!(perks.count(), 4);
        assert_eq!(
            perks.in_slot(0).collect::<Vec<_>>(),
            [PerkId::SleightOfHand, PerkId::Scavenger]
        );
        let effects = data.perk_effects(perks.iter());
        assert!(
            (effects.reload_time_mul - 0.7).abs() < 1e-6,
            "first of slot 1"
        );
        assert!(effects.scavenger, "second of slot 1");
        assert!((effects.damage_mul - 1.25).abs() < 1e-6, "slot 2 unchanged");
    }

    #[test]
    fn every_perk_has_a_slot_a_description_and_five_to_a_slot() {
        let data = data();
        for id in PerkId::ALL {
            let perk = data.perk(id);
            assert!(
                (1..=PERK_SLOTS).contains(&perk.slot),
                "{id:?} has a slot 1 to 3"
            );
            assert!(!perk.name.trim().is_empty(), "{id:?} has a name");
            assert!(
                !perk.description.trim().is_empty(),
                "{id:?} has a description"
            );
        }
        for slot in 1..=PERK_SLOTS {
            assert_eq!(data.perks_in_slot(slot).count(), 5, "Perk Slot {slot}");
        }
    }

    #[test]
    fn new_perks_do_nothing_but_their_own_effect() {
        let data = data();
        let none = PerkEffects::default();
        let only = |id: PerkId, expected: PerkEffects| {
            assert_eq!(data.perk(id).effects, expected, "{id:?}");
        };
        only(
            PerkId::Bandolier,
            PerkEffects {
                extra_magazines: 1,
                ..none
            },
        );
        only(
            PerkId::SteadyAim,
            PerkEffects {
                hip_spread_mul: 0.7,
                ..none
            },
        );
        only(
            PerkId::Quickdraw,
            PerkEffects {
                ads_secs_mul: 0.7,
                ..none
            },
        );
        only(
            PerkId::QuickRecovery,
            PerkEffects {
                regen_delay_mul: 0.6,
                regen_rate_mul: 1.5,
                ..none
            },
        );
        only(
            PerkId::Juggernaut,
            PerkEffects {
                max_health_mul: 1.25,
                ..none
            },
        );
        only(
            PerkId::DangerClose,
            PerkEffects {
                explosive_damage_mul: 1.3,
                ..none
            },
        );
        only(
            PerkId::Hardened,
            PerkEffects {
                flinch_mul: 0.5,
                ..none
            },
        );
        only(
            PerkId::TacticalMask,
            PerkEffects {
                flash_time_mul: 0.5,
                ..none
            },
        );
        only(
            PerkId::FallGuard,
            PerkEffects {
                fall_guard: true,
                ..none
            },
        );
    }

    #[test]
    fn merging_effects_multiplies_adds_and_ors_each_new_field() {
        let none = PerkEffects::default();
        let a = PerkEffects {
            extra_magazines: 1,
            hip_spread_mul: 0.7,
            ads_secs_mul: 0.7,
            regen_delay_mul: 0.6,
            regen_rate_mul: 1.5,
            max_health_mul: 1.25,
            explosive_damage_mul: 1.3,
            flinch_mul: 0.5,
            flash_time_mul: 0.5,
            fall_guard: true,
            ..none
        };
        assert_eq!(a.combine(none), a, "the defaults change nothing");
        assert_eq!(none.combine(a), a);

        let both = a.combine(a);
        assert_eq!(both.extra_magazines, 2);
        assert!((both.hip_spread_mul - 0.49).abs() < 1e-6);
        assert!((both.ads_secs_mul - 0.49).abs() < 1e-6);
        assert!((both.regen_delay_mul - 0.36).abs() < 1e-6);
        assert!((both.regen_rate_mul - 2.25).abs() < 1e-6);
        assert!((both.max_health_mul - 1.5625).abs() < 1e-6);
        assert!((both.explosive_damage_mul - 1.69).abs() < 1e-6);
        assert!((both.flinch_mul - 0.25).abs() < 1e-6);
        assert!((both.flash_time_mul - 0.25).abs() < 1e-6);
        assert!(both.fall_guard);

        let other = PerkEffects {
            fall_guard: false,
            ..none
        };
        assert!(
            a.combine(other).fall_guard && other.combine(a).fall_guard,
            "either's"
        );
        assert!(!other.combine(other).fall_guard);
    }

    #[test]
    fn perk_greed_pairs_of_the_new_perks_stack() {
        let data = data();
        let effects = data.perk_effects([PerkId::Bandolier, PerkId::SteadyAim, PerkId::Quickdraw]);
        assert_eq!(effects.extra_magazines, 1);
        assert!((effects.hip_spread_mul - 0.7).abs() < 1e-6);
        assert!((effects.ads_secs_mul - 0.7).abs() < 1e-6);
        let tank = data.perk_effects([
            PerkId::Juggernaut,
            PerkId::QuickRecovery,
            PerkId::DangerClose,
        ]);
        assert!((tank.max_health_mul - 1.25).abs() < 1e-6);
        assert!((tank.regen_rate_mul - 1.5).abs() < 1e-6);
        assert!((tank.explosive_damage_mul - 1.3).abs() < 1e-6);
        let tough = data.perk_effects([PerkId::Hardened, PerkId::TacticalMask, PerkId::FallGuard]);
        assert!((tough.flinch_mul - 0.5).abs() < 1e-6);
        assert!((tough.flash_time_mul - 0.5).abs() < 1e-6);
        assert!(tough.fall_guard);
    }

    #[test]
    fn weapon_perks_change_reserve_spread_aim_time_and_flinch_of_every_weapon() {
        let data = data();
        let effects = data.perk_effects([
            PerkId::Bandolier,
            PerkId::SteadyAim,
            PerkId::Quickdraw,
            PerkId::Hardened,
        ]);
        for id in data.weapons.keys().copied() {
            let base = data.weapon(id);
            let stats = data.effective_weapon(id, Attachments::default(), &effects);
            assert_eq!(stats.reserve_mags, base.reserve_mags + 1, "{id:?} Reserve");
            assert_eq!(
                stats.reserve_max(),
                (base.reserve_mags + 1) * base.magazine,
                "{id:?} one more magazine of rounds"
            );
            assert!((stats.hip_spread_deg - base.hip_spread_deg * 0.7).abs() < 1e-5);
            assert!((stats.move_spread_deg - base.move_spread_deg * 0.7).abs() < 1e-5);
            assert!((stats.air_spread_deg - base.air_spread_deg * 0.7).abs() < 1e-5);
            assert_eq!(
                stats.ads_spread_deg, base.ads_spread_deg,
                "aimed spread is untouched"
            );
            assert!((stats.ads_secs - base.ads_secs * 0.7).abs() < 1e-5);
            assert!((stats.flinch_deg - base.flinch_deg * 0.5).abs() < 1e-5);
        }
        let plain = data.effective_weapon(
            WeaponId::Smg,
            Attachments::default(),
            &PerkEffects::default(),
        );
        assert_eq!(plain.reserve_mags, data.weapon(WeaponId::Smg).reserve_mags);
    }

    #[test]
    fn quickdraw_stacks_with_a_foregrips_slower_aim() {
        let data = data();
        let gripped = Attachments {
            underbarrel: Some(AttachmentId::Foregrip),
            ..Attachments::default()
        };
        let slow = data.effective_weapon(WeaponId::AssaultRifle, gripped, &PerkEffects::default());
        let quick = data.effective_weapon(
            WeaponId::AssaultRifle,
            gripped,
            &data.perk_effects([PerkId::Quickdraw]),
        );
        assert!((quick.ads_secs - slow.ads_secs * 0.7).abs() < 1e-5);
    }

    #[test]
    fn health_perks_scale_maximum_regeneration_and_falls() {
        let data = data();
        let none = PerkEffects::default();
        assert_eq!(none.max_health(100.0), 100.0);
        assert_eq!(none.regen_delay_secs(5.0), 5.0);
        assert_eq!(none.regen_per_sec(20.0), 20.0);
        assert_eq!(none.fall_damage(37.0), 37.0);

        let juggernaut = data.perk_effects([PerkId::Juggernaut]);
        assert!((juggernaut.max_health(100.0) - 125.0).abs() < 1e-4);
        assert_eq!(
            juggernaut.regen_per_sec(20.0),
            20.0,
            "only the maximum changes"
        );

        let recovery = data.perk_effects([PerkId::QuickRecovery]);
        assert!(
            (recovery.regen_delay_secs(5.0) - 3.0).abs() < 1e-5,
            "40% sooner"
        );
        assert!(
            (recovery.regen_per_sec(20.0) - 30.0).abs() < 1e-4,
            "50% faster"
        );
        assert_eq!(recovery.max_health(100.0), 100.0);

        let guard = data.perk_effects([PerkId::FallGuard]);
        assert_eq!(guard.fall_damage(37.0), 0.0);
        assert_eq!(guard.fall_damage(0.0), 0.0);
    }

    #[test]
    fn danger_close_boosts_blasts_and_the_mask_shortens_flashes() {
        let data = data();
        let none = PerkEffects::default();
        assert_eq!(none.blast_damage(100.0), 100.0);
        assert_eq!(none.flash_secs(4.0), 4.0);

        let danger = data.perk_effects([PerkId::DangerClose]);
        assert!((danger.blast_damage(100.0) - 130.0).abs() < 1e-4);
        assert_eq!(danger.flash_secs(4.0), 4.0, "it doesn't touch flashes");

        let mask = data.perk_effects([PerkId::TacticalMask]);
        assert!((mask.flash_secs(4.0) - 2.0).abs() < 1e-6);
        assert_eq!(mask.blast_damage(100.0), 100.0);
        assert_eq!(mask.flash_secs(0.0), 0.0);
    }

    #[test]
    fn validation_repairs_a_bad_loadout() {
        let data = data();
        let bad = Loadout {
            name: "   a very long class name indeed  ".into(),
            primary: WeaponId::Pistol,
            attachments: Attachments {
                optic: Some(AttachmentId::Silencer),
                muzzle: Some(AttachmentId::Acog),
                underbarrel: Some(AttachmentId::RedDot),
            },
            secondary: Some(WeaponId::Smg),
            // Slot 1 holds a Slot 2 Perk and vice versa; the Greed Perk repeats its slot's pick.
            perks: Perks {
                picks: [
                    Some(PerkId::Ninja),
                    Some(PerkId::SleightOfHand),
                    Some(PerkId::Marathon),
                ],
                greed: [None, None, Some(PerkId::Marathon)],
            },
            lethal: None,
            tactical: None,
            wildcards: vec![
                WildcardId::Overkill,
                WildcardId::Overkill,
                WildcardId::Gunfighter,
            ],
            camo: String::new(),
        }
        .validated(&data);
        assert_eq!(data.weapon(bad.primary).class, WeaponClass::Primary);
        assert_eq!(
            bad.secondary,
            Some(WeaponId::Smg),
            "left for the class rules"
        );
        assert_eq!(bad.attachments, Attachments::default());
        assert_eq!(
            bad.perks,
            Perks {
                picks: [None, None, Some(PerkId::Marathon)],
                greed: [None; PERK_SLOTS],
            }
        );
        assert_eq!(
            bad.wildcards,
            [WildcardId::Overkill, WildcardId::Gunfighter]
        );
        assert_eq!(bad.name.chars().count(), MAX_LOADOUT_NAME_CHARS);
        assert_eq!(bad.camo, NO_CAMO);
    }

    #[test]
    fn a_scoped_primary_loses_its_optic() {
        let data = data();
        let loadout = Loadout {
            name: "Sniper".into(),
            primary: WeaponId::SniperRifle,
            attachments: Attachments {
                optic: Some(AttachmentId::Acog),
                muzzle: Some(AttachmentId::Silencer),
                underbarrel: None,
            },
            secondary: Some(WeaponId::Pistol),
            perks: picks(
                PerkId::SleightOfHand,
                PerkId::StoppingPower,
                PerkId::Marathon,
            ),
            lethal: None,
            tactical: None,
            wildcards: Vec::new(),
            camo: NO_CAMO.into(),
        }
        .validated(&data);
        assert_eq!(loadout.attachments.optic, None);
        assert_eq!(loadout.attachments.muzzle, Some(AttachmentId::Silencer));
    }

    #[test]
    fn default_loadouts_are_unchanged_by_validation() {
        let data = data();
        for loadout in &data.default_loadouts {
            assert_eq!(
                loadout,
                &loadout.clone().validated(&data),
                "{}",
                loadout.name
            );
        }
    }

    #[test]
    fn an_ammo_pack_gives_everyone_a_share_and_a_scavenger_the_full_refill() {
        let data = data();
        let pack = data.ammo_pack;
        assert_eq!(pack.share_for(false), pack.share);
        assert_eq!(pack.share_for(true), 1.0);
        assert!(pack.share < 1.0, "a share is only part of a Reserve");

        // For the M4 a share is about one spare magazine; the full refill is the whole Reserve.
        let m4 = data.weapon(WeaponId::AssaultRifle);
        assert_eq!(m4.reserve_share(pack.share_for(false)), m4.magazine);
        assert_eq!(m4.reserve_share(pack.share_for(true)), m4.reserve_max());

        // Only the Scavenger Perk makes the difference (Perk Greed included).
        let scavenger = |perks: Perks| data.perk_effects(perks.iter()).scavenger;
        assert!(scavenger(picks(
            PerkId::Scavenger,
            PerkId::StoppingPower,
            PerkId::Marathon
        )));
        assert!(!scavenger(picks(
            PerkId::SleightOfHand,
            PerkId::StoppingPower,
            PerkId::Marathon
        )));
    }
}
