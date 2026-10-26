//! Whether the local player is alive, from the server's `Health`, and who killed them last.

use bevy::prelude::*;

use crate::net::protocol::KillFeed;
use crate::shared::{FromServer, Health, LocalPlayer, LocalPlayerReplica, PlayerName};

/// A kill-feed line naming us counts as our death if it arrived this long before `Health`
/// showed it (the two travel separately).
const KILLER_MATCH_SECS: f64 = 2.0;

/// Life state of the local player. Movement, look and firing are locked while dead.
#[derive(Resource, Default, Debug)]
pub struct LocalLife {
    dead: bool,
    /// `Time::elapsed_secs_f64` of the latest death.
    died_at: f64,
    /// Killer named by the latest kill-feed line with us as victim, and when it arrived.
    last_killer: Option<(String, f64)>,
}

impl LocalLife {
    pub fn is_dead(&self) -> bool {
        self.dead
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
}

pub(super) fn track_life(
    time: Res<Time>,
    mut life: ResMut<LocalLife>,
    mut feed: MessageReader<FromServer<KillFeed>>,
    me: Query<&PlayerName, With<LocalPlayer>>,
    replica: Query<&Health, With<LocalPlayerReplica>>,
) {
    let now = time.elapsed_secs_f64();
    if let Ok(me) = me.single() {
        for FromServer(line) in feed.read() {
            if line.victim == me.0 {
                life.last_killer = Some((line.killer.clone(), now));
            }
        }
    }
    let dead = replica.single().is_ok_and(|health| health.is_dead());
    if dead != life.dead {
        life.dead = dead;
        if dead {
            life.died_at = now;
            info!("Local player died");
        }
    }
}
