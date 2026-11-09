//! Where an entity is drawn: a little in the past, between two of the server's poses
//! (m5-plan Decision 13; worth teaching).
//!
//! **Why the past.** The server moves an entity once a tick, 20 times a second, and the
//! window draws far more often. Drawing each move as it arrives would make every mob
//! jump along in steps. So each entity keeps its last few poses, with when each arrived,
//! and a frame draws it as it was [`DELAY`] ago. By then the pose after that moment has
//! almost always arrived too, so the frame blends the two either side of it, and the
//! motion is smooth. The price is that everything else is drawn [`DELAY`] late.
//!
//! **Why a tick and a margin.** Moves come a tick apart, but not exactly: the server's
//! tick runs late sometimes, and the client only reads its packets once a frame. A delay
//! of exactly one tick would often find no pose after its moment, and the entity would
//! stop for a frame, then jump. The margin covers that. It's measured, not guessed: the
//! log reports how far apart moves arrive (`Entities::log`).
//!
//! **Standing still sends nothing.** The server only sends what changed. So when a mob
//! stops, the next move can come seconds later, and blending from the old pose to it
//! would slide the mob slowly all that time. A move after a long gap is taken to mean
//! the entity stood still until a tick before it.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use glam::DVec3;

use crate::clock::TICK;

/// How far in the past entities are drawn: a tick, and a margin for the jitter in when
/// the server's moves arrive.
pub const DELAY: Duration = Duration::from_millis(75);

/// A gap between two moves longer than this means the entity stood still in between.
pub const STILL: Duration = Duration::from_millis(75);

/// The most poses an entity keeps. A frame needs the two around [`DELAY`] ago, and at a
/// move a tick, that's never more than the last three.
const KEPT: usize = 8;

/// Where an entity is, and which way it faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// Where its feet are, in blocks.
    pub feet: DVec3,
    /// In degrees, as Bedrock sends them: yaw 0 faces +Z (south), and grows clockwise
    /// seen from above.
    pub yaw: f32,
    pub pitch: f32,
    pub head_yaw: f32,
}

impl Pose {
    /// Part of the way from `self` to `to`: 0 is `self`, and 1 is `to`.
    #[must_use]
    pub fn lerp(&self, to: &Self, t: f64) -> Self {
        Self {
            feet: self.feet.lerp(to.feet, t),
            yaw: lerp_degrees(self.yaw, to.yaw, t as f32),
            pitch: lerp_degrees(self.pitch, to.pitch, t as f32),
            head_yaw: lerp_degrees(self.head_yaw, to.head_yaw, t as f32),
        }
    }
}

/// Part of the way between two angles, the short way round: from 350° to 10° goes
/// through 0°, not back through 180°.
fn lerp_degrees(from: f32, to: f32, t: f32) -> f32 {
    let turn = (to - from).rem_euclid(360.0);
    let turn = if turn > 180.0 { turn - 360.0 } else { turn };
    from + turn * t
}

/// An entity's last few poses, oldest first, with when each arrived.
#[derive(Clone, Debug)]
pub struct Track {
    poses: VecDeque<(Instant, Pose)>,
}

impl Track {
    #[must_use]
    pub fn new(at: Instant, pose: Pose) -> Self {
        Self {
            poses: VecDeque::from([(at, pose)]),
        }
    }

    /// The newest pose the server sent.
    #[must_use]
    pub fn latest(&self) -> Pose {
        self.poses.back().map_or_else(|| unreachable!("a track always has a pose"), |(_, pose)| *pose)
    }

    /// A move from the server, which arrived `at`. Returns how long after the one before
    /// it came.
    pub fn moved(&mut self, at: Instant, pose: Pose) -> Duration {
        let (last_at, last) = *self.poses.back().unwrap_or_else(|| unreachable!("a track always has a pose"));
        let gap = at.saturating_duration_since(last_at);
        if gap > STILL
            && let Some(held_until) = at.checked_sub(TICK)
        {
            self.poses.push_back((held_until, last));
        }
        self.poses.push_back((at, pose));
        while self.poses.len() > KEPT {
            self.poses.pop_front();
        }
        gap
    }

    /// A teleport: drawn there at once, with nothing blended from where it was.
    pub fn jumped(&mut self, at: Instant, pose: Pose) {
        self.poses.clear();
        self.poses.push_back((at, pose));
    }

    /// Where to draw it in a frame at `now`: where it was [`DELAY`] before.
    #[must_use]
    pub fn drawn(&self, now: Instant) -> Pose {
        self.at(now.checked_sub(DELAY).unwrap_or(now))
    }

    /// Where it was at `when`: between the poses either side, or the oldest or newest
    /// when `when` is outside them. Past the newest, it stays there: nothing is guessed.
    #[must_use]
    pub fn at(&self, when: Instant) -> Pose {
        let Some(next) = self.poses.iter().position(|(at, _)| *at > when) else {
            return self.latest();
        };
        if next == 0 {
            return self.poses[0].1;
        }
        let (from_at, from) = self.poses[next - 1];
        let (to_at, to) = self.poses[next];
        let span = to_at.duration_since(from_at).as_secs_f64();
        let t = if span > 0.0 { when.duration_since(from_at).as_secs_f64() / span } else { 1.0 };
        from.lerp(&to, t)
    }

    /// Forgets the poses no frame from `now` on will want: all but the newest one older
    /// than [`DELAY`].
    pub fn forget_old(&mut self, now: Instant) {
        let Some(drawn_at) = now.checked_sub(DELAY) else {
            return;
        };
        while self.poses.len() > 1 && self.poses[1].0 <= drawn_at {
            self.poses.pop_front();
        }
    }

    /// How many poses it keeps.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.poses.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_x(x: f64) -> Pose {
        Pose {
            feet: DVec3::new(x, 64.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            head_yaw: 0.0,
        }
    }

    const MS: Duration = Duration::from_millis(1);

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_pose_halfway_between_two_updates() {
        let start = Instant::now();
        let mut track = Track::new(start, at_x(0.0));
        track.moved(start + 50 * MS, at_x(1.0));
        assert!(close(track.at(start + 25 * MS).feet.x, 0.5));
        assert!(close(track.at(start + 40 * MS).feet.x, 0.8));
    }

    #[test]
    fn a_frame_draws_the_pose_of_the_delay_before() {
        let start = Instant::now();
        let mut track = Track::new(start, at_x(0.0));
        track.moved(start + 50 * MS, at_x(1.0));
        track.moved(start + 100 * MS, at_x(2.0));
        // 75 ms after the second move, the frame shows the second move exactly...
        assert!(close(track.drawn(start + 125 * MS).feet.x, 1.0));
        // ...and 25 ms later, halfway to the third, which has long arrived.
        assert!(close(track.drawn(start + 150 * MS).feet.x, 1.5));
        // Before anything is a delay old, it shows the first.
        assert!(close(track.drawn(start + 10 * MS).feet.x, 0.0));
    }

    #[test]
    fn past_the_newest_pose_it_stays_there() {
        let start = Instant::now();
        let mut track = Track::new(start, at_x(0.0));
        track.moved(start + 50 * MS, at_x(1.0));
        assert!(close(track.drawn(start + 500 * MS).feet.x, 1.0));
    }

    #[test]
    fn after_standing_still_it_moves_only_in_the_last_tick() {
        let start = Instant::now();
        let mut track = Track::new(start, at_x(0.0));
        let gap = track.moved(start + 1000 * MS, at_x(1.0));
        assert_eq!(gap, 1000 * MS);
        // Still where it was until a tick before the move...
        assert!(close(track.at(start + 900 * MS).feet.x, 0.0));
        assert!(close(track.at(start + 950 * MS).feet.x, 0.0));
        // ...and then over that tick.
        assert!(close(track.at(start + 975 * MS).feet.x, 0.5));
        // A gap of a tick doesn't count as standing still.
        let gap = track.moved(start + 1050 * MS, at_x(2.0));
        assert_eq!(gap, 50 * MS);
        assert!(close(track.at(start + 1025 * MS).feet.x, 1.5));
    }

    #[test]
    fn angles_turn_the_short_way() {
        let start = Instant::now();
        let mut facing = at_x(0.0);
        facing.yaw = 350.0;
        let mut track = Track::new(start, facing);
        facing.yaw = 10.0;
        track.moved(start + 50 * MS, facing);
        let yaw = track.at(start + 25 * MS).yaw.rem_euclid(360.0);
        assert!(yaw.abs() < 1e-3 || (yaw - 360.0).abs() < 1e-3, "{yaw}");
        assert!((lerp_degrees(10.0, 350.0, 0.25) - 5.0).abs() < 1e-4);
        assert!((lerp_degrees(-170.0, 170.0, 0.5) - -180.0).abs() < 1e-4);
    }

    #[test]
    fn a_teleport_blends_nothing() {
        let start = Instant::now();
        let mut track = Track::new(start, at_x(0.0));
        track.moved(start + 50 * MS, at_x(1.0));
        track.jumped(start + 100 * MS, at_x(100.0));
        assert!(close(track.at(start + 60 * MS).feet.x, 100.0));
        assert_eq!(track.len(), 1);
    }

    #[test]
    fn old_poses_are_forgotten_but_the_two_a_frame_needs_stay() {
        let start = Instant::now();
        let mut track = Track::new(start, at_x(0.0));
        for i in 1..=6 {
            track.moved(start + i * 50 * MS, at_x(i as f64));
        }
        let now = start + 300 * MS;
        let before = track.drawn(now);
        track.forget_old(now);
        // Drawn 75 ms back, at 225 ms: between the moves at 200 and 250.
        assert_eq!(track.len(), 3);
        assert_eq!(track.drawn(now), before);
        assert!(close(before.feet.x, 4.5));
    }

    #[test]
    fn no_more_than_kept_poses() {
        let start = Instant::now();
        let mut track = Track::new(start, at_x(0.0));
        for i in 1..100 {
            track.moved(start + i * 50 * MS, at_x(i as f64));
        }
        assert_eq!(track.len(), KEPT);
        assert!(close(track.latest().feet.x, 99.0));
    }
}
