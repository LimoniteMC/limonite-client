//! `--debug-world` (m5-plan Step 7d): a flat world holding every block state, to go
//! and look at them, as Java's debug world has.
//!
//! **What's in it:** one block for each of the Bedrock ids Pumpkin can send, in a grid
//! with a gap between blocks. Java states that share a Bedrock id look the same to a
//! Bedrock client, so one stands for them all. Beside the grid is a corner of water
//! and lava, as pools, flows and falls, since nothing in the grid flows.
//!
//! **How it gets there:** it's written straight into the server's chunks through
//! Pumpkin's public API, each time the world opens, so nothing has to be added to the
//! fork. Going through chunks skips everything a placed block would normally set off:
//! sand doesn't fall, and doors don't break for want of a floor. The rules that would
//! still change it, random ticks, mobs and the time of day, are off in a new world's
//! `level.dat`.
//!
//! **Getting around:** `]` and `[` go to the next and last block, `.` and `,` to the
//! next and last state of it, and typing in the terminal goes to what matches. The
//! title bar says where you are.

use std::{collections::BTreeMap, io, path::Path, sync::mpsc, thread};

use pumpkin_data::{Block, BlockState, BlockStateId, dimension::Dimension};
use pumpkin_util::{math::vector2::Vector2, world_seed::Seed};
use pumpkin_world::{
    chunk::io::Dirtiable,
    world_info::{LevelData, WorldInfoWriter, anvil::AnvilLevelInfo},
};
use tracing::{info, warn};

use crate::{
    blocks::rules::JavaBlock,
    embedded::EmbeddedServer,
    player::EYE_HEIGHT,
    render::camera::look_direction,
    world::BlockTable,
};

/// The top of the classic flat preset's ground: blocks stand on it from here.
pub const SURFACE: i32 = -60;
/// Between one block in the grid and the next.
const SPACING: i32 = 2;
/// How far from a block a jump lands.
const VIEW_DISTANCE: f64 = 5.0;
/// How far from the middle of a scene: they're a dozen blocks across.
const SCENE_DISTANCE: f64 = 18.0;
/// The view a jump lands with: as the contact sheets have it, from the south-east.
const VIEW_PITCH: f32 = 35.0;
const VIEW_YAW: f32 = 135.0;
/// Where the scenes start, west of the grid, and how far apart they are.
const SCENES_X: i32 = -24;
const SCENE_SPACING: i32 = 24;

/// A level.dat for a new debug world: flat, with nothing to change what's placed.
fn level_data(seed: Seed) -> LevelData {
    let mut data = LevelData::default_with_preset(seed, "flat");
    data.spawn_y = SURFACE;
    let rules = &mut data.game_rules;
    rules.random_tick_speed = 0;
    rules.advance_time = false;
    rules.advance_weather = false;
    rules.spawn_mobs = false;
    rules.spawn_monsters = false;
    rules.spawn_patrols = false;
    rules.spawn_phantoms = false;
    rules.spawn_wandering_traders = false;
    rules.spawn_wardens = false;
    data
}

/// Makes the world folder a debug world if it isn't a world yet. It must happen before
/// the server starts, as the server makes a normal world where it finds none.
///
/// # Errors
/// If the world's files can't be written.
pub fn prepare(world: &Path, seed: Seed) -> Result<(), String> {
    if world.join("level.dat").exists() {
        return Ok(());
    }
    std::fs::create_dir_all(world).map_err(|err| format!("can't make {}: {err}", world.display()))?;
    AnvilLevelInfo
        .write_world_info(&level_data(seed), world)
        .map_err(|err| format!("can't write the debug world's level.dat: {err}"))?;
    info!("A new flat world for the debug world is at {}", world.display());
    Ok(())
}

/// A set of blocks to look at together, around its own origin.
pub struct Scene {
    pub name: &'static str,
    pub blocks: Vec<([i32; 3], BlockStateId)>,
}

/// A liquid's Java state at this level: 0 is a source, 1 to 7 flow, and 8 falls.
fn liquid(block: &Block, level: usize) -> BlockStateId {
    block.states[level].id
}

/// A pool of sources in a low wall.
fn pool(block: &Block) -> Vec<([i32; 3], BlockStateId)> {
    let mut blocks = Vec::new();
    for x in -4i32..=4 {
        for z in -4i32..=4 {
            let wall = x.abs() == 4 || z.abs() == 4;
            let state = if wall { Block::STONE_BRICKS.default_state.id } else { liquid(block, 0) };
            blocks.push(([x, 0, z], state));
        }
    }
    blocks
}

/// What a source does on flat ground: a diamond that loses `step` levels a block.
fn flow(block: &Block, reach: i32, step: i32) -> Vec<([i32; 3], BlockStateId)> {
    let mut blocks = Vec::new();
    for x in -reach..=reach {
        for z in -reach..=reach {
            let distance = x.abs() + z.abs();
            if distance <= reach {
                blocks.push(([x, 0, z], liquid(block, (distance * step) as usize)));
            }
        }
    }
    blocks
}

/// A source held up, falling, and spreading where it lands.
fn fall(block: &Block, reach: i32, step: i32) -> Vec<([i32; 3], BlockStateId)> {
    let mut blocks = flow(block, reach, step);
    blocks.retain(|(at, _)| *at != [0, 0, 0]);
    for y in 0..6 {
        blocks.push(([0, y, 0], liquid(block, 8)));
    }
    blocks.push(([0, 6, 0], liquid(block, 0)));
    blocks
}

/// Water and lava, set out by hand. Water loses one level a block, and lava two.
#[must_use]
pub fn fluid_scenes() -> Vec<Scene> {
    let (water, lava) = (&Block::WATER, &Block::LAVA);
    vec![
        Scene {
            name: "water pool",
            blocks: pool(water),
        },
        Scene {
            name: "water flow",
            blocks: flow(water, 7, 1),
        },
        Scene {
            name: "waterfall",
            blocks: fall(water, 7, 1),
        },
        Scene {
            name: "lava pool",
            blocks: pool(lava),
        },
        Scene {
            name: "lava flow",
            blocks: flow(lava, 3, 2),
        },
        Scene {
            name: "lavafall",
            blocks: fall(lava, 3, 2),
        },
    ]
}

/// One place to go.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The block's name, or the scene's.
    pub name: String,
    /// The block's properties, `key=value` with spaces between.
    pub state: String,
    /// The middle of what's to be looked at.
    target: [f64; 3],
    distance: f64,
}

/// The whole world: what goes where, and the places to go.
pub struct Layout {
    pub entries: Vec<Entry>,
    blocks: Vec<([i32; 3], BlockStateId)>,
}

impl Layout {
    #[must_use]
    pub fn build(table: &BlockTable) -> Self {
        // Java's order, so a block's states stay together, and so do its relatives.
        let states: Vec<BlockStateId> = (0..BlockStateId::COUNT)
            .filter_map(BlockStateId::new)
            .filter(|&state| {
                table.shape_state(BlockState::to_be_network_id(state)) == Some(state)
                    && !matches!(Block::from_state_id(state).name, "air" | "cave_air" | "void_air")
            })
            .collect();
        let side = (states.len() as f64).sqrt().ceil() as i32;
        let mut entries = Vec::with_capacity(states.len());
        let mut blocks = Vec::with_capacity(states.len());
        for (scene, offset) in fluid_scenes().into_iter().zip(0..) {
            let origin = [SCENES_X, SURFACE, offset * SCENE_SPACING];
            blocks.extend(scene.blocks.into_iter().map(|(at, state)| {
                ([origin[0] + at[0], origin[1] + at[1], origin[2] + at[2]], state)
            }));
            entries.push(Entry {
                name: scene.name.to_string(),
                state: String::new(),
                target: [f64::from(origin[0]) + 0.5, f64::from(origin[1]) + 1.0, f64::from(origin[2]) + 0.5],
                distance: SCENE_DISTANCE,
            });
        }
        for (index, state) in (0..).zip(states) {
            let at = [(index % side) * SPACING, SURFACE, (index / side) * SPACING];
            let java = JavaBlock::of(state);
            let props: Vec<String> = java.props().iter().map(|(key, value)| format!("{key}={value}")).collect();
            entries.push(Entry {
                name: java.name.to_string(),
                state: props.join(" "),
                target: [f64::from(at[0]) + 0.5, f64::from(at[1]) + 0.5, f64::from(at[2]) + 0.5],
                distance: VIEW_DISTANCE,
            });
            blocks.push((at, state));
        }
        Self { entries, blocks }
    }
}

/// Writes the layout into the server's chunks.
pub fn place(server: &EmbeddedServer, layout: &Layout) {
    let start = std::time::Instant::now();
    let mut by_chunk: BTreeMap<(i32, i32), Vec<([i32; 3], BlockStateId)>> = BTreeMap::new();
    for &(at, state) in &layout.blocks {
        by_chunk.entry((at[0].div_euclid(16), at[2].div_euclid(16))).or_default().push((at, state));
    }
    let level = server.server.get_world_from_dimension(&Dimension::OVERWORLD).level.clone();
    let chunks = by_chunk.len();
    server.runtime.block_on(async {
        for (&(x, z), blocks) in &by_chunk {
            level
                .get_or_fetch_chunk(Vector2::new(x, z), |chunk| {
                    for (at, state) in blocks {
                        let (column_x, column_z) = (at[0].rem_euclid(16) as usize, at[2].rem_euclid(16) as usize);
                        if chunk.set_block_absolute_y_if(column_x, at[1], column_z, *state, &|_| true).is_none() {
                            warn!("Couldn't place a debug block at {at:?}");
                        }
                    }
                    chunk.mark_dirty(true);
                })
                .await;
        }
    });
    info!(
        "Debug world: {} places to go, {} blocks in {chunks} chunks, written in {:.0} ms",
        layout.entries.len(),
        layout.blocks.len(),
        start.elapsed().as_secs_f64() * 1000.0,
    );
}

/// Where you are in the debug world.
pub struct Browser {
    entries: Vec<Entry>,
    at: usize,
    /// Lines typed in the terminal.
    typed: mpsc::Receiver<String>,
}

impl Browser {
    /// Starts at what `goto` matches, or at the first block.
    #[must_use]
    pub fn new(layout: Layout, goto: Option<&str>) -> Self {
        let (sender, typed) = mpsc::channel();
        // It ends when the terminal does. Nothing waits for it.
        let spawned = thread::Builder::new().name("debug world input".into()).spawn(move || {
            for line in io::stdin().lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        if let Err(err) = spawned {
            warn!("Typing names won't work: {err}");
        }
        let mut browser = Self {
            entries: layout.entries,
            at: 0,
            typed,
        };
        if let Some(goto) = goto
            && !browser.find(goto)
        {
            warn!("Nothing in the debug world matches `{goto}`");
        }
        browser
    }

    #[must_use]
    pub fn current(&self) -> &Entry {
        &self.entries[self.at]
    }

    /// What the title bar says about where you are.
    #[must_use]
    pub fn label(&self) -> String {
        let entry = self.current();
        format!("{} {} [{} of {}]", entry.name, entry.state, self.at + 1, self.entries.len())
    }

    /// The feet, yaw and pitch that look at the current entry.
    #[must_use]
    pub fn pose(&self) -> ([f64; 3], f32, f32) {
        let entry = self.current();
        let forward = look_direction(VIEW_PITCH, VIEW_YAW);
        let feet = [
            entry.target[0] - f64::from(forward.x) * entry.distance,
            entry.target[1] - f64::from(forward.y) * entry.distance - EYE_HEIGHT,
            entry.target[2] - f64::from(forward.z) * entry.distance,
        ];
        (feet, VIEW_YAW, VIEW_PITCH)
    }

    /// To the next state, or the last.
    pub fn step_state(&mut self, forward: bool) {
        let count = self.entries.len();
        self.at = if forward { (self.at + 1) % count } else { (self.at + count - 1) % count };
    }

    /// To the next block's first state, or to the last block's.
    pub fn step_block(&mut self, forward: bool) {
        let count = self.entries.len();
        let name = self.current().name.clone();
        if forward {
            for offset in 1..count {
                let at = (self.at + offset) % count;
                if self.entries[at].name != name {
                    self.at = at;
                    return;
                }
            }
        } else {
            // Back to the start of this block, then on to the one before.
            let mut at = self.at;
            while self.entries[at].name == name {
                at = (at + count - 1) % count;
                if at == self.at {
                    return;
                }
            }
            let last = self.entries[at].name.clone();
            while self.entries[(at + count - 1) % count].name == last {
                at = (at + count - 1) % count;
            }
            self.at = at;
        }
    }

    /// Goes to the first entry after this one that matches: a block's exact name
    /// first, and otherwise one with every word of `text` in its name or state. Asking
    /// again goes on to the next. Whether anything matched.
    pub fn find(&mut self, text: &str) -> bool {
        let text = text.trim().to_lowercase();
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return false;
        }
        let count = self.entries.len();
        let order = (1..=count).map(|offset| (self.at + offset) % count);
        let exact = order.clone().find(|&at| self.entries[at].name == text);
        let loose = || {
            order.clone().find(|&at| {
                let entry = &self.entries[at];
                words.iter().all(|word| entry.name.contains(word) || entry.state.contains(word))
            })
        };
        match exact.or_else(loose) {
            Some(at) => {
                self.at = at;
                true
            }
            None => false,
        }
    }

    /// The last line typed in the terminal, if one has come in.
    pub fn take_typed(&mut self) -> Option<String> {
        self.typed.try_iter().last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, state: &str) -> Entry {
        Entry {
            name: name.to_string(),
            state: state.to_string(),
            target: [0.0; 3],
            distance: VIEW_DISTANCE,
        }
    }

    fn browser(entries: Vec<Entry>) -> Browser {
        let (_, typed) = mpsc::channel();
        Browser { entries, at: 0, typed }
    }

    fn sample() -> Browser {
        browser(vec![
            entry("stone", ""),
            entry("oak_door", "facing=east half=lower"),
            entry("oak_door", "facing=north half=lower"),
            entry("oak_door", "facing=north half=upper"),
            entry("iron_door", "facing=east"),
            entry("door", ""),
        ])
    }

    #[test]
    fn the_grid_has_every_bedrock_id_once_with_a_gap_between_blocks() {
        let table = BlockTable::build();
        let layout = Layout::build(&table);
        let scenes = fluid_scenes();
        let in_grid: Vec<_> = layout.blocks.iter().filter(|(at, _)| at[0] >= 0).collect();
        // Every Bedrock id, but the air.
        let mut ids: Vec<u32> = in_grid.iter().map(|(_, state)| BlockState::to_be_network_id(*state)).collect();
        let placed = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), placed, "no id twice");
        let mut all: Vec<u32> = (0..BlockStateId::COUNT)
            .filter_map(BlockStateId::new)
            .filter(|&state| !matches!(Block::from_state_id(state).name, "air" | "cave_air" | "void_air"))
            .map(BlockState::to_be_network_id)
            .collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(ids, all, "every id Pumpkin can send");
        assert_eq!(layout.entries.len(), placed + scenes.len());
        // None touches another, or a scene.
        for (at, _) in &in_grid {
            assert_eq!(at[0] % SPACING, 0);
            assert_eq!(at[2] % SPACING, 0);
            assert_eq!(at[1], SURFACE);
        }
        let mut places: Vec<[i32; 3]> = in_grid.iter().map(|(at, _)| *at).collect();
        places.sort_unstable();
        places.dedup();
        assert_eq!(places.len(), placed, "no two in one place");
    }

    #[test]
    fn liquids_have_the_levels_their_scenes_ask_for() {
        for level in 0..=8usize {
            let java = JavaBlock::of(liquid(&Block::WATER, level));
            assert_eq!(java.get("level"), Some(level.to_string().as_str()));
            let java = JavaBlock::of(liquid(&Block::LAVA, level));
            assert_eq!(java.get("level"), Some(level.to_string().as_str()));
        }
        let flow = flow(&Block::WATER, 7, 1);
        assert_eq!(flow.iter().filter(|(at, _)| at[0].abs() + at[2].abs() == 7).count(), 28);
    }

    #[test]
    fn a_new_world_is_flat_with_nothing_to_change_it() {
        let data = level_data(Seed(1));
        let generator = &data.world_gen_settings.dimensions[Dimension::OVERWORLD.minecraft_name].generator;
        assert_eq!(generator.generator_type, "minecraft:flat");
        assert_eq!(data.game_rules.random_tick_speed, 0);
        assert!(!data.game_rules.spawn_mobs && !data.game_rules.advance_time);
        // The ground stops just below where the blocks start.
        let flat = generator.settings.as_ref().and_then(pumpkin_world::world_info::GeneratorSettings::as_flat_settings);
        let height: i32 = flat.expect("flat settings").to_flat_layers().iter().map(|layer| layer.height).sum();
        assert_eq!(Dimension::OVERWORLD.min_y + height, SURFACE);
    }

    #[test]
    fn states_and_blocks_step_forwards_and_back_and_wrap_around() {
        let mut browser = sample();
        browser.step_state(true);
        assert_eq!(browser.at, 1);
        browser.step_state(false);
        browser.step_state(false);
        assert_eq!(browser.at, 5);
        browser.at = 0;
        browser.step_block(true);
        assert_eq!(browser.at, 1, "the door's first state");
        browser.step_block(true);
        assert_eq!(browser.at, 4, "past the door's other states");
        browser.step_block(false);
        assert_eq!(browser.at, 1, "back to the door's first state");
        browser.step_block(false);
        assert_eq!(browser.at, 0);
        browser.step_block(false);
        assert_eq!(browser.at, 5, "wrapping around");
    }

    #[test]
    fn typing_finds_exact_names_first_then_every_word_and_goes_on_when_asked_again() {
        let mut browser = sample();
        assert!(browser.find("door"));
        assert_eq!(browser.at, 5, "the exact name comes before names holding it");
        assert!(browser.find(" OAK_DOOR "));
        assert_eq!(browser.at, 1);
        assert!(browser.find("oak_door"));
        assert_eq!(browser.at, 2, "asked again, the next");
        assert!(browser.find("door upper"));
        assert_eq!(browser.at, 3);
        assert!(!browser.find("no such thing"));
        assert_eq!(browser.at, 3, "it stays where it was");
        assert!(!browser.find("   "));
    }

    #[test]
    fn a_jump_lands_south_east_of_a_block_and_looks_at_it() {
        let mut browser = browser(vec![Entry {
            name: "x".to_string(),
            state: String::new(),
            target: [10.5, -59.5, 20.5],
            distance: VIEW_DISTANCE,
        }]);
        browser.at = 0;
        let (feet, yaw, pitch) = browser.pose();
        assert!(feet[0] > 10.5 && feet[2] > 20.5 && feet[1] > -59.5, "{feet:?}");
        let eyes = glam::DVec3::new(feet[0], feet[1] + EYE_HEIGHT, feet[2]);
        let looking = eyes + look_direction(pitch, yaw).as_dvec3() * VIEW_DISTANCE;
        assert!(looking.distance(glam::DVec3::new(10.5, -59.5, 20.5)) < 1e-4);
    }
}
