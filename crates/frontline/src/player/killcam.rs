//! The Kill-cam (CONTEXT.md): a player killed by someone else watches the last `killcam_secs`
//! (Match Rules) before their death through the killer's eyes, ending on the kill. SPACE or a
//! click skips it; the death screen follows.
//!
//! The server keeps no history (docs/adr/0002), so the client records what it shows, all the
//! time: every remote player's pose and whether they were drawn, our own pose and hitscan shots,
//! the shots, bullets and Grenade blasts the server reported, and the Grenades in the air. A death
//! by someone else replays that recording:
//! - the eye camera rides the killer's recorded eye, zoomed as far as their sight was;
//! - live soldiers step aside for stand-ins (`ReplayGhost`) posed from the recording, ourselves
//!   among them, and recorded Grenades fly again as stand-ins too;
//! - the recorded shots, bullets and blasts are handed to the effects and sounds as if they had
//!   just arrived from the server, while live ones are held back (smoke and gas, which linger
//!   and blind, are neither replayed nor held back);
//! - the killer's gun is drawn where ours would be (`ReplayViewmodel`), kicking on each shot.
//!
//! Hitscan shots replay when they reached us, a remote player's pose as interpolation showed it:
//! on a LAN the two are tens of milliseconds apart.

use std::collections::{HashMap, VecDeque};

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::prelude::*;
use bevy::text::LetterSpacing;
use lightyear::prelude::PeerId;

use super::controller::{EyeCamera, MenuOpen};
use super::life::LocalLife;
use super::remote::{FOLLOW_RATE, RemoteVisuals, SPEED_RATE, faction_index, pose_transform};
use super::soldier::{self, Gait, SoldierBones, Soldiers};
use crate::data::{ActiveRules, GameData, STEADY_MAGNIFICATION, WeaponId, ads_fov_scale};
use crate::equipment::{Effect, Grenade};
use crate::flow::{AppState, Settings};
use crate::maps::MapRegistry;
use crate::match_hud::is_friendly;
use crate::net::protocol::{
    DeathCause, FireShot, GrenadeDetonated, PlayerOwner, ProjectileFired, ProjectileLanded,
    ShotTracer,
};
use crate::shared::{
    FromServer, GameLayer, LocalPlayer, MatchPhase, MatchState, PlayerPose, PlayerTeam,
    RemotePlayer, ReplayGhost, Stance, Team, ToServer,
};
use crate::ui::{self, AMBER, UiFonts};
use crate::weapon::ReplayViewmodel;

/// The recording reaches back this far: the longest Kill-cam the Match Rules allow, plus slack.
const HISTORY_SECS: f64 = 11.0;
/// How long the kill-feed line naming the killer may take to arrive after `Health` says we died
/// before the Kill-cam gives up.
const CAUSE_WAIT_SECS: f64 = 1.0;
/// The replay runs this far past the moment `Health` said we died, so it holds the killing shot
/// (its tracer travels on another channel and may arrive a frame later).
const TAIL_SECS: f64 = 0.1;
/// A replay shorter than this (the killer had only just spawned) isn't worth showing.
const MIN_REPLAY_SECS: f64 = 0.5;

const KICKER_SIZE: f32 = 30.0;
const NAME_SIZE: f32 = 26.0;
const HINT_SIZE: f32 = 18.0;
const BAR_WIDTH: f32 = 320.0;

/// One recorded moment of a player: their pose and whether they were drawn (alive, at their
/// spawn).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sample {
    at: f64,
    pose: PlayerPose,
    shown: bool,
}

/// One player's recording, oldest first, and the Team they were last seen on.
#[derive(Debug, Default)]
struct Track {
    samples: VecDeque<Sample>,
    team: Option<Team>,
}

impl Track {
    fn push(&mut self, sample: Sample) {
        self.samples.push_back(sample);
    }

    fn prune(&mut self, before: f64) {
        while self
            .samples
            .front()
            .is_some_and(|sample| sample.at < before)
        {
            self.samples.pop_front();
        }
    }

    /// The player at time `at`: between two samples the poses blend (`lerp_pose`) and the
    /// earlier one says whether they were drawn; after the last sample they stay as last seen.
    /// `None` before the first.
    fn at(&self, at: f64) -> Option<Sample> {
        let next = self.samples.partition_point(|sample| sample.at <= at);
        let before = *self.samples.get(next.checked_sub(1)?)?;
        let Some(after) = self.samples.get(next) else {
            return Some(before);
        };
        let span = after.at - before.at;
        let t = if span > 0.0 {
            ((at - before.at) / span) as f32
        } else {
            1.0
        };
        Some(Sample {
            at,
            pose: crate::net::protocol::lerp_pose(before.pose, after.pose, t),
            shown: before.shown,
        })
    }

    /// When the life the player had at `at` began: the first sample of the run of drawn
    /// samples that holds the last drawn one at or before `at`. `None` if they were never drawn.
    fn life_start(&self, at: f64) -> Option<f64> {
        let upto = self.samples.partition_point(|sample| sample.at <= at);
        let last = self
            .samples
            .range(..upto)
            .rposition(|sample| sample.shown)?;
        let first = self
            .samples
            .range(..=last)
            .rposition(|sample| !sample.shown)
            .map_or(0, |gap| gap + 1);
        Some(self.samples[first].at)
    }
}

/// The span of the recording a Kill-cam shows, in recorded time: up to just past `died_at`,
/// starting `secs` before the death or when the killer last spawned, whichever is later.
/// `None` when there is too little of the killer to show.
fn replay_window(killer: &Track, died_at: f64, secs: f64) -> Option<(f64, f64)> {
    let to = died_at + TAIL_SECS;
    let oldest = killer.samples.front()?.at;
    let from = (died_at - secs).max(killer.life_start(to)?).max(oldest);
    (to - from >= MIN_REPLAY_SECS).then_some((from, to))
}

/// Something the server told us that the replay shows again.
#[derive(Clone, Copy, Debug)]
enum Event {
    Tracer(ShotTracer),
    Fired(ProjectileFired),
    Landed(ProjectileLanded),
    Blast(GrenadeDetonated),
}

/// The events due when the replay clock moves from `after` (exclusive) to `upto` (inclusive),
/// in the order they were recorded.
fn due_events(
    events: &VecDeque<(f64, Event)>,
    after: f64,
    upto: f64,
) -> impl Iterator<Item = &Event> {
    let start = events.partition_point(|(at, _)| *at <= after);
    events
        .range(start..)
        .take_while(move |(at, _)| *at <= upto)
        .map(|(_, event)| event)
}

/// Everything recorded for a Kill-cam, at most `HISTORY_SECS` back.
#[derive(Resource, Default, Debug)]
pub(super) struct History {
    players: HashMap<PeerId, Track>,
    me: Track,
    events: VecDeque<(f64, Event)>,
    /// The Grenades out at each recorded moment, by their (live) entity.
    grenades: VecDeque<(f64, Vec<(Entity, Grenade)>)>,
}

impl History {
    fn prune(&mut self, now: f64) {
        let before = now - HISTORY_SECS;
        for track in self.players.values_mut() {
            track.prune(before);
        }
        self.players.retain(|_, track| !track.samples.is_empty());
        self.me.prune(before);
        while self.events.front().is_some_and(|(at, _)| *at < before) {
            self.events.pop_front();
        }
        while self.grenades.front().is_some_and(|(at, _)| *at < before) {
            self.grenades.pop_front();
        }
    }

    /// The Grenades out at `at`: those of the last moment recorded at or before it.
    fn grenades_at(&self, at: f64) -> &[(Entity, Grenade)] {
        let next = self.grenades.partition_point(|(when, _)| *when <= at);
        next.checked_sub(1)
            .and_then(|last| self.grenades.get(last))
            .map_or(&[], |(_, grenades)| grenades.as_slice())
    }
}

/// A replay under way.
#[derive(Debug)]
struct Replay {
    killer: PeerId,
    /// The killer's name and what they used, for the caption.
    killer_name: String,
    how: String,
    /// The killer's Weapon, for the gun in view; `None` for a Grenade kill.
    weapon: Option<WeaponId>,
    /// Recorded time the replay covers.
    from: f64,
    to: f64,
    /// When it started playing (`Time::elapsed_secs_f64`).
    started: f64,
    /// Recorded time on screen now; events up to it have been handed out.
    clock: f64,
}

impl Replay {
    fn progress(&self) -> f32 {
        ((self.clock - self.from) / (self.to - self.from)).clamp(0.0, 1.0) as f32
    }
}

#[derive(Debug, Default)]
enum Phase {
    /// Alive, or out of play without a death to show.
    #[default]
    Idle,
    /// Just died: waiting for the kill-feed line that names the killer, and for the tail of
    /// the recording.
    Waiting,
    Playing(Box<Replay>),
    /// Shown, skipped or not to be shown: nothing more until the next life.
    Over,
}

/// The Kill-cam's state, for whatever steps aside while it plays (the death screen, damage
/// effects, name tags, spectating and the respawn request).
#[derive(Resource, Debug, Default)]
pub struct Killcam {
    phase: Phase,
    /// The press that skipped it this frame, which must not also ask to respawn.
    skipped_now: bool,
}

impl Killcam {
    pub fn is_playing(&self) -> bool {
        matches!(self.phase, Phase::Playing(_))
    }

    /// Whether SPACE and clicks belong to the Kill-cam this frame (playing, or just skipped).
    pub fn holds_respawn_input(&self) -> bool {
        self.is_playing() || self.skipped_now
    }

    fn replay(&self) -> Option<&Replay> {
        match &self.phase {
            Phase::Playing(replay) => Some(replay),
            _ => None,
        }
    }
}

/// Forgets the recording and any replay when leaving the game (the stand-ins go with the
/// game's other entities).
pub(super) fn reset_killcam(
    mut killcam: ResMut<Killcam>,
    mut history: ResMut<History>,
    mut stage: ResMut<Stage>,
    mut viewmodel: ResMut<ReplayViewmodel>,
) {
    *killcam = Killcam::default();
    *history = History::default();
    *stage = Stage::default();
    *viewmodel = ReplayViewmodel::default();
}

/// Who killed us and with what, for the caption: the killer's name, then the Weapon (with
/// HEADSHOT) or Grenade.
fn describe(data: &GameData, cause: &DeathCause) -> Option<(String, String, Option<WeaponId>)> {
    match cause {
        DeathCause::Shot {
            killer,
            weapon,
            headshot,
            ..
        } => Some((
            killer.clone(),
            format!(
                "{}{}",
                data.weapon(*weapon).name.to_uppercase(),
                if *headshot { "   HEADSHOT" } else { "" }
            ),
            Some(*weapon),
        )),
        DeathCause::Explosive { killer, kind, .. } => Some((
            killer.clone(),
            data.grenade(*kind).name.to_uppercase(),
            None,
        )),
        DeathCause::Melee { killer, .. } => Some((killer.clone(), "KNIFE".to_owned(), None)),
        DeathCause::Fall | DeathCause::Respawn => None,
    }
}

/// Starts a Kill-cam when someone else kills us, ends it when it has played out, is skipped
/// (SPACE or a click), we are back in play or the Match ends.
#[allow(clippy::too_many_arguments)]
pub(super) fn track_killcam(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    menu: Res<MenuOpen>,
    rules: ActiveRules,
    data: Res<GameData>,
    life: Res<LocalLife>,
    history: Res<History>,
    state: Query<&MatchState>,
    mut killcam: ResMut<Killcam>,
) {
    let now = time.elapsed_secs_f64();
    let killcam = &mut *killcam;
    killcam.skipped_now = false;
    let dead = life.is_dead() && life.team().is_some();
    let ended = state
        .single()
        .is_ok_and(|state| matches!(state.phase, MatchPhase::Ended { .. }));
    let secs = f64::from(rules.get().killcam_secs);
    let next = match &killcam.phase {
        Phase::Idle if dead && secs > 0.0 && !ended && !life.switched_team() => {
            Some(Phase::Waiting)
        }
        Phase::Idle => None,
        _ if !dead => Some(Phase::Idle),
        Phase::Waiting => {
            let killer = life.killer();
            if ended || (killer.is_none() && life.secs_dead(now) > CAUSE_WAIT_SECS) {
                Some(Phase::Over)
            } else if let Some(killer) = killer
                && now >= life.died_at() + TAIL_SECS
            {
                let shown = life.cause().and_then(|cause| describe(&data, cause));
                let window = history
                    .players
                    .get(&killer)
                    .and_then(|track| replay_window(track, life.died_at(), secs));
                Some(match (shown, window) {
                    (Some((killer_name, how, weapon)), Some((from, to))) => {
                        info!("Kill-cam: {:.1} s of {killer_name}", to - from);
                        Phase::Playing(Box::new(Replay {
                            killer,
                            killer_name,
                            how,
                            weapon,
                            from,
                            to,
                            started: now,
                            clock: from,
                        }))
                    }
                    _ => {
                        info!("Kill-cam: nothing recorded of the killer");
                        Phase::Over
                    }
                })
            } else {
                None
            }
        }
        Phase::Playing(replay) => {
            let skip = !menu.0
                && (keys.just_pressed(KeyCode::Space) || mouse.just_pressed(MouseButton::Left));
            killcam.skipped_now = skip;
            (skip || ended || replay.clock >= replay.to).then_some(Phase::Over)
        }
        Phase::Over => None,
    };
    if let Some(next) = next {
        killcam.phase = next;
    }
}

/// Grenade blasts the replay shows again: those that are over in a moment. Smoke and gas clouds
/// linger (and blind whoever is in them), so their live ones are left alone and recorded ones
/// aren't replayed.
fn replays_blast(data: &GameData, blast: &GrenadeDetonated) -> bool {
    !matches!(
        data.grenade(blast.kind).effect,
        Effect::Smoke { .. } | Effect::Gas { .. }
    )
}

/// Records the shots, bullets and blasts the server reports. While a Kill-cam plays it moves the
/// replay clock, holds back live shots and bullets and hands out the recorded ones that fall due,
/// as if they had just arrived (`FromServer`), counting the killer's shots for the gun in view.
#[allow(clippy::too_many_arguments)]
pub(super) fn feed_events(
    time: Res<Time>,
    data: Res<GameData>,
    mut killcam: ResMut<Killcam>,
    mut history: ResMut<History>,
    mut viewmodel: ResMut<ReplayViewmodel>,
    mut tracers: ResMut<Messages<FromServer<ShotTracer>>>,
    mut fired: ResMut<Messages<FromServer<ProjectileFired>>>,
    mut landed: ResMut<Messages<FromServer<ProjectileLanded>>>,
    mut blasts: ResMut<Messages<FromServer<GrenadeDetonated>>>,
    mut cursors: Local<(
        MessageCursor<FromServer<ShotTracer>>,
        MessageCursor<FromServer<ProjectileFired>>,
        MessageCursor<FromServer<ProjectileLanded>>,
        MessageCursor<FromServer<GrenadeDetonated>>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    let (tracer_cursor, fired_cursor, landed_cursor, blast_cursor) = &mut *cursors;
    let Phase::Playing(replay) = &mut killcam.phase else {
        let history = &mut *history;
        let events = &mut history.events;
        events.extend(
            tracer_cursor
                .read(&tracers)
                .map(|FromServer(m)| (now, Event::Tracer(*m))),
        );
        events.extend(
            fired_cursor
                .read(&fired)
                .map(|FromServer(m)| (now, Event::Fired(*m))),
        );
        events.extend(
            landed_cursor
                .read(&landed)
                .map(|FromServer(m)| (now, Event::Landed(*m))),
        );
        events.extend(
            blast_cursor
                .read(&blasts)
                .filter(|FromServer(m)| replays_blast(&data, m))
                .map(|FromServer(m)| (now, Event::Blast(*m))),
        );
        history.prune(now);
        return;
    };

    // Live shots and bullets wait: the world on screen is the recorded one.
    tracers.drain().for_each(drop);
    fired.drain().for_each(drop);
    landed.drain().for_each(drop);
    let clock = (replay.from + (now - replay.started)).min(replay.to);
    for event in due_events(&history.events, replay.clock, clock) {
        match *event {
            Event::Tracer(tracer) => {
                if tracer.shooter == replay.killer && tracer.pellet == 0 {
                    viewmodel.shots += 1;
                }
                tracers.write(FromServer(tracer));
            }
            Event::Fired(shot) => {
                if shot.shooter == replay.killer {
                    viewmodel.shots += 1;
                }
                fired.write(FromServer(shot));
            }
            Event::Landed(landing) => {
                landed.write(FromServer(landing));
            }
            Event::Blast(blast) => {
                blasts.write(FromServer(blast));
            }
        }
    }
    replay.clock = clock;
    // What was handed out is not news to record once the replay is over.
    tracer_cursor.clear(&tracers);
    fired_cursor.clear(&fired);
    landed_cursor.clear(&landed);
    blast_cursor.clear(&blasts);
}

/// Records every remote player's pose (as drawn), our own pose and hitscan shots, and the
/// Grenades out. Paused while a replay plays.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_poses(
    time: Res<Time>,
    data: Res<GameData>,
    life: Res<LocalLife>,
    killcam: Res<Killcam>,
    mut history: ResMut<History>,
    remotes: Query<
        (
            &PlayerOwner,
            &PlayerPose,
            Option<&PlayerTeam>,
            &RemoteVisuals,
        ),
        With<RemotePlayer>,
    >,
    me: Query<&PlayerPose, With<LocalPlayer>>,
    grenades: Query<(Entity, &Grenade), Without<ReplayGhost>>,
    mut shots: MessageReader<ToServer<FireShot>>,
    spatial: SpatialQuery,
) {
    if killcam.is_playing() {
        shots.clear();
        return;
    }
    let now = time.elapsed_secs_f64();
    let history = &mut *history;
    for (owner, pose, team, visuals) in &remotes {
        let track = history.players.entry(owner.0).or_default();
        track.team = team.and_then(|team| team.0);
        track.push(Sample {
            at: now,
            pose: *pose,
            shown: visuals.shown(),
        });
    }
    if let Ok(pose) = me.single() {
        history.me.team = life.team();
        history.me.push(Sample {
            at: now,
            pose: *pose,
            shown: !life.is_dead(),
        });
    }
    // Our hitscan shots, as others saw them: the server sends us no tracers of our own.
    // Bullets (projectile Weapons) come back from the server for everyone, us included.
    let world = SpatialQueryFilter::from_mask(GameLayer::World);
    for ToServer(shot) in shots.read() {
        let stats = data.weapon(shot.weapon);
        let (Some(peer), None, Ok(direction)) =
            (life.peer(), stats.projectile, Dir3::new(shot.direction))
        else {
            continue;
        };
        let reach = spatial
            .cast_ray(shot.origin, direction, stats.range, true, &world)
            .map_or(stats.range, |hit| hit.distance);
        history.events.push_back((
            now,
            Event::Tracer(ShotTracer {
                shooter: peer,
                weapon: shot.weapon,
                attachments: shot.attachments,
                origin: shot.origin,
                end: shot.origin + *direction * reach,
                pellet: shot.pellet,
            }),
        ));
    }
    let out: Vec<(Entity, Grenade)> = grenades
        .iter()
        .map(|(entity, grenade)| (entity, *grenade))
        .collect();
    if !out.is_empty()
        || history
            .grenades
            .back()
            .is_some_and(|(_, last)| !last.is_empty())
    {
        history.grenades.push_back((now, out));
    }
}

/// A soldier standing in for a recorded player during a replay.
#[derive(Debug)]
struct Ghost {
    root: Entity,
    soldier: Entity,
    gait: Gait,
    speed: f32,
    last: Option<Vec3>,
}

/// The stand-ins of the replay on screen: soldiers by player (`None` is us) and Grenades by the
/// live entity they were recorded from.
#[derive(Resource, Default, Debug)]
pub(super) struct Stage {
    soldiers: HashMap<Option<PeerId>, Ghost>,
    grenades: HashMap<Entity, Entity>,
}

impl Stage {
    fn clear(&mut self, commands: &mut Commands) {
        for ghost in self.soldiers.drain().map(|(_, ghost)| ghost.root) {
            commands.entity(ghost).try_despawn();
        }
        for ghost in self.grenades.drain().map(|(_, ghost)| ghost) {
            commands.entity(ghost).try_despawn();
        }
    }
}

/// Plays the replay: the eye camera at the killer's recorded eye, a stand-in soldier for every
/// other recorded player (us included) and Grenade, and the killer's gun in view. Clears it all
/// away once it is over.
#[allow(clippy::too_many_arguments)]
pub(super) fn play_killcam(
    time: Res<Time>,
    (data, maps, settings): (Res<GameData>, Res<MapRegistry>, Res<Settings>),
    life: Res<LocalLife>,
    killcam: Res<Killcam>,
    history: Res<History>,
    soldiers: Res<Soldiers>,
    mut stage: ResMut<Stage>,
    mut viewmodel: ResMut<ReplayViewmodel>,
    state: Query<&MatchState>,
    local: Query<&PlayerPose, With<LocalPlayer>>,
    mut camera: Query<(Entity, &mut Projection), With<EyeCamera>>,
    bones: Query<&SoldierBones>,
    mut transforms: Query<&mut Transform>,
    mut visibilities: Query<&mut Visibility>,
    mut replayed_grenades: Query<&mut Grenade, With<ReplayGhost>>,
    mut commands: Commands,
) {
    let Some(replay) = killcam.replay() else {
        stage.clear(&mut commands);
        if *viewmodel != ReplayViewmodel::default() {
            *viewmodel = ReplayViewmodel::default();
        }
        return;
    };
    let dt = time.delta_secs();
    let at = replay.clock;
    let killer = history
        .players
        .get(&replay.killer)
        .and_then(|track| track.at(at));

    // The camera at the killer's eye, zoomed as far as their sight was.
    if let (Some(killer), Ok(local), Ok((eye, mut projection))) =
        (killer, local.single(), camera.single_mut())
    {
        let pose = killer.pose;
        if let Ok(mut transform) = transforms.get_mut(eye) {
            let eye_at = pose.position + Vec3::Y * pose.stance.eye_height();
            let look = Quat::from_rotation_y(pose.yaw) * Quat::from_rotation_x(pose.pitch);
            let body = Quat::from_rotation_y(local.yaw);
            transform.translation = body.inverse() * (eye_at - local.position);
            transform.rotation = body.inverse() * look;
        }
        let zoom = if pose.aim_magnification > 0.0 {
            ads_fov_scale(pose.aim_magnification)
        } else {
            1.0
        };
        if let Projection::Perspective(perspective) = &mut *projection {
            let half = (settings.fov_deg * zoom).to_radians() / 2.0;
            perspective.fov = 2.0 * (half.tan() / perspective.aspect_ratio).atan();
        }
        let wanted = ReplayViewmodel {
            weapon: replay.weapon,
            scoped: pose.aim_magnification >= STEADY_MAGNIFICATION,
            shots: viewmodel.shots,
        };
        if *viewmodel != wanted {
            *viewmodel = wanted;
        }
    }

    // Everyone else, as recorded.
    let match_state = state.single().ok();
    let cast = history
        .players
        .iter()
        .filter(|(peer, _)| **peer != replay.killer)
        .map(|(peer, track)| (Some(*peer), track))
        .chain(std::iter::once((None, &history.me)));
    for (who, track) in cast {
        let Some(sample) = track.at(at) else {
            continue;
        };
        let ghost = stage.soldiers.entry(who).or_insert_with(|| {
            let friendly = who.is_none() || is_friendly(life.team(), track.team);
            let faction = faction_index(&data, &maps, match_state, track.team);
            let root = commands
                .spawn((
                    Name::new("Kill-cam soldier"),
                    ReplayGhost,
                    DespawnOnExit(AppState::InGame),
                    pose_transform(&sample.pose),
                    Visibility::Hidden,
                ))
                .id();
            let soldier = soldiers.spawn(
                &mut commands,
                root,
                faction,
                friendly,
                soldier::pose(sample.pose.stance, Gait::default(), sample.pose.pitch),
                false,
            );
            Ghost {
                root,
                soldier,
                gait: Gait::default(),
                speed: 0.0,
                last: None,
            }
        });
        pose_ghost(ghost, &sample, dt, &data, &bones, &mut transforms);
        // We stand to the end: the replay runs a moment past our death to catch the shot.
        let shown = sample.shown || (who.is_none() && at >= life.died_at());
        if let Ok(mut visibility) = visibilities.get_mut(ghost.root) {
            visibility.set_if_neq(if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }

    // The Grenades out at this moment of the recording.
    let out = history.grenades_at(at);
    stage.grenades.retain(|recorded, ghost| {
        let keep = out.iter().any(|(entity, _)| entity == recorded);
        if !keep {
            commands.entity(*ghost).try_despawn();
        }
        keep
    });
    for (recorded, grenade) in out {
        match stage.grenades.get(recorded) {
            Some(ghost) => {
                if let Ok(mut replayed) = replayed_grenades.get_mut(*ghost) {
                    *replayed = *grenade;
                }
            }
            None => {
                let ghost = commands
                    .spawn((
                        Name::new("Kill-cam grenade"),
                        ReplayGhost,
                        DespawnOnExit(AppState::InGame),
                        *grenade,
                    ))
                    .id();
                stage.grenades.insert(*recorded, ghost);
            }
        }
    }
}

/// Moves a stand-in soldier to its recorded pose and poses its limbs (stance, walk cycle from how
/// fast it moves, aim).
fn pose_ghost(
    ghost: &mut Ghost,
    sample: &Sample,
    dt: f32,
    data: &GameData,
    bones: &Query<&SoldierBones>,
    transforms: &mut Query<&mut Transform>,
) {
    let pose = &sample.pose;
    let moved = ghost
        .last
        .filter(|last| last.distance(pose.position) < crate::shared::TELEPORT_DISTANCE)
        .map_or(0.0, |last| (pose.position - last).xz().length());
    ghost.last = Some(pose.position);
    if dt > 0.0 {
        ghost.speed.smooth_nudge(&(moved / dt), SPEED_RATE, dt);
    }
    let (walk, sprint) = match pose.stance {
        Stance::Standing => (data.movement.walk_speed, data.movement.sprint_speed),
        Stance::Crouching => (data.movement.crouch_speed, f32::MAX),
        Stance::Prone => (data.movement.prone_speed, f32::MAX),
    };
    ghost.gait.advance(ghost.speed, walk, sprint, dt);
    if let Ok(mut transform) = transforms.get_mut(ghost.root) {
        let target = pose_transform(pose);
        if transform.translation.distance(target.translation) > crate::shared::TELEPORT_DISTANCE {
            *transform = target;
        } else {
            transform
                .translation
                .smooth_nudge(&target.translation, FOLLOW_RATE * 2.0, dt);
            transform
                .rotation
                .smooth_nudge(&target.rotation, FOLLOW_RATE * 2.0, dt);
        }
    }
    let Ok(soldier) = bones.get(ghost.soldier) else {
        return;
    };
    let locals = soldier
        .rest
        .locals(&soldier::pose(pose.stance, ghost.gait, pose.pitch));
    for (&bone, target) in soldier.bones.iter().zip(&locals) {
        if let Ok(mut limb) = transforms.get_mut(bone) {
            *limb = *target;
        }
    }
}

#[derive(Component)]
pub(super) struct KillcamScreen;

/// The text lines and progress bar of the Kill-cam screen.
#[derive(Component, PartialEq)]
pub(super) enum KillcamLine {
    /// The killer's name.
    Killer,
    /// Their Weapon or Grenade.
    How,
    /// The filled part of the progress bar.
    Progress,
}

/// A black fade at the top or bottom edge (letterbox), like the death screen's.
fn letterbox(to_bottom: bool) -> impl Bundle {
    let (from, to) = if to_bottom { (0.85, 0.0) } else { (0.0, 0.85) };
    (
        Node {
            width: percent(100.0),
            height: percent(16.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(6.0),
            ..default()
        },
        BackgroundGradient::from(LinearGradient::new(
            LinearGradient::TO_BOTTOM,
            vec![
                ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, from), 0.0),
                ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, to), 100.0),
            ],
        )),
    )
}

pub(super) fn spawn_killcam_screen(mut commands: Commands, fonts: Res<UiFonts>) {
    commands.spawn((
        Name::new("Kill-cam screen"),
        DespawnOnExit(AppState::InGame),
        KillcamScreen,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
        Visibility::Hidden,
        children![
            (
                letterbox(false),
                children![(
                    ui::hud_text(&fonts.display, "KILLCAM", KICKER_SIZE, AMBER),
                    LetterSpacing::Px(8.0),
                )],
            ),
            (
                letterbox(true),
                children![
                    (
                        KillcamLine::Killer,
                        ui::hud_text(&fonts.bold, "", NAME_SIZE, Color::WHITE),
                        LetterSpacing::Px(2.0),
                    ),
                    (
                        KillcamLine::How,
                        ui::hud_text(&fonts.bold, "", HINT_SIZE, ui::TEXT),
                        LetterSpacing::Px(2.0),
                    ),
                    (
                        Node {
                            width: px(BAR_WIDTH),
                            height: px(3.0),
                            margin: UiRect::vertical(px(4.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.2)),
                        children![(
                            KillcamLine::Progress,
                            Node {
                                width: percent(0.0),
                                height: percent(100.0),
                                ..default()
                            },
                            BackgroundColor(AMBER),
                        )],
                    ),
                    (
                        ui::hud_text(
                            &fonts.bold,
                            "PRESS SPACE OR CLICK TO SKIP",
                            HINT_SIZE,
                            Color::srgba(1.0, 1.0, 1.0, 0.7),
                        ),
                        LetterSpacing::Px(2.0),
                    ),
                ],
            ),
        ],
    ));
}

/// Shows the Kill-cam screen while it plays: who, with what, and how far along.
pub(super) fn update_killcam_screen(
    killcam: Res<Killcam>,
    mut screen: Single<&mut Visibility, With<KillcamScreen>>,
    mut lines: Query<(&KillcamLine, Option<&mut Text>, &mut Node)>,
) {
    let Some(replay) = killcam.replay() else {
        screen.set_if_neq(Visibility::Hidden);
        return;
    };
    screen.set_if_neq(Visibility::Inherited);
    for (line, text, mut node) in &mut lines {
        match (line, text) {
            (KillcamLine::Killer, Some(mut text)) => {
                if text.0 != replay.killer_name {
                    text.0.clone_from(&replay.killer_name);
                }
            }
            (KillcamLine::How, Some(mut text)) => {
                if text.0 != replay.how {
                    text.0.clone_from(&replay.how);
                }
            }
            (KillcamLine::Progress, _) => {
                node.width = percent(100.0 * replay.progress());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at: f64, x: f32, shown: bool) -> Sample {
        Sample {
            at,
            pose: PlayerPose {
                position: Vec3::new(x, 0.0, 0.0),
                ..default()
            },
            shown,
        }
    }

    fn track(samples: &[Sample]) -> Track {
        Track {
            samples: samples.iter().copied().collect(),
            team: None,
        }
    }

    #[test]
    fn a_track_blends_between_samples_and_holds_after_the_last() {
        let track = track(&[sample(1.0, 0.0, true), sample(2.0, 1.0, false)]);
        assert_eq!(track.at(0.5), None, "nothing before the recording");
        let middle = track.at(1.25).unwrap();
        assert!((middle.pose.position.x - 0.25).abs() < 1e-5);
        assert!(middle.shown, "the earlier sample says whether drawn");
        assert_eq!(track.at(5.0).unwrap().pose.position.x, 1.0);
        assert!(!track.at(5.0).unwrap().shown);
    }

    #[test]
    fn the_window_ends_just_after_the_death_and_starts_at_most_secs_before() {
        let samples: Vec<Sample> = (0..=100)
            .map(|i| sample(f64::from(i) * 0.1, 0.0, true))
            .collect();
        let (from, to) = replay_window(&track(&samples), 9.0, 5.0).unwrap();
        assert!((from - 4.0).abs() < 1e-9);
        assert!((to - (9.0 + TAIL_SECS)).abs() < 1e-9);
        // Not before the recording starts.
        let (from, _) = replay_window(&track(&samples), 3.0, 5.0).unwrap();
        assert_eq!(from, 0.0);
    }

    #[test]
    fn the_window_never_shows_the_killers_previous_life() {
        // Dead from 3 to 4, then back.
        let samples: Vec<Sample> = (0..=100)
            .map(|i| {
                let at = f64::from(i) * 0.1;
                sample(at, 0.0, !(3.0..4.0).contains(&at))
            })
            .collect();
        let (from, _) = replay_window(&track(&samples), 7.0, 5.0).unwrap();
        assert!((from - 4.0).abs() < 1e-6, "starts at the respawn, {from}");
    }

    #[test]
    fn too_little_of_the_killer_shows_nothing() {
        let samples = [sample(9.8, 0.0, false), sample(9.9, 0.0, true)];
        assert_eq!(replay_window(&track(&samples), 10.0, 5.0), None);
        let never = [sample(1.0, 0.0, false), sample(9.0, 0.0, false)];
        assert_eq!(replay_window(&track(&never), 10.0, 5.0), None);
        assert_eq!(replay_window(&Track::default(), 10.0, 5.0), None);
    }

    #[test]
    fn events_fall_due_once_and_in_order() {
        let landing = |id| {
            Event::Landed(ProjectileLanded {
                id,
                position: Vec3::ZERO,
                struck: true,
            })
        };
        let events: VecDeque<(f64, Event)> = [
            (1.0, landing(1)),
            (2.0, landing(2)),
            (2.0, landing(3)),
            (3.0, landing(4)),
        ]
        .into_iter()
        .collect();
        let ids = |after, upto| -> Vec<u32> {
            due_events(&events, after, upto)
                .map(|event| match event {
                    Event::Landed(landing) => landing.id,
                    _ => unreachable!(),
                })
                .collect()
        };
        assert_eq!(ids(0.0, 1.0), [1]);
        assert_eq!(ids(1.0, 2.5), [2, 3]);
        assert_eq!(ids(2.5, 2.9), Vec::<u32>::new());
        assert_eq!(ids(2.9, 10.0), [4]);
    }

    #[test]
    fn grenades_are_those_of_the_last_moment_recorded() {
        let mut history = History::default();
        let grenade = Grenade {
            kind: crate::equipment::GrenadeKind::Frag,
            position: Vec3::ONE,
            armed: false,
        };
        history
            .grenades
            .push_back((1.0, vec![(Entity::PLACEHOLDER, grenade)]));
        history.grenades.push_back((2.0, Vec::new()));
        assert!(history.grenades_at(0.5).is_empty());
        assert_eq!(history.grenades_at(1.5).len(), 1);
        assert!(history.grenades_at(2.5).is_empty());
    }

    #[test]
    fn the_history_forgets_what_is_too_old() {
        let mut history = History::default();
        let peer = PeerId::Netcode(7);
        history
            .players
            .entry(peer)
            .or_default()
            .push(sample(0.0, 0.0, true));
        history.me.push(sample(0.0, 0.0, true));
        history.me.push(sample(20.0, 0.0, true));
        history.prune(20.0);
        assert!(!history.players.contains_key(&peer));
        assert_eq!(history.me.samples.len(), 1);
    }
}
