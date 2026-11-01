//! The crack on a block being broken (m5-plan Decision 9): the install's
//! `destroy_stage_0` to `_9`, over the block's own model.
//!
//! **The block's own quads** (worth teaching). A crack belongs on what the player
//! sees, so a fence's isn't on the boxes around it, but on its posts and rails. The
//! mesher already knows every shape, so it meshes the block alone, as it does for an
//! icon, and the crack draws those quads again with another texture. Each quad's
//! texture coordinates go with it, so a crack's pixels are the size of the block's.
//!
//! **Drawn over the block, not on it.** The same quads again would fight the block
//! for the same pixels, so the crack's pipeline has a depth bias, which pulls it a
//! hair towards the eye (`terrain.rs`). It multiplies what's there, so it's cracks,
//! not gray patches (`terrain.wgsl`).

use glam::{DVec3, IVec3};

use super::{
    Gpu,
    icons::PLAINS,
    mesher::{self, Mesh, Neighbourhood, Vertex},
    tint::Tints,
};
use crate::{
    blocks::{appearance::AppearanceTable, models::POSITION_SCALE},
    edit::CRACK_STAGES,
};

/// A lone block's mesh has it in the middle of its sub-chunk: 8 blocks along each way.
const MIDDLE: i16 = 8 * 16 * POSITION_SCALE;

/// What's cracked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cracked {
    pub block: IVec3,
    /// Which block is there, by its Bedrock id.
    pub id: u32,
    pub stage: usize,
}

/// The crack's GPU state.
pub struct Crack {
    /// The stages' layers in the texture array, if the install has them.
    layers: Option<[u16; CRACK_STAGES]>,
    vertices: wgpu::Buffer,
    /// How many vertices the buffer holds.
    capacity: usize,
    quads: u32,
    /// What the buffer shows.
    shown: Option<Cracked>,
    /// The quads of that block, whatever the stage.
    mesh: Vec<Vertex>,
}

impl Crack {
    #[must_use]
    pub fn new(device: &wgpu::Device, layers: Option<[u16; CRACK_STAGES]>) -> Self {
        // A cube's worth, which is most of what it ever draws.
        let capacity = 24;
        Self {
            layers,
            vertices: vertex_buffer(device, capacity),
            capacity,
            quads: 0,
            shown: None,
            mesh: Vec::new(),
        }
    }

    /// What the next draw shows. `None` draws nothing.
    pub fn show(&mut self, gpu: &Gpu, table: &AppearanceTable, tints: &Tints, cracked: Option<Cracked>) {
        if cracked == self.shown {
            return;
        }
        let same_block = matches!(
            (self.shown, cracked),
            (Some(old), Some(new)) if (old.block, old.id) == (new.block, new.id)
        );
        self.shown = cracked;
        let (Some(cracked), Some(layers)) = (cracked, self.layers) else {
            self.quads = 0;
            return;
        };
        if !same_block {
            let lone = Neighbourhood::single_at(table, PLAINS, cracked.id, cracked.block.to_array());
            self.mesh = from_block(&mesher::mesh(&lone, tints));
        }
        for vertex in &mut self.mesh {
            vertex.layer = layers[cracked.stage.min(CRACK_STAGES - 1)];
        }
        self.quads = (self.mesh.len() / 4) as u32;
        if self.mesh.len() > self.capacity {
            self.capacity = self.mesh.len().next_power_of_two();
            self.vertices = vertex_buffer(&gpu.device, self.capacity);
        }
        if !self.mesh.is_empty() {
            gpu.queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&self.mesh));
        }
    }

    #[must_use]
    pub const fn quads(&self) -> u32 {
        self.quads
    }

    /// The block's corner, in blocks.
    #[must_use]
    pub fn origin(&self) -> DVec3 {
        self.shown.map_or(DVec3::ZERO, |cracked| cracked.block.as_dvec3())
    }

    #[must_use]
    pub const fn vertices(&self) -> &wgpu::Buffer {
        &self.vertices
    }
}

fn vertex_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("crack"),
        size: (capacity * size_of::<Vertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// A lone block's quads, opaque and blended alike, from its own corner.
fn from_block(mesh: &Mesh) -> Vec<Vertex> {
    mesh.opaque
        .iter()
        .chain(&mesh.blend)
        .map(|vertex| Vertex {
            position: vertex.position.map(|axis| axis - MIDDLE),
            ..*vertex
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lone_blocks_quads_are_counted_from_its_own_corner() {
        let vertex = |position| Vertex {
            position,
            layer: 3,
            uv: [8, 16],
            face_class: 1,
            unused: 0,
            tint: [255; 4],
        };
        let whole = 16 * POSITION_SCALE;
        let mesh = Mesh {
            opaque: vec![vertex([MIDDLE, MIDDLE + whole, MIDDLE])],
            blend: vec![vertex([MIDDLE + whole, MIDDLE, MIDDLE + whole / 2])],
        };
        let vertices = from_block(&mesh);
        assert_eq!(vertices[0].position, [0, whole, 0]);
        assert_eq!(vertices[1].position, [whole, 0, whole / 2], "the blended ones too");
        assert_eq!(vertices[0].uv, [8, 16], "the block's own texture coordinates");
    }
}
