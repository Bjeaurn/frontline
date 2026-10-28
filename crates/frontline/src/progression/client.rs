//! Client side of progression: applies each server `XpAward` to the Profile (which saves
//! itself), counts what this Match has paid for the end screen, and announces XP, Levels and
//! camo unlocks to the HUD with messages.

use std::collections::HashMap;

use bevy::prelude::*;

use super::{XpReason, hud};
use crate::data::{GameData, WeaponId};
use crate::flow::AppState;
use crate::gear::AttachmentId;
use crate::net::protocol::XpAward;
use crate::profile::Profile;
use crate::shared::{FromServer, MatchPhase, MatchState};

/// XP just earned, for the popup.
#[derive(Message, Clone, Copy, Debug)]
pub struct XpGained {
    pub amount: u32,
    pub reason: XpReason,
}

/// The Profile reached `level`; `unlocked` names what it opened up.
#[derive(Message, Clone, Debug)]
pub struct LevelUp {
    pub level: u32,
    pub unlocked: Vec<String>,
}

/// A camo challenge was met: `camo` (an id from `data/progression.ron`) is now yours on `weapon`.
#[derive(Message, Clone, Debug)]
pub struct CamoUnlocked {
    pub weapon: WeaponId,
    pub camo: String,
}

/// A Weapon reached `level` (CONTEXT.md: Weapon Level); `attachments` are what it unlocked.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct WeaponLevelUp {
    pub weapon: WeaponId,
    pub level: u32,
    pub attachments: Vec<AttachmentId>,
}

impl WeaponLevelUp {
    /// The toast text: `M4 LEVEL 3: HOLOGRAPHIC UNLOCKED`, or just `M4 LEVEL 2` when the level
    /// opened nothing.
    pub fn text(&self, data: &GameData) -> String {
        let weapon = data.weapon(self.weapon).short_name();
        if self.attachments.is_empty() {
            return format!("{weapon} LEVEL {}", self.level).to_uppercase();
        }
        let names: Vec<&str> = self
            .attachments
            .iter()
            .map(|id| data.attachment(*id).name.as_str())
            .collect();
        format!(
            "{weapon} LEVEL {}: {} UNLOCKED",
            self.level,
            names.join(", ")
        )
        .to_uppercase()
    }
}

/// What the current Match has paid this player, for the end-of-Match summary.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct MatchXp {
    /// The Profile's XP when the Match began.
    pub start_xp: u32,
    pub gained: u32,
    /// Each Weapon's Weapon XP when the Match began.
    pub start_weapon_xp: HashMap<WeaponId, u32>,
}

impl MatchXp {
    /// A Match starting now, from `profile`.
    fn starting_at(profile: &Profile) -> MatchXp {
        MatchXp {
            start_xp: profile.xp,
            gained: 0,
            start_weapon_xp: profile
                .weapons
                .iter()
                .map(|(weapon, record)| (*weapon, record.xp))
                .collect(),
        }
    }
}

pub struct ProgressionPlugin;

impl Plugin for ProgressionPlugin {
    /// Add after `ProfilePlugin`.
    fn build(&self, app: &mut App) {
        app.init_resource::<MatchXp>()
            .add_message::<XpGained>()
            .add_message::<LevelUp>()
            .add_message::<CamoUnlocked>()
            .add_message::<WeaponLevelUp>()
            .add_plugins(hud::ProgressionHudPlugin)
            .add_systems(OnEnter(AppState::InGame), start_match_xp)
            .add_systems(
                Update,
                (reset_on_new_match, apply_awards)
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

fn start_match_xp(profile: Res<Profile>, mut match_xp: ResMut<MatchXp>) {
    *match_xp = MatchXp::starting_at(&profile);
}

/// A new Match (the end screen is over) starts the summary from the current XP.
fn reset_on_new_match(
    state: Query<&MatchState>,
    profile: Res<Profile>,
    mut match_xp: ResMut<MatchXp>,
    mut was_ended: Local<bool>,
) {
    let Ok(state) = state.single() else {
        return;
    };
    let ended = matches!(state.phase, MatchPhase::Ended { .. });
    if std::mem::replace(&mut *was_ended, ended) && !ended {
        *match_xp = MatchXp::starting_at(&profile);
    }
}

fn apply_awards(
    mut awards: MessageReader<FromServer<XpAward>>,
    data: Res<GameData>,
    mut profile: ResMut<Profile>,
    mut match_xp: ResMut<MatchXp>,
    mut gained: MessageWriter<XpGained>,
    mut level_ups: MessageWriter<LevelUp>,
    mut camos: MessageWriter<CamoUnlocked>,
    mut weapon_levels: MessageWriter<WeaponLevelUp>,
) {
    for FromServer(award) in awards.read() {
        let outcome = profile.apply_award(*award, &data);
        match_xp.gained += award.amount;
        info!(
            "XP +{} ({}), total {} (Level {})",
            award.amount,
            award.reason.label(),
            profile.xp,
            outcome.level_after
        );
        gained.write(XpGained {
            amount: award.amount,
            reason: award.reason,
        });
        if outcome.level_after > outcome.level_before {
            let unlocked: Vec<String> = (outcome.level_before + 1..=outcome.level_after)
                .flat_map(|level| data.progression.unlocked_at(level))
                .map(|item| data.unlock_name(item))
                .collect();
            info!(
                "Level up: Level {}{}",
                outcome.level_after,
                if unlocked.is_empty() {
                    String::new()
                } else {
                    format!(", unlocked {}", unlocked.join(", "))
                }
            );
            level_ups.write(LevelUp {
                level: outcome.level_after,
                unlocked,
            });
        }
        for (weapon, camo) in outcome.camos {
            info!("Camo unlocked: {camo} on {weapon:?}");
            camos.write(CamoUnlocked { weapon, camo });
        }
        for gain in outcome.weapon_levels {
            let level_up = WeaponLevelUp {
                weapon: gain.weapon,
                level: gain.level,
                attachments: gain.attachments,
            };
            info!("Weapon level up: {}", level_up.text(&data));
            weapon_levels.write(level_up);
        }
    }
}
