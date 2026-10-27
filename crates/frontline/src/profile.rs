//! The Profile (CONTEXT.md: Profile): the player's own data, kept in
//! `<config dir>/frontline/profile.ron` next to `settings.ron`. It travels with the player, not
//! with any Host, so it is a client-local file the server never sees.
//!
//! For now it holds the three custom Loadouts and which Loadout is chosen. XP, Level and unlocks
//! are meant to live here later: new fields need `#[serde(default)]` so existing files keep
//! loading. Client and Host only.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::data::GameData;
use crate::flow::{Settings, load_config, save_config};
use crate::gear::Loadout;

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
    /// Reserved for progression (XP and Level are derived from it later); nothing earns it yet.
    #[serde(default)]
    pub xp: u32,
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
        }
        .validated(data)
    }

    /// The Profile with everything the data doesn't allow repaired (a hand-edited file or one
    /// written against other data): a Loadout is validated, slots are padded or trimmed to
    /// `CUSTOM_SLOTS`, and an out-of-range choice means the first Default Loadout.
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
            LoadoutChoice::Custom(slot) if slot < CUSTOM_SLOTS => LoadoutChoice::Custom(slot),
            LoadoutChoice::Default(index) if index < data.default_loadouts.len() => {
                LoadoutChoice::Default(index)
            }
            _ => LoadoutChoice::Default(0),
        };
        self
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
}
