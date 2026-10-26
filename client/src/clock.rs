//! Time, counted in ticks (m5-plan Decision 3): when the next tick is due,
//! how far a frame is between two ticks, and the time of day.
//!
//! **Fixed ticks, smooth frames** (worth teaching). The game steps in ticks of
//! 50 ms, as the server does: jump height, fall speed and breaking time are
//! all defined per tick. Frames come at the display's rate, which is faster or
//! slower and never in step. So each frame draws the player between where it
//! was at the last two ticks, `alpha` of the way from the older to the newer,
//! where alpha is how far the frame is into the tick that follows them. The
//! picture runs up to one tick behind the simulation; that's the price of
//! never stuttering. Mouse look skips it, and turns the camera every frame.

use std::time::{Duration, Instant};

/// One tick, as the server counts them.
pub const TICK: Duration = Duration::from_millis(50);
/// After a stall, at most this many overdue ticks run back to back. The rest
/// are skipped: a burst of inputs would only make the server's player jump.
pub const MAX_CATCH_UP: u32 = 3;
/// The length of a day, in ticks.
pub const DAY: i64 = 24_000;

/// When ticks are due.
#[derive(Clone, Copy, Debug)]
pub struct Ticks {
    next: Instant,
}

impl Ticks {
    /// The first tick is due at `start`.
    #[must_use]
    pub const fn new(start: Instant) -> Self {
        Self { next: start }
    }

    /// How many ticks to run at `now`: the ones that are due, at most
    /// [`MAX_CATCH_UP`]. After a longer stall the rest are dropped, and the
    /// next tick is a whole tick away.
    pub fn due(&mut self, now: Instant) -> u32 {
        let mut due = 0;
        while now >= self.next {
            if due == MAX_CATCH_UP {
                self.next = now + TICK;
                break;
            }
            self.next += TICK;
            due += 1;
        }
        due
    }

    /// When the next tick is due.
    #[must_use]
    pub const fn next(&self) -> Instant {
        self.next
    }

    /// How far `now` is past the last tick that ran: 0 just after it, close
    /// to 1 just before the next. Frames draw the player this far from the
    /// last tick's pose to this tick's.
    #[must_use]
    pub fn alpha(&self, now: Instant) -> f64 {
        let to_next = self.next.saturating_duration_since(now);
        (1.0 - to_next.as_secs_f64() / TICK.as_secs_f64()).clamp(0.0, 1.0)
    }
}

/// The time of day: ticks into a 24,000-tick day, where 0 is sunrise and
/// 6,000 is noon.
///
/// The server says what it is with `SetTime`, at join and every 20 ticks
/// after (Pumpkin's `send_game_time_sync`), and the client counts on by
/// itself in between. Pumpkin doesn't tell Bedrock clients whether the day
/// is moving (its `GameRulesChanged` carries no values, and it never sends
/// it), so the client works it out: two `SetTime`s a while apart with the
/// same time mean the day stands still.
#[derive(Clone, Copy, Debug, Default)]
pub struct TimeOfDay {
    time: Option<i64>,
    frozen: bool,
    /// The last `SetTime`, and the game tick it arrived on.
    last_set: Option<(i64, u64)>,
}

impl TimeOfDay {
    /// The server's time, arriving on game tick `tick`.
    pub fn set(&mut self, time: i64, tick: u64) {
        let time = time.rem_euclid(DAY);
        if let Some((last, last_tick)) = self.last_set
            && tick > last_tick
        {
            self.frozen = time == last;
        }
        self.last_set = Some((time, tick));
        self.time = Some(time);
    }

    /// A tick passes.
    pub fn tick(&mut self) {
        if !self.frozen
            && let Some(time) = &mut self.time
        {
            *time = (*time + 1) % DAY;
        }
    }

    /// The time of day, once the server has said.
    #[must_use]
    pub const fn get(&self) -> Option<i64> {
        self.time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn ticks_come_every_50_ms_whatever_the_frames() {
        let start = Instant::now();
        let mut ticks = Ticks::new(start);
        assert_eq!(ticks.due(start), 1, "the first tick is due at once");
        // Frames at about 165 Hz: a tick every 8 or so.
        let mut ran = 0;
        for frame in 1..=330 {
            ran += ticks.due(start + Duration::from_micros(frame * 6_061));
        }
        assert_eq!(ran, 40, "two seconds of frames run 40 more ticks");
        assert_eq!(ticks.next(), start + ms(2_050));
    }

    #[test]
    fn after_a_stall_three_ticks_catch_up_and_the_rest_are_dropped() {
        let start = Instant::now();
        let mut ticks = Ticks::new(start);
        assert_eq!(ticks.due(start), 1);
        // A 1-second stall: 20 ticks are overdue.
        let late = start + ms(1_000);
        assert_eq!(ticks.due(late), MAX_CATCH_UP);
        assert_eq!(ticks.next(), late + TICK, "the next tick is a whole tick away");
        assert_eq!(ticks.due(late + ms(49)), 0);
        assert_eq!(ticks.due(late + ms(50)), 1);
        // Two overdue ticks catch up in full.
        assert_eq!(ticks.due(late + ms(150)), 2);
    }

    #[test]
    fn alpha_is_how_far_a_frame_is_into_the_tick() {
        let start = Instant::now();
        let mut ticks = Ticks::new(start);
        ticks.due(start);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(close(ticks.alpha(start), 0.0));
        assert!(close(ticks.alpha(start + ms(20)), 0.4));
        assert!(close(ticks.alpha(start + ms(45)), 0.9));
        // A frame drawn late, before the overdue tick has run, stays at 1:
        // it never draws past the newest pose.
        assert!(close(ticks.alpha(start + ms(80)), 1.0));
    }

    #[test]
    fn the_day_counts_on_between_set_times_and_can_stand_still() {
        let mut day = TimeOfDay::default();
        day.tick();
        assert_eq!(day.get(), None, "unknown until the server says");
        day.set(1_000, 0);
        for _ in 0..20 {
            day.tick();
        }
        assert_eq!(day.get(), Some(1_020));
        // The server agrees, a second later: the day is moving.
        day.set(1_020, 20);
        day.tick();
        assert_eq!(day.get(), Some(1_021));
        // It says the same time again a second later: the day stands still.
        day.set(1_020, 40);
        day.tick();
        assert_eq!(day.get(), Some(1_020));
        // Midnight wraps round to the next day.
        day.set(23_999, 60);
        day.tick();
        assert_eq!(day.get(), Some(0));
        // A negative time is still a time of day.
        day.set(-1, 80);
        assert_eq!(day.get(), Some(23_999));
    }
}
