//! Class points ("Pick 10", CONTEXT.md: Class Points, Wildcard): every class (Loadout) has a
//! point budget and each choice costs points. This module holds the data of
//! `data/class_points.ron` (budget, costs, slot limits, Wildcards, equipment) and the pure rules:
//! what a class costs, which classes are valid (`GameData::class_errors`: budget, slot rules,
//! Wildcard rules, unlocks) and how to trim one that is not (`Loadout::fitted`).
//!
//! The numbers live in `data/class_points.ron`; which account Level unlocks a Wildcard is in
//! `data/progression.ron`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::data::{GameData, WeaponClass, WeaponId};
use crate::gear::{AttachmentId, AttachmentSlot, Loadout, PERK_SLOTS, PerkId};
use crate::progression::WeaponRecord;

/// A Wildcard: a class choice that breaks one slot rule (CONTEXT.md: Wildcard).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WildcardId {
    /// A second Perk in Perk Slot 1.
    PerkGreed1,
    PerkGreed2,
    PerkGreed3,
    /// A third Attachment on the Primary.
    Gunfighter,
    /// A Primary Weapon as the second Weapon.
    Overkill,
}

impl WildcardId {
    /// Every Wildcard, in the order the editor lists them.
    pub const ALL: [WildcardId; 5] = [
        WildcardId::PerkGreed1,
        WildcardId::PerkGreed2,
        WildcardId::PerkGreed3,
        WildcardId::Gunfighter,
        WildcardId::Overkill,
    ];

    /// The Perk Greed of Perk Slot `slot` (counting from 0).
    pub fn perk_greed(slot: usize) -> WildcardId {
        [
            WildcardId::PerkGreed1,
            WildcardId::PerkGreed2,
            WildcardId::PerkGreed3,
        ][slot]
    }
}

/// A piece of equipment (lethal or tactical): each has an `equipment` entry in
/// `class_points.ron` (name, slot, cost) and, as a thrown Grenade, its tuning in
/// `data/equipment.ron` (`equipment::GrenadeKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EquipmentId {
    Frag,
    Semtex,
    Mine,
    Flashbang,
    Stun,
    Smoke,
    TearGas,
}

impl EquipmentId {
    /// Every piece of equipment, in the order the editor lists them (lethal ones, then tactical).
    pub const ALL: [EquipmentId; 7] = [
        EquipmentId::Frag,
        EquipmentId::Semtex,
        EquipmentId::Mine,
        EquipmentId::Flashbang,
        EquipmentId::Stun,
        EquipmentId::Smoke,
        EquipmentId::TearGas,
    ];
}

/// The two equipment slots of a class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub enum EquipmentSlot {
    Lethal,
    Tactical,
}

impl EquipmentSlot {
    pub const ALL: [EquipmentSlot; 2] = [EquipmentSlot::Lethal, EquipmentSlot::Tactical];
}

/// What a point buys; the numbers of `class_points.ron`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct PointCosts {
    pub primary: u32,
    pub secondary: u32,
    /// An Attachment, unless its own `cost` says otherwise.
    pub attachment: u32,
    /// An Optic of at least this magnification costs `heavy_optic` instead.
    pub heavy_optic_magnification: f32,
    pub heavy_optic: u32,
    pub perk: u32,
    /// A piece of equipment, unless its own `cost` says otherwise.
    pub equipment: u32,
    pub wildcard: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WildcardInfo {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct EquipmentInfo {
    pub name: String,
    pub description: String,
    pub slot: EquipmentSlot,
    #[serde(default)]
    pub cost: Option<u32>,
}

/// `data/class_points.ron`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ClassPoints {
    pub budget: u32,
    pub costs: PointCosts,
    /// Attachments on the Primary; Gunfighter adds `gunfighter_extra_attachments`.
    pub max_attachments: usize,
    pub gunfighter_extra_attachments: usize,
    pub max_wildcards: usize,
    pub wildcards: HashMap<WildcardId, WildcardInfo>,
    pub equipment: HashMap<EquipmentId, EquipmentInfo>,
}

impl ClassPoints {
    /// Panics (stopping the game at startup) on a file that contradicts itself.
    pub fn check(&self, file: &str) {
        let costs = &self.costs;
        assert!(
            self.budget >= costs.primary,
            "{file}: the budget must at least pay for a Primary"
        );
        assert!(
            self.max_attachments >= 1 && self.max_wildcards >= 1,
            "{file}: max_attachments and max_wildcards must be at least 1"
        );
        assert!(
            costs.heavy_optic_magnification > 1.0,
            "{file}: heavy_optic_magnification must be above 1"
        );
        for id in WildcardId::ALL {
            assert!(
                self.wildcards.contains_key(&id),
                "{file} must define the Wildcard {id:?}"
            );
        }
        for id in EquipmentId::ALL {
            assert!(
                self.equipment.contains_key(&id),
                "{file} must define the equipment {id:?}"
            );
        }
    }
}

/// How far a player is, for the unlock part of `GameData::class_errors`: the account Level and
/// the Primary's Weapon XP and camo counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Standing {
    pub level: u32,
    pub primary: WeaponRecord,
}

/// One reason a class isn't valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassError {
    OverBudget {
        spent: u32,
        budget: u32,
    },
    /// The Primary slot holds a Weapon that isn't a Primary.
    NotAPrimary(WeaponId),
    /// A Primary Weapon in the second slot needs Overkill.
    SecondaryNeedsOverkill(WeaponId),
    /// The Primary doesn't offer this Attachment in this Slot.
    AttachmentNotOffered(AttachmentId, WeaponId),
    TooManyAttachments {
        count: usize,
        max: usize,
    },
    PerkInWrongSlot(PerkId),
    /// A second Perk in Perk Slot `n` (counting from 0) without its Perk Greed.
    GreedNeedsWildcard(usize),
    /// Equipment in the wrong slot or one that doesn't exist.
    BadEquipment(EquipmentSlot),
    TooManyWildcards {
        max: usize,
    },
    DuplicateWildcard(WildcardId),
    /// Something the player hasn't unlocked, as a ready text.
    Locked(String),
}

impl ClassError {
    /// A short player-facing sentence.
    pub fn describe(&self, data: &GameData) -> String {
        match self {
            ClassError::OverBudget { spent, budget } => {
                format!("Not enough points: {spent} / {budget}")
            }
            ClassError::NotAPrimary(id) => {
                format!("{} isn't a primary weapon", data.weapon(*id).name)
            }
            ClassError::SecondaryNeedsOverkill(id) => format!(
                "{} as a second weapon needs Overkill",
                data.weapon(*id).short_name()
            ),
            ClassError::AttachmentNotOffered(id, weapon) => format!(
                "The {} has no {}",
                data.weapon(*weapon).short_name(),
                data.attachment(*id).name
            ),
            ClassError::TooManyAttachments { max, .. } => {
                let gunfighter = data.wildcard_name(WildcardId::Gunfighter);
                if *max <= data.class_points.max_attachments {
                    format!("{max} attachments at most without {gunfighter}")
                } else {
                    format!("{max} attachments at most")
                }
            }
            ClassError::PerkInWrongSlot(id) => {
                format!("{} doesn't fit that Perk Slot", data.perk(*id).name)
            }
            ClassError::GreedNeedsWildcard(slot) => format!(
                "A second Perk in slot {} needs {}",
                slot + 1,
                data.wildcard_name(WildcardId::perk_greed(*slot))
            ),
            ClassError::BadEquipment(slot) => format!("That doesn't fit the {slot:?} slot"),
            ClassError::TooManyWildcards { max } => format!("{max} wildcards at most"),
            ClassError::DuplicateWildcard(id) => {
                format!("{} is already in the class", data.wildcard_name(*id))
            }
            ClassError::Locked(text) => text.clone(),
        }
    }
}

impl GameData {
    pub fn wildcard_name(&self, id: WildcardId) -> &str {
        &self.class_points.wildcards[&id].name
    }

    /// Class points `id` costs as an Attachment.
    pub fn attachment_cost(&self, id: AttachmentId) -> u32 {
        let attachment = self.attachment(id);
        let costs = &self.class_points.costs;
        attachment.cost.unwrap_or_else(|| {
            if attachment.slot == AttachmentSlot::Optic
                && attachment.magnification >= costs.heavy_optic_magnification
            {
                costs.heavy_optic
            } else {
                costs.attachment
            }
        })
    }

    /// Class points a piece of equipment costs.
    pub fn equipment_cost(&self, id: EquipmentId) -> u32 {
        self.class_points
            .equipment
            .get(&id)
            .and_then(|equipment| equipment.cost)
            .unwrap_or(self.class_points.costs.equipment)
    }

    /// Points the whole class costs.
    pub fn class_cost(&self, loadout: &Loadout) -> u32 {
        let costs = &self.class_points.costs;
        let attachments: u32 = loadout
            .attachments
            .iter()
            .map(|id| self.attachment_cost(id))
            .sum();
        let equipment: u32 = [loadout.lethal, loadout.tactical]
            .into_iter()
            .flatten()
            .map(|id| self.equipment_cost(id))
            .sum();
        costs.primary
            + loadout.secondary.map_or(0, |_| costs.secondary)
            + attachments
            + loadout.perks.count() as u32 * costs.perk
            + equipment
            + loadout.wildcards.len() as u32 * costs.wildcard
    }

    /// How many Attachments the Primary may carry: Gunfighter adds one.
    pub fn max_attachments(&self, loadout: &Loadout) -> usize {
        self.class_points.max_attachments
            + if loadout.has(WildcardId::Gunfighter) {
                self.class_points.gunfighter_extra_attachments
            } else {
                0
            }
    }

    /// Everything wrong with `loadout` as a Pick 10 class: slot rules, Wildcard rules, the
    /// budget and, when `standing` is given, what the player hasn't unlocked. Empty means the
    /// class is valid (and usable).
    pub fn class_errors(&self, loadout: &Loadout, standing: Option<Standing>) -> Vec<ClassError> {
        let mut errors = Vec::new();
        if self.weapon(loadout.primary).class != WeaponClass::Primary {
            errors.push(ClassError::NotAPrimary(loadout.primary));
        }
        if let Some(secondary) = loadout.secondary
            && self.weapon(secondary).class == WeaponClass::Primary
            && !loadout.has(WildcardId::Overkill)
        {
            errors.push(ClassError::SecondaryNeedsOverkill(secondary));
        }
        let stats = self.weapon(loadout.primary);
        for slot in AttachmentSlot::ALL {
            if let Some(id) = loadout.attachments.get(slot)
                && !(self.attachment(id).slot == slot
                    && stats.accepts(slot)
                    && self
                        .progression
                        .attachment_level(loadout.primary, id)
                        .is_some())
            {
                errors.push(ClassError::AttachmentNotOffered(id, loadout.primary));
            }
        }
        let max = self.max_attachments(loadout);
        let count = loadout.attachments.count();
        if count > max {
            errors.push(ClassError::TooManyAttachments { count, max });
        }
        for slot in 0..PERK_SLOTS {
            for perk in loadout.perks.in_slot(slot) {
                if self.perk(perk).slot != slot + 1 {
                    errors.push(ClassError::PerkInWrongSlot(perk));
                }
            }
            if loadout.perks.greed[slot].is_some() && !loadout.has(WildcardId::perk_greed(slot)) {
                errors.push(ClassError::GreedNeedsWildcard(slot));
            }
        }
        for slot in EquipmentSlot::ALL {
            if let Some(id) = loadout.equipment(slot)
                && self
                    .class_points
                    .equipment
                    .get(&id)
                    .is_none_or(|equipment| equipment.slot != slot)
            {
                errors.push(ClassError::BadEquipment(slot));
            }
        }
        let max = self.class_points.max_wildcards;
        if loadout.wildcards.len() > max {
            errors.push(ClassError::TooManyWildcards { max });
        }
        for (index, wildcard) in loadout.wildcards.iter().enumerate() {
            if loadout.wildcards[..index].contains(wildcard) {
                errors.push(ClassError::DuplicateWildcard(*wildcard));
            }
        }
        let spent = self.class_cost(loadout);
        let budget = self.class_points.budget;
        if spent > budget {
            errors.push(ClassError::OverBudget { spent, budget });
        }
        if let Some(standing) = standing {
            errors.extend(
                self.loadout_locks(loadout, standing.level, &standing.primary)
                    .into_iter()
                    .map(ClassError::Locked),
            );
        }
        errors
    }
}

/// A thing of a class that can be dropped, from the first the editor lists to the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Piece {
    Attachment(AttachmentSlot),
    Secondary,
    Perk(usize),
    Greed(usize),
    Equipment(EquipmentSlot),
    Wildcard(usize),
}

impl Loadout {
    /// The pieces the class holds, in drop order reversed (the last one is dropped first).
    /// The Primary isn't one: a class always has it.
    fn pieces(&self) -> Vec<Piece> {
        let attachments = AttachmentSlot::ALL
            .into_iter()
            .filter(|slot| self.attachments.get(*slot).is_some())
            .map(Piece::Attachment);
        let secondary = self.secondary.map(|_| Piece::Secondary);
        let perks = (0..PERK_SLOTS)
            .filter(|slot| self.perks.picks[*slot].is_some())
            .map(Piece::Perk);
        let greed = (0..PERK_SLOTS)
            .filter(|slot| self.perks.greed[*slot].is_some())
            .map(Piece::Greed);
        let equipment = EquipmentSlot::ALL
            .into_iter()
            .filter(|slot| self.equipment(*slot).is_some())
            .map(Piece::Equipment);
        let wildcards = (0..self.wildcards.len()).map(Piece::Wildcard);
        attachments
            .chain(secondary)
            .chain(perks)
            .chain(greed)
            .chain(equipment)
            .chain(wildcards)
            .collect()
    }

    /// Empties `piece`, naming what was in it.
    fn drop_piece(&mut self, piece: Piece, data: &GameData) -> String {
        match piece {
            Piece::Attachment(slot) => {
                let id = self.attachments.get(slot);
                self.attachments.set(slot, None);
                id.map_or_else(String::new, |id| data.attachment(id).name.clone())
            }
            Piece::Secondary => self
                .secondary
                .take()
                .map_or_else(String::new, |id| data.weapon(id).short_name().to_owned()),
            Piece::Perk(slot) => self.perks.picks[slot]
                .take()
                .map_or_else(String::new, |id| data.perk(id).name.clone()),
            Piece::Greed(slot) => self.perks.greed[slot]
                .take()
                .map_or_else(String::new, |id| data.perk(id).name.clone()),
            Piece::Equipment(slot) => {
                let id = self.equipment(slot);
                self.set_equipment(slot, None);
                id.map_or_else(String::new, |id| {
                    data.class_points.equipment[&id].name.clone()
                })
            }
            Piece::Wildcard(index) => data.wildcard_name(self.wildcards.remove(index)).to_owned(),
        }
    }

    /// The class made valid by dropping things (a profile written under other rules): first
    /// what breaks a slot or Wildcard rule, then, while over budget, things from the end (the
    /// Wildcards, equipment, Perks, the Secondary, the Attachments). The Primary always stays.
    /// Returns what was dropped; unlocks are not checked.
    pub fn fitted(self, data: &GameData) -> (Loadout, Vec<String>) {
        let mut class = self.validated(data);
        let mut dropped = Vec::new();
        loop {
            let errors = data.class_errors(&class, None);
            let pieces = class.pieces();
            let last = |keep: fn(&Piece) -> bool| pieces.iter().rev().find(|piece| keep(piece));
            let piece = match errors.first() {
                None => break,
                Some(ClassError::SecondaryNeedsOverkill(_)) => Some(&Piece::Secondary),
                Some(ClassError::GreedNeedsWildcard(slot)) => {
                    pieces.iter().find(|piece| **piece == Piece::Greed(*slot))
                }
                Some(ClassError::TooManyAttachments { .. }) => {
                    last(|piece| matches!(piece, Piece::Attachment(_)))
                }
                Some(ClassError::TooManyWildcards { .. } | ClassError::DuplicateWildcard(_)) => {
                    last(|piece| matches!(piece, Piece::Wildcard(_)))
                }
                Some(_) => pieces.last(),
            };
            let Some(piece) = piece.copied() else {
                break;
            };
            dropped.push(class.drop_piece(piece, data));
        }
        (class, dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::{Attachments, NO_CAMO, Perks};

    fn data() -> GameData {
        GameData::load()
    }

    /// A Primary and nothing else.
    fn bare(primary: WeaponId) -> Loadout {
        Loadout {
            name: "Test".into(),
            primary,
            attachments: Attachments::default(),
            secondary: None,
            perks: Perks::default(),
            lethal: None,
            tactical: None,
            wildcards: Vec::new(),
            camo: NO_CAMO.into(),
        }
    }

    fn errors(data: &GameData, class: &Loadout) -> Vec<ClassError> {
        data.class_errors(class, None)
    }

    /// Rifle + Red Dot, Silencer and Foregrip (Gunfighter), 3 Perks and a second Perk in
    /// slot 1 (Perk 1 Greed): 1 + 3 + 4 + 2 = exactly 10 points, no Secondary.
    fn ten_points() -> Loadout {
        Loadout {
            attachments: Attachments {
                optic: Some(AttachmentId::RedDot),
                muzzle: Some(AttachmentId::Silencer),
                underbarrel: Some(AttachmentId::Foregrip),
            },
            perks: Perks {
                picks: [
                    Some(PerkId::SleightOfHand),
                    Some(PerkId::StoppingPower),
                    Some(PerkId::Marathon),
                ],
                greed: [Some(PerkId::Scavenger), None, None],
            },
            wildcards: vec![WildcardId::PerkGreed1, WildcardId::Gunfighter],
            ..bare(WeaponId::AssaultRifle)
        }
    }

    #[test]
    fn an_empty_class_is_valid_and_costs_the_primary() {
        let data = data();
        let class = bare(WeaponId::Smg);
        assert_eq!(data.class_cost(&class), 1);
        assert!(errors(&data, &class).is_empty());
    }

    #[test]
    fn every_choice_costs_what_the_data_says() {
        let data = data();
        let mut class = bare(WeaponId::AssaultRifle);
        class.secondary = Some(WeaponId::Pistol);
        assert_eq!(data.class_cost(&class), 2, "Primary 1 + Secondary 1");
        class.attachments.muzzle = Some(AttachmentId::Silencer);
        assert_eq!(data.class_cost(&class), 3, "an Attachment is 1");
        class.attachments.optic = Some(AttachmentId::Prism);
        assert_eq!(data.class_cost(&class), 4, "a 2x Optic is still 1");
        class.attachments.optic = Some(AttachmentId::Acog);
        assert_eq!(data.class_cost(&class), 5, "a 4x Optic is 2");
        class.perks.picks[0] = Some(PerkId::Scavenger);
        assert_eq!(data.class_cost(&class), 6, "a Perk is 1");
        class.wildcards.push(WildcardId::Overkill);
        assert_eq!(data.class_cost(&class), 7, "a Wildcard is 1");
        assert_eq!(data.attachment_cost(AttachmentId::Holographic), 1);
        assert_eq!(data.attachment_cost(AttachmentId::Acog), 2);
    }

    #[test]
    fn the_grenade_launcher_costs_two_points_and_only_the_m16_offers_it() {
        let data = data();
        let mut class = bare(WeaponId::M16);
        class.attachments.underbarrel = Some(AttachmentId::GrenadeLauncher);
        assert_eq!(data.attachment_cost(AttachmentId::GrenadeLauncher), 2);
        assert_eq!(data.class_cost(&class), 3, "Primary 1 + launcher 2");
        assert!(errors(&data, &class).is_empty());
        for other in [WeaponId::AssaultRifle, WeaponId::Smg, WeaponId::SniperRifle] {
            class.primary = other;
            assert_eq!(
                errors(&data, &class),
                [ClassError::AttachmentNotOffered(
                    AttachmentId::GrenadeLauncher,
                    other
                )]
            );
        }
    }

    #[test]
    fn equipment_costs_a_point_and_belongs_in_its_own_slot() {
        let data = data();
        let mut class = bare(WeaponId::AssaultRifle);
        class.lethal = Some(EquipmentId::Frag);
        class.tactical = Some(EquipmentId::Smoke);
        assert_eq!(
            data.class_cost(&class),
            3,
            "Primary 1 + two pieces of equipment"
        );
        assert!(errors(&data, &class).is_empty());
        class.lethal = Some(EquipmentId::Flashbang);
        assert_eq!(
            errors(&data, &class),
            [ClassError::BadEquipment(EquipmentSlot::Lethal)],
            "a tactical item in the lethal slot"
        );
    }

    #[test]
    fn a_class_at_exactly_ten_points_with_greed_and_gunfighter_is_valid() {
        let data = data();
        let class = ten_points();
        assert_eq!(data.class_cost(&class), 10);
        assert!(
            errors(&data, &class).is_empty(),
            "{:?}",
            errors(&data, &class)
        );
    }

    #[test]
    fn an_eleventh_point_is_over_budget() {
        let data = data();
        let mut class = ten_points();
        class.secondary = Some(WeaponId::Pistol);
        assert_eq!(
            errors(&data, &class),
            [ClassError::OverBudget {
                spent: 11,
                budget: 10
            }]
        );
        // A heavier Optic over the line is refused the same way.
        let mut class = ten_points();
        class.attachments.optic = Some(AttachmentId::Acog);
        assert_eq!(data.class_cost(&class), 11);
        assert!(matches!(
            errors(&data, &class)[..],
            [ClassError::OverBudget { .. }]
        ));
    }

    #[test]
    fn a_third_attachment_needs_gunfighter() {
        let data = data();
        let mut class = ten_points();
        class.wildcards = vec![WildcardId::PerkGreed1];
        assert_eq!(
            errors(&data, &class),
            [ClassError::TooManyAttachments { count: 3, max: 2 }]
        );
        assert_eq!(data.max_attachments(&class), 2);
        class.wildcards.push(WildcardId::Gunfighter);
        assert_eq!(data.max_attachments(&class), 3);
        assert!(errors(&data, &class).is_empty());
    }

    #[test]
    fn a_second_perk_needs_that_slots_greed() {
        let data = data();
        let mut class = ten_points();
        class.wildcards = vec![WildcardId::Gunfighter];
        assert_eq!(errors(&data, &class), [ClassError::GreedNeedsWildcard(0)]);
        // Another slot's Greed doesn't do.
        class.wildcards = vec![WildcardId::Gunfighter, WildcardId::PerkGreed2];
        assert_eq!(errors(&data, &class), [ClassError::GreedNeedsWildcard(0)]);
        // A Perk from the wrong Perk Slot is wrong however it got there.
        let mut class = bare(WeaponId::Smg);
        class.perks.picks[1] = Some(PerkId::Ninja);
        assert_eq!(
            errors(&data, &class),
            [ClassError::PerkInWrongSlot(PerkId::Ninja)]
        );
    }

    #[test]
    fn a_primary_weapon_as_the_second_needs_overkill() {
        let data = data();
        let mut class = bare(WeaponId::AssaultRifle);
        class.secondary = Some(WeaponId::SniperRifle);
        assert_eq!(
            errors(&data, &class),
            [ClassError::SecondaryNeedsOverkill(WeaponId::SniperRifle)]
        );
        class.wildcards.push(WildcardId::Overkill);
        assert!(errors(&data, &class).is_empty());
        assert_eq!(data.class_cost(&class), 3, "Primary + Secondary + Overkill");
        // A Secondary-class Weapon never needs it, and a Primary can't sit in the first slot's
        // place of a Secondary.
        class.wildcards.clear();
        class.secondary = Some(WeaponId::Pistol);
        assert!(errors(&data, &class).is_empty());
        let class = bare(WeaponId::Pistol);
        assert_eq!(
            errors(&data, &class),
            [ClassError::NotAPrimary(WeaponId::Pistol)]
        );
    }

    #[test]
    fn wildcards_are_limited_and_not_repeated() {
        let data = data();
        let mut class = bare(WeaponId::AssaultRifle);
        class.wildcards = vec![
            WildcardId::Gunfighter,
            WildcardId::Overkill,
            WildcardId::PerkGreed1,
        ];
        assert!(errors(&data, &class).is_empty(), "three is the limit");
        class.wildcards.push(WildcardId::PerkGreed2);
        assert_eq!(
            errors(&data, &class),
            [ClassError::TooManyWildcards { max: 3 }]
        );
        class.wildcards = vec![WildcardId::Overkill, WildcardId::Overkill];
        assert_eq!(
            errors(&data, &class),
            [ClassError::DuplicateWildcard(WildcardId::Overkill)]
        );
    }

    #[test]
    fn attachments_must_be_offered_by_the_primary() {
        let data = data();
        // The sniper has no Optics.
        let mut class = bare(WeaponId::SniperRifle);
        class.attachments.optic = Some(AttachmentId::RedDot);
        assert_eq!(
            errors(&data, &class),
            [ClassError::AttachmentNotOffered(
                AttachmentId::RedDot,
                WeaponId::SniperRifle
            )]
        );
        // An Optic can't sit in the Muzzle slot either.
        let mut class = bare(WeaponId::AssaultRifle);
        class.attachments.muzzle = Some(AttachmentId::Acog);
        assert!(matches!(
            errors(&data, &class)[..],
            [ClassError::AttachmentNotOffered(AttachmentId::Acog, _)]
        ));
    }

    #[test]
    fn unlocks_are_part_of_validation_when_a_standing_is_given() {
        let data = data();
        let class = ten_points();
        let level_one = Standing {
            level: 1,
            primary: WeaponRecord::default(),
        };
        let locks = data.class_errors(&class, Some(level_one));
        assert!(
            locks
                .iter()
                .any(|error| matches!(error, ClassError::Locked(text)
                if text.contains("Gunfighter") && text.contains("Level 6"))),
            "{locks:?}"
        );
        assert!(
            locks
                .iter()
                .any(|error| matches!(error, ClassError::Locked(text)
                if text.contains("Foregrip") && text.contains("M4 level 4"))),
            "{locks:?}"
        );
        let veteran = Standing {
            level: 30,
            primary: WeaponRecord {
                xp: data.progression.weapon_levels.xp_to_reach(5),
                ..Default::default()
            },
        };
        assert!(data.class_errors(&class, Some(veteran)).is_empty());
    }

    #[test]
    fn the_shipped_default_classes_are_valid_pick_10_classes() {
        let data = data();
        for class in &data.default_loadouts {
            assert!(
                errors(&data, class).is_empty(),
                "{}: {:?}",
                class.name,
                errors(&data, class)
            );
            assert!(
                data.class_cost(class) <= data.class_points.budget,
                "{}",
                class.name
            );
        }
    }

    #[test]
    fn fitting_drops_what_breaks_the_rules_then_things_from_the_end() {
        let data = data();
        // A class from another set of rules: three Greeds and Gunfighter (4 Wildcards), four
        // Attachments' worth of points and a Primary in the second slot, 15+ points.
        let class = Loadout {
            attachments: Attachments {
                optic: Some(AttachmentId::Acog),
                muzzle: Some(AttachmentId::Silencer),
                underbarrel: Some(AttachmentId::Foregrip),
            },
            secondary: Some(WeaponId::Smg),
            perks: Perks {
                picks: [
                    Some(PerkId::SleightOfHand),
                    Some(PerkId::StoppingPower),
                    Some(PerkId::Marathon),
                ],
                greed: [
                    Some(PerkId::Scavenger),
                    Some(PerkId::Lightweight),
                    Some(PerkId::Ninja),
                ],
            },
            wildcards: vec![
                WildcardId::PerkGreed1,
                WildcardId::PerkGreed2,
                WildcardId::PerkGreed3,
                WildcardId::Gunfighter,
            ],
            ..bare(WeaponId::AssaultRifle)
        };
        assert!(data.class_cost(&class) > 10);
        let (fitted, dropped) = class.clone().fitted(&data);
        assert!(
            errors(&data, &fitted).is_empty(),
            "{:?}",
            errors(&data, &fitted)
        );
        assert!(data.class_cost(&fitted) <= 10);
        assert_eq!(fitted.primary, WeaponId::AssaultRifle);
        // The Wildcard over the limit and the Secondary that needed Overkill went first, then
        // points came off the end; the Primary's own Attachments are the last to go.
        assert!(dropped.contains(&"Gunfighter".to_owned()), "{dropped:?}");
        assert!(dropped.contains(&"MP5".to_owned()), "{dropped:?}");
        assert_eq!(fitted.secondary, None);
        assert!(fitted.attachments.optic.is_some() && fitted.attachments.muzzle.is_some());

        // A valid class passes through untouched.
        let (same, none) = ten_points().fitted(&data);
        assert_eq!(same, ten_points());
        assert!(none.is_empty());
    }

    #[test]
    fn a_legacy_style_class_loses_nothing_that_fits() {
        let data = data();
        let legacy = Loadout {
            attachments: Attachments {
                optic: Some(AttachmentId::Acog),
                muzzle: Some(AttachmentId::Silencer),
                underbarrel: None,
            },
            secondary: Some(WeaponId::Pistol),
            perks: Perks {
                picks: [
                    Some(PerkId::SleightOfHand),
                    Some(PerkId::StoppingPower),
                    Some(PerkId::Marathon),
                ],
                greed: [None; PERK_SLOTS],
            },
            ..bare(WeaponId::AssaultRifle)
        };
        let (fitted, dropped) = legacy.clone().fitted(&data);
        assert_eq!(fitted, legacy);
        assert!(dropped.is_empty());
        assert_eq!(data.class_cost(&fitted), 1 + 2 + 1 + 1 + 3);
    }
}
