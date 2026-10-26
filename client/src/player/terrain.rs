//! The game's world as physics sees it: collision boxes and surfaces by
//! position, read from the decoded chunks through M3's Bedrock-to-Java
//! table and pumpkin-data.

use glam::DVec3;
use pumpkin_data::{
    Block,
    tag::{self, Taggable},
};

use super::physics::{Fluid, Surface, Terrain};
use crate::{
    blocks::shapes::{self, Aabb},
    world::World,
};

/// How many chunks round the player's must have arrived before it moves.
const READY_RADIUS: i32 = 1;

pub struct WorldTerrain<'a> {
    world: &'a World,
}

impl<'a> WorldTerrain<'a> {
    pub const fn new(world: &'a World) -> Self {
        Self { world }
    }

    /// The Java block at `(x, y, z)` on `layer`: 0 the block, 1 the water a
    /// Bedrock block can hold.
    fn block(&self, x: i32, y: i32, z: i32, layer: usize) -> Option<&'static Block> {
        let id = self.world.block(x, y, z, layer)?;
        self.world.blocks().java_state(id).map(Block::from_state_id)
    }
}

impl Terrain for WorldTerrain<'_> {
    fn collision_boxes(&self, x: i32, y: i32, z: i32, out: &mut Vec<Aabb>) {
        if let Some(id) = self.world.block(x, y, z, 0) {
            shapes::collision_boxes(self.world.blocks(), id, x, y, z, out);
        }
    }

    fn surface(&self, x: i32, y: i32, z: i32) -> Surface {
        let Some(block) = self.block(x, y, z, 0) else {
            return Surface::default();
        };
        Surface {
            slipperiness: f64::from(block.slipperiness),
            speed_factor: f64::from(block.velocity_multiplier),
            jump_factor: f64::from(block.jump_velocity_multiplier),
            climbable: block.has_tag(&tag::Block::MINECRAFT_CLIMBABLE),
            // Bedrock keeps a waterlogged block's water in layer 1.
            fluid: fluid(block).or_else(|| self.block(x, y, z, 1).and_then(fluid)),
        }
    }

    fn ready(&self, feet: DVec3) -> bool {
        let (x, z) = (feet.x.floor() as i32 >> 4, feet.z.floor() as i32 >> 4);
        let near = -READY_RADIUS..=READY_RADIUS;
        near.clone()
            .all(|dx| near.clone().all(|dz| self.world.chunk(x + dx, z + dz).is_some()))
    }
}

fn fluid(block: &Block) -> Option<Fluid> {
    if block.id == Block::WATER.id {
        Some(Fluid::Water)
    } else if block.id == Block::LAVA.id {
        Some(Fluid::Lava)
    } else {
        None
    }
}
