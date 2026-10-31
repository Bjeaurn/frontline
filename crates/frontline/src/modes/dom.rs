//! Domination rules (pure): three Flags, each a capture zone; every interval each team scores
//! for the Flags it holds, and the ammo crate at a held Flag resupplies the holding Team.

use bevy::math::Vec3;

use super::capture::{Capture, CaptureEvent, in_cylinder};
use crate::data::DominationTuning;
use crate::shared::Team;

#[derive(Clone, Debug, PartialEq)]
pub struct Flag {
    /// A, B, C.
    pub label: char,
    pub position: Vec3,
    pub capture: Capture,
    /// A player of the holding Team stands at the Flag's ammo crate: it is lit and resupplying.
    pub crate_lit: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomState {
    pub flags: Vec<Flag>,
    score_timer: f32,
    resupply_timer: f32,
}

/// What one tick changed.
#[derive(Debug, Default, PartialEq)]
pub struct DomTick {
    /// Which Flag (index into `flags`) changed hands, and how.
    pub events: Vec<(usize, CaptureEvent)>,
    /// Points the teams scored this tick, indexed by `Team::index`.
    pub points: [u32; 2],
    /// The crates' pulse is due: everyone at a crate of their Team gets their share now.
    pub resupply: bool,
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
                    crate_lit: false,
                })
                .collect(),
            score_timer: 0.0,
            resupply_timer: 0.0,
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
        // One pulse per interval, however long the tick: a hitch doesn't pay twice.
        self.resupply_timer += dt;
        if self.resupply_timer >= tuning.resupply.interval_secs {
            self.resupply_timer %= tuning.resupply.interval_secs;
            tick.resupply = true;
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

    /// The Flag whose ammo crate resupplies a living player of `team` standing at `feet`: one
    /// `team` holds (a neutral or enemy Flag never resupplies) and within the resupply radius.
    pub fn resupply_flag(
        &self,
        team: Team,
        feet: Vec3,
        tuning: &DominationTuning,
    ) -> Option<usize> {
        self.flags.iter().position(|flag| {
            flag.capture.owner == Some(team)
                && in_cylinder(
                    feet,
                    flag.position,
                    tuning.resupply.radius,
                    tuning.capture.zone_height,
                )
        })
    }

    /// Lights the crate of every Flag a living player of its holding Team stands at, and darkens
    /// the others. `living` is the position and Team of each living player.
    pub fn light_crates(&mut self, living: &[(Vec3, Team)], tuning: &DominationTuning) {
        let lit: Vec<bool> = (0..self.flags.len())
            .map(|index| {
                living
                    .iter()
                    .any(|&(feet, team)| self.resupply_flag(team, feet, tuning) == Some(index))
            })
            .collect();
        for (flag, lit) in self.flags.iter_mut().zip(lit) {
            flag.crate_lit = lit;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ResupplyTuning;
    use crate::modes::capture::test_tuning;

    fn tuning() -> DominationTuning {
        DominationTuning {
            capture: test_tuning(),
            score_interval_secs: 5.0,
            points_per_flag: 1,
            resupply: ResupplyTuning {
                radius: 4.0,
                interval_secs: 2.0,
                share: 0.1,
            },
        }
    }

    #[test]
    fn each_held_flag_scores_one_point_every_five_seconds() {
        let tuning = tuning();
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

    #[test]
    fn a_flag_crate_resupplies_only_the_team_that_holds_the_flag() {
        let tuning = tuning();
        let mut dom = DomState::new(vec![('A', Vec3::ZERO), ('B', Vec3::X * 40.0)]);
        dom.flags[0].capture = Capture::owned_by(Team::Alpha);
        // B is neutral.
        let at_a = Vec3::new(1.0, 0.0, 1.0);
        assert_eq!(dom.resupply_flag(Team::Alpha, at_a, &tuning), Some(0));
        assert_eq!(
            dom.resupply_flag(Team::Bravo, at_a, &tuning),
            None,
            "the enemy at your Flag gets nothing"
        );
        assert_eq!(
            dom.resupply_flag(Team::Alpha, Vec3::X * 40.0, &tuning),
            None,
            "a neutral Flag has no crate in use"
        );

        // Once Bravo takes A, the crate is theirs.
        dom.flags[0].capture = Capture::owned_by(Team::Bravo);
        assert_eq!(dom.resupply_flag(Team::Alpha, at_a, &tuning), None);
        assert_eq!(dom.resupply_flag(Team::Bravo, at_a, &tuning), Some(0));
    }

    #[test]
    fn the_crate_reaches_as_far_as_the_resupply_radius_and_not_through_floors() {
        let tuning = tuning();
        let mut dom = DomState::new(vec![('A', Vec3::new(0.0, 2.0, 0.0))]);
        dom.flags[0].capture = Capture::owned_by(Team::Alpha);
        let at = |x: f32, y: f32| dom.resupply_flag(Team::Alpha, Vec3::new(x, y, 0.0), &tuning);
        assert_eq!(at(4.0, 2.0), Some(0), "the edge counts");
        assert_eq!(at(4.1, 2.0), None, "just outside");
        assert_eq!(at(0.0, 7.5), None, "a floor above the Flag");
        assert_eq!(at(0.0, 1.6), Some(0), "a little below is a slope");
        assert_eq!(at(0.0, 1.0), None, "well below is another level");
    }

    #[test]
    fn the_crate_is_lit_only_while_its_team_stands_there() {
        let tuning = tuning();
        let mut dom = DomState::new(vec![('A', Vec3::ZERO), ('B', Vec3::X * 40.0)]);
        dom.flags[0].capture = Capture::owned_by(Team::Alpha);
        dom.flags[1].capture = Capture::owned_by(Team::Alpha);
        dom.light_crates(&[(Vec3::X * 2.0, Team::Bravo)], &tuning);
        assert!(!dom.flags[0].crate_lit, "only the enemy is at A");
        dom.light_crates(
            &[(Vec3::X * 2.0, Team::Bravo), (Vec3::X * 38.0, Team::Alpha)],
            &tuning,
        );
        assert_eq!(
            [dom.flags[0].crate_lit, dom.flags[1].crate_lit],
            [false, true],
            "an Alpha at B lights B"
        );
        dom.light_crates(&[], &tuning);
        assert!(!dom.flags[1].crate_lit, "nobody there, dark again");
    }

    #[test]
    fn the_crates_pulse_once_per_interval() {
        let tuning = tuning();
        let mut dom = DomState::new(vec![('A', Vec3::ZERO)]);
        // 11 s at 2 s a pulse: the 2, 4, 6, 8 and 10 s marks.
        let pulses = (0..110)
            .filter(|_| dom.tick(&[[0, 0]], 0.1, &tuning).resupply)
            .count();
        assert_eq!(pulses, 5);
        // A long hitch is one pulse, not a catch-up burst.
        let mut hitched = DomState::new(vec![('A', Vec3::ZERO)]);
        assert!(hitched.tick(&[[0, 0]], 7.0, &tuning).resupply);
        assert!(!hitched.tick(&[[0, 0]], 0.1, &tuning).resupply);
    }
}
