//! The rules side of Create a Class, with no UI: what each slot of a class can hold
//! (`choices`), what each pick costs, whether the Level, Weapon Level or a camo Challenge has
//! unlocked it, and whether the class would still obey `class.rs` afterwards. Navigation
//! helpers (grid steps, paging) live here too so they can be tested.

use crate::class::{ClassError, EquipmentId, EquipmentSlot, WildcardId};
use crate::data::{GameData, WeaponClass, WeaponId};
use crate::gear::{AttachmentId, AttachmentSlot, Loadout, NO_CAMO, PerkId};
use crate::loadout::stats;
use crate::profile::Profile;
use crate::progression::UnlockItem;

/// Rows a picker shows at once.
pub(super) const PAGE_SIZE: usize = 7;

/// One place of a class that has a picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    Primary,
    Secondary,
    /// The n-th Attachment on the Primary, counting from 0 (fitted ones in Slot order).
    Attachment(usize),
    Camo,
    /// Perk Slot, counting from 0.
    Perk(usize),
    /// The second Perk of a Perk Slot (its Perk Greed Wildcard), counting from 0.
    Greed(usize),
    Equipment(EquipmentSlot),
    Wildcards,
}

impl Slot {
    pub(super) fn label(self) -> String {
        match self {
            Slot::Primary => "PRIMARY".into(),
            Slot::Secondary => "SECONDARY".into(),
            Slot::Attachment(index) => format!("ATTACHMENT {}", index + 1),
            Slot::Camo => "CAMO".into(),
            Slot::Perk(slot) => format!("PERK {}", slot + 1),
            Slot::Greed(slot) => format!("PERK {} GREED", slot + 1),
            Slot::Equipment(EquipmentSlot::Lethal) => "LETHAL".into(),
            Slot::Equipment(EquipmentSlot::Tactical) => "TACTICAL".into(),
            Slot::Wildcards => "WILDCARDS".into(),
        }
    }

    /// Whether this is picked from the Primary's own screen (attachments and camo).
    pub(super) fn on_gun(self) -> bool {
        matches!(self, Slot::Attachment(_) | Slot::Camo)
    }
}

/// What a slot can be set to.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Pick {
    /// Nothing in the slot (or no camo).
    Empty,
    Weapon(WeaponId),
    Attachment(AttachmentId),
    Camo(String),
    Perk(PerkId),
    Equipment(EquipmentId),
    Wildcard(WildcardId),
}

/// One row of a picker, with everything the list and the preview show about it.
#[derive(Clone, Debug)]
pub(super) struct Choice {
    pub pick: Pick,
    pub name: String,
    pub info: String,
    /// Class points the pick itself costs.
    pub cost: u32,
    /// What the slot holds now.
    pub current: bool,
    /// What the Level, Weapon Level or Challenge still has to unlock (`Unlocks at Level 8`).
    pub lock: Option<String>,
    /// Why the class would break a rule with this pick (first new reason).
    pub refusal: Option<String>,
    /// The refusal is the points budget.
    pub over_budget: bool,
    /// The class with the pick made.
    pub after: Loadout,
}

impl Choice {
    pub(super) fn usable(&self) -> bool {
        self.lock.is_none() && self.refusal.is_none()
    }
}

/// The class with `pick` made in `slot`.
pub(super) fn apply(loadout: &Loadout, slot: Slot, pick: &Pick, data: &GameData) -> Loadout {
    let mut next = loadout.clone();
    match (slot, pick) {
        (Slot::Primary, Pick::Weapon(id)) => {
            next.primary = *id;
            // A Primary that doesn't offer the old Attachments drops them.
            next = next.validated(data);
        }
        (Slot::Secondary, Pick::Weapon(id)) => next.secondary = Some(*id),
        (Slot::Secondary, Pick::Empty) => next.secondary = None,
        (Slot::Attachment(index), pick) => {
            if let Some(old) = loadout.attachments.iter().nth(index) {
                next.attachments.set(data.attachment(old).slot, None);
            }
            if let Pick::Attachment(id) = pick {
                next.attachments.set(data.attachment(*id).slot, Some(*id));
            }
        }
        (Slot::Camo, Pick::Camo(id)) => next.camo = id.clone(),
        (Slot::Camo, Pick::Empty) => next.camo = NO_CAMO.into(),
        (Slot::Perk(slot), Pick::Perk(id)) => next.perks.picks[slot] = Some(*id),
        (Slot::Perk(slot), Pick::Empty) => next.perks.picks[slot] = None,
        (Slot::Greed(slot), Pick::Perk(id)) => next.perks.greed[slot] = Some(*id),
        (Slot::Greed(slot), Pick::Empty) => next.perks.greed[slot] = None,
        (Slot::Equipment(slot), Pick::Equipment(id)) => next.set_equipment(slot, Some(*id)),
        (Slot::Equipment(slot), Pick::Empty) => next.set_equipment(slot, None),
        (Slot::Wildcards, Pick::Wildcard(id)) => {
            if let Some(at) = next.wildcards.iter().position(|held| held == id) {
                next.wildcards.remove(at);
            } else {
                next.wildcards.push(*id);
            }
        }
        _ => {}
    }
    next
}

/// What `slot` holds in `loadout`, as a pick.
fn current(loadout: &Loadout, slot: Slot) -> Pick {
    let some = |pick: Option<Pick>| pick.unwrap_or(Pick::Empty);
    match slot {
        Slot::Primary => Pick::Weapon(loadout.primary),
        Slot::Secondary => some(loadout.secondary.map(Pick::Weapon)),
        Slot::Attachment(index) => {
            some(loadout.attachments.iter().nth(index).map(Pick::Attachment))
        }
        Slot::Camo if loadout.camo == NO_CAMO => Pick::Empty,
        Slot::Camo => Pick::Camo(loadout.camo.clone()),
        Slot::Perk(slot) => some(loadout.perks.picks[slot].map(Pick::Perk)),
        Slot::Greed(slot) => some(loadout.perks.greed[slot].map(Pick::Perk)),
        Slot::Equipment(slot) => some(loadout.equipment(slot).map(Pick::Equipment)),
        Slot::Wildcards => Pick::Empty,
    }
}

/// Whether `slot` has `pick` in `loadout`; a Wildcard counts when the class carries it.
fn holds(loadout: &Loadout, slot: Slot, pick: &Pick) -> bool {
    match pick {
        Pick::Wildcard(id) => loadout.has(*id),
        _ => current(loadout, slot) == *pick,
    }
}

/// A short line about a Weapon for its preview: `Full auto, 30 rounds`.
pub(super) fn weapon_info(id: WeaponId, data: &GameData) -> String {
    let stats = data.weapon(id);
    format!(
        "{}, {} rounds",
        stats::fire_mode_text(stats),
        stats.magazine
    )
}

/// `Unlocks at Level N` while the account Level hasn't unlocked `item`.
fn level_lock(item: UnlockItem, profile: &Profile, data: &GameData) -> Option<String> {
    let progression = &data.progression;
    (!progression.is_unlocked(item, profile.level(data)))
        .then(|| format!("Unlocks at Level {}", progression.unlock_level(item)))
}

/// Everything wrong with `loadout` for this Profile (rules and unlocks), one text each.
pub(super) fn problems(loadout: &Loadout, profile: &Profile, data: &GameData) -> Vec<String> {
    data.class_errors(loadout, Some(profile.standing(loadout, data)))
        .iter()
        .map(|error| error.describe(data))
        .collect()
}

/// What the Primary's Weapon Level has reached, the progress to the next and what it opens.
pub(super) fn weapon_level_text(loadout: &Loadout, profile: &Profile, data: &GameData) -> String {
    let levels = &data.progression.weapon_levels;
    let progress = levels.progress(profile.record(loadout.primary).xp);
    let next = if progress.maxed() {
        "MAX".to_owned()
    } else {
        let opens = data
            .progression
            .attachments_unlocked_at(loadout.primary, progress.level + 1);
        let opens = if opens.is_empty() {
            String::new()
        } else {
            let names: Vec<&str> = opens
                .iter()
                .map(|id| data.attachment(*id).name.as_str())
                .collect();
            format!("  -  NEXT: {}", names.join(", ").to_uppercase())
        };
        format!("{} / {} XP{opens}", progress.into_level, progress.span)
    };
    format!("WEAPON LEVEL {}  -  {next}", progress.level)
}

/// One candidate of a picker before the rules are applied to it.
struct Entry {
    pick: Pick,
    name: String,
    info: String,
    cost: u32,
}

impl Entry {
    fn empty(info: &str) -> Entry {
        Entry {
            pick: Pick::Empty,
            name: "None".into(),
            info: info.into(),
            cost: 0,
        }
    }
}

/// Everything `slot` could hold, in list order, each with its cost, lock and what the class
/// would look like (and whether it would still be valid) after picking it.
pub(super) fn choices(
    loadout: &Loadout,
    slot: Slot,
    profile: &Profile,
    data: &GameData,
) -> Vec<Choice> {
    let costs = &data.class_points.costs;
    let weapon = |id: WeaponId, cost: u32| Entry {
        pick: Pick::Weapon(id),
        name: data.weapon(id).name.clone(),
        info: weapon_info(id, data),
        cost,
    };
    let weapons_of = |class: WeaponClass| {
        WeaponId::ALL
            .into_iter()
            .filter(move |id| data.weapon(*id).class == class)
    };
    let perk = |id: PerkId| Entry {
        pick: Pick::Perk(id),
        name: data.perk(id).name.clone(),
        info: data.perk(id).description.clone(),
        cost: costs.perk,
    };
    let entries: Vec<Entry> = match slot {
        Slot::Primary => weapons_of(WeaponClass::Primary)
            .map(|id| weapon(id, costs.primary))
            .collect(),
        Slot::Secondary => std::iter::once(Entry::empty("No second weapon: one point free."))
            .chain(weapons_of(WeaponClass::Secondary).map(|id| weapon(id, costs.secondary)))
            // Primaries are only on offer with Overkill.
            .chain(
                weapons_of(WeaponClass::Primary)
                    .filter(|_| loadout.has(WildcardId::Overkill))
                    .map(|id| weapon(id, costs.secondary)),
            )
            .collect(),
        Slot::Attachment(index) => {
            let taken: Vec<AttachmentSlot> = loadout
                .attachments
                .iter()
                .enumerate()
                .filter(|(at, _)| *at != index)
                .map(|(_, id)| data.attachment(id).slot)
                .collect();
            std::iter::once(Entry::empty("Nothing fitted: frees the points."))
                .chain(
                    AttachmentSlot::ALL
                        .into_iter()
                        .filter(|slot| {
                            data.weapon(loadout.primary).accepts(*slot) && !taken.contains(slot)
                        })
                        .flat_map(|slot| data.weapon_attachments(loadout.primary, slot))
                        .map(|id| Entry {
                            pick: Pick::Attachment(id),
                            name: data.attachment(id).name.clone(),
                            info: data.attachment(id).description.clone(),
                            cost: data.attachment_cost(id),
                        }),
                )
                .collect()
        }
        Slot::Camo => {
            let record = profile.record(loadout.primary);
            std::iter::once(Entry::empty("The bare weapon."))
                .chain(data.progression.camos.iter().map(|camo| Entry {
                    pick: Pick::Camo(camo.id.clone()),
                    name: camo.name.clone(),
                    info: format!(
                        "Challenge: {}",
                        data.progression.camo_progress(camo, &record).text()
                    ),
                    cost: 0,
                }))
                .collect()
        }
        Slot::Perk(index) | Slot::Greed(index) => {
            let other = match slot {
                Slot::Perk(_) => loadout.perks.greed[index],
                _ => loadout.perks.picks[index],
            };
            std::iter::once(Entry::empty("No Perk: frees the point."))
                .chain(
                    data.perks_in_slot(index + 1)
                        .filter(|id| Some(*id) != other)
                        .map(perk),
                )
                .collect()
        }
        Slot::Equipment(equipment_slot) => {
            std::iter::once(Entry::empty("Nothing: frees the point."))
                .chain(
                    EquipmentId::ALL
                        .into_iter()
                        .filter(|id| data.class_points.equipment[id].slot == equipment_slot)
                        .map(|id| {
                            let info = &data.class_points.equipment[&id];
                            Entry {
                                pick: Pick::Equipment(id),
                                name: info.name.clone(),
                                info: info.description.clone(),
                                cost: data.equipment_cost(id),
                            }
                        }),
                )
                .collect()
        }
        Slot::Wildcards => WildcardId::ALL
            .into_iter()
            .map(|id| Entry {
                pick: Pick::Wildcard(id),
                name: data.wildcard_name(id).to_owned(),
                info: data.class_points.wildcards[&id].description.clone(),
                cost: costs.wildcard,
            })
            .collect(),
    };
    let had = data.class_errors(loadout, None);
    entries
        .into_iter()
        .map(|entry| {
            let after = apply(loadout, slot, &entry.pick, data);
            // A class that already breaks a rule isn't blamed for it again.
            let new: Vec<ClassError> = data
                .class_errors(&after, None)
                .into_iter()
                .filter(|error| !had.contains(error))
                .collect();
            Choice {
                current: holds(loadout, slot, &entry.pick),
                over_budget: new
                    .iter()
                    .any(|error| matches!(error, ClassError::OverBudget { .. })),
                refusal: new.first().map(|error| error.describe(data)),
                name: entry.name,
                info: entry.info,
                cost: entry.cost,
                lock: pick_lock(loadout, &entry.pick, profile, data),
                pick: entry.pick,
                after,
            }
        })
        .collect()
}

/// Points `slot` costs in `loadout` right now (a Wildcard tile: all of them).
pub(super) fn slot_cost(loadout: &Loadout, slot: Slot, data: &GameData) -> u32 {
    let costs = &data.class_points.costs;
    match slot {
        Slot::Primary => costs.primary,
        Slot::Secondary => loadout.secondary.map_or(0, |_| costs.secondary),
        Slot::Attachment(index) => loadout
            .attachments
            .iter()
            .nth(index)
            .map_or(0, |id| data.attachment_cost(id)),
        Slot::Camo => 0,
        Slot::Perk(slot) => loadout.perks.picks[slot].map_or(0, |_| costs.perk),
        Slot::Greed(slot) => loadout.perks.greed[slot].map_or(0, |_| costs.perk),
        Slot::Equipment(slot) => loadout
            .equipment(slot)
            .map_or(0, |id| data.equipment_cost(id)),
        Slot::Wildcards => loadout.wildcards.len() as u32 * costs.wildcard,
    }
}

/// What still locks the current pick of `slot` (the account Level, Weapon Level or Challenge).
pub(super) fn slot_lock(
    loadout: &Loadout,
    slot: Slot,
    profile: &Profile,
    data: &GameData,
) -> Option<String> {
    pick_lock(loadout, &current(loadout, slot), profile, data)
}

/// What still locks `pick`: the account Level for a Weapon, Perk, equipment or Wildcard, the
/// Primary's Weapon Level for an Attachment, the Challenge for a camo.
pub(super) fn pick_lock(
    loadout: &Loadout,
    pick: &Pick,
    profile: &Profile,
    data: &GameData,
) -> Option<String> {
    match pick {
        Pick::Empty => None,
        Pick::Weapon(id) => level_lock(UnlockItem::Weapon(*id), profile, data),
        Pick::Attachment(id) => data
            .attachment_lock(loadout.primary, *id, &profile.record(loadout.primary))
            .map(|need| format!("Unlocks at {need}")),
        Pick::Camo(id) => {
            let camo = data.progression.camo(id)?;
            let progress = data
                .progression
                .camo_progress(camo, &profile.record(loadout.primary));
            (!progress.done()).then(|| format!("Locked: {}", progress.text()))
        }
        Pick::Perk(id) => level_lock(UnlockItem::Perk(*id), profile, data),
        Pick::Equipment(id) => level_lock(UnlockItem::Equipment(*id), profile, data),
        Pick::Wildcard(id) => level_lock(UnlockItem::Wildcard(*id), profile, data),
    }
}

/// Number of tiles on the Primary's screen taking an Attachment: what the class may carry
/// (three with Gunfighter), or more if it somehow holds more.
pub(super) fn attachment_tiles(loadout: &Loadout, data: &GameData) -> usize {
    data.max_attachments(loadout)
        .max(loadout.attachments.count())
}

/// A direction of the arrow keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// A selectable thing on a grid screen: its key and its place (rows grow downwards; a tile's
/// extra part sits in between rows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Spot {
    pub key: usize,
    pub x: f32,
    pub y: f32,
}

/// The spot an arrow key from `from` moves to: the nearest in that direction, preferring the
/// same row (or column). Stays put when there is none.
pub(super) fn neighbour(spots: &[Spot], from: usize, dir: Dir) -> usize {
    let Some(here) = spots.iter().find(|spot| spot.key == from) else {
        return spots.first().map_or(from, |spot| spot.key);
    };
    spots
        .iter()
        .filter_map(|spot| {
            let (dx, dy) = (spot.x - here.x, spot.y - here.y);
            let (along, across) = match dir {
                Dir::Up => (-dy, dx.abs()),
                Dir::Down => (dy, dx.abs()),
                Dir::Left => (-dx, dy.abs()),
                Dir::Right => (dx, dy.abs()),
            };
            (along > 0.01).then_some((spot.key, along + 4.0 * across))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(from, |(key, _)| key)
}

/// The picker highlight after `delta` rows (clamped to the list).
pub(super) fn step(cursor: usize, len: usize, delta: isize) -> usize {
    cursor
        .saturating_add_signed(delta)
        .min(len.saturating_sub(1))
}

/// The page of a picker `cursor` is on.
pub(super) fn page_of(cursor: usize) -> usize {
    cursor / PAGE_SIZE
}

/// How many pages `len` rows take.
pub(super) fn pages(len: usize) -> usize {
    len.div_ceil(PAGE_SIZE).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::{Attachments, Perks};

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

    /// A Profile that has unlocked everything.
    fn veteran(data: &GameData) -> Profile {
        let mut profile = Profile::fresh(data, 0);
        profile.xp = data.progression.levels.xp_to_reach(30);
        for id in WeaponId::ALL {
            profile.weapons.insert(
                id,
                crate::progression::WeaponRecord {
                    kills: 1000,
                    headshots: 1000,
                    xp: 1_000_000,
                },
            );
        }
        profile
    }

    fn named<'a>(choices: &'a [Choice], name: &str) -> &'a Choice {
        choices
            .iter()
            .find(|choice| choice.name == name)
            .unwrap_or_else(|| panic!("no choice {name}"))
    }

    /// Takes the pick named `name` in `slot`, which must be allowed.
    fn take(loadout: &mut Loadout, slot: Slot, name: &str, profile: &Profile, data: &GameData) {
        let all = choices(loadout, slot, profile, data);
        let choice = named(&all, name);
        assert!(
            choice.usable(),
            "{name}: {:?} {:?}",
            choice.lock,
            choice.refusal
        );
        *loadout = choice.after.clone();
    }

    #[test]
    fn the_attachment_picker_lists_what_the_weapon_offers_and_fills_free_slots_only() {
        let data = data();
        let profile = veteran(&data);
        let mut class = bare(WeaponId::AssaultRifle);
        take(&mut class, Slot::Attachment(0), "Red Dot", &profile, &data);
        assert_eq!(class.attachments.optic, Some(AttachmentId::RedDot));
        take(&mut class, Slot::Attachment(1), "Silencer", &profile, &data);
        assert_eq!(class.attachments.muzzle, Some(AttachmentId::Silencer));
        // Tile 2 can't offer a second Optic, and tile 1 can swap to another one.
        let second = choices(&class, Slot::Attachment(1), &profile, &data);
        assert!(second.iter().all(|choice| choice.name != "ACOG"));
        assert!(
            second
                .iter()
                .any(|choice| choice.name == "Silencer" && choice.current)
        );
        take(
            &mut class,
            Slot::Attachment(0),
            "Holographic",
            &profile,
            &data,
        );
        assert_eq!(class.attachments.optic, Some(AttachmentId::Holographic));
        // None clears the tile's own attachment, nothing else.
        take(&mut class, Slot::Attachment(0), "None", &profile, &data);
        assert_eq!(class.attachments.optic, None);
        assert_eq!(class.attachments.muzzle, Some(AttachmentId::Silencer));
    }

    #[test]
    fn a_third_attachment_needs_gunfighter_and_a_heavy_optic_costs_two() {
        let data = data();
        let profile = veteran(&data);
        let mut class = bare(WeaponId::AssaultRifle);
        take(&mut class, Slot::Attachment(0), "Red Dot", &profile, &data);
        take(&mut class, Slot::Attachment(1), "Silencer", &profile, &data);
        let third = choices(&class, Slot::Attachment(2), &profile, &data);
        let foregrip = named(&third, "Foregrip");
        assert!(
            foregrip
                .refusal
                .as_deref()
                .is_some_and(|text| text.contains("Gunfighter")),
            "{:?}",
            foregrip.refusal
        );
        take(&mut class, Slot::Wildcards, "Gunfighter", &profile, &data);
        assert_eq!(attachment_tiles(&class, &data), 3);
        take(&mut class, Slot::Attachment(2), "Foregrip", &profile, &data);
        assert_eq!(data.class_cost(&class), 5);
        assert_eq!(
            named(
                &choices(
                    &bare(WeaponId::AssaultRifle),
                    Slot::Attachment(0),
                    &profile,
                    &data
                ),
                "ACOG"
            )
            .cost,
            2
        );
    }

    #[test]
    fn what_does_not_fit_the_budget_is_marked_and_what_is_locked_says_where_it_unlocks() {
        let data = data();
        let mut class = bare(WeaponId::AssaultRifle);
        class.secondary = Some(WeaponId::Pistol);
        class.perks.picks = [
            Some(PerkId::SleightOfHand),
            Some(PerkId::StoppingPower),
            Some(PerkId::Marathon),
        ];
        class.attachments.muzzle = Some(AttachmentId::Silencer);
        class.wildcards = vec![WildcardId::Gunfighter, WildcardId::PerkGreed1];
        class.perks.greed[0] = Some(PerkId::Scavenger);
        // Primary 1 + Secondary 1 + 3 Perks + Silencer + 2 Wildcards + Greed Perk = 9.
        assert_eq!(data.class_cost(&class), 9);
        let veteran = veteran(&data);
        let optics = choices(&class, Slot::Attachment(1), &veteran, &data);
        let acog = named(&optics, "ACOG");
        assert!(acog.over_budget);
        assert_eq!(acog.refusal.as_deref(), Some("Not enough points: 11 / 10"));
        assert!(named(&optics, "Red Dot").usable());

        let fresh = Profile::fresh(&data, 0);
        let wildcards = choices(
            &bare(WeaponId::AssaultRifle),
            Slot::Wildcards,
            &fresh,
            &data,
        );
        assert_eq!(
            named(&wildcards, "Overkill").lock.as_deref(),
            Some("Unlocks at Level 12")
        );
        let optics = choices(
            &bare(WeaponId::AssaultRifle),
            Slot::Attachment(0),
            &fresh,
            &data,
        );
        assert_eq!(
            named(&optics, "Holographic").lock.as_deref(),
            Some("Unlocks at M4 level 3")
        );
        // Even the first Attachment waits for the Weapon's first level (5 kills).
        assert_eq!(
            named(&optics, "Red Dot").lock.as_deref(),
            Some("Unlocks at M4 level 1")
        );
        assert!(named(&optics, "None").usable());
    }

    #[test]
    fn the_secondary_picker_offers_primaries_only_with_overkill() {
        let data = data();
        let profile = veteran(&data);
        let mut class = bare(WeaponId::AssaultRifle);
        let names = |class: &Loadout| -> Vec<String> {
            choices(class, Slot::Secondary, &profile, &data)
                .into_iter()
                .map(|choice| choice.name)
                .collect()
        };
        assert_eq!(names(&class), ["None", "M9 (Pistol)"]);
        take(&mut class, Slot::Wildcards, "Overkill", &profile, &data);
        assert_eq!(
            names(&class).len(),
            6,
            "None, the pistol and the four primaries"
        );
        take(&mut class, Slot::Secondary, "M40 (Sniper)", &profile, &data);
        // Overkill can't be taken out from under a Primary in the second slot.
        let wildcards = choices(&class, Slot::Wildcards, &profile, &data);
        assert!(
            named(&wildcards, "Overkill")
                .refusal
                .as_deref()
                .is_some_and(|text| text.contains("Overkill"))
        );
    }

    #[test]
    fn a_new_primary_drops_attachments_it_cannot_take() {
        let data = data();
        let profile = veteran(&data);
        let mut class = bare(WeaponId::Smg);
        class.attachments.optic = Some(AttachmentId::RedDot);
        take(&mut class, Slot::Primary, "M40 (Sniper)", &profile, &data);
        assert_eq!(class.primary, WeaponId::SniperRifle);
        assert_eq!(
            class.attachments.optic, None,
            "built-in scope leaves no room"
        );
        // The sniper's Attachment picker offers no Optic.
        let offered = choices(&class, Slot::Attachment(0), &profile, &data);
        assert!(offered.iter().all(|choice| choice.name != "Red Dot"));
    }

    #[test]
    fn perks_come_from_their_own_slot_and_greed_never_repeats_the_first_perk() {
        let data = data();
        let profile = veteran(&data);
        let mut class = bare(WeaponId::AssaultRifle);
        take(
            &mut class,
            Slot::Perk(0),
            "Sleight of Hand",
            &profile,
            &data,
        );
        take(&mut class, Slot::Wildcards, "Perk 1 Greed", &profile, &data);
        let greed = choices(&class, Slot::Greed(0), &profile, &data);
        assert!(greed.iter().all(|choice| choice.name != "Sleight of Hand"));
        assert!(greed.iter().any(|choice| choice.name == "Scavenger"));
        take(&mut class, Slot::Greed(0), "Scavenger", &profile, &data);
        assert_eq!(data.class_cost(&class), 4);
        let first = choices(&class, Slot::Perk(0), &profile, &data);
        assert!(first.iter().all(|choice| choice.name != "Scavenger"));
        assert_eq!(slot_cost(&class, Slot::Greed(0), &data), 1);
        assert_eq!(slot_cost(&class, Slot::Wildcards, &data), 1);
    }

    #[test]
    fn camos_list_none_then_each_with_its_challenge() {
        let data = data();
        let fresh = Profile::fresh(&data, 0);
        let camos = choices(&bare(WeaponId::AssaultRifle), Slot::Camo, &fresh, &data);
        assert_eq!(camos.len(), data.progression.camos.len() + 1);
        assert!(camos[0].current && camos[0].usable());
        assert_eq!(camos[1].lock.as_deref(), Some("Locked: 0/25 kills"));
        let veteran = veteran(&data);
        assert!(
            choices(&bare(WeaponId::AssaultRifle), Slot::Camo, &veteran, &data)
                .iter()
                .all(Choice::usable)
        );
    }

    #[test]
    fn the_current_pick_is_flagged_and_costs_follow_the_class() {
        let data = data();
        let profile = veteran(&data);
        let mut class = bare(WeaponId::Smg);
        let primaries = choices(&class, Slot::Primary, &profile, &data);
        assert!(named(&primaries, "MP5 (SMG)").current);
        assert_eq!(slot_cost(&class, Slot::Secondary, &data), 0);
        take(&mut class, Slot::Secondary, "M9 (Pistol)", &profile, &data);
        assert_eq!(slot_cost(&class, Slot::Secondary, &data), 1);
        assert_eq!(
            slot_lock(&class, Slot::Primary, &Profile::fresh(&data, 0), &data),
            None
        );
    }

    #[test]
    fn arrows_move_between_tiles_and_the_greed_part_of_a_tile() {
        // 0 1 / 2 3 on rows 0 and 1, a Greed part under tile 2, a header above.
        let spots = [
            Spot {
                key: 9,
                x: 0.0,
                y: -1.0,
            },
            Spot {
                key: 0,
                x: 0.0,
                y: 0.0,
            },
            Spot {
                key: 1,
                x: 1.0,
                y: 0.0,
            },
            Spot {
                key: 2,
                x: 0.0,
                y: 1.0,
            },
            Spot {
                key: 3,
                x: 1.0,
                y: 1.0,
            },
            Spot {
                key: 4,
                x: 0.0,
                y: 1.5,
            },
        ];
        assert_eq!(neighbour(&spots, 0, Dir::Right), 1);
        assert_eq!(neighbour(&spots, 1, Dir::Right), 1);
        assert_eq!(neighbour(&spots, 0, Dir::Down), 2);
        assert_eq!(neighbour(&spots, 2, Dir::Down), 4);
        assert_eq!(neighbour(&spots, 4, Dir::Up), 2);
        assert_eq!(neighbour(&spots, 3, Dir::Up), 1);
        assert_eq!(neighbour(&spots, 4, Dir::Right), 3);
        assert_eq!(neighbour(&spots, 0, Dir::Up), 9);
        assert_eq!(neighbour(&spots, 9, Dir::Down), 0);
        assert_eq!(neighbour(&spots, 9, Dir::Up), 9);
    }

    #[test]
    fn long_lists_page_without_scrolling() {
        assert_eq!(pages(0), 1);
        assert_eq!(pages(PAGE_SIZE), 1);
        assert_eq!(pages(PAGE_SIZE + 1), 2);
        assert_eq!(page_of(PAGE_SIZE - 1), 0);
        assert_eq!(page_of(PAGE_SIZE), 1);
        assert_eq!(step(0, 10, -1), 0);
        assert_eq!(step(9, 10, 1), 9);
        assert_eq!(step(2, 10, PAGE_SIZE as isize), 9);
        assert_eq!(step(3, 0, 1), 0);
    }
}
