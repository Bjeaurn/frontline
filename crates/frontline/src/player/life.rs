//! Whether the local player is in play, from the server's `Health` and `PlayerTeam`, and who
//! killed them last.

use bevy::prelude::*;

use crate::net::protocol::KillFeed;
use crate::shared::{
    FromServer, Health, LocalPlayer, LocalPlayerReplica, PlayerName, PlayerTeam, Team,
};

/// A kill-feed line naming us (or a team switch) counts as the cause of our death if it
/// happened this long before `Health` showed it (the two travel separately).
const KILLER_MATCH_SECS: f64 = 2.0;

/// Life state of the local player. Movement, look and firing are locked while out of play.
#[derive(Resource, Default, Debug)]
pub struct LocalLife {
    /// Out of play: dead, or not on a Team yet.
    dead: bool,
    team: Option<Team>,
    /// `Time::elapsed_secs_f64` of the latest death.
    died_at: f64,
    /// Killer named by the latest kill-feed line with us as victim, and when it arrived.
    last_killer: Option<(String, f64)>,
    /// When we last switched from one Team to the other.
    switched_at: Option<f64>,
}

impl LocalLife {
    /// Whether the local player is out of play: dead, or still choosing a Team.
    pub fn is_dead(&self) -> bool {
        self.dead
    }

    /// The local player's Team, `None` while choosing.
    pub fn team(&self) -> Option<Team> {
        self.team
    }

    /// Seconds since the latest death.
    pub fn secs_dead(&self, now: f64) -> f64 {
        now - self.died_at
    }

    /// Who killed us this death, if the kill-feed line has arrived.
    pub fn killer(&self) -> Option<&str> {
        self.last_killer
            .as_ref()
            .filter(|(_, at)| *at >= self.died_at - KILLER_MATCH_SECS)
            .map(|(name, _)| name.as_str())
    }

    /// Whether this death came from switching Team.
    pub fn switched_team(&self) -> bool {
        self.switched_at
            .is_some_and(|at| at >= self.died_at - KILLER_MATCH_SECS)
    }
}

/// Forgets the finished session's life state.
pub(super) fn reset_life(mut life: ResMut<LocalLife>) {
    *life = LocalLife::default();
}

pub(super) fn track_life(
    time: Res<Time>,
    mut life: ResMut<LocalLife>,
    mut feed: MessageReader<FromServer<KillFeed>>,
    me: Query<&PlayerName, With<LocalPlayer>>,
    replica: Query<(&Health, &PlayerTeam), With<LocalPlayerReplica>>,
) {
    let now = time.elapsed_secs_f64();
    if let Ok(me) = me.single() {
        for FromServer(line) in feed.read() {
            if line.victim == me.0 {
                life.last_killer = Some((line.killer.clone(), now));
            }
        }
    }
    let (health_dead, team) = replica
        .single()
        .map_or((true, None), |(health, team)| (health.is_dead(), team.0));
    if team != life.team {
        if life.team.is_some() && team.is_some() {
            life.switched_at = Some(now);
        }
        info!("Local player team: {team:?}");
        life.team = team;
    }
    let dead = health_dead || team.is_none();
    if dead != life.dead {
        life.dead = dead;
        if dead && team.is_some() {
            life.died_at = now;
            info!("Local player died");
        }
    }
}
