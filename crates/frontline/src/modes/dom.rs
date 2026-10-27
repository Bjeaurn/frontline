//! Domination rules (pure): three Flags, each a capture zone; every interval each team scores
//! for the Flags it holds.

use bevy::math::Vec3;

use super::capture::{Capture, CaptureEvent};
use crate::data::DominationTuning;
use crate::shared::Team;

#[derive(Clone, Debug, PartialEq)]
pub struct Flag {
    /// A, B, C.
    pub label: char,
    pub position: Vec3,
    pub capture: Capture,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomState {
    pub flags: Vec<Flag>,
    score_timer: f32,
}

/// What one tick changed.
#[derive(Debug, Default, PartialEq)]
pub struct DomTick {
    /// Which Flag (index into `flags`) changed hands, and how.
    pub events: Vec<(usize, CaptureEvent)>,
    /// Points the teams scored this tick, indexed by `Team::index`.
    pub points: [u32; 2],
}

impl DomState {
    pub fn new(flags: Vec<(char, Vec3)>) -> Self {
        Self {
            flags: flags
                .into_iter()
                .map(|(label, position)| Flag {
                    label,
                    position,
                    capture: Capture::default(),
                })
                .collect(),
            score_timer: 0.0,
        }
    }

    /// `presence[flag][team]`: living players of each team on each Flag.
    pub fn tick(&mut self, presence: &[[u32; 2]], dt: f32, tuning: &DominationTuning) -> DomTick {
        let mut tick = DomTick::default();
        for (index, flag) in self.flags.iter_mut().enumerate() {
            let here = presence.get(index).copied().unwrap_or_default();
            if let Some(event) = flag.capture.tick(here, dt, &tuning.capture) {
                tick.events.push((index, event));
            }
        }
        self.score_timer += dt;
        while self.score_timer >= tuning.score_interval_secs {
            self.score_timer -= tuning.score_interval_secs;
            for team in Team::ALL {
                tick.points[team.index()] += self.held_by(team) * tuning.points_per_flag;
            }
        }
        tick
    }

    pub fn held_by(&self, team: Team) -> u32 {
        self.flags
            .iter()
            .filter(|flag| flag.capture.owner == Some(team))
            .count() as u32
    }

    /// Positions of the Flags `team` holds.
    pub fn held_positions(&self, team: Team) -> Vec<Vec3> {
        self.flags
            .iter()
            .filter(|flag| flag.capture.owner == Some(team))
            .map(|flag| flag.position)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modes::capture::test_tuning;

    #[test]
    fn each_held_flag_scores_one_point_every_five_seconds() {
        let tuning = DominationTuning {
            capture: test_tuning(),
            score_interval_secs: 5.0,
            points_per_flag: 1,
        };
        let mut dom = DomState::new(vec![('A', Vec3::ZERO), ('B', Vec3::X * 20.0)]);
        dom.flags[0].capture = Capture::owned_by(Team::Alpha);
        dom.flags[1].capture = Capture::owned_by(Team::Alpha);
        let mut points = [0; 2];
        for _ in 0..110 {
            let tick = dom.tick(&[[0, 0], [0, 0]], 0.1, &tuning);
            points[0] += tick.points[0];
            points[1] += tick.points[1];
        }
        // 11 s: the 5 s and 10 s marks, two Flags each.
        assert_eq!(points, [4, 0]);
        assert_eq!(dom.held_by(Team::Alpha), 2);
    }
}
