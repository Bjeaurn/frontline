//! Team Deathmatch HUD (Client and Host): the team select screen, the top-centre score, the
//! Scoreboard (held with Tab, forced open at Match end) and friendly name tags.
//!
//! Teams are always coloured relative to the local player (friendly blue, enemy red); their
//! Faction (from data) gives them a name and an emblem.

mod name_tags;
mod score;
mod scoreboard;
mod team_select;

use bevy::prelude::*;

use crate::data::Faction;
use crate::flow::{AppState, Overlay};
use crate::player::ControllerSystems;
use crate::shared::{LocalPlayerReplica, MatchState, PlayerTeam, Team};

/// UI colour of friendly names and scores.
pub const FRIENDLY_COLOR: Color = Color::srgb(0.4, 0.7, 1.0);
/// UI colour of enemy names and scores.
pub const ENEMY_COLOR: Color = Color::srgb(1.0, 0.38, 0.32);

/// Team the local player asked to join that the server hasn't confirmed yet, so the team
/// select isn't reopened while a first choice is in flight.
#[derive(Resource, Default, Debug)]
pub struct PendingTeam(pub Option<Team>);

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

/// The accent colour a Faction's data asks for.
fn faction_color(faction: &Faction) -> Color {
    let (r, g, b) = faction.color;
    Color::srgb(r, g, b)
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

/// Forgets the previous session's choices so a new Host/Join starts clean.
fn reset_session(mut pending: ResMut<PendingTeam>) {
    pending.0 = None;
}

/// Clears `PendingTeam` once the server has put us on that Team.
fn settle_pending_team(
    mut pending: ResMut<PendingTeam>,
    replica: Query<&PlayerTeam, With<LocalPlayerReplica>>,
) {
    if pending.0.is_some_and(|team| {
        replica
            .single()
            .is_ok_and(|current| current.0 == Some(team))
    }) {
        pending.0 = None;
    }
}

pub struct MatchHudPlugin;

impl Plugin for MatchHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingTeam>()
            .init_resource::<team_select::TeamSelect>()
            .add_systems(
                OnEnter(AppState::InGame),
                (score::spawn_score, scoreboard::spawn_scoreboard),
            )
            .add_systems(OnExit(AppState::InGame), reset_session)
            .add_systems(
                Update,
                (
                    settle_pending_team,
                    (
                        team_select::spawn_team_select,
                        team_select::drive_team_select,
                        team_select::update_team_select,
                    )
                        .chain()
                        .run_if(in_state(Overlay::TeamSelect)),
                    (
                        score::update_score,
                        scoreboard::update_scoreboard,
                        name_tags::attach_name_tags,
                        name_tags::place_name_tags,
                    )
                        .after(ControllerSystems::View)
                        .run_if(in_state(AppState::InGame)),
                ),
            );
    }
}
