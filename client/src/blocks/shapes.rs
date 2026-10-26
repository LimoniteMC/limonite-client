//! Block shapes by Bedrock id, from pumpkin-data (m5-plan Decision 7,
//! Finding 9).
//!
//! The collision boxes come first, in Step 6, because physics walks on them.
//! Step 7 adds the outline boxes, draws them where M4 drew stand-in cubes,
//! and checks that every Java state sharing a Bedrock id has the same boxes.

use glam::DVec3;
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
/// The Java state is the one M3's table maps the id to (the lowest that
/// shares it). An id with no Java state counts as a full cube, so nothing
/// unknown can be walked through.
pub fn collision_boxes(table: &BlockTable, bedrock: u32, x: i32, y: i32, z: i32, out: &mut Vec<Aabb>) {
    let Some(state) = table.java_state(bedrock) else {
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

#[cfg(test)]
mod tests {
    use pumpkin_data::{Block, BlockState};

    use super::*;

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
    }
}
