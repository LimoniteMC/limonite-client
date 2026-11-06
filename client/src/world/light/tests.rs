//! The light's tests (m5-plan Step 13), with a slow light of their own to check the flood fills
//! against.

use std::{collections::HashMap, time::Duration};

use pumpkin_data::{Block, BlockState, BlockStateId};
use pumpkin_world::chunk::palette::bedrock_water_state;

use super::{
    Kind, Levels,
    area::spread,
    table::{Props, table},
};
use crate::world::{AIR, World};

fn id(block: &Block) -> u32 {
    BlockState::to_be_network_id(block.default_state.id)
}

fn stone() -> u32 {
    id(&Block::STONE)
}

/// A world of these chunks, all air, waiting for light.
fn world_of(chunks: &[(i32, i32)]) -> World {
    let mut world = World::new();
    for &(x, z) in chunks {
        world.add_empty_chunk(x, z);
    }
    world
}

fn fill(world: &mut World, from: (i32, i32, i32), to: (i32, i32, i32), block: u32) {
    for x in from.0..=to.0 {
        for y in from.1..=to.1 {
            for z in from.2..=to.2 {
                assert!(world.set_block(x, y, z, 0, block), "({x}, {y}, {z}) isn't loaded");
            }
        }
    }
}

fn light_all(world: &mut World) {
    world.update_light(Duration::MAX, (0, 0));
}

fn sky(world: &World, x: i32, y: i32, z: i32) -> u8 {
    world.light().level(Kind::Sky, x, y, z).expect("lit")
}

fn block(world: &World, x: i32, y: i32, z: i32) -> u8 {
    world.light().level(Kind::Block, x, y, z).expect("lit")
}

/// The light by brute force: every block starts at its own light, then takes the best its
/// neighbours offer, sweeping the world again and again until nothing changes. Slow, and nothing
/// like the flood fills, which is why it's here.
struct Reference {
    x0: i32,
    z0: i32,
    wide: i32,
    deep: i32,
    min_y: i32,
    high: i32,
    loaded: Vec<bool>,
    props: Vec<Props>,
}

impl Reference {
    fn new(world: &World) -> Self {
        let xs: Vec<i32> = world.chunks().map(|chunk| chunk.x).collect();
        let zs: Vec<i32> = world.chunks().map(|chunk| chunk.z).collect();
        let first = world.chunks().next().expect("a chunk");
        let (x0, z0) = (*xs.iter().min().unwrap(), *zs.iter().min().unwrap());
        let (wide, deep) = (xs.iter().max().unwrap() - x0 + 1, zs.iter().max().unwrap() - z0 + 1);
        let (min_y, high) = (first.min_y(), first.max_y() - first.min_y());
        let mut reference = Self {
            x0,
            z0,
            wide,
            deep,
            min_y,
            high,
            loaded: vec![false; (wide * deep) as usize],
            props: vec![Props::AIR; (wide * 16 * deep * 16 * high) as usize],
        };
        for chunk in world.chunks() {
            reference.loaded[((chunk.x - x0) * deep + (chunk.z - z0)) as usize] = true;
            for x in 0..16 {
                for z in 0..16 {
                    for y in chunk.min_y()..chunk.max_y() {
                        let wet = chunk.block(x, y, z, 1) != AIR;
                        let at = reference.index((chunk.x - x0) * 16 + x as i32, y - min_y, (chunk.z - z0) * 16 + z as i32);
                        reference.props[at] = table().get(chunk.block(x, y, z, 0), wet);
                    }
                }
            }
        }
        reference
    }

    const fn index(&self, x: i32, y: i32, z: i32) -> usize {
        ((x * self.deep * 16 + z) * self.high + y) as usize
    }

    fn inside(&self, x: i32, y: i32, z: i32) -> bool {
        (0..self.wide * 16).contains(&x)
            && (0..self.deep * 16).contains(&z)
            && (0..self.high).contains(&y)
            && self.loaded[((x >> 4) * self.deep + (z >> 4)) as usize]
    }

    fn levels(&self, kind: Kind) -> Vec<u8> {
        let mut levels = vec![0u8; self.props.len()];
        // Each block's own light.
        for x in 0..self.wide * 16 {
            for z in 0..self.deep * 16 {
                if !self.inside(x, 0, z) {
                    continue;
                }
                let mut run = 15u8;
                for y in (0..self.high).rev() {
                    let props = self.props[self.index(x, y, z)];
                    run = run.saturating_sub(props.dims());
                    levels[self.index(x, y, z)] = match kind {
                        Kind::Sky => run,
                        Kind::Block => props.emits(),
                    };
                }
            }
        }
        let neighbours = [(0, 1, 0), (0, -1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)];
        for sweep in 0.. {
            let mut changed = false;
            let flip = sweep % 2 == 1;
            for i in 0..self.wide * 16 {
                let x = if flip { self.wide * 16 - 1 - i } else { i };
                for j in 0..self.deep * 16 {
                    let z = if flip { self.deep * 16 - 1 - j } else { j };
                    if !self.inside(x, 0, z) {
                        continue;
                    }
                    for y in (0..self.high).rev() {
                        let at = self.index(x, y, z);
                        let dims = self.props[at].dims();
                        let mut best = levels[at];
                        for (dx, dy, dz) in neighbours {
                            let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                            if self.inside(nx, ny, nz) {
                                // From above, the light goes down.
                                best = best.max(spread(kind, levels[self.index(nx, ny, nz)], dims, dy == 1));
                            }
                        }
                        if best != levels[at] {
                            levels[at] = best;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        levels
    }

    /// Checks the world's light against the reference's, everywhere.
    fn check(&self, world: &World) {
        for kind in [Kind::Sky, Kind::Block] {
            let levels = self.levels(kind);
            let mut wrong = Vec::new();
            for x in 0..self.wide * 16 {
                for z in 0..self.deep * 16 {
                    if !self.inside(x, 0, z) {
                        continue;
                    }
                    for y in 0..self.high {
                        let (wx, wy, wz) = (self.x0 * 16 + x, self.min_y + y, self.z0 * 16 + z);
                        let ours = world.light().level(kind, wx, wy, wz).expect("lit");
                        let theirs = levels[self.index(x, y, z)];
                        if ours != theirs {
                            wrong.push(format!("({wx}, {wy}, {wz}): {ours}, reference {theirs}"));
                        }
                    }
                }
            }
            assert!(wrong.is_empty(), "{kind:?} light differs at {} blocks: {:?}", wrong.len(), &wrong[..wrong.len().min(10)]);
        }
    }
}

/// A stone floor at y 60 to 63 under every chunk.
fn floor(world: &mut World, chunks: &[(i32, i32)]) {
    for &(cx, cz) in chunks {
        fill(world, (cx * 16, 60, cz * 16), (cx * 16 + 15, 63, cz * 16 + 15), stone());
    }
}

/// A hollow stone box from `low` to `high`, its walls one block thick.
fn hollow_box(world: &mut World, low: (i32, i32, i32), high: (i32, i32, i32)) {
    fill(world, low, high, stone());
    fill(world, (low.0 + 1, low.1 + 1, low.2 + 1), (high.0 - 1, high.1 - 1, high.2 - 1), AIR);
}

#[test]
fn levels_keep_one_value_until_they_differ() {
    let mut levels = Levels::Uniform(15);
    assert!(!levels.set(100, 15));
    assert_eq!(levels, Levels::Uniform(15));
    assert!(levels.set(4095, 3));
    assert_eq!((levels.get(4095), levels.get(4094), levels.get(0)), (3, 15, 15));
    assert!(levels.set(4094, 7));
    assert_eq!((levels.get(4095), levels.get(4094)), (3, 7));
    levels.compact();
    assert!(matches!(levels, Levels::Mixed(_)));
    levels.set(4095, 15);
    levels.set(4094, 15);
    levels.compact();
    assert_eq!(levels, Levels::Uniform(15));
}

#[test]
fn a_torch_in_a_closed_room() {
    let mut world = world_of(&[(0, 0)]);
    hollow_box(&mut world, (0, 60, 0), (10, 70, 10));
    world.set_block(5, 61, 5, 0, id(&Block::TORCH));
    light_all(&mut world);
    // Pumpkin's torch gives out 14 (m5-plan Deviation 56), and each block on loses 1.
    assert_eq!(block(&world, 5, 61, 5), 14);
    for d in 1..=4 {
        assert_eq!(block(&world, 5 + d, 61, 5), 14 - d as u8, "{d} blocks east");
        assert_eq!(block(&world, 5, 61 + d, 5), 14 - d as u8, "{d} blocks up");
    }
    assert_eq!(block(&world, 1, 61, 1), 6, "eight blocks away, round the corner");
    assert_eq!(block(&world, 1, 69, 1), 0, "sixteen away");
    // The walls take it all, and no sky gets in.
    assert_eq!(block(&world, 10, 61, 5), 0);
    assert_eq!(block(&world, 11, 61, 5), 0);
    assert_eq!(sky(&world, 5, 65, 5), 0);
    assert_eq!(sky(&world, 5, 71, 5), 15, "above the roof");
    Reference::new(&world).check(&world);
}

#[test]
fn sky_light_under_a_roof_and_through_a_hole_in_it() {
    let mut world = world_of(&[(0, 0)]);
    floor(&mut world, &[(0, 0)]);
    fill(&mut world, (4, 70, 4), (11, 70, 11), stone());
    light_all(&mut world);
    assert_eq!(sky(&world, 3, 65, 7), 15, "beside the roof, open to the sky");
    assert_eq!(sky(&world, 4, 65, 7), 14, "just under its edge");
    assert_eq!(sky(&world, 7, 65, 7), 11, "four blocks in");
    assert_eq!(sky(&world, 7, 70, 7), 0, "in the roof");
    assert_eq!(sky(&world, 7, 71, 7), 15, "on it");
    Reference::new(&world).check(&world);

    // A hole: the sky comes straight down it, and spreads from there.
    world.set_block(7, 70, 7, 0, AIR);
    light_all(&mut world);
    assert_eq!(sky(&world, 7, 70, 7), 15);
    assert_eq!(sky(&world, 7, 64, 7), 15, "down to the floor");
    assert_eq!(sky(&world, 8, 65, 8), 13);
    Reference::new(&world).check(&world);

    // And covered again.
    world.set_block(7, 70, 7, 0, stone());
    light_all(&mut world);
    assert_eq!(sky(&world, 7, 65, 7), 11);
    Reference::new(&world).check(&world);
}

#[test]
fn a_torch_taken_away_takes_its_light() {
    let mut world = world_of(&[(0, 0)]);
    hollow_box(&mut world, (0, 60, 0), (12, 70, 12));
    world.set_block(3, 61, 3, 0, id(&Block::TORCH));
    world.set_block(9, 61, 9, 0, id(&Block::LANTERN));
    light_all(&mut world);
    assert_eq!(block(&world, 4, 61, 3), 13, "the torch's, beside it");
    assert_eq!(block(&world, 6, 61, 6), 9, "the lantern's 15 less 6 beats the torch's 14 less 6");

    // The lowering pass: the torch's light goes, the lantern's stays, exactly.
    world.set_block(3, 61, 3, 0, AIR);
    light_all(&mut world);
    assert_eq!(block(&world, 4, 61, 3), 4, "the lantern's, eleven blocks off");
    assert_eq!(block(&world, 3, 61, 3), 3);
    assert_eq!(block(&world, 6, 61, 6), 9);
    Reference::new(&world).check(&world);

    world.set_block(9, 61, 9, 0, AIR);
    light_all(&mut world);
    for (x, z) in [(3, 3), (6, 6), (9, 9)] {
        assert_eq!(block(&world, x, 61, z), 0);
    }
}

#[test]
fn water_dims_the_sky() {
    let mut world = world_of(&[(0, 0)]);
    floor(&mut world, &[(0, 0)]);
    // A pool six deep, with stone round it.
    hollow_box(&mut world, (2, 60, 2), (12, 70, 12));
    fill(&mut world, (2, 70, 2), (12, 70, 12), AIR);
    fill(&mut world, (3, 61, 3), (11, 66, 11), id(&Block::WATER));
    light_all(&mut world);
    // Down through the water, 1 a block, as Pumpkin's water dims by 1.
    for (y, level) in [(67, 15), (66, 14), (65, 13), (61, 9)] {
        assert_eq!(sky(&world, 7, y, 7), level, "y {y}");
    }
    Reference::new(&world).check(&world);
}

#[test]
fn leaves_dim_the_sky_once_straight_down() {
    let mut world = world_of(&[(0, 0)]);
    floor(&mut world, &[(0, 0)]);
    fill(&mut world, (0, 70, 0), (15, 70, 15), id(&Block::OAK_LEAVES));
    light_all(&mut world);
    // Pumpkin's column: 15 less what each block dims, not 1 more a block (m5-plan Deviation 56).
    assert_eq!(sky(&world, 7, 70, 7), 14);
    assert_eq!(sky(&world, 7, 64, 7), 14);
    Reference::new(&world).check(&world);
}

#[test]
fn light_crosses_chunk_borders() {
    let chunks = [(0, 0), (1, 0)];
    let mut world = world_of(&chunks);
    floor(&mut world, &chunks);
    fill(&mut world, (0, 64, 0), (31, 64, 15), stone());
    hollow_box(&mut world, (8, 63, 2), (24, 70, 14));
    world.set_block(15, 64, 8, 0, id(&Block::TORCH));
    light_all(&mut world);
    assert_eq!(block(&world, 16, 64, 8), 13);
    assert_eq!(block(&world, 20, 64, 8), 9);
    Reference::new(&world).check(&world);
}

#[test]
fn a_chunk_lit_before_its_neighbour_arrives_ends_up_the_same() {
    let build = |world: &mut World| {
        floor(world, &[(0, 0), (1, 0)]);
        fill(world, (10, 70, 0), (21, 70, 15), stone());
        world.set_block(15, 64, 8, 0, id(&Block::TORCH));
    };
    // Both chunks lit together.
    let mut together = world_of(&[(0, 0), (1, 0)]);
    build(&mut together);
    light_all(&mut together);

    // One lit first, then the other arrives: the first's blocks are set before it's lit.
    let mut apart = world_of(&[(0, 0)]);
    floor(&mut apart, &[(0, 0)]);
    fill(&mut apart, (10, 70, 0), (15, 70, 15), stone());
    apart.set_block(15, 64, 8, 0, id(&Block::TORCH));
    light_all(&mut apart);
    apart.add_empty_chunk(1, 0);
    floor(&mut apart, &[(1, 0)]);
    fill(&mut apart, (16, 70, 0), (21, 70, 15), stone());
    light_all(&mut apart);

    for cx in 0..2 {
        assert_eq!(together.light().chunk(cx, 0), apart.light().chunk(cx, 0), "chunk {cx}");
    }
    Reference::new(&apart).check(&apart);
}

#[test]
fn the_order_chunks_are_lit_in_makes_no_difference() {
    let chunks = [(0, 0), (1, 0), (0, 1), (1, 1)];
    let build = |around: (i32, i32)| {
        let mut world = world_of(&chunks);
        floor(&mut world, &chunks);
        fill(&mut world, (8, 66, 8), (23, 66, 23), stone());
        for (x, z) in [(15, 15), (16, 16), (10, 20)] {
            world.set_block(x, 64, z, 0, id(&Block::LANTERN));
        }
        // One chunk at a time, nearest `around` first.
        for _ in 0..chunks.len() {
            world.update_light(Duration::from_nanos(1), around);
        }
        world
    };
    let first = build((0, 0));
    for around in [(1, 1), (5, -3), (0, 1)] {
        let other = build(around);
        for &(x, z) in &chunks {
            assert_eq!(first.light().chunk(x, z), other.light().chunk(x, z), "chunk ({x}, {z}), from {around:?}");
        }
    }
    Reference::new(&first).check(&first);
}

#[test]
fn a_dropped_chunk_takes_its_light_with_it() {
    let chunks = [(0, 0), (1, 0)];
    let mut world = world_of(&chunks);
    floor(&mut world, &chunks);
    // A roof over both chunks, open at x 7 and below, and at 28 and above.
    fill(&mut world, (8, 70, 0), (27, 70, 15), stone());
    world.set_block(15, 64, 8, 0, id(&Block::LANTERN));
    light_all(&mut world);
    assert_eq!(block(&world, 17, 64, 8), 13);
    assert_eq!(sky(&world, 16, 64, 8), 6, "from the gone chunk's open side, nine blocks off");

    world.drop_chunk(0, 0);
    assert_eq!(block(&world, 17, 64, 8), 0, "the lantern's gone with its chunk");
    assert_eq!(sky(&world, 16, 64, 8), 3, "now from its own open side, twelve off");
    Reference::new(&world).check(&world);

    // And it comes back.
    world.add_empty_chunk(0, 0);
    floor(&mut world, &[(0, 0)]);
    fill(&mut world, (8, 70, 0), (15, 70, 15), stone());
    world.set_block(15, 64, 8, 0, id(&Block::LANTERN));
    light_all(&mut world);
    assert_eq!(block(&world, 17, 64, 8), 13);
    Reference::new(&world).check(&world);
}

#[test]
fn open_sky_and_solid_rock_keep_one_value_a_sub_chunk() {
    let mut world = world_of(&[(0, 0)]);
    fill(&mut world, (0, 48, 0), (15, 63, 15), stone());
    light_all(&mut world);
    let light = world.light().chunk(0, 0).expect("lit");
    let section = |y: i32| (y / 16 - light.min_section) as usize;
    assert_eq!(light.sky[section(64)], Levels::Uniform(15), "the air over the floor");
    assert_eq!(light.sky[section(300)], Levels::Uniform(15));
    assert_eq!(light.sky[section(50)], Levels::Uniform(0), "the rock");
    assert_eq!(light.sky[section(0)], Levels::Uniform(0), "under it, where no light gets");
    assert!(light.block.iter().all(|levels| *levels == Levels::Uniform(0)));
}

/// A small random number generator, so the test needs nothing and runs the same every time.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: i32) -> i32 {
        (self.next() % n as u64) as i32
    }
}

#[test]
fn edits_end_where_lighting_from_scratch_does() {
    let chunks = [(0, 0), (1, 0), (0, 1), (1, 1)];
    let palette = [
        AIR,
        AIR,
        AIR,
        stone(),
        stone(),
        id(&Block::GLASS),
        id(&Block::OAK_LEAVES),
        id(&Block::WATER),
        id(&Block::TORCH),
        id(&Block::LANTERN),
        id(&Block::GLOWSTONE),
        id(&Block::OAK_SLAB),
    ];
    let mut world = world_of(&chunks);
    floor(&mut world, &chunks);
    light_all(&mut world);
    let mut placed = HashMap::new();
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for edit in 1..=240 {
        let at = (8 + rng.below(16), 64 + rng.below(10), 8 + rng.below(16));
        let what = palette[rng.below(palette.len() as i32) as usize];
        world.set_block(at.0, at.1, at.2, 0, what);
        placed.insert(at, what);
        light_all(&mut world);
        if edit % 40 == 0 {
            // The same blocks, lit in one go.
            let mut fresh = world_of(&chunks);
            floor(&mut fresh, &chunks);
            for (&(x, y, z), &what) in &placed {
                fresh.set_block(x, y, z, 0, what);
            }
            light_all(&mut fresh);
            for &(x, z) in &chunks {
                assert_eq!(world.light().chunk(x, z), fresh.light().chunk(x, z), "chunk ({x}, {z}) after {edit} edits");
            }
        }
    }
    Reference::new(&world).check(&world);
}

#[test]
fn the_table_has_every_java_states_light() {
    let mut differ = Vec::new();
    let ambiguous: Vec<_> = table().ambiguous().iter().map(|(key, _)| *key).collect();
    for raw in 0..BlockStateId::COUNT {
        let Some(state) = BlockStateId::new(raw) else {
            continue;
        };
        let key = (BlockState::to_be_network_id(state), bedrock_water_state(state) != Block::AIR.default_state.id);
        if table().get(key.0, key.1) != Props::of_state(state) {
            assert!(ambiguous.contains(&key), "{} gets the wrong light", Block::from_state_id(state).name);
            differ.push(Block::from_state_id(state).name);
        }
    }
    differ.sort_unstable();
    differ.dedup();
    // Bedrock keeps whether a piston is pushed out in its block actor.
    assert_eq!(differ, ["piston", "sticky_piston", "test_instance_block"]);
    // A pushed-out piston dims by 1, a pulled-in one by 15, and the table draws them pulled in.
    let piston = BlockState::to_be_network_id(Block::PISTON.default_state.id);
    assert_eq!(table().get(piston, false), Props::new(15, 0));
}
