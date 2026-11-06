//! Sky light and block light, worked out on the client (m5-plan Decision 11; worth teaching).
//!
//! **Why the client does it.** A Bedrock server never sends light: the official client lights the
//! world itself, from its blocks. So does this one, by the rules Pumpkin's own light engine uses
//! (`pumpkin-world/src/lighting/engine.rs`), so that the two can be checked against each other
//! (`--headless`, "light matches server").
//!
//! **Two kinds of light, each a level from 0 to 15 in every block** ([`Kind`]):
//! - **block light** comes from blocks that give it out, a torch 14 and lava 15. Each block it
//!   spreads into takes at least 1 from it, and as much as the block dims: stone takes it all;
//! - **sky light** comes down from the sky. Straight down a column, each block takes only what it
//!   dims, so it stays 15 through air and glass, and loses 1 through leaves and each block of water.
//!   From there it spreads like block light, except that 15 going down into a block that dims
//!   nothing stays 15.
//!
//! [`table`] has what each block gives out and dims, and [`area`] the flood fills that spread light
//! and take it away again.
//!
//! **Storage:** per sub-chunk, one [`Levels`] for each kind: 4,096 levels, two to a byte, or a single
//! value when they're all the same, as open sky and solid rock are. Most sub-chunks are one or the
//! other.
//!
//! **When light is worked out** ([`Light::update`], once a frame before meshing):
//! - **a chunk that arrives** waits, and is lit nearest the camera first, within a time budget. It's
//!   lit with its lit neighbours' borders, and lights them in turn. Until it's lit, it isn't meshed,
//!   and light neither goes in nor out of it;
//! - **a chunk that goes** takes its light out of its neighbours first, and so does one replaced by a
//!   newer copy, which then waits to be lit again;
//! - **a block that changes** in a lit chunk is lit again at the next update, before its sub-chunk
//!   is meshed, whatever the budget says. The player's own edits ask for one at once.
//!
//! Each sub-chunk whose light changes is reported to the meshes ([`Change::Light`]), and each chunk
//! lit for the first time too ([`Change::Lit`]).

pub mod area;
pub mod table;

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use tracing::{debug, info};

use self::{
    area::{Area, MIDDLE, Slot},
    table::LightTable,
};
use super::{Change, chunk::Chunk};

/// How long lighting arrived chunks may take each frame (m5-plan Decision 11).
pub const BUDGET: Duration = Duration::from_millis(3);
/// A join is over once no chunk has arrived for this long, as for meshing.
const JOIN_QUIET: Duration = Duration::from_secs(2);

/// Which light.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// From the sky.
    Sky,
    /// From blocks that give out light.
    Block,
}

/// One sub-chunk's light of one kind: 4,096 levels from 0 to 15.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Levels {
    /// Every block has this level.
    Uniform(u8),
    /// Two levels a byte, the even index in the low half, in Bedrock's order (x, then z, then y,
    /// with y changing fastest), like the blocks' own storage.
    Mixed(Box<[u8; 2048]>),
}

impl Levels {
    /// The level at index `i`: `(x << 8) | (z << 4) | y`, each below 16.
    #[must_use]
    pub fn get(&self, i: usize) -> u8 {
        match self {
            Self::Uniform(level) => *level,
            Self::Mixed(bytes) => (bytes[i >> 1] >> ((i & 1) << 2)) & 15,
        }
    }

    /// Sets the level at index `i`, and says whether it changed.
    pub fn set(&mut self, i: usize, level: u8) -> bool {
        match self {
            Self::Uniform(old) if *old == level => false,
            Self::Uniform(old) => {
                let mut bytes = Box::new([*old | *old << 4; 2048]);
                let shift = (i & 1) << 2;
                bytes[i >> 1] = (bytes[i >> 1] & !(15 << shift)) | level << shift;
                *self = Self::Mixed(bytes);
                true
            }
            Self::Mixed(bytes) => {
                let shift = (i & 1) << 2;
                let byte = &mut bytes[i >> 1];
                if (*byte >> shift) & 15 == level {
                    return false;
                }
                *byte = (*byte & !(15 << shift)) | level << shift;
                true
            }
        }
    }

    /// Goes back to one value if every level is the same.
    pub fn compact(&mut self) {
        if let Self::Mixed(bytes) = self {
            let first = bytes[0];
            if first & 15 == first >> 4 && bytes.iter().all(|&byte| byte == first) {
                *self = Self::Uniform(first & 15);
            }
        }
    }
}

/// One chunk's light: a [`Levels`] of each kind for each of its sub-chunks, from the bottom up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkLight {
    /// The section index (block y / 16) of the lowest sub-chunk, as in [`Chunk`].
    pub min_section: i32,
    pub sky: Vec<Levels>,
    pub block: Vec<Levels>,
}

impl ChunkLight {
    /// A chunk's light before it's worked out: dark.
    fn dark(min_section: i32, sections: usize) -> Self {
        Self {
            min_section,
            sky: vec![Levels::Uniform(0); sections],
            block: vec![Levels::Uniform(0); sections],
        }
    }

    #[must_use]
    pub fn levels(&self, kind: Kind) -> &[Levels] {
        match kind {
            Kind::Sky => &self.sky,
            Kind::Block => &self.block,
        }
    }

    fn levels_mut(&mut self, kind: Kind) -> &mut [Levels] {
        match kind {
            Kind::Sky => &mut self.sky,
            Kind::Block => &mut self.block,
        }
    }

    /// The level at `(x, y, z)`: x and z within the chunk (0 to 15), y absolute. `None` outside its
    /// height.
    #[must_use]
    pub fn get(&self, kind: Kind, x: usize, y: i32, z: usize) -> Option<u8> {
        let section = usize::try_from((y >> 4) - self.min_section).ok()?;
        let levels = self.levels(kind).get(section)?;
        Some(levels.get((x << 8) | (z << 4) | (y & 15) as usize))
    }
}

/// What lighting has cost, for the logs.
#[derive(Debug, Default)]
pub struct LightStats {
    /// Chunks lit as they arrived, and the time that took.
    pub lit: u64,
    pub lit_time: Duration,
    /// Batches of changed blocks lit again, the blocks in them, the time, and the longest batch.
    pub relights: u64,
    pub relit_blocks: u64,
    pub relight_time: Duration,
    pub longest_relight: Duration,
    /// Of those, the batches of one block, as a click makes, and the longest.
    pub single_relights: u64,
    pub longest_single: Duration,
    /// Chunks whose light was taken out of their neighbours, and the time.
    pub forgotten: u64,
    pub forget_time: Duration,
}

impl LightStats {
    /// The average time to light an arrived chunk, in microseconds.
    #[must_use]
    pub fn micros_each(&self) -> f64 {
        if self.lit == 0 {
            0.0
        } else {
            self.lit_time.as_secs_f64() * 1e6 / self.lit as f64
        }
    }
}

/// The world's light, and the work waiting on it.
pub struct Light {
    table: &'static LightTable,
    chunks: HashMap<(i32, i32), ChunkLight>,
    /// Chunks that have arrived and wait to be lit.
    pending: HashSet<(i32, i32)>,
    /// Blocks that changed in lit chunks, to light again.
    dirty: Vec<(i32, i32, i32)>,
    /// What changed, for the world's log.
    changes: Vec<Change>,
    pub stats: LightStats,
    /// When the first chunk arrived, and the latest, and whether lighting caught up with the join.
    join_started: Option<Instant>,
    last_chunk: Option<Instant>,
    emptied_at: Option<Instant>,
    caught_up_once: bool,
}

impl Default for Light {
    fn default() -> Self {
        Self::new()
    }
}

impl Light {
    #[must_use]
    pub fn new() -> Self {
        Self {
            table: table::table(),
            chunks: HashMap::new(),
            pending: HashSet::new(),
            dirty: Vec::new(),
            changes: Vec::new(),
            stats: LightStats::default(),
            join_started: None,
            last_chunk: None,
            emptied_at: None,
            caught_up_once: false,
        }
    }

    /// A chunk arrived. A copy that replaces a lit one takes the old light out of its neighbours
    /// first.
    pub fn arrived(&mut self, chunks: &HashMap<(i32, i32), Chunk>, x: i32, z: i32) {
        if self.chunks.contains_key(&(x, z)) {
            self.forget(chunks, x, z);
        }
        self.pending.insert((x, z));
        let now = Instant::now();
        self.join_started.get_or_insert(now);
        self.last_chunk = Some(now);
        self.emptied_at = None;
    }

    /// A chunk went: its light leaves its neighbours with it. Call this once it's out of `chunks`.
    pub fn dropped(&mut self, chunks: &HashMap<(i32, i32), Chunk>, x: i32, z: i32) {
        self.pending.remove(&(x, z));
        if self.chunks.contains_key(&(x, z)) {
            self.forget(chunks, x, z);
        }
    }

    /// A block changed. In a chunk not lit yet, there's nothing to do: it will be lit as it is.
    pub fn block_changed(&mut self, x: i32, y: i32, z: i32) {
        if self.chunks.contains_key(&(x >> 4, z >> 4)) {
            self.dirty.push((x, y, z));
        }
    }

    /// Whether the chunk has been lit.
    #[must_use]
    pub fn is_lit(&self, x: i32, z: i32) -> bool {
        self.chunks.contains_key(&(x, z))
    }

    /// A lit chunk's light.
    #[must_use]
    pub fn chunk(&self, x: i32, z: i32) -> Option<&ChunkLight> {
        self.chunks.get(&(x, z))
    }

    /// The level of light at a world position, if its chunk is lit and it's inside the world.
    #[must_use]
    pub fn level(&self, kind: Kind, x: i32, y: i32, z: i32) -> Option<u8> {
        self.chunks.get(&(x >> 4, z >> 4))?.get(kind, (x & 15) as usize, y, (z & 15) as usize)
    }

    /// Chunks waiting to be lit.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.pending.len()
    }

    /// What changed since the last call.
    pub fn take_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }

    /// Lights the changed blocks again, then arrived chunks nearest `around` (a chunk position) first,
    /// until `budget` is spent: at least one, unless the budget is zero, which lights only the
    /// changed blocks.
    pub fn update(&mut self, chunks: &HashMap<(i32, i32), Chunk>, budget: Duration, around: (i32, i32)) {
        let start = Instant::now();
        self.relight_dirty(chunks);
        let mut first = true;
        while !self.pending.is_empty() && !budget.is_zero() && (first || start.elapsed() < budget) {
            first = false;
            let next = *self
                .pending
                .iter()
                .min_by_key(|&&(x, z)| {
                    let (dx, dz) = (i64::from(x - around.0), i64::from(z - around.1));
                    (dx * dx + dz * dz, x, z)
                })
                .expect("not empty");
            self.pending.remove(&next);
            if chunks.contains_key(&next) {
                self.light_chunk(chunks, next.0, next.1);
            }
        }
        self.check_caught_up();
    }

    fn light_chunk(&mut self, chunks: &HashMap<(i32, i32), Chunk>, x: i32, z: i32) {
        let started = Instant::now();
        let Some(chunk) = chunks.get(&(x, z)) else {
            return;
        };
        let fresh = ChunkLight::dark(chunk.min_section, chunk.sub_chunks.len());
        let mut area = self.gather(chunks, x, z, Job::Arrived, fresh);
        area.light_middle();
        self.put_back(area, x, z, Job::Arrived);
        self.changes.push(Change::Lit { x, z });
        self.stats.lit += 1;
        self.stats.lit_time += started.elapsed();
    }

    fn forget(&mut self, chunks: &HashMap<(i32, i32), Chunk>, x: i32, z: i32) {
        let started = Instant::now();
        let Some(light) = self.chunks.remove(&(x, z)) else {
            return;
        };
        let mut area = self.gather(chunks, x, z, Job::Forgotten, light);
        area.forget_middle();
        self.put_back(area, x, z, Job::Forgotten);
        self.stats.forgotten += 1;
        self.stats.forget_time += started.elapsed();
    }

    fn relight_dirty(&mut self, chunks: &HashMap<(i32, i32), Chunk>) {
        if self.dirty.is_empty() {
            return;
        }
        let started = Instant::now();
        let mut cells = std::mem::take(&mut self.dirty);
        cells.sort_unstable_by_key(|&(x, y, z)| (x >> 4, z >> 4, x, z, std::cmp::Reverse(y)));
        cells.dedup();
        let blocks = cells.len() as u64;
        // One job per chunk: what a change does stays within one chunk of it.
        let mut rest = cells.as_slice();
        while let Some(&(x, _, z)) = rest.first() {
            let (cx, cz) = (x >> 4, z >> 4);
            let end = rest.iter().position(|&(x, _, z)| (x >> 4, z >> 4) != (cx, cz)).unwrap_or(rest.len());
            let (group, after) = rest.split_at(end);
            rest = after;
            let (Some(chunk), true) = (chunks.get(&(cx, cz)), self.chunks.contains_key(&(cx, cz))) else {
                continue;
            };
            let min_y = chunk.min_y();
            let local: Vec<_> = group
                .iter()
                .filter(|&&(_, y, _)| (min_y..chunk.max_y()).contains(&y))
                .map(|&(x, y, z)| ((x & 15) + 16, y - min_y, (z & 15) + 16))
                .collect();
            let light = self.chunks.remove(&(cx, cz)).expect("checked above");
            let mut area = self.gather(chunks, cx, cz, Job::Changed, light);
            area.relight(&local);
            self.put_back(area, cx, cz, Job::Changed);
        }
        let took = started.elapsed();
        self.stats.relights += 1;
        self.stats.relit_blocks += blocks;
        self.stats.relight_time += took;
        self.stats.longest_relight = self.stats.longest_relight.max(took);
        if blocks == 1 {
            self.stats.single_relights += 1;
            self.stats.longest_single = self.stats.longest_single.max(took);
        }
    }

    /// Lends the light of the chunk at (`x`, `z`), which is `light`, and its lit neighbours' out of
    /// the store, for `job`.
    fn gather<'w>(
        &mut self,
        chunks: &'w HashMap<(i32, i32), Chunk>,
        x: i32,
        z: i32,
        job: Job,
        light: ChunkLight,
    ) -> Area<'w> {
        let sections = light.sky.len();
        let mut area = Area::new(self.table, sections);
        let mut light = Some(light);
        for dx in -1..=1 {
            for dz in -1..=1 {
                let slot = ((dx + 1) * 3 + (dz + 1)) as usize;
                let (cx, cz) = (x + dx, z + dz);
                if slot == MIDDLE {
                    // A forgotten chunk lends its light without its blocks, so no fill goes into it.
                    let blocks = if job == Job::Forgotten { None } else { chunks.get(&(cx, cz)) };
                    area.slots[slot] = Some(Slot::new(blocks, light.take().expect("once")));
                } else if let (Some(blocks), Some(light)) = (chunks.get(&(cx, cz)), self.chunks.remove(&(cx, cz))) {
                    debug_assert_eq!(light.sky.len(), sections, "every chunk has the same height");
                    area.slots[slot] = Some(Slot::new(Some(blocks), light));
                }
            }
        }
        area
    }

    /// Puts the area's light back in the store, and reports what changed.
    fn put_back(&mut self, area: Area<'_>, x: i32, z: i32, job: Job) {
        for (slot, entry) in area.slots.into_iter().enumerate() {
            let Some(Slot { mut light, changed, .. }) = entry else {
                continue;
            };
            let (dx, dz) = (slot as i32 / 3 - 1, slot as i32 % 3 - 1);
            let (cx, cz) = (x + dx, z + dz);
            if slot == MIDDLE && job == Job::Forgotten {
                continue;
            }
            // A chunk lit for the first time is reported whole, as `Change::Lit`.
            let fresh = slot == MIDDLE && job == Job::Arrived;
            for section in 0..light.sky.len() {
                if fresh || changed & (1 << section) != 0 {
                    light.sky[section].compact();
                    light.block[section].compact();
                    if !fresh {
                        self.changes.push(Change::Light {
                            x: cx,
                            y: light.min_section + section as i32,
                            z: cz,
                        });
                    }
                }
            }
            self.chunks.insert((cx, cz), light);
        }
    }

    /// Once nothing waits and chunks have stopped arriving, logs that lighting caught up: the first
    /// time at info level, with how long the join's light took, and later at debug level.
    fn check_caught_up(&mut self) {
        if !self.pending.is_empty() {
            self.emptied_at = None;
            return;
        }
        let now = Instant::now();
        let emptied = *self.emptied_at.get_or_insert(now);
        let (Some(started), Some(last_chunk)) = (self.join_started, self.last_chunk) else {
            return;
        };
        if last_chunk.elapsed() < JOIN_QUIET {
            return;
        }
        let line = format!(
            "{} chunks lit, {:.0} µs each; {} changed blocks lit again in {:.1} ms",
            self.stats.lit,
            self.stats.micros_each(),
            self.stats.relit_blocks,
            self.stats.relight_time.as_secs_f64() * 1000.0,
        );
        if self.caught_up_once {
            debug!("Light caught up: {line}");
        } else {
            info!(
                "Light caught up {:.1} s after the first chunk (the last came at {:.1} s): {line}",
                (emptied - started).as_secs_f64(),
                (last_chunk - started).as_secs_f64(),
            );
            self.caught_up_once = true;
        }
        // Until the next chunk arrives.
        self.last_chunk = None;
    }
}

/// What a job does to the middle chunk of its area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    /// It has just arrived, and is lit.
    Arrived,
    /// It's lit, and some of its blocks changed.
    Changed,
    /// It's gone, or being replaced: its light leaves its neighbours.
    Forgotten,
}

#[cfg(test)]
mod tests;
