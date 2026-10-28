//! What a player carries: Attachments, Perks and the Loadout that picks them (CONTEXT.md:
//! Loadout, Attachment, Perk), plus the pure rules that turn them into effective Weapon stats and
//! Perk effects. Shared by the client (viewmodel, HUD, editor) and the server (damage, range).
//!
//! The numbers live in `data/attachments.ron` and `data/perks.ron`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::data::{GameData, SightKind, WeaponClass, WeaponId, WeaponStats};

/// Longest Loadout name the editor accepts.
pub const MAX_LOADOUT_NAME_CHARS: usize = 16;

/// The camo of a Loadout that has none.
pub const NO_CAMO: &str = "none";

fn one() -> f32 {
    1.0
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
}

impl AttachmentId {
    /// Every Attachment, in the order menus list them.
    pub const ALL: [AttachmentId; 5] = [
        AttachmentId::RedDot,
        AttachmentId::Holographic,
        AttachmentId::Prism,
        AttachmentId::Acog,
        AttachmentId::Silencer,
    ];
}

/// A position on a Weapon that holds at most one Attachment (CONTEXT.md: Attachment Slot).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub enum AttachmentSlot {
    Optic,
    Muzzle,
}

/// Multipliers an Attachment applies to a Weapon's stats; 1 changes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct StatMods {
    /// Scales both falloff distances: below 1 the damage drops off sooner.
    pub falloff_mul: f32,
    /// Scales the time to aim down sights: above 1 is slower.
    pub ads_secs_mul: f32,
}

impl Default for StatMods {
    fn default() -> Self {
        Self {
            falloff_mul: 1.0,
            ads_secs_mul: 1.0,
        }
    }
}

impl StatMods {
    /// Both sets of modifiers at once.
    pub fn stack(self, other: StatMods) -> StatMods {
        StatMods {
            falloff_mul: self.falloff_mul * other.falloff_mul,
            ads_secs_mul: self.ads_secs_mul * other.ads_secs_mul,
        }
    }

    pub fn apply(&self, stats: &mut WeaponStats) {
        stats.falloff_start *= self.falloff_mul;
        stats.falloff_end *= self.falloff_mul;
        stats.ads_secs *= self.ads_secs_mul;
    }
}

/// One fitting for an Attachment Slot.
#[derive(Clone, Debug, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub description: String,
    pub slot: AttachmentSlot,
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
}

/// What is fitted to a Weapon. Copy, so a shot can carry it to the server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachments {
    #[serde(default)]
    pub optic: Option<AttachmentId>,
    #[serde(default)]
    pub muzzle: Option<AttachmentId>,
}

impl Attachments {
    pub fn iter(self) -> impl Iterator<Item = AttachmentId> {
        [self.optic, self.muzzle].into_iter().flatten()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PerkId {
    SleightOfHand,
    Scavenger,
    StoppingPower,
    Lightweight,
    Marathon,
    Ninja,
}

impl PerkId {
    /// Every Perk, in the order menus list them (each Perk Slot's two, in turn).
    pub const ALL: [PerkId; 6] = [
        PerkId::SleightOfHand,
        PerkId::Scavenger,
        PerkId::StoppingPower,
        PerkId::Lightweight,
        PerkId::Marathon,
        PerkId::Ninja,
    ];
}

/// One Perk per Perk Slot, in slot order.
pub type PerkSet = [PerkId; 3];

/// Number of Perk Slots.
pub const PERK_SLOTS: usize = 3;

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
    /// Walking over a dead player's ammo pack refills ammo.
    pub scavenger: bool,
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
        }
    }
}

impl PerkEffects {
    /// Both sets of effects at once.
    pub fn combine(self, other: PerkEffects) -> PerkEffects {
        PerkEffects {
            reload_time_mul: self.reload_time_mul * other.reload_time_mul,
            damage_mul: self.damage_mul * other.damage_mul,
            move_speed_mul: self.move_speed_mul * other.move_speed_mul,
            sprint_stamina_mul: self.sprint_stamina_mul * other.sprint_stamina_mul,
            silent_footsteps: self.silent_footsteps || other.silent_footsteps,
            scavenger: self.scavenger || other.scavenger,
        }
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

/// Dropped ammo packs (the Scavenger Perk's pickups).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct AmmoPackTuning {
    /// A Scavenger within this many metres picks a pack up.
    pub pickup_radius: f32,
    /// Seconds before an untouched pack disappears.
    pub lifetime_secs: f32,
}

/// The Perks a player has equipped. Sent by the client with its Loadout, kept and replicated by
/// the server so it can apply them (damage) and every client can too (footsteps).
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EquippedPerks(pub PerkSet);

/// An ammo pack a dead player dropped at `position`, replicated to everyone. Server-owned.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmmoPack {
    pub position: Vec3,
}

/// A Loadout: the Weapons, Attachments and Perks a player spawns with. Default and custom
/// Loadouts share this shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loadout {
    pub name: String,
    pub primary: WeaponId,
    /// Fitted to the Primary.
    #[serde(default)]
    pub attachments: Attachments,
    pub secondary: WeaponId,
    pub perks: PerkSet,
    /// The Primary's camo: an id from `data/progression.ron`, or `none`.
    #[serde(default = "no_camo")]
    pub camo: String,
}

impl Loadout {
    /// The Loadout with everything the data doesn't allow replaced (a hand-edited or stale
    /// profile): the wrong kind of Weapon, an Attachment in the wrong Slot or on a Weapon that
    /// can't take it, a Perk from another Perk Slot. Replacements are the first fitting choice.
    pub fn validated(mut self, data: &GameData) -> Loadout {
        let first_weapon = |class| {
            WeaponId::ALL
                .into_iter()
                .find(|id| data.weapon(*id).class == class)
                .expect("checked at startup")
        };
        if data.weapon(self.primary).class != WeaponClass::Primary {
            self.primary = first_weapon(WeaponClass::Primary);
        }
        if data.weapon(self.secondary).class != WeaponClass::Secondary {
            self.secondary = first_weapon(WeaponClass::Secondary);
        }
        let stats = data.weapon(self.primary);
        let fits = |id: Option<AttachmentId>, slot: AttachmentSlot| {
            id.filter(|id| data.attachment(*id).slot == slot && stats.accepts(slot))
        };
        self.attachments = Attachments {
            optic: fits(self.attachments.optic, AttachmentSlot::Optic),
            muzzle: fits(self.attachments.muzzle, AttachmentSlot::Muzzle),
        };
        for (index, perk) in self.perks.iter_mut().enumerate() {
            if data.perk(*perk).slot != index + 1 {
                *perk = data
                    .perks_in_slot(index + 1)
                    .next()
                    .expect("checked at startup");
            }
        }
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
    /// Whether a Weapon has a place for an Attachment in `slot`: Secondaries have none, and a
    /// built-in scope leaves no room for an Optic.
    pub fn accepts(&self, slot: AttachmentSlot) -> bool {
        match slot {
            AttachmentSlot::Optic => self.class == WeaponClass::Primary && !self.scope,
            AttachmentSlot::Muzzle => self.class == WeaponClass::Primary,
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

    /// The volume and playback speed multipliers the Attachments fitted put on a gunshot.
    pub fn shot_sound(&self, attachments: Attachments) -> (f32, f32) {
        attachments.iter().fold((1.0, 1.0), |(volume, pitch), id| {
            let attachment = self.attachment(id);
            (
                volume * attachment.shot_volume,
                pitch * attachment.shot_pitch,
            )
        })
    }

    /// The Attachments fitted at once, combined.
    pub fn attachment_mods(&self, attachments: Attachments) -> StatMods {
        attachments.iter().fold(StatMods::default(), |mods, id| {
            mods.stack(self.attachment(id).mods)
        })
    }

    /// What `perks` do together.
    pub fn perk_effects(&self, perks: &[PerkId]) -> PerkEffects {
        perks.iter().fold(PerkEffects::default(), |effects, id| {
            effects.combine(self.perk(*id).effects)
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
        if let Some(muzzle) = attachments
            .muzzle
            .filter(|_| stats.accepts(AttachmentSlot::Muzzle))
            .map(|id| self.attachment(id))
        {
            stats.silenced = muzzle.silenced;
            stats.shot_volume = muzzle.shot_volume;
            stats.shot_pitch = muzzle.shot_pitch;
            stats.muzzle_length = muzzle.muzzle_length;
            stats.muzzle_offset.2 -= muzzle.muzzle_length;
        }
        stats.reload_secs *= perks.reload_time_mul;
        stats.empty_reload_secs *= perks.reload_time_mul;
        stats.move_scale *= perks.move_speed_mul;
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> GameData {
        GameData::load()
    }

    #[test]
    fn silencer_shortens_falloff_and_quiets_the_shot() {
        let data = data();
        let base = data.weapon(WeaponId::Smg);
        let silenced = data.effective_weapon(
            WeaponId::Smg,
            Attachments {
                optic: None,
                muzzle: Some(AttachmentId::Silencer),
            },
            &PerkEffects::default(),
        );
        assert!((silenced.falloff_end - base.falloff_end * 0.75).abs() < 1e-4);
        assert!((silenced.falloff_start - base.falloff_start * 0.75).abs() < 1e-4);
        assert!(silenced.silenced && silenced.shot_volume < 1.0 && silenced.shot_pitch < 1.0);
        assert!(!base.silenced);
    }

    #[test]
    fn optics_set_magnification_sway_and_the_field_of_view() {
        let data = data();
        let rifle = |optic: Option<AttachmentId>| {
            data.effective_weapon(
                WeaponId::AssaultRifle,
                Attachments {
                    optic,
                    muzzle: None,
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
            muzzle: None,
        };
        let sniper = data.effective_weapon(WeaponId::SniperRifle, red_dot, &PerkEffects::default());
        assert_eq!(sniper.sight, SightKind::Scope);
        assert_eq!((sniper.magnification, sniper.sway_scale), (8.0, 1.3));
        assert!(sniper.steady_sight());
    }

    #[test]
    fn perks_stack_and_change_reload_and_speed() {
        let data = data();
        let effects = data.perk_effects(&[
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

        let light = data.perk_effects(&[PerkId::Lightweight, PerkId::Ninja, PerkId::Scavenger]);
        assert!((light.move_speed_mul - 1.1).abs() < 1e-6);
        assert!(light.silent_footsteps && light.scavenger);
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
            },
            secondary: WeaponId::Smg,
            // Slot 1 holds a Slot 2 Perk and vice versa.
            perks: [PerkId::Ninja, PerkId::SleightOfHand, PerkId::Marathon],
            camo: String::new(),
        }
        .validated(&data);
        assert_eq!(data.weapon(bad.primary).class, WeaponClass::Primary);
        assert_eq!(data.weapon(bad.secondary).class, WeaponClass::Secondary);
        assert_eq!(bad.attachments, Attachments::default());
        assert_eq!(
            bad.perks,
            [
                PerkId::SleightOfHand,
                PerkId::StoppingPower,
                PerkId::Marathon
            ]
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
            },
            secondary: WeaponId::Pistol,
            perks: [
                PerkId::SleightOfHand,
                PerkId::StoppingPower,
                PerkId::Marathon,
            ],
            camo: NO_CAMO.into(),
        }
        .validated(&data);
        assert_eq!(loadout.attachments.optic, None);
        assert_eq!(loadout.attachments.muzzle, Some(AttachmentId::Silencer));
    }

    #[test]
    fn default_loadouts_are_valid_as_shipped() {
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
}
