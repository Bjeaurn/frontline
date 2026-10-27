//! Server side of the objective Game Modes: builds each Match's objective from the Map's
//! entities, feeds the pure rules in `modes` what the players are doing, applies what they
//! decide (scores, respawn rounds, the explosion) and publishes the `ModeView` clients draw.

use std::hash::{BuildHasher, RandomState};
use std::time::Duration;

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::health::PendingRespawn;
use super::hitbox::{Hitboxes, set_hitboxes_enabled};
use super::match_flow::is_live;
use crate::data::{GameData, GameModeId};
use crate::maps::{BombSite, DomFlag, HqSite, MapReady, MapRoot};
use crate::modes::{
    ActionView, Bomb, BombView, Capture, CaptureEvent, DomState, FlagView, HqEvent, HqPhase,
    HqState, HqView, ModeView, SdEvent, SdInput, SdPhase, SdPhaseView, SdState, SdView, ZoneView,
    in_zone,
};
use crate::net::LinkPlayer;
use crate::net::protocol::{BombExploded, EventChannel, ObjectiveHold, PlayerOwner};
use crate::rotation::ServerConfig;
use crate::shared::{Health, MatchState, PlayerPose, PlayerStats, PlayerTeam, Team};

/// Whether the player is holding the use key (F), as the client last reported. Server-only.
#[derive(Component, Debug, Default)]
pub(super) struct UseHeld(pub bool);

/// The server's running objective of the current Match.
#[derive(Resource, Debug)]
pub(super) enum ModeRuntime {
    /// A Match just (re)started; the objective is built once its Map is loaded.
    Pending,
    /// Team Deathmatch, or a Map without the entities the mode needs.
    Nothing,
    Dom(DomState),
    Hq(HqState),
    Sd(Box<SdRuntime>),
}

#[derive(Debug)]
pub(super) struct SdRuntime {
    state: SdState,
    /// The round clock, seconds.
    round_secs: f32,
    carrier: Option<(Entity, PeerId)>,
    carrier_pos: Vec3,
    action: Option<ActionView>,
    /// Living players per team at the last tick.
    alive: [u32; 2],
}

impl ModeRuntime {
    /// Whether a dead player of `team` may come back now: never mid-round in Search & Destroy,
    /// and not while their team holds the Headquarters.
    pub(super) fn respawn_open(&self, team: Team) -> bool {
        match self {
            ModeRuntime::Hq(hq) => hq.owner() != Some(team),
            ModeRuntime::Sd(sd) => sd.state.respawn_open(),
            ModeRuntime::Pending | ModeRuntime::Nothing | ModeRuntime::Dom(_) => true,
        }
    }

    /// Places a team's respawns are drawn toward: the Flags it holds in Domination.
    pub(super) fn spawn_anchors(&self, team: Team) -> Vec<Vec3> {
        match self {
            ModeRuntime::Dom(dom) => dom.held_positions(team),
            _ => Vec::new(),
        }
    }

    fn view(&self) -> ModeView {
        match self {
            ModeRuntime::Pending | ModeRuntime::Nothing => ModeView::None,
            ModeRuntime::Dom(dom) => ModeView::Domination(
                dom.flags
                    .iter()
                    .map(|flag| FlagView {
                        label: flag.label,
                        position: flag.position,
                        zone: zone_view(&flag.capture),
                    })
                    .collect(),
            ),
            ModeRuntime::Hq(hq) => ModeView::Headquarters(HqView {
                position: hq.position(),
                zone: zone_view(&hq.capture),
                seconds_left: match hq.phase {
                    HqPhase::Active { life_left } => life_left,
                    HqPhase::Relocating { left } => left,
                }
                .max(0.0)
                .ceil() as u32,
            }),
            ModeRuntime::Sd(sd) => {
                let state = &sd.state;
                let fuse_left = match state.bomb {
                    Bomb::Planted { fuse_left, .. } => fuse_left.max(0.0).ceil() as u32,
                    _ => 0,
                };
                ModeView::SearchAndDestroy(SdView {
                    round: state.round,
                    attackers: state.attackers,
                    phase: match state.phase {
                        SdPhase::Waiting => SdPhaseView::Waiting,
                        SdPhase::Starting { .. } => SdPhaseView::Starting,
                        SdPhase::Live { .. } => SdPhaseView::Live,
                        SdPhase::Over { winner, reason, .. } => {
                            SdPhaseView::Over { winner, reason }
                        }
                    },
                    alive: sd.alive,
                    bomb: match state.bomb {
                        Bomb::Gone => BombView::Gone,
                        Bomb::Carried => BombView::Carried,
                        Bomb::Dropped(position) => BombView::Dropped(position),
                        Bomb::Planted { site, .. } => BombView::Planted {
                            site: site_letter(site),
                            position: state.sites.get(site).copied().unwrap_or_default(),
                        },
                    },
                    fuse_left,
                    sites: state
                        .sites
                        .iter()
                        .enumerate()
                        .map(|(index, &site)| (site_letter(index), site))
                        .collect(),
                    carrier: sd.carrier.map(|(_, peer)| peer),
                    action: sd.action,
                })
            }
        }
    }
}

fn site_letter(index: usize) -> char {
    char::from(b'A' + index as u8)
}

fn zone_view(capture: &Capture) -> ZoneView {
    ZoneView {
        owner: capture.owner,
        capturer: capture.capturer,
        progress: capture.percent(),
        contested: capture.contested,
    }
}

/// Run condition: a Match has started its objective and it still needs building.
pub(super) fn is_pending(runtime: Option<Res<ModeRuntime>>) -> bool {
    runtime.is_some_and(|runtime| matches!(*runtime, ModeRuntime::Pending))
}

type Players<'w, 's> =
    Query<'w, 's, (&'static PlayerPose, &'static Health, &'static PlayerTeam), With<ControlledBy>>;

/// Position and Team of every living player on a Team.
fn living(players: &Players) -> Vec<(Vec3, Team)> {
    players
        .iter()
        .filter(|(_, health, _)| !health.is_dead())
        .filter_map(|(pose, _, team)| team.0.map(|team| (pose.position, team)))
        .collect()
}

/// Whether `feet` is within `radius` of `point` on the ground plane and about level with it.
fn near(feet: Vec3, point: Vec3, radius: f32) -> bool {
    feet.xz().distance(point.xz()) <= radius && (feet.y - point.y).abs() <= 2.5
}

fn by_position(a: &Vec3, b: &Vec3) -> std::cmp::Ordering {
    a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z))
}

/// Builds the objective once the Match's Map is loaded: Flags, HQ sites or bomb sites from its
/// entities.
pub(super) fn init_mode(
    mut runtime: ResMut<ModeRuntime>,
    state: Single<&MatchState>,
    config: Res<ServerConfig>,
    data: Res<GameData>,
    roots: Query<(Entity, &MapRoot), With<MapReady>>,
    children: Query<&Children>,
    flags: Query<(&DomFlag, &Transform)>,
    hq_sites: Query<&Transform, With<HqSite>>,
    bomb_sites: Query<(&BombSite, &Transform)>,
) {
    if state.mode == GameModeId::TeamDeathmatch {
        *runtime = ModeRuntime::Nothing;
        return;
    }
    let Some((root, _)) = roots.iter().find(|(_, root)| root.id == state.map) else {
        return;
    };
    let entities: Vec<Entity> = children.iter_descendants(root).collect();
    let built = match state.mode {
        GameModeId::TeamDeathmatch => ModeRuntime::Nothing,
        GameModeId::Domination => {
            let mut found: Vec<(char, Vec3)> = entities
                .iter()
                .filter_map(|&e| flags.get(e).ok())
                .map(|(flag, transform)| (flag.label.letter(), transform.translation))
                .collect();
            found.sort_by_key(|(label, _)| *label);
            if found.is_empty() {
                warn!(
                    "Map \"{}\" has no Flags; Domination can't be played on it",
                    state.map
                );
                ModeRuntime::Nothing
            } else {
                info!("Domination: {} Flags", found.len());
                ModeRuntime::Dom(DomState::new(found))
            }
        }
        GameModeId::Headquarters => {
            let mut sites: Vec<Vec3> = entities
                .iter()
                .filter_map(|&e| hq_sites.get(e).ok())
                .map(|transform| transform.translation)
                .collect();
            sites.sort_by(by_position);
            if sites.is_empty() {
                warn!(
                    "Map \"{}\" has no HQ sites; Headquarters can't be played on it",
                    state.map
                );
                ModeRuntime::Nothing
            } else {
                info!("Headquarters: {} sites", sites.len());
                let hq = HqState::new(sites, 0, data.headquarters());
                info!("HQ appeared at site {}", hq.site + 1);
                ModeRuntime::Hq(hq)
            }
        }
        GameModeId::SearchAndDestroy => {
            let mut found: Vec<(char, Vec3)> = entities
                .iter()
                .filter_map(|&e| bomb_sites.get(e).ok())
                .map(|(site, transform)| (site.label.letter(), transform.translation))
                .collect();
            found.sort_by_key(|(label, _)| *label);
            if found.is_empty() {
                warn!(
                    "Map \"{}\" has no bomb sites; the bomb can't be planted",
                    state.map
                );
            }
            info!("Search & Destroy: {} bomb sites", found.len());
            let round_secs = config.limits(&data, GameModeId::SearchAndDestroy).1 as f32;
            ModeRuntime::Sd(Box::new(SdRuntime {
                state: SdState::new(found.into_iter().map(|(_, at)| at).collect(), Team::Alpha),
                round_secs,
                carrier: None,
                carrier_pos: Vec3::ZERO,
                action: None,
                alive: [0; 2],
            }))
        }
    };
    *runtime = built;
}

pub(super) fn tick_dom(
    mut runtime: ResMut<ModeRuntime>,
    mut state: Single<&mut MatchState>,
    players: Players,
    data: Res<GameData>,
    time: Res<Time>,
) {
    let ModeRuntime::Dom(dom) = &mut *runtime else {
        return;
    };
    if !is_live(&state) {
        return;
    }
    let tuning = data.domination();
    let living = living(&players);
    let presence: Vec<[u32; 2]> = dom
        .flags
        .iter()
        .map(|flag| {
            let mut here = [0; 2];
            for (position, team) in &living {
                if in_zone(*position, flag.position, &tuning.capture) {
                    here[team.index()] += 1;
                }
            }
            here
        })
        .collect();
    let tick = dom.tick(&presence, time.delta_secs(), tuning);
    for (index, event) in tick.events {
        let label = dom.flags[index].label;
        match event {
            CaptureEvent::Captured(team) => info!("Flag {label} captured by {team:?}"),
            CaptureEvent::Neutralized(owner) => {
                info!("Flag {label} neutralised (was {owner:?}'s)");
            }
        }
    }
    if tick.points != [0; 2] {
        for team in Team::ALL {
            state.scores[team.index()] += tick.points[team.index()];
        }
        info!(
            "Domination score: Alpha {} - Bravo {} (Flags held: Alpha {}, Bravo {})",
            state.scores[Team::Alpha.index()],
            state.scores[Team::Bravo.index()],
            dom.held_by(Team::Alpha),
            dom.held_by(Team::Bravo),
        );
    }
}

pub(super) fn tick_hq(
    mut runtime: ResMut<ModeRuntime>,
    mut state: Single<&mut MatchState>,
    players: Players,
    data: Res<GameData>,
    time: Res<Time>,
) {
    let ModeRuntime::Hq(hq) = &mut *runtime else {
        return;
    };
    if !is_live(&state) {
        return;
    }
    let tuning = data.headquarters();
    let mut presence = [0; 2];
    if let Some(position) = hq.position() {
        for (feet, team) in living(&players) {
            if in_zone(feet, position, &tuning.capture) {
                presence[team.index()] += 1;
            }
        }
    }
    let tick = hq.tick(presence, time.delta_secs(), tuning);
    match tick.event {
        Some(HqEvent::Captured(team)) => info!("HQ captured by {team:?}: they stop respawning"),
        Some(HqEvent::Destroyed(owner)) => {
            info!("HQ destroyed (was {owner:?}'s): everyone respawns, it relocates");
        }
        Some(HqEvent::Expired) => info!("HQ expired: everyone respawns, it relocates"),
        Some(HqEvent::Appeared(site)) => info!("HQ appeared at site {}", site + 1),
        None => {}
    }
    if tick.points != [0; 2] {
        for team in Team::ALL {
            state.scores[team.index()] += tick.points[team.index()];
        }
        debug!(
            "HQ score: Alpha {} - Bravo {}",
            state.scores[Team::Alpha.index()],
            state.scores[Team::Bravo.index()]
        );
    }
}

pub(super) fn tick_sd(
    mut runtime: ResMut<ModeRuntime>,
    mut state: Single<&mut MatchState>,
    mut players: Query<
        (
            Entity,
            &PlayerPose,
            &mut Health,
            &mut PlayerStats,
            &PlayerTeam,
            &PlayerOwner,
            &UseHeld,
            &Hitboxes,
        ),
        With<ControlledBy>,
    >,
    waiting: Query<&PlayerTeam, With<PendingRespawn>>,
    mut notices: Query<&mut MessageSender<BombExploded>, (With<ClientOf>, With<Connected>)>,
    data: Res<GameData>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let ModeRuntime::Sd(sd) = &mut *runtime else {
        return;
    };
    if !is_live(&state) {
        return;
    }
    let tuning = *data.search_and_destroy();
    let attackers = sd.state.attackers;
    let defenders = sd.state.defenders();

    let mut input = SdInput {
        spawned: waiting.iter().all(|team| team.0.is_none()),
        ..SdInput::default()
    };
    for (_, _, health, _, team, ..) in &players {
        if let Some(team) = team.0 {
            input.totals[team.index()] += 1;
            if !health.is_dead() {
                input.alive[team.index()] += 1;
            }
        }
    }

    if sd.state.is_live() {
        // The carrier dying (or leaving) drops the bomb where they last stood.
        if let Some((entity, _)) = sd.carrier {
            match players.get(entity) {
                Ok((_, pose, health, _, team, ..))
                    if !health.is_dead() && team.0 == Some(attackers) =>
                {
                    sd.carrier_pos = pose.position;
                }
                _ => {
                    let at = sd.carrier_pos;
                    sd.state.drop_bomb(at);
                    sd.carrier = None;
                    info!("The bomb was dropped at {at}");
                }
            }
        }
        // An attacker walking over it picks it up.
        if let Bomb::Dropped(at) = sd.state.bomb
            && let Some((entity, _, _, _, _, owner, ..)) =
                players.iter().find(|(_, pose, health, _, team, ..)| {
                    !health.is_dead()
                        && team.0 == Some(attackers)
                        && near(pose.position, at, tuning.pickup_radius)
                })
        {
            sd.carrier = Some((entity, owner.0));
            sd.state.pick_up();
            info!("An attacker picked up the bomb");
        }
    }

    // Who is planting or defusing.
    let mut actor = None;
    if sd.state.is_live() {
        match sd.state.bomb {
            Bomb::Carried => {
                if let Some((entity, peer)) = sd.carrier
                    && let Ok((_, pose, _, _, _, _, held, _)) = players.get(entity)
                    && held.0
                    && let Some(site) = sd
                        .state
                        .sites
                        .iter()
                        .position(|&site| near(pose.position, site, tuning.site_radius))
                {
                    input.planting_at = Some(site);
                    actor = Some((peer, true));
                }
            }
            Bomb::Planted { .. } => {
                if let Some(bomb) = sd.state.planted_at()
                    && let Some((_, _, _, _, _, owner, ..)) =
                        players
                            .iter()
                            .find(|(_, pose, health, _, team, _, held, _)| {
                                !health.is_dead()
                                    && team.0 == Some(defenders)
                                    && held.0
                                    && near(pose.position, bomb, tuning.site_radius)
                            })
                {
                    input.defusing = true;
                    actor = Some((owner.0, false));
                }
            }
            Bomb::Dropped(_) | Bomb::Gone => {}
        }
    }

    let events = sd
        .state
        .tick(&input, time.delta_secs(), sd.round_secs, &tuning);
    sd.alive = input.alive;
    sd.action = actor.and_then(|(peer, planting)| {
        let (done, secs) = if planting {
            (sd.state.plant, tuning.plant_secs)
        } else {
            (sd.state.defuse, tuning.defuse_secs)
        };
        (done > 0.0).then(|| ActionView {
            peer,
            planting,
            progress: (done / secs * 100.0).clamp(0.0, 100.0) as u8,
        })
    });

    for event in events {
        match event {
            SdEvent::RoundStarting {
                round,
                attackers_swapped,
            } => {
                for (entity, _, _, _, team, ..) in &players {
                    if team.0.is_some() {
                        commands
                            .entity(entity)
                            .insert(PendingRespawn::at(Duration::ZERO));
                    }
                }
                sd.carrier = None;
                info!(
                    "Search & Destroy round {round} starting: {:?} attack{}",
                    sd.state.attackers,
                    if attackers_swapped {
                        " (sides swapped)"
                    } else {
                        ""
                    }
                );
            }
            SdEvent::RoundLive => {
                let candidates: Vec<(Entity, PeerId, Vec3)> = players
                    .iter()
                    .filter(|(_, _, health, _, team, ..)| {
                        !health.is_dead() && team.0 == Some(sd.state.attackers)
                    })
                    .map(|(entity, pose, _, _, _, owner, ..)| (entity, owner.0, pose.position))
                    .collect();
                if candidates.is_empty() {
                    sd.state.bomb = Bomb::Gone;
                } else {
                    let pick =
                        RandomState::new().hash_one(time.elapsed()) as usize % candidates.len();
                    let (entity, peer, at) = candidates[pick];
                    sd.carrier = Some((entity, peer));
                    sd.carrier_pos = at;
                }
                info!(
                    "Round live: {}",
                    if sd.carrier.is_some() {
                        "an attacker carries the bomb"
                    } else {
                        "no attacker to carry the bomb"
                    }
                );
            }
            SdEvent::Planted { site } => {
                info!("Bomb planted at site {}", site_letter(site));
            }
            SdEvent::Defused => info!("Bomb defused"),
            SdEvent::Exploded { position } => {
                let mut killed = 0;
                for (_, pose, mut health, mut stats, _, _, _, hitboxes) in &mut players {
                    if !health.is_dead()
                        && (pose.position + Vec3::Y).distance(position) <= tuning.blast_radius
                    {
                        health.0 = 0.0;
                        stats.deaths += 1;
                        set_hitboxes_enabled(&mut commands, hitboxes, false);
                        killed += 1;
                    }
                }
                for mut sender in &mut notices {
                    sender.send::<EventChannel>(BombExploded { position });
                }
                info!("The bomb exploded at {position}, killing {killed} players");
            }
            SdEvent::RoundOver { winner, reason } => {
                state.scores[winner.index()] += 1;
                info!(
                    "Round won by {winner:?}: {}. Alpha {} - Bravo {}",
                    reason.text(true),
                    state.scores[Team::Alpha.index()],
                    state.scores[Team::Bravo.index()]
                );
                sd.carrier = None;
            }
        }
    }
    let seconds = sd.state.clock_secs(sd.round_secs);
    if state.seconds_left != seconds {
        state.seconds_left = seconds;
    }
}

/// Mirrors the objective into the replicated `ModeView`.
pub(super) fn publish_view(runtime: Res<ModeRuntime>, mut view: Single<&mut ModeView>) {
    let next = runtime.view();
    if **view != next {
        **view = next;
    }
}

/// Records each client's use-key state on its Player.
pub(super) fn receive_use(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<ObjectiveHold>)>,
    mut players: Query<&mut UseHeld>,
) {
    for (player, mut receiver) in &mut links {
        if let Some(ObjectiveHold(held)) = receiver.receive().last()
            && let Ok(mut current) = players.get_mut(player.0)
        {
            current.0 = held;
        }
    }
}
