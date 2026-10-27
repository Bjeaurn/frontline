//! The capture rules shared by Domination Flags and the Headquarters: pure state, no Bevy.
//!
//! One team alone on a zone makes progress (`n` players count `min(n, max_speedup)` times a
//! lone player); both teams on it is contested and nothing moves. A zone the enemy holds is
//! neutralised first, then captured. A team that walks onto a zone the other is part-way through
//! taking first rolls that progress back.

use bevy::prelude::*;

use crate::data::CaptureTuning;
use crate::shared::Team;

/// A capturable zone's state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Capture {
    /// Who holds it (and scores for it).
    pub owner: Option<Team>,
    /// Whose progress `progress` is: capturing a neutral zone, or neutralising an owned one.
    pub capturer: Option<Team>,
    /// 0..1 through the current capture or neutralisation.
    pub progress: f32,
    /// Both teams are on the zone, so `progress` is frozen.
    pub contested: bool,
}

/// A change of ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureEvent {
    /// The team took a neutral zone.
    Captured(Team),
    /// The previous owner lost the zone; it is neutral now.
    Neutralized(Team),
}

impl Capture {
    #[cfg(test)]
    pub fn owned_by(owner: Team) -> Self {
        Self {
            owner: Some(owner),
            ..Self::default()
        }
    }

    /// Advances by `dt` seconds with `presence[team]` living players of each team on the zone.
    pub fn tick(
        &mut self,
        presence: [u32; 2],
        dt: f32,
        tuning: &CaptureTuning,
    ) -> Option<CaptureEvent> {
        let [alpha, bravo] = presence;
        self.contested = alpha > 0 && bravo > 0;
        let team = match (alpha > 0, bravo > 0) {
            (true, false) => Team::Alpha,
            (false, true) => Team::Bravo,
            _ => return None,
        };
        let speed = (presence[team.index()] as f32).min(tuning.max_speedup);
        if let Some(other) = self.capturer.filter(|&capturer| capturer != team) {
            // Undo the other team's progress at the pace it was made.
            let secs = self.phase_secs(other, tuning);
            self.progress -= dt * speed / secs;
            if self.progress <= 0.0 {
                self.progress = 0.0;
                self.capturer = None;
            }
            return None;
        }
        if self.owner == Some(team) {
            return None;
        }
        self.capturer = Some(team);
        self.progress += dt * speed / self.phase_secs(team, tuning);
        if self.progress < 1.0 {
            return None;
        }
        self.progress = 0.0;
        match self.owner.take() {
            Some(previous) => Some(CaptureEvent::Neutralized(previous)),
            None => {
                self.owner = Some(team);
                self.capturer = None;
                Some(CaptureEvent::Captured(team))
            }
        }
    }

    /// Seconds one player needs for what `team` is doing now: neutralising an owned zone, else
    /// capturing a neutral one.
    fn phase_secs(&self, team: Team, tuning: &CaptureTuning) -> f32 {
        let secs = if self.owner.is_some_and(|owner| owner != team) {
            tuning.neutralize_secs
        } else {
            tuning.capture_secs
        };
        secs.max(f32::EPSILON)
    }

    /// Progress in whole percent, for replication.
    pub fn percent(&self) -> u8 {
        (self.progress.clamp(0.0, 1.0) * 100.0).round() as u8
    }
}

/// Whether a player standing at `feet` is inside the zone cylinder around `center`.
pub fn in_zone(feet: Vec3, center: Vec3, tuning: &CaptureTuning) -> bool {
    feet.xz().distance(center.xz()) <= tuning.zone_radius
        && feet.y >= center.y - 0.5
        && feet.y <= center.y + tuning.zone_height
}

#[cfg(test)]
pub(super) fn test_tuning() -> CaptureTuning {
    CaptureTuning {
        capture_secs: 10.0,
        neutralize_secs: 5.0,
        max_speedup: 3.0,
        zone_radius: 3.5,
        zone_height: 3.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(capture: &mut Capture, presence: [u32; 2], secs: f32) -> Vec<CaptureEvent> {
        let tuning = test_tuning();
        let steps = (secs * 10.0).round() as u32;
        (0..steps)
            .filter_map(|_| capture.tick(presence, 0.1, &tuning))
            .collect()
    }

    #[test]
    fn a_lone_player_captures_in_ten_seconds_and_three_are_capped_at_triple_speed() {
        let mut flag = Capture::default();
        assert!(run(&mut flag, [1, 0], 9.5).is_empty());
        assert_eq!(
            run(&mut flag, [1, 0], 1.0),
            [CaptureEvent::Captured(Team::Alpha)]
        );
        let mut fast = Capture::default();
        // Five players count as three: 10 s / 3 = 3.33 s.
        assert!(run(&mut fast, [5, 0], 3.0).is_empty());
        assert_eq!(
            run(&mut fast, [5, 0], 0.5),
            [CaptureEvent::Captured(Team::Alpha)]
        );
    }

    #[test]
    fn contested_freezes_progress_and_an_enemy_zone_is_neutralised_first() {
        let mut flag = Capture::owned_by(Team::Bravo);
        run(&mut flag, [1, 0], 2.0);
        let before = flag.progress;
        assert!(before > 0.3);
        run(&mut flag, [1, 1], 5.0);
        assert!(flag.contested);
        assert_eq!(flag.progress, before);
        // 5 s to neutralise in total, then 10 s to capture.
        assert_eq!(
            run(&mut flag, [1, 0], 3.2),
            [CaptureEvent::Neutralized(Team::Bravo)]
        );
        assert_eq!(flag.owner, None);
        assert_eq!(
            run(&mut flag, [1, 0], 10.1),
            [CaptureEvent::Captured(Team::Alpha)]
        );
    }

    #[test]
    fn defenders_roll_back_part_made_progress() {
        let mut flag = Capture::default();
        run(&mut flag, [0, 1], 5.0);
        assert_eq!(flag.capturer, Some(Team::Bravo));
        run(&mut flag, [1, 0], 5.5);
        // Bravo's half-made capture is gone; Alpha has only just started its own.
        assert_ne!(flag.capturer, Some(Team::Bravo));
        assert!(flag.progress < 0.1);
        assert_eq!(flag.owner, None);
    }
}
