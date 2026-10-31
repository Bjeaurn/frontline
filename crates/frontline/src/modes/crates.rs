//! The fixed ammo crates of Team Deathmatch (pure): a player walking up to a ready crate is fully
//! resupplied and the crate closes for `respawn_secs`, then opens again. Only Team Deathmatch has
//! them: the other Game Modes ignore the Map's `ammo_crate` entities.

use bevy::math::Vec3;

use super::capture::in_cylinder;
use crate::data::{AmmoCrateTuning, GameModeId, GameModeSettings};

/// How far above a crate's origin a player's feet may be and still reach it, metres.
const REACH_HEIGHT: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrateSlot {
    pub position: Vec3,
    /// Seconds until it opens again; 0 when it is ready.
    closed_left: f32,
}

impl CrateSlot {
    pub fn ready(&self) -> bool {
        self.closed_left <= 0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AmmoCrates {
    pub slots: Vec<CrateSlot>,
}

/// What one tick did to the crates.
#[derive(Debug, PartialEq, Eq)]
pub struct CrateTick<T> {
    /// Crates (by index) a player resupplied from, and who.
    pub taken: Vec<(usize, T)>,
    /// Crates that opened again.
    pub reopened: Vec<usize>,
}

impl<T> Default for CrateTick<T> {
    fn default() -> Self {
        Self {
            taken: Vec::new(),
            reopened: Vec::new(),
        }
    }
}

impl AmmoCrates {
    /// The crates a Match of `mode` has at `positions` (the Map's `ammo_crate` entities): ready,
    /// in Team Deathmatch only. `None` for every other mode, whatever the Map places, and for a
    /// Map with no crates.
    pub fn for_mode(
        mode: GameModeId,
        settings: &GameModeSettings,
        positions: impl IntoIterator<Item = Vec3>,
    ) -> Option<Self> {
        if mode != GameModeId::TeamDeathmatch || settings.ammo_crates.is_none() {
            return None;
        }
        let slots: Vec<CrateSlot> = positions
            .into_iter()
            .map(|position| CrateSlot {
                position,
                closed_left: 0.0,
            })
            .collect();
        (!slots.is_empty()).then_some(Self { slots })
    }

    /// Advances by `dt` seconds with `players` (an id and the feet of each living player). The
    /// first player in reach of each ready crate takes it, closing it; a closed crate counts
    /// down and opens again (it takes nobody on the tick it opens).
    pub fn tick<T: Copy>(
        &mut self,
        players: &[(T, Vec3)],
        dt: f32,
        tuning: &AmmoCrateTuning,
    ) -> CrateTick<T> {
        let mut tick = CrateTick::default();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if !slot.ready() {
                slot.closed_left -= dt;
                if slot.ready() {
                    slot.closed_left = 0.0;
                    tick.reopened.push(index);
                }
                continue;
            }
            let taker = players
                .iter()
                .find(|(_, feet)| in_cylinder(*feet, slot.position, tuning.radius, REACH_HEIGHT));
            if let Some(&(who, _)) = taker {
                slot.closed_left = tuning.respawn_secs;
                tick.taken.push((index, who));
            }
        }
        tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::GameData;

    const TUNING: AmmoCrateTuning = AmmoCrateTuning {
        radius: 1.5,
        respawn_secs: 30.0,
    };

    fn crates() -> AmmoCrates {
        AmmoCrates {
            slots: vec![
                CrateSlot {
                    position: Vec3::new(10.0, 0.0, 0.0),
                    closed_left: 0.0,
                },
                CrateSlot {
                    position: Vec3::new(-10.0, 0.0, 0.0),
                    closed_left: 0.0,
                },
            ],
        }
    }

    #[test]
    fn only_team_deathmatch_has_crates() {
        let data = GameData::load();
        let at = || [Vec3::ZERO, Vec3::X];
        for mode in GameModeId::ALL {
            let made = AmmoCrates::for_mode(mode, data.game_mode(mode), at());
            assert_eq!(
                made.is_some(),
                mode == GameModeId::TeamDeathmatch,
                "{mode:?}"
            );
        }
        // Even tuning the other modes' settings with crates doesn't turn them on.
        let mut settings = data.game_mode(GameModeId::Headquarters).clone();
        settings.ammo_crates = Some(TUNING);
        assert!(AmmoCrates::for_mode(GameModeId::Headquarters, &settings, at()).is_none());
        // And a Map without any has none to run.
        let none = AmmoCrates::for_mode(
            GameModeId::TeamDeathmatch,
            data.game_mode(GameModeId::TeamDeathmatch),
            [],
        );
        assert!(none.is_none());
    }

    #[test]
    fn walking_up_to_a_ready_crate_resupplies_and_closes_it() {
        let mut crates = crates();
        let near = Vec3::new(10.5, 0.0, 0.5);
        let tick = crates.tick(&[(7_u32, near)], 0.1, &TUNING);
        assert_eq!(tick.taken, [(0, 7)]);
        assert!(!crates.slots[0].ready() && crates.slots[1].ready());
        let again = crates.tick(&[(7, near)], 0.1, &TUNING);
        assert!(again.taken.is_empty(), "a closed crate gives nothing");
    }

    #[test]
    fn a_crate_needs_you_within_its_radius_and_level_with_it() {
        let mut crates = crates();
        let tick = crates.tick(
            &[
                (1_u32, Vec3::new(8.0, 0.0, 0.0)),
                (2, Vec3::new(10.0, 6.0, 0.0)),
                (3, Vec3::new(10.0, -3.0, 0.0)),
            ],
            0.1,
            &TUNING,
        );
        assert!(
            tick.taken.is_empty(),
            "too far, a floor above, a level below"
        );
        let edge = crates.tick(&[(4, Vec3::new(11.5, 0.0, 0.0))], 0.1, &TUNING);
        assert_eq!(edge.taken, [(0, 4)], "the radius edge counts");
    }

    #[test]
    fn two_players_share_nothing_one_takes_the_crate() {
        let mut crates = crates();
        let tick = crates.tick(
            &[
                (1_u32, Vec3::new(10.2, 0.0, 0.0)),
                (2, Vec3::new(9.8, 0.0, 0.0)),
            ],
            0.1,
            &TUNING,
        );
        assert_eq!(tick.taken, [(0, 1)]);
    }

    #[test]
    fn a_closed_crate_opens_again_after_its_respawn_time() {
        let mut crates = crates();
        let near = Vec3::new(10.0, 0.0, 0.0);
        crates.tick(&[(1_u32, near)], 0.1, &TUNING);
        let mut reopened_at = None;
        for step in 1..=400 {
            let tick = crates.tick::<u32>(&[], 0.1, &TUNING);
            if tick.reopened == [0] {
                reopened_at = Some(step as f32 * 0.1);
                break;
            }
        }
        let at = reopened_at.expect("it reopens");
        assert!((at - 30.0).abs() < 0.3, "after about 30 s, not {at}");
        assert!(crates.slots[0].ready());
        // Standing there as it opens takes it on the next tick, not the same one.
        let mut standing = crates.clone();
        standing.slots[1].closed_left = 0.05;
        let tick = standing.tick(&[(9_u32, Vec3::new(-10.0, 0.0, 0.0))], 0.1, &TUNING);
        assert_eq!((tick.taken.len(), tick.reopened.as_slice()), (0, &[1][..]));
        let next = standing.tick(&[(9, Vec3::new(-10.0, 0.0, 0.0))], 0.1, &TUNING);
        assert_eq!(next.taken, [(1, 9)]);
    }
}
