//! What the player is aiming at (m5-plan Decision 8).
//!
//! **A ray through a grid** (worth teaching). Testing every block near the
//! player would work, but a ray only ever passes through a dozen. So it walks
//! them in order, from the block the eyes are in to the next one it enters
//! (Amanatides and Woo's traversal). On each axis it keeps how far along the ray
//! the next block boundary is. The nearest of the three says which neighbour
//! comes next, and that axis's boundary then moves on by one block's worth of
//! ray. Only a few additions a step, and it can't skip a block.
//!
//! The first block with an outline box in the ray's way is the answer. Air and
//! liquids have no outline boxes, so the ray passes through them.

use glam::{DVec3, IVec3};
use pumpkin_data::BlockDirection;
use pumpkin_util::GameMode;

use crate::{
    blocks::shapes::{self, Aabb},
    game::Game,
    render::camera::Camera,
    world::World,
};

/// How far a player reaches, in blocks: Pumpkin's numbers
/// (`Player::block_interaction_range`). The server allows one more, so an edit
/// in reach is never refused for distance.
const SURVIVAL_REACH: f64 = 4.5;
const CREATIVE_REACH: f64 = 5.0;

/// What a ray hit.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub block: IVec3,
    /// The face it came in through, whose numbers are Bedrock's: Step 11's
    /// packets say which was clicked.
    pub face: BlockDirection,
    /// Exactly where, in world blocks: Step 11's place packet says this too.
    pub point: DVec3,
    /// The block's outline boxes, in world blocks.
    pub boxes: Vec<Aabb>,
}

/// How far a player in `gamemode` reaches. A spectator can't aim at anything.
const fn reach(gamemode: GameMode) -> Option<f64> {
    match gamemode {
        GameMode::Creative => Some(CREATIVE_REACH),
        GameMode::Spectator => None,
        GameMode::Survival | GameMode::Adventure => Some(SURVIVAL_REACH),
    }
}

/// What the camera is aimed at in `game`, within the player's reach. Frames
/// call this, so the outline follows the mouse between ticks.
#[must_use]
pub fn from_camera(game: &Game, camera: &Camera) -> Option<Hit> {
    let reach = reach(game.player()?.gamemode)?;
    in_world(game.world(), camera.position, camera.forward().as_dvec3(), reach)
}

/// What a ray from `eyes` along `look` hits in `world`, within `reach`.
#[must_use]
pub fn in_world(world: &World, eyes: DVec3, look: DVec3, reach: f64) -> Option<Hit> {
    trace(eyes, look, reach, |block, out| {
        if let Some(id) = world.block(block.x, block.y, block.z, 0) {
            shapes::outline_boxes(world.blocks(), id, block.x, block.y, block.z, out);
        }
    })
}

/// What a ray from `eyes` along `look`, one block long, hits within `reach`.
/// `outlines` adds a block's outline boxes, in world blocks.
pub fn trace(eyes: DVec3, look: DVec3, reach: f64, mut outlines: impl FnMut(IVec3, &mut Vec<Aabb>)) -> Option<Hit> {
    let mut block = eyes.floor().as_ivec3();
    // Along the ray, on each axis: how far to the next block boundary, and how
    // far from one boundary to the next. A ray that never moves on an axis
    // never reaches a boundary on it.
    let mut next = DVec3::INFINITY;
    let mut each = DVec3::INFINITY;
    let mut step = IVec3::ZERO;
    for axis in 0..3 {
        if look[axis] != 0.0 {
            let forward = look[axis] > 0.0;
            let boundary = f64::from(block[axis] + i32::from(forward));
            next[axis] = (boundary - eyes[axis]) / look[axis];
            each[axis] = look[axis].recip().abs();
            step[axis] = if forward { 1 } else { -1 };
        }
    }

    let mut boxes = Vec::new();
    let mut crossed = 0.0;
    while crossed <= reach {
        boxes.clear();
        outlines(block, &mut boxes);
        let nearest = boxes
            .iter()
            .filter_map(|area| entry(area, eyes, look, reach))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((distance, across)) = nearest {
            return Some(Hit {
                block,
                face: face(across.unwrap_or_else(|| main_axis(look)), look),
                point: eyes + look * distance,
                boxes,
            });
        }
        let axis = if next.x < next.y && next.x < next.z {
            0
        } else if next.y < next.z {
            1
        } else {
            2
        };
        crossed = next[axis];
        block[axis] += step[axis];
        next[axis] += each[axis];
    }
    None
}

/// How far along the ray it enters `area`, within `reach`, and which axis's
/// faces it came across. It starts inside `area` if no axis is named (worth
/// teaching).
///
/// A box is where three slabs overlap, one an axis. The ray is in each slab for
/// a stretch of its length, and it's in the box where all three stretches
/// overlap: from the latest entry to the earliest exit. If the ray leaves one
/// slab before it enters another, it misses.
fn entry(area: &Aabb, eyes: DVec3, look: DVec3, reach: f64) -> Option<(f64, Option<usize>)> {
    let (mut near, mut far, mut across) = (0.0, reach, None);
    for axis in 0..3 {
        if look[axis] == 0.0 {
            if eyes[axis] <= area.min[axis] || eyes[axis] >= area.max[axis] {
                return None;
            }
            continue;
        }
        let low = (area.min[axis] - eyes[axis]) / look[axis];
        let high = (area.max[axis] - eyes[axis]) / look[axis];
        let (enters, leaves) = if low < high { (low, high) } else { (high, low) };
        if enters > near {
            near = enters;
            across = Some(axis);
        }
        far = far.min(leaves);
    }
    (near <= far && far > 0.0).then_some((near, across))
}

/// The axis `look` goes furthest along: which face a ray starting inside a
/// block counts as coming through, as in vanilla.
fn main_axis(look: DVec3) -> usize {
    let size = look.abs();
    if size.x >= size.y && size.x >= size.z {
        0
    } else if size.y >= size.z {
        1
    } else {
        2
    }
}

/// The face a ray going along `look` comes in through, across `axis`: going
/// east, it comes through the west face.
fn face(axis: usize, look: DVec3) -> BlockDirection {
    match (axis, look[axis] > 0.0) {
        (0, true) => BlockDirection::West,
        (0, false) => BlockDirection::East,
        (1, true) => BlockDirection::Down,
        (1, false) => BlockDirection::Up,
        (_, true) => BlockDirection::North,
        (_, false) => BlockDirection::South,
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_data::{Block, BlockState};

    use super::*;
    use crate::world::BlockTable;

    /// Whole blocks wherever `solid` says.
    fn cubes(solid: impl Fn(IVec3) -> bool) -> impl FnMut(IVec3, &mut Vec<Aabb>) {
        move |block, out| {
            if solid(block) {
                out.push(Aabb::cube(block.x, block.y, block.z));
            }
        }
    }

    /// Real blocks from Pumpkin, through the same table the game uses.
    fn real(table: &BlockTable, at: impl Fn(IVec3) -> &'static Block) -> impl FnMut(IVec3, &mut Vec<Aabb>) {
        move |block, out| {
            let id = BlockState::to_be_network_id(at(block).default_state.id);
            shapes::outline_boxes(table, id, block.x, block.y, block.z, out);
        }
    }

    #[test]
    fn the_near_face_is_hit() {
        let stone = IVec3::new(3, 64, 0);
        let centre = stone.as_dvec3() + DVec3::splat(0.5);
        let world = || cubes(move |block| block == stone);
        for (look, face, along) in [
            (DVec3::X, BlockDirection::West, 3.0),
            (DVec3::NEG_X, BlockDirection::East, 4.0),
            (DVec3::Z, BlockDirection::North, 0.0),
            (DVec3::NEG_Z, BlockDirection::South, 1.0),
            (DVec3::Y, BlockDirection::Down, 64.0),
            (DVec3::NEG_Y, BlockDirection::Up, 65.0),
        ] {
            let hit = trace(centre - look * 3.0, look, 5.0, world()).expect("the stone");
            assert_eq!((hit.block, hit.face), (stone, face), "{look}");
            let axis = main_axis(look);
            assert!((hit.point[axis] - along).abs() < 1e-9, "{look}: {:?}", hit.point);
        }
    }

    #[test]
    fn air_and_liquids_are_passed_through() {
        let table = BlockTable::build();
        let at = |block: IVec3| match block.x {
            1 => &Block::WATER,
            2 => &Block::LAVA,
            3 => &Block::STONE,
            _ => &Block::AIR,
        };
        let hit = trace(DVec3::new(0.5, 64.5, 0.5), DVec3::X, 5.0, real(&table, at)).expect("the stone");
        assert_eq!(hit.block, IVec3::new(3, 64, 0));
        assert_eq!(hit.face, BlockDirection::West);
    }

    #[test]
    fn a_slab_is_hit_at_its_top_half_a_block_up() {
        let table = BlockTable::build();
        let at = |block: IVec3| if block == IVec3::new(0, 64, 0) { &Block::OAK_SLAB } else { &Block::AIR };
        let hit = trace(DVec3::new(0.5, 66.5, 0.5), DVec3::NEG_Y, 5.0, real(&table, at)).expect("the slab");
        assert_eq!(hit.face, BlockDirection::Up);
        assert!((hit.point.y - 64.5).abs() < 1e-9, "{:?}", hit.point);
        // Over the slab, through the top half of its block, is clear.
        assert_eq!(trace(DVec3::new(-2.5, 64.75, 0.5), DVec3::X, 5.0, real(&table, at)), None);
        // Through the bottom half isn't.
        assert!(trace(DVec3::new(-2.5, 64.25, 0.5), DVec3::X, 5.0, real(&table, at)).is_some());
    }

    #[test]
    fn a_fence_post_is_hit_at_its_own_box() {
        let table = BlockTable::build();
        let at = |block: IVec3| if block == IVec3::new(0, 64, 0) { &Block::OAK_FENCE } else { &Block::AIR };
        let hit = trace(DVec3::new(-2.5, 64.5, 0.5), DVec3::X, 5.0, real(&table, at)).expect("the post");
        assert!((hit.point.x - 0.375).abs() < 1e-9, "{:?}", hit.point);
        // Beside the post, through its block, is clear.
        assert_eq!(trace(DVec3::new(-2.5, 64.5, 0.1), DVec3::X, 5.0, real(&table, at)), None);
    }

    #[test]
    fn nothing_is_hit_beyond_the_reach() {
        let world = || cubes(|block| block.x == 5);
        let eyes = DVec3::new(0.5, 64.5, 0.5);
        // The wall's face is 4.5 away: survival just reaches it.
        assert!(trace(eyes, DVec3::X, SURVIVAL_REACH, world()).is_some());
        assert_eq!(trace(eyes, DVec3::X, SURVIVAL_REACH - 0.01, world()), None);
        // One a block further is only in creative's reach.
        let further = || cubes(|block| block.x == 5 && block.z == 1);
        let across = DVec3::new(0.9, 0.0, 0.1).normalize();
        assert!(trace(eyes, across, CREATIVE_REACH, further()).is_some());
        assert_eq!(trace(eyes, across, SURVIVAL_REACH, further()), None);
        assert_eq!(reach(GameMode::Survival), Some(4.5));
        assert_eq!(reach(GameMode::Adventure), Some(4.5));
        assert_eq!(reach(GameMode::Creative), Some(5.0));
        assert_eq!(reach(GameMode::Spectator), None);
    }

    #[test]
    fn a_ray_starting_inside_a_block_hits_it() {
        let hit = trace(DVec3::new(0.5, 64.5, 0.5), DVec3::X, 5.0, cubes(|_| true)).expect("the block it's in");
        assert_eq!((hit.block, hit.face), (IVec3::new(0, 64, 0), BlockDirection::West));
        assert_eq!(hit.point, DVec3::new(0.5, 64.5, 0.5));
    }

    #[test]
    fn a_ray_with_nowhere_to_go_ends() {
        assert_eq!(trace(DVec3::new(0.5, 64.5, 0.5), DVec3::ZERO, 5.0, cubes(|_| false)), None);
        assert_eq!(trace(DVec3::splat(0.5), DVec3::splat(f64::NAN), 5.0, cubes(|_| false)), None);
    }

    /// A small, repeatable stream of numbers from 0 to 1.
    fn numbers() -> impl FnMut() -> f64 {
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1_u64 << 53) as f64
        }
    }

    /// The walk has to find what testing every block finds, whichever way it
    /// goes and wherever it is, on either side of every axis's zero.
    #[test]
    fn the_walk_finds_what_testing_every_block_finds() {
        let mut number = numbers();
        // Solid here and there, and always with a box smaller than its block.
        let scattered = |block: IVec3| (block.x * 7 + block.y * 13 + block.z * 19).rem_euclid(11) == 0;
        let inset = |block: IVec3| {
            let mut area = Aabb::cube(block.x, block.y, block.z).shrunk(DVec3::splat(0.125));
            area.max.y -= 0.25;
            area
        };
        let mut hits = 0;
        for &origin in &[
            DVec3::ZERO,
            DVec3::new(-535.88, -42.0, -10.13),
            DVec3::new(1000.0, 64.0, -5000.0),
            DVec3::new(29_999_984.0, 300.0, -29_999_984.0),
        ] {
            for _ in 0..5000 {
                let eyes = origin + DVec3::new(number(), number(), number()) * 4.0 - DVec3::splat(2.0);
                let look = DVec3::new(number() - 0.5, number() - 0.5, number() - 0.5).normalize();
                let found = trace(eyes, look, CREATIVE_REACH, |block, out| {
                    if scattered(block) {
                        out.push(inset(block));
                    }
                });

                // Every block in reach, the nearest hit first.
                let (low, high) = (
                    (eyes - DVec3::splat(CREATIVE_REACH)).floor().as_ivec3(),
                    (eyes + DVec3::splat(CREATIVE_REACH)).floor().as_ivec3(),
                );
                let mut nearest: Option<(f64, IVec3)> = None;
                for x in low.x..=high.x {
                    for y in low.y..=high.y {
                        for z in low.z..=high.z {
                            let block = IVec3::new(x, y, z);
                            if scattered(block)
                                && let Some((distance, _)) = entry(&inset(block), eyes, look, CREATIVE_REACH)
                                && nearest.is_none_or(|(best, _)| distance < best)
                            {
                                nearest = Some((distance, block));
                            }
                        }
                    }
                }
                assert_eq!(found.as_ref().map(|hit| hit.block), nearest.map(|(_, block)| block), "from {eyes} along {look}");
                hits += i32::from(found.is_some());
            }
        }
        assert!(hits > 5000, "only {hits} of the rays hit anything");
    }
}
