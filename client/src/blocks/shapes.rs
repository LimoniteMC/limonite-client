//! Block shapes by Bedrock id, from pumpkin-data (m5-plan Decision 7,
//! Finding 9): the collision boxes physics walks on, and the outline boxes
//! the mesher draws for blocks with no model of their own (`blocks/models.rs`),
//! which aiming hits too (`edit/aim.rs`).
//!
//! A Bedrock id's boxes are its Java state's, the one
//! [`BlockTable::shape_state`] picks. The tests check that the Java states
//! sharing an id have the same boxes, and name the ones that don't, each with
//! its reason.

use glam::DVec3;
use pumpkin_data::BlockStateId;
use pumpkin_util::math::position::BlockPos;

use crate::world::BlockTable;

/// Overlaps thinner than this don't count, so boxes that only touch never
/// collide, and a box this far inside another is pushed back out.
///
/// It's wider than Java's 1e-7 because Bedrock keeps positions in single
/// precision (`player/physics.rs`): near y 200 a position moves in steps of
/// 0.000015, so a player standing on the ground can end up 0.000005 inside
/// it. Yet it's narrow enough that the official client's player, 0.000035
/// below a ladder's top, still counts as touching it (`ladder.bin`).
///
/// Further from the origin the steps are wider than this, so physics doesn't
/// lean on it there: a position never rounds into a box at all
/// (`single_precision`, in `player/physics.rs`).
pub const EPSILON: f64 = 1e-5;

/// An axis-aligned box, in blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    #[must_use]
    pub const fn new(min: DVec3, max: DVec3) -> Self {
        Self { min, max }
    }

    /// The whole block at `(x, y, z)`.
    #[must_use]
    pub fn cube(x: i32, y: i32, z: i32) -> Self {
        let min = DVec3::new(f64::from(x), f64::from(y), f64::from(z));
        Self::new(min, min + DVec3::ONE)
    }

    #[must_use]
    pub fn offset(self, by: DVec3) -> Self {
        Self::new(self.min + by, self.max + by)
    }

    /// Grown on the side `by` points to, to cover a move by it.
    #[must_use]
    pub fn expanded_towards(self, by: DVec3) -> Self {
        Self::new(self.min + by.min(DVec3::ZERO), self.max + by.max(DVec3::ZERO))
    }

    /// Smaller by `by` on every side.
    #[must_use]
    pub fn shrunk(self, by: DVec3) -> Self {
        Self::new(self.min + by, self.max - by)
    }

    /// Whether the two overlap by more than [`EPSILON`] on every axis.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        (0..3).all(|axis| self.overlaps_on(other, axis))
    }

    /// Whether the two overlap along `axis` by more than [`EPSILON`].
    #[must_use]
    pub fn overlaps_on(&self, other: &Self, axis: usize) -> bool {
        self.min[axis] < other.max[axis] - EPSILON && self.max[axis] > other.min[axis] + EPSILON
    }

    /// How far this box can move by `distance` along `axis` before it
    /// touches `other`: `distance` itself if `other` isn't in the way.
    #[must_use]
    pub fn clip(&self, other: &Self, axis: usize, distance: f64) -> f64 {
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        if !self.overlaps_on(other, a) || !self.overlaps_on(other, b) {
            return distance;
        }
        if distance > 0.0 && other.min[axis] >= self.max[axis] - EPSILON {
            distance.min(other.min[axis] - self.max[axis])
        } else if distance < 0.0 && other.max[axis] <= self.min[axis] + EPSILON {
            distance.max(other.max[axis] - self.min[axis])
        } else {
            distance
        }
    }
}

/// Adds the collision boxes of the block with Bedrock id `bedrock` at
/// `(x, y, z)`, in world blocks.
///
/// An id with no Java state counts as a full cube, so nothing unknown can be
/// walked through.
pub fn collision_boxes(table: &BlockTable, bedrock: u32, x: i32, y: i32, z: i32, out: &mut Vec<Aabb>) {
    let Some(state) = table.shape_state(bedrock) else {
        out.push(Aabb::cube(x, y, z));
        return;
    };
    let corner = DVec3::new(f64::from(x), f64::from(y), f64::from(z));
    // Some blocks (bamboo, pointed dripstone) sit off-centre by their position.
    for shape in state.to_state().get_block_collision_shapes_at(&BlockPos::new(x, y, z)) {
        let min = DVec3::new(shape.min.x, shape.min.y, shape.min.z);
        let max = DVec3::new(shape.max.x, shape.max.y, shape.max.z);
        out.push(Aabb::new(min + corner, max + corner));
    }
}

/// Adds the outline boxes of the block with Bedrock id `bedrock` at `(x, y, z)`,
/// in world blocks: what aiming hits, and what the outline draws. These are
/// exact, where [`outline`] rounds them for the mesher.
///
/// An id with no Java state counts as a full cube, as it does for collisions.
pub fn outline_boxes(table: &BlockTable, bedrock: u32, x: i32, y: i32, z: i32, out: &mut Vec<Aabb>) {
    let Some(state) = table.shape_state(bedrock) else {
        out.push(Aabb::cube(x, y, z));
        return;
    };
    let corner = DVec3::new(f64::from(x), f64::from(y), f64::from(z));
    let java = state.to_state();
    for shape in java.get_block_outline_shapes_at(&BlockPos::new(x, y, z)).take(java.outline_shapes.len()) {
        let min = DVec3::new(shape.min.x, shape.min.y, shape.min.z);
        let max = DVec3::new(shape.max.x, shape.max.y, shape.max.z);
        out.push(Aabb::new(min + corner, max + corner));
    }
}

/// Whether a Java state's outline is exactly one whole block.
#[must_use]
pub fn is_full_cube(state: BlockStateId) -> bool {
    outline(state, 0, 0, 0) == [([0; 3], [16; 3])]
}

/// A Java state's outline boxes at `(x, y, z)`, inside the block, in
/// sixteenths: the boxes the mesher draws.
///
/// Rounded to the nearest sixteenth, which the vertex positions hold, and at
/// least one sixteenth thick, so no face lies on top of another. A
/// waterlogged state's water isn't among them: Bedrock sends that water in
/// the block's second layer, which is drawn as water.
#[must_use]
pub fn outline(state: BlockStateId, x: i32, y: i32, z: i32) -> Vec<([u8; 3], [u8; 3])> {
    let java = state.to_state();
    let sixteenths = |v: f64| (v * 16.0).round().clamp(0.0, 16.0) as u8;
    java.get_block_outline_shapes_at(&BlockPos::new(x, y, z))
        .take(java.outline_shapes.len())
        .map(|shape| {
            let mut min = [shape.min.x, shape.min.y, shape.min.z].map(sixteenths);
            let mut max = [shape.max.x, shape.max.y, shape.max.z].map(sixteenths);
            for axis in 0..3 {
                if max[axis] <= min[axis] {
                    if min[axis] < 16 {
                        max[axis] = min[axis] + 1;
                    } else {
                        min[axis] = 15;
                    }
                }
            }
            (min, max)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use pumpkin_data::{Block, BlockState};

    use super::*;
    use crate::blocks::{palette::BedrockState, palette_source};

    /// Every Java state's Bedrock id, and the states that share each one.
    fn states_by_bedrock_id() -> BTreeMap<u32, Vec<BlockStateId>> {
        let mut by_id: BTreeMap<u32, Vec<BlockStateId>> = BTreeMap::new();
        for state in (0..BlockStateId::COUNT).filter_map(BlockStateId::new) {
            by_id.entry(BlockState::to_be_network_id(state)).or_default().push(state);
        }
        by_id
    }

    /// A Java state's properties, as strings.
    fn props(state: BlockStateId) -> HashMap<&'static str, &'static str> {
        Block::from_state_id(state)
            .properties(state)
            .map(|props| props.to_props().into_iter().map(|p| (p.key().to_str(), p.value_str())).collect())
            .unwrap_or_default()
    }

    /// Why the Java states sharing a Bedrock id may differ in their boxes, by
    /// block name (m5-plan Step 7). Each is a property Java keeps in the block
    /// state and Bedrock keeps somewhere else, or works out for itself. A new
    /// difference fails the test, to be looked at the same way.
    fn explained(name: &str) -> Option<&'static str> {
        Some(match name {
            "piston" | "sticky_piston" => {
                "Java's `extended`: Bedrock keeps a piston's push in its block actor. The shape is \
                 drawn back, the default"
            }
            "piston_head" => "Java's `short` arm is a frame of a piston's push",
            "scaffolding" => "Java's `bottom` only adds to the outline, and Bedrock works it out itself",
            "fire" | "redstone_wire" | "chorus_plant" | "vine" => {
                "connections Java keeps in the state, and Bedrock works out from the blocks around"
            }
            "kelp" | "kelp_plant" | "weeping_vines" | "weeping_vines_plant" | "twisting_vines"
            | "twisting_vines_plant" => {
                "Java's head and body are two blocks, Bedrock's one: drawn as crossed planes, with no \
                 collision"
            }
            _ if name.ends_with("chest") => {
                "Java's `type` pairs chests; Bedrock keeps the pairing in the block actor. The shape is \
                 a single chest, the default"
            }
            _ if name.ends_with("_button") => {
                "Java turns a floor or ceiling button with `facing`; Bedrock's don't turn. Only the \
                 outline, a small box, differs"
            }
            _ => return None,
        })
    }

    #[test]
    fn every_bedrock_id_has_one_set_of_boxes_or_a_reason() {
        let mut unexplained = String::new();
        let mut by_reason: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (bedrock, states) in states_by_bedrock_id() {
            let first = states[0].to_state();
            let same = states.iter().all(|state| {
                let java = state.to_state();
                java.collision_shapes == first.collision_shapes && java.outline_shapes == first.outline_shapes
            });
            if same {
                continue;
            }
            let name = Block::from_state_id(states[0]).name;
            if let Some(reason) = explained(name) {
                by_reason.entry(reason).or_default().push(name);
                continue;
            }
            unexplained.push_str(&format!("Bedrock id {bedrock:#010x}:\n"));
            for state in states {
                let java = state.to_state();
                unexplained.push_str(&format!(
                    "    {} {:?}: collision {:?}, outline {:?}\n",
                    Block::from_state_id(state).name,
                    props(state),
                    java.collision_shapes,
                    java.outline_shapes
                ));
            }
        }
        for (reason, names) in &by_reason {
            let mut distinct = names.clone();
            distinct.sort_unstable();
            distinct.dedup();
            eprintln!("{} ids, {}: {reason}", names.len(), distinct.join(", "));
        }
        assert!(
            unexplained.is_empty(),
            "Bedrock ids whose Java states differ, with no reason yet:\n{unexplained}"
        );
    }

    /// The outline of the Java state with these properties, through its
    /// Bedrock id and the table, as the game reads it.
    fn outline_of(table: &BlockTable, block: &Block, wanted: &[(&str, &str)]) -> Vec<([u8; 3], [u8; 3])> {
        let java = block
            .states
            .iter()
            .map(|state| state.id)
            .find(|&state| {
                let props = props(state);
                wanted.iter().all(|(key, value)| props.get(key) == Some(value))
            })
            .expect("a matching Java state");
        let shape = table
            .shape_state(BlockState::to_be_network_id(java))
            .expect("in the table");
        outline(shape, 0, 64, 0)
    }

    #[test]
    fn blocks_take_their_shapes_from_their_state() {
        let table = BlockTable::build();
        let stair = |shape| {
            outline_of(
                &table,
                &Block::OAK_STAIRS,
                &[("facing", "east"), ("half", "bottom"), ("shape", shape)],
            )
        };
        assert_eq!(stair("straight").len(), 2, "a slab and a step");
        assert_eq!(stair("inner_left").len(), 3, "an inner corner adds a quarter");

        let door = |open| {
            outline_of(
                &table,
                &Block::OAK_DOOR,
                &[("facing", "east"), ("half", "lower"), ("hinge", "left"), ("open", open)],
            )
        };
        let (closed, open) = (door("false"), door("true"));
        assert_ne!(closed, open);
        for boxes in [&closed, &open] {
            assert_eq!(boxes.len(), 1);
            let (min, max) = boxes[0];
            let thinnest = (0..3).map(|axis| max[axis] - min[axis]).min();
            assert_eq!(thinnest, Some(3), "a door is 3 sixteenths thick: {boxes:?}");
        }

        assert_eq!(outline_of(&table, &Block::TORCH, &[]), [([6, 0, 6], [10, 10, 10])]);
        assert_eq!(
            outline_of(&table, &Block::LADDER, &[("facing", "north")]),
            [([0, 0, 13], [16, 16, 16])]
        );
        // A piston is drawn back: its id holds its pushed-out states too.
        assert_eq!(
            outline_of(&table, &Block::PISTON, &[("extended", "false"), ("facing", "up")]),
            [([0; 3], [16; 3])]
        );
        assert!(is_full_cube(Block::STONE.default_state.id));
        assert!(!is_full_cube(Block::OAK_SLAB.default_state.id));
    }

    #[test]
    fn a_fences_arms_come_with_its_bedrock_state() {
        let table = BlockTable::build();
        let palette = palette_source::load().unwrap();
        let connected = |state: &BedrockState, side: &str| {
            let key = format!("minecraft:connection_{side}");
            state
                .flag(&key)
                .or_else(|| state.int(&key).map(|value| value != 0))
                .unwrap_or(false)
        };
        let (id, _) = palette
            .iter()
            .find(|(_, state)| {
                &*state.name == "oak_fence"
                    && connected(state, "east")
                    && !["north", "south", "west"].iter().any(|side| connected(state, side))
            })
            .expect("a fence connected east only");
        // Pumpkin's shapes come merged: the post and its arm are one box,
        // from the post's west side to the block's east edge.
        let boxes = outline(table.shape_state(id).expect("in the table"), 0, 64, 0);
        assert!(
            boxes.iter().any(|(min, max)| max[0] == 16 && min[0] > 0),
            "the arm reaches east: {boxes:?}"
        );
        assert!(boxes.iter().all(|(min, _)| min[0] >= 6), "nothing to the west: {boxes:?}");
        assert!(boxes.iter().all(|(min, max)| min[2] >= 6 && max[2] <= 10), "nor north or south: {boxes:?}");
    }

    fn boxes(table: &BlockTable, block: &Block) -> Vec<Aabb> {
        let bedrock = BlockState::to_be_network_id(block.default_state.id);
        let mut out = Vec::new();
        collision_boxes(table, bedrock, 10, 64, -3, &mut out);
        out
    }

    #[test]
    fn blocks_collide_with_pumpkins_boxes_where_they_stand() {
        let table = BlockTable::build();
        assert_eq!(boxes(&table, &Block::STONE), [Aabb::cube(10, 64, -3)]);
        assert!(boxes(&table, &Block::AIR).is_empty());
        assert!(boxes(&table, &Block::WATER).is_empty());
        // A bottom slab: half a block high.
        let slab = boxes(&table, &Block::OAK_SLAB);
        assert_eq!(slab.len(), 1);
        assert!((slab[0].max.y - 64.5).abs() < 1e-9, "{slab:?}");
        // An id the table doesn't know is solid.
        let mut out = Vec::new();
        collision_boxes(&table, 0xDEAD_BEEF, 1, 2, 3, &mut out);
        assert_eq!(out, [Aabb::cube(1, 2, 3)]);
    }

    #[test]
    fn blocks_are_aimed_at_by_pumpkins_outlines() {
        let table = BlockTable::build();
        let at = |block: &Block| {
            let mut out = Vec::new();
            outline_boxes(&table, BlockState::to_be_network_id(block.default_state.id), 10, 64, -3, &mut out);
            out
        };
        assert_eq!(at(&Block::STONE), [Aabb::cube(10, 64, -3)]);
        // Nothing to aim at in air or a liquid.
        assert!(at(&Block::AIR).is_empty());
        assert!(at(&Block::WATER).is_empty());
        assert!(at(&Block::LAVA).is_empty());
        // A bottom slab: half a block high.
        let slab = at(&Block::OAK_SLAB);
        assert_eq!(slab.len(), 1);
        assert!((slab[0].max.y - 64.5).abs() < 1e-9, "{slab:?}");
        // A fence's post is a block tall to aim at, and collides half a block higher.
        let post = at(&Block::OAK_FENCE);
        assert_eq!(post.len(), 1, "{post:?}");
        assert!((post[0].min.x - 10.375).abs() < 1e-9 && (post[0].max.y - 65.0).abs() < 1e-9, "{post:?}");
        // An id the table doesn't know is solid.
        let mut out = Vec::new();
        outline_boxes(&table, 0xDEAD_BEEF, 1, 2, 3, &mut out);
        assert_eq!(out, [Aabb::cube(1, 2, 3)]);
    }

    #[test]
    fn a_box_stops_at_what_is_in_its_way() {
        let player = Aabb::new(DVec3::new(0.2, 0.0, 0.2), DVec3::new(0.8, 1.8, 0.8));
        let wall = Aabb::cube(1, 0, 0);
        assert!((player.clip(&wall, 0, 0.5) - 0.2).abs() < 1e-12, "stops at the wall");
        assert!((player.clip(&wall, 0, -0.5) + 0.5).abs() < 1e-12, "moving away");
        let floor = Aabb::cube(0, -1, 0);
        assert!(player.clip(&floor, 1, -0.3).abs() < 1e-12, "standing on it");
        // Touching the floor isn't overlapping it, so it doesn't stop a step sideways.
        assert!((player.clip(&floor, 0, 0.5) - 0.5).abs() < 1e-12);
        assert!(!player.intersects(&floor));
        assert!(player.offset(DVec3::new(0.0, -0.01, 0.0)).intersects(&floor));
        // A box a hair inside the floor is pushed back out by a fall.
        let sunk = player.offset(DVec3::new(0.0, -0.000_005, 0.0));
        assert!((sunk.clip(&floor, 1, -0.3) - 0.000_005).abs() < 1e-9);
    }
}
