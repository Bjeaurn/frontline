//! The Profile (CONTEXT.md: Profile): the player's own data, kept in
//! `<config dir>/frontline/profile.ron` next to `settings.ron`. It travels with the player, not
//! with any Host, so it is a client-local file the server never sees.
//!
//! It holds the three custom Loadouts, which Loadout is chosen, the XP the server awarded (the
//! Level is derived from it, `data/progression.ron`) and the per-Weapon kill counters the camo
//! Challenges read. New fields need `#[serde(default)]` so existing files keep loading. Client
//! and Host only.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::data::{GameData, WeaponId};
use crate::flow::{Settings, load_config, save_config};
use crate::gear::Loadout;
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

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// The custom Loadouts, always `CUSTOM_SLOTS` of them.
    pub custom: Vec<Loadout>,
    pub chosen: LoadoutChoice,
    /// Total XP earned; the Level is derived from it.
    #[serde(default)]
    pub xp: u32,
    /// Kills and headshot kills per Weapon, for the camo Challenges.
    #[serde(default)]
    pub weapons: HashMap<WeaponId, WeaponRecord>,
}

/// What an XP award changed, for the HUD to announce.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AwardOutcome {
    pub level_before: u32,
    pub level_after: u32,
    /// Camos (by id) the award's kill unlocked, with the Weapon they are unlocked on.
    pub camos: Vec<(WeaponId, String)>,
}

impl Profile {
    /// A first-run Profile: custom slots start as copies of the Default Loadouts (so a new class
    /// begins as something playable), and `default_choice` is the Default Loadout chosen before
    /// Profiles existed.
    fn fresh(data: &GameData, default_choice: usize) -> Profile {
        Profile {
            custom: Vec::new(),
            chosen: LoadoutChoice::Default(default_choice),
            xp: 0,
            weapons: HashMap::new(),
        }
        .validated(data)
    }

    /// The Profile with everything the data doesn't allow repaired (a hand-edited file or one
    /// written against other data): a Loadout is validated, slots are padded or trimmed to
    /// `CUSTOM_SLOTS`, and an out-of-range choice (or a custom one the Level hasn't unlocked)
    /// means the first Default Loadout.
    pub fn validated(mut self, data: &GameData) -> Profile {
        self.custom.truncate(CUSTOM_SLOTS);
        let mut slot = self.custom.len();
        while self.custom.len() < CUSTOM_SLOTS {
            let mut loadout = data.default_loadouts[slot % data.default_loadouts.len()].clone();
            loadout.name = format!("Custom {}", slot + 1);
            self.custom.push(loadout);
            slot += 1;
        }
        self.custom = self
            .custom
            .into_iter()
            .map(|loadout| loadout.validated(data))
            .collect();
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

    /// What `weapon` has done so far, for its camo Challenges.
    pub fn record(&self, weapon: WeaponId) -> WeaponRecord {
        self.weapons.get(&weapon).copied().unwrap_or_default()
    }

    /// Whether the Level has unlocked editing and choosing the custom Loadouts.
    pub fn custom_unlocked(&self, data: &GameData) -> bool {
        data.progression
            .is_unlocked(UnlockItem::CustomLoadouts, self.level(data))
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

    /// Adds a server award: the XP, and for a kill the Weapon's counters. Reports the Levels
    /// and camos it unlocked.
    pub fn apply_award(&mut self, award: XpAward, data: &GameData) -> AwardOutcome {
        let level_before = self.level(data);
        self.xp = self.xp.saturating_add(award.amount);
        let mut camos = Vec::new();
        let counted = match award.reason {
            XpReason::Kill(weapon) => Some((weapon, 1, 0)),
            XpReason::Headshot(weapon) => Some((weapon, 0, 1)),
            _ => None,
        };
        if let Some((weapon, kills, headshots)) = counted {
            let before = self.record(weapon);
            let record = self.weapons.entry(weapon).or_default();
            record.kills += kills;
            record.headshots += headshots;
            let after = *record;
            camos.extend(
                data.progression
                    .newly_unlocked(&before, &after)
                    .into_iter()
                    .map(|camo| (weapon, camo.id.clone())),
            );
        }
        AwardOutcome {
            level_before,
            level_after: self.level(data),
            camos,
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
/// `loadout` choice.
fn load_profile(data: &GameData, settings: &Settings) -> Profile {
    match load_config::<Profile>(PROFILE_FILE) {
        Some(profile) => profile.validated(data),
        None => Profile::fresh(data, settings.loadout),
    }
}

fn save_profile(profile: Res<Profile>) {
    save_config(PROFILE_FILE, &*profile);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::WeaponId;
    use crate::gear::{NO_CAMO, PerkId};

    #[test]
    fn a_first_run_profile_has_three_playable_custom_classes() {
        let data = GameData::load();
        let profile = Profile::fresh(&data, 1);
        assert_eq!(profile.custom.len(), CUSTOM_SLOTS);
        assert_eq!(profile.custom[0].name, "Custom 1");
        assert_eq!(profile.chosen, LoadoutChoice::Default(1));
        assert_eq!(profile.loadout(&data), &data.default_loadouts[1]);
        assert_eq!(profile.custom[0].camo, NO_CAMO);
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
            attachments: crate::gear::Attachments {
                optic: None,
                muzzle: Some(crate::gear::AttachmentId::Silencer),
            },
            secondary: WeaponId::Pistol,
            perks: [
                PerkId::SleightOfHand,
                PerkId::StoppingPower,
                PerkId::Marathon,
            ],
            camo: NO_CAMO.into(),
        };
        profile.chosen = LoadoutChoice::Custom(1);
        let text = ron::ser::to_string_pretty(&profile, ron::ser::PrettyConfig::default()).unwrap();
        let back: Profile = ron::from_str(&text).unwrap();
        assert_eq!(back, profile);
        assert_eq!(back.loadout(&data).name, "Silent");
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
                headshots: 1
            }
        );
        // Other Weapons are untouched.
        assert_eq!(
            profile.record(WeaponId::AssaultRifle),
            WeaponRecord::default()
        );
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
