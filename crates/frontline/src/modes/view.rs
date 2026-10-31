//! What the server tells every client about the running Game Mode: a snapshot (`ModeView`) on
//! the Match entity, rewritten only when something a player can see changes (percentages and
//! whole seconds, not raw floats).

use bevy::prelude::*;
use lightyear::prelude::PeerId;
use serde::{Deserialize, Serialize};

use super::sd::RoundEnd;
use crate::shared::Team;

/// The objective state of the current Match; `None` in a Game Mode without one.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum ModeView {
    #[default]
    None,
    /// The Map's fixed ammo crates, in Team Deathmatch.
    TeamDeathmatch(Vec<CrateView>),
    /// The Flags in label order.
    Domination(Vec<FlagView>),
    Headquarters(HqView),
    SearchAndDestroy(SdView),
}

/// A capture zone as players see it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneView {
    pub owner: Option<Team>,
    /// Whose progress `progress` is.
    pub capturer: Option<Team>,
    /// Percent through capturing or neutralising.
    pub progress: u8,
    pub contested: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlagView {
    pub label: char,
    pub position: Vec3,
    pub zone: ZoneView,
    /// A player of the holding Team stands at the Flag's ammo crate: it is lit.
    pub resupplying: bool,
}

/// A fixed ammo crate as players see it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrateView {
    pub position: Vec3,
    /// Open and ready, not closed on its respawn timer.
    pub ready: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HqView {
    /// `None` while the next HQ is on its way.
    pub position: Option<Vec3>,
    pub zone: ZoneView,
    /// Seconds until it relocates, or until the next one appears.
    pub seconds_left: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SdPhaseView {
    Waiting,
    Starting,
    Live,
    Over { winner: Team, reason: RoundEnd },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum BombView {
    /// Not in play (before the round, or spent).
    Gone,
    Carried,
    Dropped(Vec3),
    Planted {
        site: char,
        position: Vec3,
    },
}

/// A player planting or defusing right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionView {
    pub peer: PeerId,
    pub planting: bool,
    pub progress: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SdView {
    pub round: u32,
    pub attackers: Team,
    pub phase: SdPhaseView,
    /// Living players per team, indexed by `Team::index`.
    pub alive: [u32; 2],
    pub bomb: BombView,
    /// Whole seconds of fuse left once planted.
    pub fuse_left: u32,
    /// The bomb sites, A first.
    pub sites: Vec<(char, Vec3)>,
    pub carrier: Option<PeerId>,
    pub action: Option<ActionView>,
}
