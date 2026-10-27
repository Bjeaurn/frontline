//! Client side of progression: applies each server `XpAward` to the Profile (which saves
//! itself), counts what this Match has paid for the end screen, and announces XP, Levels and
//! camo unlocks to the HUD with messages.

use bevy::prelude::*;

use super::{XpReason, hud};
use crate::data::{GameData, WeaponId};
use crate::flow::AppState;
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

/// What the current Match has paid this player, for the end-of-Match summary.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct MatchXp {
    /// The Profile's XP when the Match began.
    pub start_xp: u32,
    pub gained: u32,
}

pub struct ProgressionPlugin;

impl Plugin for ProgressionPlugin {
    /// Add after `ProfilePlugin`.
    fn build(&self, app: &mut App) {
        app.init_resource::<MatchXp>()
            .add_message::<XpGained>()
            .add_message::<LevelUp>()
            .add_message::<CamoUnlocked>()
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
    *match_xp = MatchXp {
        start_xp: profile.xp,
        gained: 0,
    };
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
        *match_xp = MatchXp {
            start_xp: profile.xp,
            gained: 0,
        };
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
    }
}
