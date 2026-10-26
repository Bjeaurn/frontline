//! Team Deathmatch HUD (Client and Host): the team picker, the top-centre score, the
//! Scoreboard (held with Tab, forced open at Match end) and friendly name tags.
//!
//! Teams are always shown relative to the local player: friendly blue, enemy red.

mod name_tags;
mod picker;
mod score;
mod scoreboard;

use bevy::prelude::*;

use crate::player::ControllerSystems;
use crate::shared::{MatchState, Team};

/// UI colour of friendly names and scores.
pub const FRIENDLY_COLOR: Color = Color::srgb(0.4, 0.7, 1.0);
/// UI colour of enemy names and scores.
pub const ENEMY_COLOR: Color = Color::srgb(1.0, 0.38, 0.32);

/// Whether a player on `other` is on the local player's Team (`local`).
pub fn is_friendly(local: Option<Team>, other: Option<Team>) -> bool {
    other.is_some() && other == local
}

/// Friendly or enemy UI colour of a player on `other`, seen by a player on `local`.
pub fn relation_color(local: Option<Team>, other: Option<Team>) -> Color {
    if is_friendly(local, other) {
        FRIENDLY_COLOR
    } else {
        ENEMY_COLOR
    }
}

/// The side shown as "ours": the local Team, or Alpha while still choosing.
fn own_side(local: Option<Team>) -> Team {
    local.unwrap_or(Team::Alpha)
}

/// `m:ss` of a whole number of seconds.
fn clock(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The replicated Match state (the server's own entity on a Host).
type MatchQuery<'w, 's> = Query<'w, 's, &'static MatchState>;

pub struct MatchHudPlugin;

impl Plugin for MatchHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<picker::TeamPicker>()
            .add_systems(
                Startup,
                (
                    picker::spawn_picker,
                    score::spawn_score,
                    scoreboard::spawn_scoreboard,
                ),
            )
            .add_systems(
                Update,
                (
                    picker::drive_picker.before(ControllerSystems::Look),
                    (
                        picker::update_picker,
                        score::update_score,
                        scoreboard::update_scoreboard,
                        name_tags::attach_name_tags,
                        name_tags::place_name_tags,
                    )
                        .after(ControllerSystems::View),
                ),
            );
    }
}
