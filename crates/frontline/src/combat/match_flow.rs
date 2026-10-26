//! The Match clock and phases (server): counts the time limit down, ends the Match at the score
//! or time limit, and after the end screen starts the next one with scores and stats reset.

use std::time::Duration;

use bevy::prelude::*;
use lightyear::prelude::*;

use super::health::PendingRespawn;
use crate::data::{GameData, GameModeId};
use crate::shared::{MatchPhase, MatchState, PlayerStats, PlayerTeam, Team};

/// The only Game Mode until Rotations exist.
const MODE: GameModeId = GameModeId::TeamDeathmatch;

/// Exact seconds left in the current phase (playing or end screen); `MatchState` only carries
/// whole seconds, so it is written once per second.
#[derive(Resource, Debug)]
pub(super) struct MatchClock {
    left: f32,
}

/// Whether shots and damage count: the Match is playing and no team has reached the limit.
pub(super) fn is_live(state: &MatchState) -> bool {
    state.phase == MatchPhase::Playing && state.scores.iter().all(|&s| s < state.score_limit)
}

/// The team with the higher score, `None` on a tie.
fn leader(scores: [u32; 2]) -> Option<Team> {
    match scores[Team::Alpha.index()].cmp(&scores[Team::Bravo.index()]) {
        std::cmp::Ordering::Greater => Some(Team::Alpha),
        std::cmp::Ordering::Less => Some(Team::Bravo),
        std::cmp::Ordering::Equal => None,
    }
}

fn whole_secs(left: f32) -> u32 {
    left.max(0.0).ceil() as u32
}

/// Spawns the replicated `MatchState` entity of the first Match.
pub(super) fn spawn_match_state(data: Res<GameData>, mut commands: Commands) {
    let settings = data.game_mode(MODE);
    commands.insert_resource(MatchClock {
        left: settings.time_limit_secs as f32,
    });
    commands.spawn((
        Name::new("Match"),
        MatchState {
            mode: MODE,
            scores: [0; 2],
            score_limit: settings.score_limit,
            seconds_left: settings.time_limit_secs,
            phase: MatchPhase::Playing,
        },
        Replicate::to_clients(NetworkTarget::All),
    ));
}

/// Advances the Match: ends it at the score or time limit, and restarts it after the end screen.
pub(super) fn tick_match(
    mut state: Single<&mut MatchState>,
    mut clock: ResMut<MatchClock>,
    mut players: Query<(Entity, &PlayerTeam, &mut PlayerStats)>,
    data: Res<GameData>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let settings = data.game_mode(state.mode);
    clock.left -= time.delta_secs();
    match state.phase {
        MatchPhase::Playing => {
            let decided = !is_live(&state);
            if decided || clock.left <= 0.0 {
                let winner = leader(state.scores);
                state.seconds_left = whole_secs(clock.left);
                state.phase = MatchPhase::Ended {
                    winner,
                    next_in_secs: settings.end_screen_secs,
                };
                clock.left = settings.end_screen_secs as f32;
                info!(
                    "Match over ({}): {}, Alpha {} - Bravo {}",
                    if decided { "score limit" } else { "time limit" },
                    winner.map_or_else(|| "draw".to_owned(), |team| format!("{team:?} wins")),
                    state.scores[Team::Alpha.index()],
                    state.scores[Team::Bravo.index()],
                );
            } else if state.seconds_left != whole_secs(clock.left) {
                state.seconds_left = whole_secs(clock.left);
            }
        }
        MatchPhase::Ended { winner, .. } if clock.left > 0.0 => {
            let next_in_secs = whole_secs(clock.left);
            if state.phase != (MatchPhase::Ended { winner, next_in_secs }) {
                state.phase = MatchPhase::Ended { winner, next_in_secs };
            }
        }
        MatchPhase::Ended { .. } => {
            clock.left = settings.time_limit_secs as f32;
            **state = MatchState {
                mode: state.mode,
                scores: [0; 2],
                score_limit: settings.score_limit,
                seconds_left: settings.time_limit_secs,
                phase: MatchPhase::Playing,
            };
            for (entity, team, mut stats) in &mut players {
                *stats = PlayerStats::default();
                if team.0.is_some() {
                    commands
                        .entity(entity)
                        .insert(PendingRespawn::at(Duration::ZERO));
                }
            }
            info!("New Match started");
        }
    }
}
