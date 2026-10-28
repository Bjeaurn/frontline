//! The Profile (CONTEXT.md: Profile): the player's own data, kept in
//! `<config dir>/frontline/profile.ron` next to `settings.ron`. It travels with the player, not
//! with any Host, so it is a client-local file the server never sees.
//!
//! It holds the three custom Loadouts (classes), which Loadout is chosen, the XP the server
//! awarded (the Level is derived from it, `data/progression.ron`) and the per-Weapon counters:
//! kills and headshots for the camo Challenges, and the Weapon XP behind each Weapon Level. New
//! fields need `#[serde(default)]` so existing files keep loading. Client and Host only.
//!
//! Profiles written before Pick 10 stored their classes under `custom` in the old shape; they
//! are migrated on load: what fits is kept and, if a class is over the rules, things are dropped
//! from the end (and logged).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::data::{GameData, WeaponId};
use crate::flow::{Settings, load_config, save_config};
use crate::gear::{AttachmentId, Attachments, Loadout, PERK_SLOTS, PerkId, Perks};
use crate::net::protocol::XpAward;
use crate::progression::{LevelProgress, UnlockItem, WeaponRecord, XpReason};

const PROFILE_FILE: &str = "profile.ron";

/// Custom Loadouts a Profile holds (CONTEXT.md: a Profile holds three custom Loadouts).
pub const CUSTOM_SLOTS: usize = 3;

pub struct ProfilePlugin;

impl Plugin for ProfilePlugin {
    /// Loads the Profile now, from the data and Settings already inserted, so it exists before
    /// anything (the first spawn included) asks for it. Add after `DataPlugin` and `FlowPlugin`.
    fn build(&self, app: &mut App) {
        let profile = load_profile(
            app.world().resource::<GameData>(),
            app.world().resource::<Settings>(),
        );
        app.insert_resource(profile).add_systems(
            Update,
            save_profile
                .run_if(resource_changed::<Profile>)
                .run_if(not(resource_added::<Profile>)),
        );
    }
}

/// Which Loadout the player spawns with: a Default Loadout (an index into
/// `GameData::default_loadouts`) or one of the Profile's custom slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadoutChoice {
    Default(usize),
    Custom(usize),
}

/// A custom class as Profiles stored it before Pick 10: a Secondary always, one Perk per slot,
/// two Attachment slots. Only read, to migrate it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct LegacyLoadout {
    name: String,
    primary: WeaponId,
    #[serde(default)]
    attachments: Attachments,
    secondary: WeaponId,
    perks: [PerkId; PERK_SLOTS],
    #[serde(default)]
    camo: String,
}

impl From<LegacyLoadout> for Loadout {
    fn from(old: LegacyLoadout) -> Loadout {
        Loadout {
            name: old.name,
            primary: old.primary,
            attachments: old.attachments,
            secondary: Some(old.secondary),
            perks: Perks {
                picks: old.perks.map(Some),
                ..Perks::default()
            },
            lethal: None,
            tactical: None,
            wildcards: Vec::new(),
            camo: old.camo,
        }
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// The custom Loadouts, always `CUSTOM_SLOTS` of them.
    #[serde(rename = "classes", default)]
    pub custom: Vec<Loadout>,
    /// The custom Loadouts of an older Profile file, read once to migrate them into `custom`.
    #[serde(rename = "custom", default, skip_serializing)]
    legacy_custom: Vec<LegacyLoadout>,
    pub chosen: LoadoutChoice,
    /// Total XP earned; the Level is derived from it.
    #[serde(default)]
    pub xp: u32,
    /// Kills, headshot kills and Weapon XP per Weapon, for the camo Challenges and the Weapon
    /// Levels.
    #[serde(default)]
    pub weapons: HashMap<WeaponId, WeaponRecord>,
}

/// A Weapon reaching a new Weapon Level, and what that unlocked on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponLevelGain {
    pub weapon: WeaponId,
    pub level: u32,
    pub attachments: Vec<AttachmentId>,
}

/// What an XP award changed, for the HUD to announce.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AwardOutcome {
    pub level_before: u32,
    pub level_after: u32,
    /// Camos (by id) the award's kill unlocked, with the Weapon they are unlocked on.
    pub camos: Vec<(WeaponId, String)>,
    /// Weapon Levels the award's kill reached, lowest first.
    pub weapon_levels: Vec<WeaponLevelGain>,
}

impl Profile {
    /// A first-run Profile: custom slots start as copies of the Default Loadouts (so a new class
    /// begins as something playable), and `default_choice` is the Default Loadout chosen before
    /// Profiles existed.
    pub(crate) fn fresh(data: &GameData, default_choice: usize) -> Profile {
        Profile {
            custom: Vec::new(),
            legacy_custom: Vec::new(),
            chosen: LoadoutChoice::Default(default_choice),
            xp: 0,
            weapons: HashMap::new(),
        }
        .validated(data)
    }

    /// Whether this was read from a file in the pre-Pick 10 shape, so it should be saved anew.
    fn needs_migration(&self) -> bool {
        self.custom.is_empty() && !self.legacy_custom.is_empty()
    }

    /// The Profile with everything the data doesn't allow repaired (a hand-edited file or one
    /// written against other data): classes of an older Profile are brought over, each class is
    /// fitted to the Pick 10 rules (dropping, and logging, what doesn't fit), slots are padded or
    /// trimmed to `CUSTOM_SLOTS`, Weapon XP catches up with the kills already counted, and an
    /// out-of-range choice (or a custom one the Level hasn't unlocked) means the first Default
    /// Loadout.
    pub fn validated(mut self, data: &GameData) -> Profile {
        if self.needs_migration() {
            info!(
                "Profile: migrating {} custom classes to Pick 10",
                self.legacy_custom.len()
            );
            self.custom = self.legacy_custom.drain(..).map(Loadout::from).collect();
        }
        self.legacy_custom.clear();
        self.custom.truncate(CUSTOM_SLOTS);
        let mut slot = self.custom.len();
        while self.custom.len() < CUSTOM_SLOTS {
            let mut loadout = data.default_loadouts[slot % data.default_loadouts.len()].clone();
            loadout.name = format!("Custom {}", slot + 1);
            // Attachments wait for Weapon Levels: a new class starts with none.
            loadout.attachments = Attachments::default();
            self.custom.push(loadout);
            slot += 1;
        }
        self.custom = self
            .custom
            .into_iter()
            .enumerate()
            .map(|(slot, loadout)| {
                let (fitted, dropped) = loadout.fitted(data);
                if !dropped.is_empty() {
                    warn!(
                        "Profile: custom class {} \"{}\" broke the Pick 10 rules; dropped {} from the end",
                        slot + 1,
                        fitted.name,
                        dropped.join(", ")
                    );
                }
                fitted
            })
            .collect();
        let levels = &data.progression.weapon_levels;
        for record in self.weapons.values_mut() {
            record.xp = record.xp.max(levels.xp_for(record.kills, record.headshots));
        }
        self.chosen = match self.chosen {
            LoadoutChoice::Custom(slot) if slot < CUSTOM_SLOTS && self.custom_unlocked(data) => {
                LoadoutChoice::Custom(slot)
            }
            LoadoutChoice::Default(index) if index < data.default_loadouts.len() => {
                LoadoutChoice::Default(index)
            }
            _ => LoadoutChoice::Default(0),
        };
        self
    }

    pub fn level(&self, data: &GameData) -> u32 {
        data.progression.levels.level_for(self.xp)
    }

    pub fn level_progress(&self, data: &GameData) -> LevelProgress {
        data.progression.levels.progress(self.xp)
    }

    /// What `weapon` has done so far: its counters for the camo Challenges and its Weapon XP.
    pub fn record(&self, weapon: WeaponId) -> WeaponRecord {
        self.weapons.get(&weapon).copied().unwrap_or_default()
    }

    /// Whether the Level has unlocked editing and choosing the custom Loadouts.
    pub fn custom_unlocked(&self, data: &GameData) -> bool {
        data.progression
            .is_unlocked(UnlockItem::CustomLoadouts, self.level(data))
    }

    /// Where a class stands for its unlocks: the account Level and its Primary's record.
    pub fn standing(&self, loadout: &Loadout, data: &GameData) -> crate::class::Standing {
        crate::class::Standing {
            level: self.level(data),
            primary: self.record(loadout.primary),
        }
    }

    /// Why `choice` can't be used yet, one text each; empty when it can. Default Loadouts are
    /// always usable.
    pub fn locks(&self, choice: LoadoutChoice, data: &GameData) -> Vec<String> {
        let LoadoutChoice::Custom(slot) = choice else {
            return Vec::new();
        };
        if !self.custom_unlocked(data) {
            return vec![format!(
                "Custom classes unlock at Level {}",
                data.progression.unlock_level(UnlockItem::CustomLoadouts)
            )];
        }
        let loadout = &self.custom[slot];
        data.loadout_locks(loadout, self.level(data), &self.record(loadout.primary))
    }

    /// Adds a server award: the XP, and for a kill the Weapon's counters and Weapon XP.
    /// Reports the Levels, Weapon Levels and camos it unlocked.
    pub fn apply_award(&mut self, award: XpAward, data: &GameData) -> AwardOutcome {
        let level_before = self.level(data);
        self.xp = self.xp.saturating_add(award.amount);
        let mut camos = Vec::new();
        let mut weapon_levels = Vec::new();
        let counted = match award.reason {
            XpReason::Kill(weapon) => Some((weapon, 1, 0)),
            XpReason::Headshot(weapon) => Some((weapon, 0, 1)),
            _ => None,
        };
        if let Some((weapon, kills, headshots)) = counted {
            let levels = &data.progression.weapon_levels;
            let before = self.record(weapon);
            let record = self.weapons.entry(weapon).or_default();
            record.kills += kills;
            record.headshots += headshots;
            record.xp = record.xp.saturating_add(levels.xp_for(kills, headshots));
            let after = *record;
            camos.extend(
                data.progression
                    .newly_unlocked(&before, &after)
                    .into_iter()
                    .map(|camo| (weapon, camo.id.clone())),
            );
            weapon_levels.extend(
                (levels.level_for(before.xp) + 1..=levels.level_for(after.xp)).map(|level| {
                    WeaponLevelGain {
                        weapon,
                        level,
                        attachments: data.progression.attachments_unlocked_at(weapon, level),
                    }
                }),
            );
        }
        AwardOutcome {
            level_before,
            level_after: self.level(data),
            camos,
            weapon_levels,
        }
    }

    /// The Loadout the player spawns with.
    pub fn loadout<'a>(&'a self, data: &'a GameData) -> &'a Loadout {
        self.get(self.chosen, data)
    }

    /// The Loadout `choice` names.
    pub fn get<'a>(&'a self, choice: LoadoutChoice, data: &'a GameData) -> &'a Loadout {
        match choice {
            LoadoutChoice::Custom(slot) => &self.custom[slot],
            LoadoutChoice::Default(index) => &data.default_loadouts[index],
        }
    }
}

/// Reads the Profile file; the first run (no file) starts from the Settings' old
/// `loadout` choice. A file in the pre-Pick 10 shape is migrated and saved right away.
fn load_profile(data: &GameData, settings: &Settings) -> Profile {
    match load_config::<Profile>(PROFILE_FILE) {
        Some(profile) => {
            let migrate = profile.needs_migration();
            let profile = profile.validated(data);
            if migrate {
                save_config(PROFILE_FILE, &profile);
            }
            profile
        }
        None => Profile::fresh(data, settings.loadout),
    }
}

fn save_profile(profile: Res<Profile>) {
    save_config(PROFILE_FILE, &*profile);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class::WildcardId;
    use crate::data::WeaponId;
    use crate::gear::{AttachmentId, NO_CAMO};

    fn level_of(profile: &Profile, weapon: WeaponId, data: &GameData) -> u32 {
        data.progression
            .weapon_levels
            .level_for(profile.record(weapon).xp)
    }

    #[test]
    fn a_first_run_profile_has_three_playable_custom_classes() {
        let data = GameData::load();
        let profile = Profile::fresh(&data, 1);
        assert_eq!(profile.custom.len(), CUSTOM_SLOTS);
        assert_eq!(profile.custom[0].name, "Custom 1");
        assert_eq!(profile.chosen, LoadoutChoice::Default(1));
        assert_eq!(profile.loadout(&data), &data.default_loadouts[1]);
        assert_eq!(profile.custom[0].camo, NO_CAMO);
        for class in &profile.custom {
            assert!(data.class_errors(class, None).is_empty(), "{}", class.name);
        }
    }

    #[test]
    fn a_stale_choice_or_slot_count_is_repaired() {
        let data = GameData::load();
        let mut profile = Profile::fresh(&data, 0);
        profile.custom.truncate(1);
        profile.chosen = LoadoutChoice::Default(99);
        let fixed = profile.validated(&data);
        assert_eq!(fixed.custom.len(), CUSTOM_SLOTS);
        assert_eq!(fixed.chosen, LoadoutChoice::Default(0));
    }

    #[test]
    fn a_profile_round_trips_through_ron() {
        let data = GameData::load();
        let mut profile = Profile::fresh(&data, 0);
        profile.custom[1] = Loadout {
            name: "Silent".into(),
            primary: WeaponId::Smg,
            attachments: Attachments {
                muzzle: Some(AttachmentId::Silencer),
                ..Attachments::default()
            },
            secondary: Some(WeaponId::Pistol),
            perks: Perks {
                picks: [
                    Some(PerkId::SleightOfHand),
                    Some(PerkId::StoppingPower),
                    Some(PerkId::Marathon),
                ],
                greed: [None, Some(PerkId::Lightweight), None],
            },
            lethal: None,
            tactical: None,
            wildcards: vec![WildcardId::PerkGreed2],
            camo: NO_CAMO.into(),
        };
        profile.chosen = LoadoutChoice::Custom(1);
        profile.weapons.insert(
            WeaponId::Smg,
            WeaponRecord {
                kills: 3,
                headshots: 1,
                xp: 4,
            },
        );
        let text = ron::ser::to_string_pretty(&profile, ron::ser::PrettyConfig::default()).unwrap();
        let back: Profile = ron::from_str(&text).unwrap();
        assert_eq!(back, profile);
        assert_eq!(back.loadout(&data).name, "Silent");
    }

    /// A profile.ron as the build before Pick 10 wrote it.
    const LEGACY: &str = r#"(
        custom: [
            (
                name: "Old Rifle",
                primary: AssaultRifle,
                attachments: (optic: Some(Acog), muzzle: Some(Silencer)),
                secondary: Pistol,
                perks: (SleightOfHand, StoppingPower, Marathon),
                camo: "tiger",
            ),
            (
                name: "Old Sniper",
                primary: SniperRifle,
                attachments: (optic: None, muzzle: None),
                secondary: Pistol,
                perks: (Scavenger, Lightweight, Ninja),
                camo: "none",
            ),
            (
                name: "Old SMG",
                primary: Smg,
                attachments: (optic: Some(RedDot), muzzle: None),
                secondary: Pistol,
                perks: (SleightOfHand, Lightweight, Ninja),
                camo: "none",
            ),
        ],
        chosen: Custom(0),
        xp: 4000,
        weapons: {
            AssaultRifle: (kills: 40, headshots: 10),
        },
    )"#;

    #[test]
    fn an_old_profile_migrates_keeping_what_fits() {
        let data = GameData::load();
        let old: Profile = ron::from_str(LEGACY).unwrap();
        assert!(old.needs_migration());
        let profile = old.validated(&data);
        assert!(!profile.needs_migration());
        let rifle = &profile.custom[0];
        assert_eq!(rifle.name, "Old Rifle");
        assert_eq!(rifle.attachments.optic, Some(AttachmentId::Acog));
        assert_eq!(rifle.attachments.muzzle, Some(AttachmentId::Silencer));
        assert_eq!(rifle.secondary, Some(WeaponId::Pistol));
        assert_eq!(
            rifle.perks.picks,
            [
                Some(PerkId::SleightOfHand),
                Some(PerkId::StoppingPower),
                Some(PerkId::Marathon)
            ]
        );
        assert_eq!(rifle.camo, "tiger");
        assert_eq!(data.class_cost(rifle), 8);
        assert_eq!(profile.custom[1].primary, WeaponId::SniperRifle);
        assert_eq!(
            profile.custom[2].attachments.optic,
            Some(AttachmentId::RedDot)
        );
        // XP and choice carry over; Weapon XP catches up with the kills (40 + 10 headshots).
        assert_eq!(
            (profile.xp, profile.chosen),
            (4000, LoadoutChoice::Custom(0))
        );
        assert_eq!(profile.record(WeaponId::AssaultRifle).xp, 50);
        assert_eq!(level_of(&profile, WeaponId::AssaultRifle, &data), 4);
        // And the migrated Profile no longer carries the old key.
        let text = ron::ser::to_string(&profile).unwrap();
        assert!(
            text.contains("classes") && !text.contains("custom"),
            "{text}"
        );
        let back: Profile = ron::from_str(&text).unwrap();
        assert_eq!(back, profile);
    }

    #[test]
    fn an_old_class_that_does_not_fit_is_trimmed_from_the_end() {
        let data = GameData::load();
        // The sniper has no Optics under the Weapon Level tracks; the old class keeps the rest.
        let mut old: Profile = ron::from_str(LEGACY).unwrap();
        old.legacy_custom[1].attachments.optic = Some(AttachmentId::Acog);
        old.legacy_custom[1].attachments.muzzle = Some(AttachmentId::Silencer);
        let profile = old.validated(&data);
        assert_eq!(profile.custom[1].attachments.optic, None);
        assert_eq!(
            profile.custom[1].attachments.muzzle,
            Some(AttachmentId::Silencer)
        );
        // An over-budget class loses its last things until it fits.
        let mut over = profile.custom[0].clone();
        over.wildcards = vec![
            WildcardId::PerkGreed1,
            WildcardId::PerkGreed2,
            WildcardId::PerkGreed3,
        ];
        over.perks.greed = [
            Some(PerkId::Scavenger),
            Some(PerkId::Lightweight),
            Some(PerkId::Ninja),
        ];
        assert!(data.class_cost(&over) > 10);
        let mut with_over = profile.clone();
        with_over.custom[0] = over;
        let fixed = with_over.validated(&data);
        assert!(data.class_cost(&fixed.custom[0]) <= 10);
        assert_eq!(fixed.custom[0].primary, WeaponId::AssaultRifle);
        assert_eq!(fixed.custom[0].attachments.optic, Some(AttachmentId::Acog));
    }

    fn award(amount: u32, reason: XpReason) -> XpAward {
        XpAward { amount, reason }
    }

    #[test]
    fn awards_add_xp_and_report_level_ups() {
        let data = GameData::load();
        let mut profile = Profile::fresh(&data, 0);
        let per_level = data.progression.levels.xp_per_level;
        let small = profile.apply_award(award(100, XpReason::Assist), &data);
        assert_eq!((small.level_before, small.level_after), (1, 1));
        assert_eq!(profile.xp, 100);
        let big = profile.apply_award(award(per_level, XpReason::MatchWin), &data);
        assert_eq!((big.level_before, big.level_after), (1, 2));
        assert_eq!(profile.level(&data), 2);
        // Non-kill awards touch no Weapon counter.
        assert!(profile.weapons.is_empty());
    }

    #[test]
    fn kills_and_headshots_count_per_weapon_and_unlock_camos() {
        let data = GameData::load();
        let mut profile = Profile::fresh(&data, 0);
        let needed = match data.progression.camo("woodland").unwrap().challenge {
            crate::progression::Challenge::Kills(n) => n,
            other => panic!("expected a kill challenge, got {other:?}"),
        };
        let mut unlocked = Vec::new();
        for _ in 0..needed {
            let outcome = profile.apply_award(award(0, XpReason::Kill(WeaponId::Smg)), &data);
            unlocked.extend(outcome.camos);
        }
        profile.apply_award(award(0, XpReason::Headshot(WeaponId::Smg)), &data);
        assert_eq!(unlocked, [(WeaponId::Smg, "woodland".to_owned())]);
        assert_eq!(
            profile.record(WeaponId::Smg),
            WeaponRecord {
                kills: needed,
                headshots: 1,
                xp: needed + 1,
            }
        );
        // Other Weapons are untouched.
        assert_eq!(
            profile.record(WeaponId::AssaultRifle),
            WeaponRecord::default()
        );
    }

    #[test]
    fn kills_level_up_a_weapon_and_unlock_its_attachments_in_order() {
        let data = GameData::load();
        let mut profile = Profile::fresh(&data, 0);
        let rifle = WeaponId::AssaultRifle;
        let mut gains = Vec::new();
        for _ in 0..30 {
            gains.extend(
                profile
                    .apply_award(award(0, XpReason::Kill(rifle)), &data)
                    .weapon_levels,
            );
        }
        // 5, 15 and 30 kills: Weapon Levels 1 (Red Dot), 2 (Silencer), 3 (Holographic).
        assert_eq!(
            gains,
            [
                WeaponLevelGain {
                    weapon: rifle,
                    level: 1,
                    attachments: vec![AttachmentId::RedDot]
                },
                WeaponLevelGain {
                    weapon: rifle,
                    level: 2,
                    attachments: vec![AttachmentId::Silencer]
                },
                WeaponLevelGain {
                    weapon: rifle,
                    level: 3,
                    attachments: vec![AttachmentId::Holographic]
                },
            ]
        );
        assert_eq!(level_of(&profile, rifle, &data), 3);
        assert_eq!(level_of(&profile, WeaponId::Smg, &data), 0);
        // A headshot kill is worth 2: kill (1) then headshot (1).
        let headshot = profile.apply_award(award(0, XpReason::Headshot(rifle)), &data);
        assert!(headshot.weapon_levels.is_empty());
        assert_eq!(profile.record(rifle).xp, 31);
    }

    #[test]
    fn custom_classes_wait_for_their_level() {
        let data = GameData::load();
        let mut profile = Profile::fresh(&data, 0);
        let locks = profile.locks(LoadoutChoice::Custom(0), &data);
        assert_eq!(locks.len(), 1);
        assert!(locks[0].contains("Level 2"));
        assert!(profile.locks(LoadoutChoice::Default(1), &data).is_empty());

        profile.chosen = LoadoutChoice::Custom(0);
        assert_eq!(
            profile.clone().validated(&data).chosen,
            LoadoutChoice::Default(0)
        );
        profile.xp = data.progression.levels.xp_to_reach(2);
        assert_eq!(
            profile.clone().validated(&data).chosen,
            LoadoutChoice::Custom(0)
        );
    }
}
