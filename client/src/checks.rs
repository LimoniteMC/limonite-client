//! The acceptance checks, each reported as one `PASS` or `FAIL` line.
//!
//! M2's "no socket is opened" can't be made from inside the process; it's
//! done from outside while the session lingers.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, PoisonError, atomic::Ordering},
    time::Duration,
};

use pumpkin_core::{net::bedrock::level_chunk::CLevelChunk, server::Server};
use pumpkin_data::{Block, BlockState, BlockStateId, biome::Biome};
use pumpkin_util::{
    PermissionLvl,
    math::{vector2::Vector2, vector3::Vector3},
};
use pumpkin_world::{
    chunk::{ChunkData, format::LightContainer, palette::bedrock_water_state},
    level::Level,
};
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    session::{Session, Spawn, TeleportReport},
    world::{
        AIR, BlockTable, World,
        chunk::Chunk,
        level_chunk,
        light::{
            Kind,
            area::spread,
            table::{Props, table},
        },
    },
};

/// How far east the host must have moved, as the server sees it.
pub const MIN_MOVE: f64 = 4.0;
/// How close to the server's teleport the client's report must be, in blocks.
const TELEPORT_TOLERANCE: f64 = 0.01;

pub struct Check {
    name: &'static str,
    passed: bool,
    detail: String,
}

impl Check {
    pub const fn new(name: &'static str, passed: bool, detail: String) -> Self {
        Self {
            name,
            passed,
            detail,
        }
    }

    pub const fn passed(&self) -> bool {
        self.passed
    }
}

/// Whether the server's copy of the host has moved at least [`MIN_MOVE`]
/// blocks east of where `StartGame` put it.
///
/// This reads the server's own state, not an echo: the position went from
/// our `PlayerAuthInput` through the `LocalChannel`, the server's read loop,
/// its packet queue and its tick thread, which applied it.
pub fn movement(server: &Server, spawn: &Spawn) -> Check {
    let players = server.get_all_players();
    let Some(player) = players.first() else {
        return Check::new("movement", false, "server has no player".to_string());
    };
    let entity = &player.living_entity.entity;
    // `StartGame` gives the eye position; the server stores the feet.
    let eye_height = f64::from(entity.entity_type.eye_height);
    let start_x = f64::from(spawn.eye_position.x);
    let start_feet_y = f64::from(spawn.eye_position.y) - eye_height;
    let now = player.position();
    let moved = now.x - start_x;
    Check::new(
        "movement",
        moved >= MIN_MOVE,
        format!(
            "the server's player moved {moved:.2} blocks east (needs {MIN_MOVE}); \
             feet y {start_feet_y:.2} -> {:.2}",
            now.y
        ),
    )
}

/// Whether the client went where the server teleported it, and said so in
/// its next input, as the official client does: `HandledTeleport`, the new
/// eye position, and a `delta` of 0 (m5-plan Step 5).
///
/// `server_waits` says whether the server still waits for the client to reach
/// it. Pumpkin ignores a Bedrock player's input until it does, and sends the
/// teleport again every 20 inputs that don't (upstream, 2026-11-07).
pub fn teleport(
    report: Option<TeleportReport>,
    expected_eyes: Vector3<f64>,
    waited: Duration,
    server_waits: bool,
) -> Check {
    const NAME: &str = "teleport";
    let Some(report) = report else {
        return Check::new(
            NAME,
            false,
            format!("no input reported the teleport within {:.1} s", waited.as_secs_f64()),
        );
    };
    let eyes = report.eyes;
    let off = ((f64::from(eyes.x) - expected_eyes.x).powi(2)
        + (f64::from(eyes.y) - expected_eyes.y).powi(2)
        + (f64::from(eyes.z) - expected_eyes.z).powi(2))
    .sqrt();
    let delta = report.delta;
    let still = delta.x == 0.0 && delta.y == 0.0 && delta.z == 0.0;
    Check::new(
        NAME,
        off < TELEPORT_TOLERANCE && still && !server_waits,
        format!(
            "input {} reported HandledTeleport {:.0} ms after the server's teleport, with the eyes \
             {off:.4} blocks from where the server put them (needs under {TELEPORT_TOLERANCE}) and a \
             delta of ({}, {}, {}); the server {}",
            report.tick,
            waited.as_secs_f64() * 1000.0,
            delta.x,
            delta.y,
            delta.z,
            if server_waits {
                "still waits for the client to reach it, and ignores its input"
            } else {
                "saw it reached"
            }
        ),
    )
}

/// Whether the server measured the host's round trip, from the
/// `NetworkStackLatency` probes it sends once a second and the client answers.
/// Pumpkin waits that long before pushing a Bedrock player again after a motion of
/// its own, and takes an unmeasured round trip as 9 ticks (`client_push.rs`).
pub fn latency(server: &Server) -> Check {
    const NAME: &str = "latency";
    let players = server.get_all_players();
    let Some(player) = players.first() else {
        return Check::new(NAME, false, "server has no player".to_string());
    };
    // 0 is Pumpkin's "not measured": it keeps every sample at 1 ms or more.
    match player.ping.load(Ordering::Relaxed) {
        0 => Check::new(NAME, false, "the server has no answer to its NetworkStackLatency probes".to_string()),
        ping => Check::new(
            NAME,
            true,
            format!("the server measured the host's round trip at {ping} ms from its NetworkStackLatency probes"),
        ),
    }
}

/// Whether the server knows the host by the UUID the client worked out for
/// it, and made it an operator at level 4 from `data/ops.json` (m5-plan
/// Decision 18).
pub fn operator(server: &Server, expected: Result<Uuid, String>) -> Check {
    const NAME: &str = "operator";
    let expected = match expected {
        Ok(uuid) => uuid,
        Err(err) => return Check::new(NAME, false, format!("no UUID for the host: {err}")),
    };
    let players = server.get_all_players();
    let Some(player) = players.first() else {
        return Check::new(NAME, false, "server has no player".to_string());
    };
    let uuid = player.gameprofile.id;
    let level = player.permission_lvl.load();
    if uuid != expected {
        return Check::new(
            NAME,
            false,
            format!("the server's host is {uuid}, but the client worked out {expected}"),
        );
    }
    Check::new(
        NAME,
        level == PermissionLvl::Four,
        format!(
            "the server knows the host by the UUID the client worked out, at permission level {} \
             (needs 4)",
            level as u8
        ),
    )
}

/// Whether every `LevelChunk` decoded, and `StartGame` said block ids are
/// hashed, which the world's id table assumes.
pub fn chunks_decoded(session: &Session) -> Check {
    let received = session.level_chunks();
    let decoded = session.chunks_decoded();
    let errors = session.world_errors();
    let hashed = session.ids_hashed();
    let hashed_detail = match hashed {
        Some(true) => "block ids are hashed",
        Some(false) => "block ids are NOT hashed",
        None => "no StartGame",
    };
    let world = session.world();
    let block_actors: usize = world.chunks().map(|chunk| chunk.block_actors.len()).sum();
    Check::new(
        "chunks decoded",
        received > 0 && decoded == received && errors == 0 && hashed == Some(true),
        format!(
            "{decoded} of {received} LevelChunk packets decoded, {errors} errors; \
             {hashed_detail}; {block_actors} block actors; {} UpdateBlocks for unloaded \
             chunks ignored",
            world.updates_not_loaded(),
        ),
    )
}

/// Compares our copy of a chunk with the server's, at every position: the
/// block on layer 0, its water on layer 1, and the biome.
///
/// Each side reads through its own code. Ours goes through the decoder's
/// palette index maths (XZY words); the server's through Pumpkin's
/// `get_block_absolute_y` on its own sections (YZX). A mistake shared by
/// both sides could hide; independent ones can't.
///
/// Returns how many values differ, and describes up to `limit` of them in
/// `out`.
pub fn compare_chunk(
    server: &ChunkData,
    ours: &Chunk,
    names: &BlockTable,
    out: &mut Vec<String>,
    limit: usize,
) -> usize {
    let sections = &server.section;
    let server_max_y = sections.min_y + 16 * sections.count as i32;
    if (ours.min_y(), ours.max_y()) != (sections.min_y, server_max_y) {
        out.push(format!(
            "chunk ({}, {}): our height {}..{}, the server's {}..{server_max_y}",
            ours.x,
            ours.z,
            ours.min_y(),
            ours.max_y(),
            sections.min_y,
        ));
        return 1;
    }

    let mut differ = 0;
    let mut note = |out: &mut Vec<String>, what: std::fmt::Arguments<'_>| {
        differ += 1;
        if out.len() < limit {
            out.push(what.to_string());
        }
    };
    let (cx, cz) = (ours.x * 16, ours.z * 16);
    for x in 0..16 {
        for z in 0..16 {
            for y in sections.min_y..server_max_y {
                let (wx, wz) = (cx + x as i32, cz + z as i32);
                let Some(state) = sections.get_block_absolute_y(x, y, z) else {
                    continue;
                };
                let ours_block = ours.block(x, y, z, 0);
                // Ids as well as names: several states share a name
                // (lit and unlit redstone ore, say).
                let server_block = BlockState::to_be_network_id(state);
                if ours_block != server_block {
                    note(
                        out,
                        format_args!(
                            "({wx}, {y}, {wz}) layer 0: ours {} {ours_block:#010x}, \
                             server {} {server_block:#010x} (Java state {})",
                            names.name(ours_block),
                            block_name(state),
                            state.as_u16(),
                        ),
                    );
                }
                let water = bedrock_water_state(state);
                let server_water = BlockState::to_be_network_id(water);
                let ours_water = ours.block(x, y, z, 1);
                if ours_water != server_water {
                    note(
                        out,
                        format_args!(
                            "({wx}, {y}, {wz}) layer 1: ours {} {ours_water:#010x}, \
                             server {} {server_water:#010x} (from {})",
                            names.name(ours_water),
                            block_name(water),
                            block_name(state),
                        ),
                    );
                }
                let server_biome = sections
                    .get_rough_biome_absolute_y(x, y, z)
                    .map(|id| Biome::from_id(id).map_or(1, |biome| biome.be_network_id));
                let ours_biome = ours.biome(x, y, z);
                if ours_biome != server_biome.map(u32::from) {
                    note(
                        out,
                        format_args!(
                            "({wx}, {y}, {wz}) biome: ours {ours_biome:?}, server {server_biome:?}"
                        ),
                    );
                }
            }
        }
    }
    differ
}

/// Encodes the server's chunk as it is now, decodes it with our decoder, and
/// compares the result with the server. True if they're equal at every
/// position.
///
/// That separates two causes when our world still differs from the server
/// after the re-checks: a decoder bug (this fails too) or a change the server
/// made without sending an `UpdateBlock` (this passes). Pumpkin's fire does
/// that: it sets blocks with `NOTIFY_NEIGHBORS` only, which never queues a
/// client update.
pub fn fresh_decode_matches(server: &ChunkData, names: &BlockTable, out: &mut Vec<String>) -> bool {
    // Block actors don't affect the comparison.
    let decoded = CLevelChunk::encode_chunk(server, 0, false, &[])
        .map_err(|err| err.to_string())
        .and_then(|(payload, _)| level_chunk::decode(&payload).map_err(|err| err.to_string()));
    match decoded {
        Ok(fresh) => compare_chunk(server, &fresh, names, out, 10) == 0,
        Err(err) => {
            out.push(format!("chunk ({}, {}): fresh encode failed: {err}", server.x, server.z));
            false
        }
    }
}

fn block_name(state: BlockStateId) -> &'static str {
    Block::from_state_id(state).name
}

/// The six neighbours, `(dx, dy, dz)`, with up first.
const NEIGHBOURS: [(i32, i32, i32); 6] = [(0, 1, 0), (0, -1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)];

/// How many blocks of one group of differences have our own light checked against the rules: the
/// rest of a big group is only walked.
const OURS_CHECKED: usize = 4096;

/// One side's light and blocks, ours or the server's, for comparing the two (m5-plan Step 13).
trait LightSide {
    fn level(&self, kind: Kind, x: i32, y: i32, z: i32) -> Option<u8>;
    fn props(&self, x: i32, y: i32, z: i32) -> Option<Props>;
    /// The world's lowest y, and one above its highest.
    fn height(&self) -> (i32, i32);

    /// The sky's own light at a block: 15, less what it and every block above it dim.
    fn sky_source(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        let mut run = 15i32;
        for cy in y..self.height().1 {
            run -= i32::from(self.props(x, cy, z)?.dims());
            if run <= 0 {
                return Some(0);
            }
        }
        Some(run as u8)
    }

    fn source(&self, kind: Kind, x: i32, y: i32, z: i32) -> Option<u8> {
        match kind {
            Kind::Block => Some(self.props(x, y, z)?.emits()),
            Kind::Sky => self.sky_source(x, y, z),
        }
    }

    /// Whether a block's level is what the rules make it from its own light and its neighbours':
    /// `None` if a neighbour is out of reach.
    fn follows_rules(&self, kind: Kind, x: i32, y: i32, z: i32) -> Option<bool> {
        let (bottom, top) = self.height();
        let level = self.level(kind, x, y, z)?;
        let dims = self.props(x, y, z)?.dims();
        let mut best = self.source(kind, x, y, z)?;
        for (dx, dy, dz) in NEIGHBOURS {
            if !(bottom..top).contains(&(y + dy)) {
                continue;
            }
            let there = self.level(kind, x + dx, y + dy, z + dz)?;
            // From above, the light comes down.
            best = best.max(spread(kind, there, dims, dy == 1));
        }
        Some(level == best)
    }
}

/// Our light, and our blocks as the light table reads them.
struct Ours<'a> {
    world: &'a World,
    height: (i32, i32),
}

impl LightSide for Ours<'_> {
    fn level(&self, kind: Kind, x: i32, y: i32, z: i32) -> Option<u8> {
        self.world.light().level(kind, x, y, z)
    }

    fn props(&self, x: i32, y: i32, z: i32) -> Option<Props> {
        let block = self.world.block(x, y, z, 0)?;
        Some(table().get(block, self.world.block(x, y, z, 1)? != AIR))
    }

    fn height(&self) -> (i32, i32) {
        self.height
    }
}

/// The server's light and blocks for the chunks a comparison looks at, copied out once so its locks
/// are taken once a chunk.
pub struct ServerLight {
    chunks: HashMap<(i32, i32), ServerChunk>,
    height: (i32, i32),
}

struct ServerChunk {
    data: Arc<ChunkData>,
    sky: Box<[LightContainer]>,
    block: Box<[LightContainer]>,
}

impl ServerLight {
    /// Copies the light of whichever of these chunks the server has loaded.
    pub fn gather(level: &Level, positions: impl IntoIterator<Item = (i32, i32)>) -> Self {
        let mut chunks = HashMap::new();
        let mut height = (0, 0);
        for (x, z) in positions {
            if chunks.contains_key(&(x, z)) {
                continue;
            }
            let Some(data) = level.loaded_chunks.get(&Vector2::new(x, z)).map(|entry| entry.value().clone()) else {
                continue;
            };
            let (sky, block) = {
                let light = data.light_engine.lock().unwrap_or_else(PoisonError::into_inner);
                (light.sky_light.clone(), light.block_light.clone())
            };
            height = (data.section.min_y, data.section.min_y + 16 * data.section.count as i32);
            chunks.insert((x, z), ServerChunk { data, sky, block });
        }
        Self { chunks, height }
    }

    /// A chunk's level at x and z inside it, and y absolute.
    fn chunk_level(&self, chunk: &ServerChunk, kind: Kind, x: usize, y: i32, z: usize) -> Option<u8> {
        let section = usize::try_from((y - self.height.0) >> 4).ok()?;
        let containers = match kind {
            Kind::Sky => &chunk.sky,
            Kind::Block => &chunk.block,
        };
        Some(containers.get(section)?.get(x, (y & 15) as usize, z))
    }
}

impl LightSide for ServerLight {
    fn level(&self, kind: Kind, x: i32, y: i32, z: i32) -> Option<u8> {
        let chunk = self.chunks.get(&(x >> 4, z >> 4))?;
        self.chunk_level(chunk, kind, (x & 15) as usize, y, (z & 15) as usize)
    }

    fn props(&self, x: i32, y: i32, z: i32) -> Option<Props> {
        let chunk = self.chunks.get(&(x >> 4, z >> 4))?;
        let state = chunk.data.section.get_block_absolute_y((x & 15) as usize, y, (z & 15) as usize)?;
        Some(Props::of_state(state))
    }

    fn height(&self) -> (i32, i32) {
        self.height
    }
}

/// What explains one group of differing levels.
enum Explained {
    /// The server's light breaks the rules it lights with, somewhere in the group.
    ServerRule,
    /// A block reads differently on the two sides: the Bedrock id doesn't carry it, or the server
    /// changed it and hasn't said yet.
    Blocks,
    /// Our light breaks the rules: a bug.
    OursWrong((i32, i32, i32)),
    Nothing,
}

/// What a light comparison found.
#[derive(Debug, Default)]
pub struct LightReport {
    pub chunks: usize,
    pub equal_chunks: usize,
    /// Levels compared, and those that differ: sky, then block.
    pub levels: u64,
    pub differ: [u64; 2],
    /// Groups of differing levels, by what explains them.
    pub server_rule: u64,
    pub blocks: u64,
    pub ours_wrong: u64,
    pub unexplained: u64,
    /// The first few groups that fail, and the first few the server's rules explain.
    pub failures: Vec<String>,
    pub explained: Vec<String>,
    /// Chunks with a group that fails.
    pub failing: Vec<(i32, i32)>,
}

/// Compares our light with the server's in chunk (`kx`, `kz`), sky and block, at every block.
///
/// **Where they differ, why** (worth teaching). Both sides light by the same rules: a block's level
/// is the best of its own light and what each neighbour gives it. Levels that follow the rules
/// everywhere are unique, so if both sides followed them on the same blocks, they'd be equal. Each
/// group of neighbouring levels that differ the same way (the server's brighter, or ours) must then
/// hold a block where the server breaks its rules, or where the blocks differ. A group with neither
/// fails, and so does one where our own light breaks the rules.
///
/// Pumpkin's breaks them, as it lights a new chunk with only a block of its neighbours around it,
/// keeps the first light that reaches a block in a flood fill, and lights changes after generation
/// another way (m5-plan Deviation 57). Groups are followed within the 3 × 3 chunks around this one.
pub fn compare_light(world: &World, server: &ServerLight, (kx, kz): (i32, i32), report: &mut LightReport) {
    let (Some(ours_chunk), Some(theirs)) = (world.light().chunk(kx, kz), server.chunks.get(&(kx, kz))) else {
        return;
    };
    let ours = Ours {
        world,
        height: server.height,
    };
    let (bottom, top) = server.height;
    report.chunks += 1;
    let (mut equal, mut failing) = (true, false);
    for (k, kind) in [Kind::Sky, Kind::Block].into_iter().enumerate() {
        let mut differing = Vec::new();
        for x in 0..16 {
            for z in 0..16 {
                for y in bottom..top {
                    if ours_chunk.get(kind, x, y, z) != server.chunk_level(theirs, kind, x, y, z) {
                        differing.push((kx * 16 + x as i32, y, kz * 16 + z as i32));
                    }
                }
            }
        }
        report.levels += 256 * (top - bottom) as u64;
        report.differ[k] += differing.len() as u64;
        if differing.is_empty() {
            continue;
        }
        equal = false;
        let mut seen = HashSet::new();
        for start in differing {
            if !seen.insert(start) {
                continue;
            }
            let (explained, size) = explain_group(&ours, server, kind, start, (kx, kz), &mut seen);
            let (sx, sy, sz) = start;
            let (o, s) = (ours.level(kind, sx, sy, sz), server.level(kind, sx, sy, sz));
            let at = format!(
                "{kind:?} light at ({sx}, {sy}, {sz}): ours {o:?}, the server's {s:?}, in a group of {size}"
            );
            match explained {
                Explained::ServerRule => {
                    report.server_rule += 1;
                    if report.explained.len() < 5 {
                        report.explained.push(at);
                    }
                }
                Explained::Blocks => report.blocks += 1,
                Explained::OursWrong((x, y, z)) => {
                    report.ours_wrong += 1;
                    failing = true;
                    if report.failures.len() < 10 {
                        report.failures.push(format!("{at}: ours breaks the rules at ({x}, {y}, {z})"));
                    }
                }
                Explained::Nothing => {
                    report.unexplained += 1;
                    failing = true;
                    if report.failures.len() < 10 {
                        report.failures.push(format!("{at}: both follow the rules there, on the same blocks"));
                    }
                }
            }
        }
    }
    if equal {
        report.equal_chunks += 1;
    }
    if failing {
        report.failing.push((kx, kz));
    }
}

/// Walks the group of neighbouring levels that differ the way `start` does, within the 3 × 3 chunks
/// around `middle`, and says what explains it, and how many levels it holds.
fn explain_group(
    ours: &Ours<'_>,
    server: &ServerLight,
    kind: Kind,
    start: (i32, i32, i32),
    middle: (i32, i32),
    seen: &mut HashSet<(i32, i32, i32)>,
) -> (Explained, usize) {
    let (bottom, top) = server.height;
    let xs = (middle.0 - 1) * 16..(middle.0 + 2) * 16;
    let zs = (middle.1 - 1) * 16..(middle.1 + 2) * 16;
    let brighter = |(x, y, z): (i32, i32, i32)| -> Option<bool> {
        let (o, s) = (ours.level(kind, x, y, z)?, server.level(kind, x, y, z)?);
        (o != s).then_some(s > o)
    };
    let way = brighter(start);
    let mut queue = VecDeque::from([start]);
    let (mut size, mut explained, mut ours_wrong) = (0, None, None);
    while let Some((x, y, z)) = queue.pop_front() {
        size += 1;
        if size <= OURS_CHECKED && ours_wrong.is_none() && ours.follows_rules(kind, x, y, z) == Some(false) {
            ours_wrong = Some((x, y, z));
        }
        if explained.is_none() {
            let blocks_differ = ours.props(x, y, z) != server.props(x, y, z)
                || (kind == Kind::Sky && ours.sky_source(x, y, z) != server.sky_source(x, y, z));
            if blocks_differ {
                explained = Some(Explained::Blocks);
            } else if server.follows_rules(kind, x, y, z) == Some(false) {
                explained = Some(Explained::ServerRule);
            }
        }
        for (dx, dy, dz) in NEIGHBOURS {
            let next = (x + dx, y + dy, z + dz);
            if xs.contains(&next.0) && zs.contains(&next.2) && (bottom..top).contains(&next.1)
                && !seen.contains(&next)
                && brighter(next).is_some()
                && brighter(next) == way
            {
                seen.insert(next);
                queue.push_back(next);
            }
        }
    }
    let explained = match (ours_wrong, explained) {
        (Some(at), _) => Explained::OursWrong(at),
        (None, Some(explained)) => explained,
        (None, None) => Explained::Nothing,
    };
    (explained, size)
}

/// Logs one line per check and returns whether they all passed.
pub fn report(checks: &[Check]) -> bool {
    for check in checks {
        if check.passed {
            info!("PASS {}: {}", check.name, check.detail);
        } else {
            error!("FAIL {}: {}", check.name, check.detail);
        }
    }
    checks.iter().all(|check| check.passed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::level_chunk;

    /// The fire case: the server changes a block after sending the chunk and
    /// never sends an `UpdateBlock`. Our copy then differs, but a fresh encode
    /// decodes equal to the server, so the decoder isn't to blame.
    #[test]
    fn a_silent_server_change_is_told_apart_from_a_decoder_bug() {
        let server = ChunkData::empty(2, -1);
        server
            .section
            .set_block_absolute_y(4, 70, 5, Block::FIRE.default_state.id);
        let (payload, _) = CLevelChunk::encode_chunk(&server, 0, false, &[]).unwrap();
        let ours = level_chunk::decode(&payload).unwrap();
        let names = BlockTable::build();
        let mut out = Vec::new();
        assert_eq!(compare_chunk(&server, &ours, &names, &mut out, 10), 0);

        // The fire goes out on the server only.
        server
            .section
            .set_block_absolute_y(4, 70, 5, Block::AIR.default_state.id);
        assert_eq!(compare_chunk(&server, &ours, &names, &mut out, 10), 1);
        assert!(out[0].starts_with("(36, 70, -11) layer 0: ours fire"), "{}", out[0]);
        assert!(fresh_decode_matches(&server, &names, &mut Vec::new()));
    }
}
