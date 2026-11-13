//! Whether the local player is in play, from the server's `Health` and `PlayerTeam`, how they
//! died last, and the request to respawn early.

use bevy::prelude::*;
use lightyear::prelude::PeerId;

use super::controller::MenuOpen;
use super::killcam::Killcam;
use crate::data::{ActiveRules, MatchRules};
use crate::modes::{ModeView, SdPhaseView};
use crate::net::protocol::{DeathCause, KillFeed, PlayerOwner, RequestRespawn};
use crate::shared::{
    FromServer, Health, LocalPlayer, LocalPlayerReplica, PlayerName, PlayerTeam, Team, ToServer,
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
    /// How the latest kill-feed line with us as victim says we died, and when it arrived.
    last_cause: Option<(DeathCause, f64)>,
    /// When we last switched from one Team to the other.
    switched_at: Option<f64>,
    /// Our own peer, once our server Player has been replicated.
    peer: Option<PeerId>,
}

impl LocalLife {
    /// A local player alive on `team`, for tools that draw remote players without a game
    /// (MapEdit's soldier viewer).
    pub fn on_team(team: Team) -> Self {
        Self {
            team: Some(team),
            ..default()
        }
    }

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

    /// `Time::elapsed_secs_f64` of the latest death.
    pub fn died_at(&self) -> f64 {
        self.died_at
    }

    /// Our own peer, once known.
    pub fn peer(&self) -> Option<PeerId> {
        self.peer
    }

    /// Who killed us this death, if it was someone else (not a fall, a suicide or a team
    /// switch): the deaths that play a Kill-cam.
    pub fn killer(&self) -> Option<PeerId> {
        self.cause()
            .and_then(DeathCause::killer_id)
            .filter(|&killer| Some(killer) != self.peer)
    }

    /// How we died this death, if the kill-feed line has arrived.
    pub fn cause(&self) -> Option<&DeathCause> {
        self.last_cause
            .as_ref()
            .filter(|(_, at)| *at >= self.died_at - KILLER_MATCH_SECS)
            .map(|(cause, _)| cause)
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
    replica: Query<(&Health, &PlayerTeam, &PlayerOwner), With<LocalPlayerReplica>>,
) {
    let now = time.elapsed_secs_f64();
    life.peer = replica.single().ok().map(|(.., owner)| owner.0);
    if let Ok(me) = me.single() {
        for FromServer(line) in feed.read() {
            if line.victim == me.0 {
                life.last_cause = Some((line.cause.clone(), now));
            }
        }
    }
    let (health_dead, team) = replica
        .single()
        .map_or((true, None), |(health, team, _)| (health.is_dead(), team.0));
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

/// What keeps a dead player from respawning on request, as the death screen says it: Search &
/// Destroy rounds and a held Headquarters.
pub(super) fn respawn_hold(view: Option<&ModeView>, team: Option<Team>) -> Option<&'static str> {
    match view {
        Some(ModeView::Headquarters(hq)) if hq.position.is_some() && hq.zone.owner == team => {
            Some("Your team holds the HQ: waiting for it to fall")
        }
        Some(ModeView::SearchAndDestroy(sd)) if sd.phase != SdPhaseView::Waiting => {
            Some("Waiting for the next round")
        }
        _ => None,
    }
}

/// Whether the death screen offers (and the server honours) a respawn on request: the player
/// has been dead for the Match Rules' minimum and nothing holds them back.
pub(super) fn can_request_respawn(
    life: &LocalLife,
    view: Option<&ModeView>,
    rules: &MatchRules,
    now: f64,
) -> bool {
    life.is_dead()
        && life.team().is_some()
        && respawn_hold(view, life.team()).is_none()
        && life.secs_dead(now) >= f64::from(rules.early_respawn_secs())
}

/// SPACE or a click asks the server to respawn once the death screen offers it. Not while a
/// Kill-cam plays, nor with the press that skipped it.
#[allow(clippy::too_many_arguments)]
pub(super) fn request_respawn(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    menu: Res<MenuOpen>,
    killcam: Res<Killcam>,
    life: Res<LocalLife>,
    rules: ActiveRules,
    modes: Query<&ModeView>,
    mut requests: MessageWriter<ToServer<RequestRespawn>>,
) {
    if menu.0
        || killcam.holds_respawn_input()
        || !(keys.just_pressed(KeyCode::Space) || mouse.just_pressed(MouseButton::Left))
    {
        return;
    }
    if can_request_respawn(
        &life,
        modes.single().ok(),
        rules.get(),
        time.elapsed_secs_f64(),
    ) {
        info!("Asking to respawn early");
        requests.write(ToServer(RequestRespawn));
    }
}
