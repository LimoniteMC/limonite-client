//! Edits the client shows before the server has agreed to them (m5-plan Decision
//! 9; worth teaching).
//!
//! **Why predict.** The server handles input at the start of its next tick, and
//! sends block changes at the end of one, so its answer comes 50 to 100 ms after
//! the click: three to six frames at 60 Hz. Waiting for it makes building feel
//! like wading. So the client changes its own world at once, and notes down what
//! it did.
//!
//! **The server still has the last word.** While a prediction stands, a server
//! `UpdateBlock` for that block is noted, not drawn, because an update already on
//! its way can be older than the edit, and drawing it would flicker. The entry
//! goes when the server says what was predicted. If it hasn't by the deadline,
//! the world takes the server's last word, whatever that is, so a refused edit
//! snaps back.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use glam::IVec3;

/// How long a prediction stands without the server agreeing, in ticks: half a
/// second, for now.
pub const DEADLINE: u64 = 10;

struct Entry {
    /// What the world shows. `None` for an edit nothing is shown for until the
    /// server answers, which is only here to be timed.
    predicted: Option<u32>,
    /// The server's last word about this block.
    server: u32,
    /// The water in the block's second layer before the edit, which a break
    /// empties.
    water: Option<u32>,
    deadline: u64,
    clicked: Instant,
}

/// What a prediction leaves behind when its time is up: what the world has to go
/// back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused {
    pub at: IVec3,
    pub server: u32,
    pub water: Option<u32>,
}

/// The table of predictions, and how long edits took.
#[derive(Default)]
pub struct Predictions {
    entries: HashMap<IVec3, Entry>,
    /// When the edits were made that the next frame shows.
    to_draw: Vec<Instant>,
    drawn: Vec<Duration>,
    confirmed: Vec<Duration>,
    refused: u32,
}

impl Predictions {
    /// The world now shows `predicted` at `at`, where the server had `server`.
    ///
    /// If an earlier prediction is still waiting there, the server's last word is
    /// what that one kept: `server`, read from the world, is only our own guess.
    pub fn add(&mut self, at: IVec3, predicted: u32, server: u32, water: Option<u32>, tick: u64, clicked: Instant) {
        let (server, water) = self
            .entries
            .get(&at)
            .filter(|entry| entry.predicted.is_some())
            .map_or((server, water), |entry| (entry.server, entry.water));
        self.entries.insert(
            at,
            Entry {
                predicted: Some(predicted),
                server,
                water,
                deadline: tick + DEADLINE,
                clicked,
            },
        );
        self.to_draw.push(clicked);
    }

    /// An edit with nothing to show until the server answers. It's here to be timed.
    #[cfg_attr(not(test), expect(dead_code, reason = "the next commit's places"))]
    pub fn expect(&mut self, at: IVec3, server: u32, tick: u64, clicked: Instant) {
        self.entries.entry(at).or_insert(Entry {
            predicted: None,
            server,
            water: None,
            deadline: tick + DEADLINE,
            clicked,
        });
    }

    /// The server says the block at `at` is `id`. True if the world should stay as
    /// it is: the prediction is confirmed, or it's still waiting.
    pub fn note(&mut self, at: IVec3, id: u32, now: Instant) -> bool {
        let Some(entry) = self.entries.get_mut(&at) else {
            return false;
        };
        match entry.predicted {
            Some(predicted) if predicted != id => {
                entry.server = id;
                true
            }
            Some(_) => {
                let clicked = entry.clicked;
                self.entries.remove(&at);
                self.confirmed.push(now.saturating_duration_since(clicked));
                true
            }
            None => {
                let clicked = entry.clicked;
                self.entries.remove(&at);
                self.confirmed.push(now.saturating_duration_since(clicked));
                self.to_draw.push(clicked);
                false
            }
        }
    }

    /// The predictions whose time is up at `tick`, which go. An edit that was only
    /// being timed has nothing to take back.
    pub fn expired(&mut self, tick: u64) -> Vec<Refused> {
        let mut refused = Vec::new();
        self.entries.retain(|&at, entry| {
            if tick < entry.deadline {
                return true;
            }
            if entry.predicted.is_some() {
                refused.push(Refused {
                    at,
                    server: entry.server,
                    water: entry.water,
                });
            }
            false
        });
        self.refused += refused.len() as u32;
        refused
    }

    /// A chunk arrived, and what it says is newer than anything here.
    pub fn forget_chunk(&mut self, x: i32, z: i32) {
        self.entries.retain(|at, _| (at.x >> 4, at.z >> 4) != (x, z));
    }

    /// A frame has been drawn, which shows every edit made before it.
    pub fn frame_drawn(&mut self, now: Instant) {
        self.drawn
            .extend(self.to_draw.drain(..).map(|clicked| now.saturating_duration_since(clicked)));
    }

    /// How long edits have taken so far, for the end of a session.
    #[must_use]
    pub fn summary(&self) -> Option<String> {
        if self.drawn.is_empty() && self.confirmed.is_empty() {
            return None;
        }
        Some(format!(
            "Edits: {} shown, {} confirmed by the server, {} refused. Click to drawn: {}. Click to confirmed: {}",
            self.drawn.len(),
            self.confirmed.len(),
            self.refused,
            spread(&self.drawn),
            spread(&self.confirmed),
        ))
    }
}

/// The median and 95th percentile of some times, in words.
fn spread(times: &[Duration]) -> String {
    if times.is_empty() {
        return "none".to_string();
    }
    let mut sorted = times.to_vec();
    sorted.sort_unstable();
    let at = |fraction: f64| sorted[((sorted.len() - 1) as f64 * fraction).round() as usize].as_secs_f64() * 1000.0;
    format!("{:.1} ms at the median, {:.1} ms at the 95th percentile", at(0.5), at(0.95))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AIR: u32 = 1;
    const DIRT: u32 = 2;
    const STONE: u32 = 3;
    const WATER: u32 = 4;
    const AT: IVec3 = IVec3::new(1, 64, 2);

    fn placed(table: &mut Predictions, tick: u64) -> Instant {
        let clicked = Instant::now();
        table.add(AT, DIRT, AIR, None, tick, clicked);
        clicked
    }

    #[test]
    fn a_confirmed_prediction_goes_and_leaves_the_world_alone() {
        let mut table = Predictions::default();
        let clicked = placed(&mut table, 100);
        assert!(table.note(AT, DIRT, clicked + Duration::from_millis(60)), "the world already shows it");
        assert_eq!(table.confirmed, [Duration::from_millis(60)]);
        assert!(table.expired(100 + DEADLINE).is_empty(), "it's gone, so there's nothing to take back");
        // After which an update for that block is the server's to draw.
        assert!(!table.note(AT, STONE, clicked));
    }

    #[test]
    fn a_refused_prediction_snaps_back_at_the_deadline() {
        let mut table = Predictions::default();
        placed(&mut table, 100);
        assert!(table.expired(100 + DEADLINE - 1).is_empty(), "not yet");
        assert_eq!(
            table.expired(100 + DEADLINE),
            [Refused {
                at: AT,
                server: AIR,
                water: None
            }]
        );
        assert!(table.expired(200).is_empty(), "once");
        assert_eq!(table.refused, 1);
    }

    #[test]
    fn an_older_update_is_noted_and_not_drawn() {
        let mut table = Predictions::default();
        let clicked = placed(&mut table, 100);
        // On its way before the edit, and not what was predicted.
        assert!(table.note(AT, STONE, clicked), "the world keeps showing the prediction");
        // If nothing else comes, that's what it goes back to, not what was there at the click.
        assert_eq!(table.expired(100 + DEADLINE)[0].server, STONE);
    }

    #[test]
    fn an_update_then_the_prediction_is_still_confirmed() {
        let mut table = Predictions::default();
        let clicked = placed(&mut table, 100);
        assert!(table.note(AT, STONE, clicked));
        assert!(table.note(AT, DIRT, clicked));
        assert!(table.expired(100 + DEADLINE).is_empty());
        assert_eq!(table.refused, 0);
    }

    #[test]
    fn a_second_edit_keeps_the_servers_last_word_and_the_water() {
        let mut table = Predictions::default();
        let clicked = Instant::now();
        // A waterlogged block broken: the water moves up a layer.
        table.add(AT, WATER, STONE, Some(WATER), 100, clicked);
        // And something placed there before the server has said anything.
        table.add(AT, DIRT, WATER, None, 103, clicked);
        assert_eq!(
            table.expired(103 + DEADLINE),
            [Refused {
                at: AT,
                server: STONE,
                water: Some(WATER)
            }],
            "the second edit's deadline, and the first's server state"
        );
    }

    #[test]
    fn an_edit_that_shows_nothing_is_only_timed() {
        let mut table = Predictions::default();
        let clicked = Instant::now();
        table.expect(AT, AIR, 100, clicked);
        assert!(!table.note(AT, STONE, clicked + Duration::from_millis(70)), "the world takes it");
        assert_eq!(table.confirmed, [Duration::from_millis(70)]);
        table.frame_drawn(clicked + Duration::from_millis(80));
        assert_eq!(table.drawn, [Duration::from_millis(80)]);

        table.expect(AT, AIR, 200, clicked);
        assert!(table.expired(200 + DEADLINE).is_empty(), "nothing to take back");
        assert_eq!(table.refused, 0);
    }

    #[test]
    fn a_chunk_arriving_outranks_what_is_in_it() {
        let mut table = Predictions::default();
        placed(&mut table, 100);
        table.forget_chunk(1, 0);
        assert_eq!(table.expired(200).len(), 1, "another chunk");
        placed(&mut table, 100);
        table.forget_chunk(0, 0);
        assert!(table.expired(200).is_empty(), "its own");
    }

    #[test]
    fn each_frame_counts_what_it_shows_once() {
        let mut table = Predictions::default();
        let clicked = placed(&mut table, 100);
        table.frame_drawn(clicked + Duration::from_millis(5));
        table.frame_drawn(clicked + Duration::from_millis(21));
        assert_eq!(table.drawn, [Duration::from_millis(5)]);
    }

    #[test]
    fn the_summary_says_how_long_edits_took() {
        let mut table = Predictions::default();
        assert_eq!(table.summary(), None);
        let clicked = placed(&mut table, 100);
        table.frame_drawn(clicked + Duration::from_millis(5));
        table.note(AT, DIRT, clicked + Duration::from_millis(60));
        let summary = table.summary().unwrap();
        assert!(summary.contains("1 shown, 1 confirmed by the server, 0 refused"), "{summary}");
        assert!(summary.contains("5.0 ms") && summary.contains("60.0 ms"), "{summary}");
    }
}
