//! Headquarters rules (pure): one HQ is active at a time, rotating through the Map's sites. A
//! team that captures it scores every interval until the other destroys it (stands on it, the
//! same as neutralising a Flag) or it runs out of time and relocates.

use bevy::math::Vec3;

use super::capture::{Capture, CaptureEvent};
use crate::data::HeadquartersTuning;
use crate::shared::Team;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HqPhase {
    /// At its site, with this many seconds to live.
    Active { life_left: f32 },
    /// Gone; the next one appears in `left` seconds.
    Relocating { left: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct HqState {
    sites: Vec<Vec3>,
    /// Index into `sites` of the current (or next) HQ.
    pub site: usize,
    pub phase: HqPhase,
    pub capture: Capture,
    score_timer: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HqEvent {
    Captured(Team),
    /// The enemy of the given (former) owner destroyed it.
    Destroyed(Team),
    /// Nobody owned it when its time ran out, or its owner held it to the end.
    Expired,
    /// The next HQ appeared at this site.
    Appeared(usize),
}

#[derive(Debug, Default, PartialEq)]
pub struct HqTick {
    pub points: [u32; 2],
    pub event: Option<HqEvent>,
}

impl HqState {
    /// `sites` must not be empty. The first HQ appears at `first`.
    pub fn new(sites: Vec<Vec3>, first: usize, tuning: &HeadquartersTuning) -> Self {
        assert!(!sites.is_empty(), "Headquarters needs at least one site");
        Self {
            site: first % sites.len(),
            sites,
            phase: HqPhase::Active {
                life_left: tuning.life_secs,
            },
            capture: Capture::default(),
            score_timer: 0.0,
        }
    }

    /// Where the HQ is, `None` while it relocates.
    pub fn position(&self) -> Option<Vec3> {
        matches!(self.phase, HqPhase::Active { .. }).then(|| self.sites[self.site])
    }

    /// The team holding the HQ, if any.
    pub fn owner(&self) -> Option<Team> {
        self.position().and(self.capture.owner)
    }

    /// `presence[team]`: living players of each team on the HQ.
    pub fn tick(&mut self, presence: [u32; 2], dt: f32, tuning: &HeadquartersTuning) -> HqTick {
        let mut tick = HqTick::default();
        match self.phase {
            HqPhase::Relocating { left } => {
                let left = left - dt;
                if left > 0.0 {
                    self.phase = HqPhase::Relocating { left };
                } else {
                    self.site = (self.site + 1) % self.sites.len();
                    self.phase = HqPhase::Active {
                        life_left: tuning.life_secs,
                    };
                    tick.event = Some(HqEvent::Appeared(self.site));
                }
            }
            HqPhase::Active { life_left } => {
                match self.capture.tick(presence, dt, &tuning.capture) {
                    Some(CaptureEvent::Captured(team)) => {
                        tick.event = Some(HqEvent::Captured(team))
                    }
                    Some(CaptureEvent::Neutralized(owner)) => {
                        tick.event = Some(HqEvent::Destroyed(owner));
                    }
                    None => {}
                }
                self.score_timer += dt;
                while self.score_timer >= tuning.score_interval_secs {
                    self.score_timer -= tuning.score_interval_secs;
                    if let Some(owner) = self.capture.owner {
                        tick.points[owner.index()] += tuning.points_per_tick;
                    }
                }
                let life_left = life_left - dt;
                if matches!(tick.event, Some(HqEvent::Destroyed(_))) {
                    self.relocate(tuning);
                } else if life_left <= 0.0 {
                    tick.event = Some(HqEvent::Expired);
                    self.relocate(tuning);
                } else {
                    self.phase = HqPhase::Active { life_left };
                }
            }
        }
        tick
    }

    fn relocate(&mut self, tuning: &HeadquartersTuning) {
        self.capture = Capture::default();
        self.score_timer = 0.0;
        self.phase = HqPhase::Relocating {
            left: tuning.relocate_secs,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modes::capture::test_tuning;

    fn tuning() -> HeadquartersTuning {
        HeadquartersTuning {
            capture: test_tuning(),
            score_interval_secs: 1.0,
            points_per_tick: 1,
            life_secs: 60.0,
            relocate_secs: 5.0,
        }
    }

    fn run(hq: &mut HqState, presence: [u32; 2], secs: f32) -> (u32, Vec<HqEvent>) {
        let (mut points, mut events) = (0, Vec::new());
        for _ in 0..(secs * 10.0).round() as u32 {
            let tick = hq.tick(presence, 0.1, &tuning());
            points += tick.points.iter().sum::<u32>();
            events.extend(tick.event);
        }
        (points, events)
    }

    #[test]
    fn capture_scores_per_second_until_destroyed_then_relocates() {
        let mut hq = HqState::new(vec![Vec3::ZERO, Vec3::X * 50.0], 0, &tuning());
        let (_, events) = run(&mut hq, [1, 0], 10.1);
        assert_eq!(events, [HqEvent::Captured(Team::Alpha)]);
        assert_eq!(hq.owner(), Some(Team::Alpha));
        let (points, _) = run(&mut hq, [0, 0], 10.0);
        assert_eq!(points, 10);
        // Bravo neutralises (5 s for one player) = destroyed.
        let (_, events) = run(&mut hq, [0, 1], 5.2);
        assert_eq!(events, [HqEvent::Destroyed(Team::Alpha)]);
        assert_eq!(hq.position(), None);
        assert_eq!(hq.owner(), None);
        let (_, events) = run(&mut hq, [0, 0], 5.1);
        assert_eq!(events, [HqEvent::Appeared(1)]);
        assert_eq!(hq.position(), Some(Vec3::X * 50.0));
    }

    #[test]
    fn an_untouched_hq_expires_after_its_life() {
        let mut hq = HqState::new(vec![Vec3::ZERO], 0, &tuning());
        let (_, events) = run(&mut hq, [0, 0], 60.1);
        assert_eq!(events, [HqEvent::Expired]);
    }
}
