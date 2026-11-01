//! The crack on a block being broken (m5-plan Decision 9): the install's
//! `destroy_stage_0` to `_9`, drawn just outside the block's outline boxes.
//!
//! **No pipeline of its own** (worth teaching). The textures are opaque cracks on
//! clear texels, which is what the terrain's blended faces already are. So the
//! crack is a few more quads in the texture array, in the mesher's own vertex
//! format, drawn by the blend pipeline after everything else.

use glam::{DVec3, IVec3};
use limonite_packs::registry::Face;

use super::{Gpu, mesher::Vertex};
use crate::{
    blocks::{
        models::{POSITION_SCALE, UV_SCALE},
        shapes::Aabb,
    },
    edit::CRACK_STAGES,
};

/// How far the crack stands off the block's faces, in the vertices' units: a
/// 512th of a block, so that it doesn't fight them for the same pixels.
const STAND_OFF: f64 = 2.0;
/// The blended class, as `Vertex::face_class` packs it.
const BLEND_CLASS: u8 = 3 << 3;

/// The crack's GPU state.
pub struct Crack {
    /// The stages' layers in the texture array, if the install has them.
    layers: Option<[u16; CRACK_STAGES]>,
    vertices: wgpu::Buffer,
    /// How many vertices the buffer holds.
    capacity: usize,
    quads: u32,
    /// The block's corner, in blocks.
    origin: DVec3,
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
            origin: DVec3::ZERO,
        }
    }

    /// What the next draw shows: `stage` of the crack on `block`, over `boxes`, in
    /// world blocks. `None` draws nothing.
    pub fn show(&mut self, gpu: &Gpu, cracked: Option<(IVec3, usize, &[Aabb])>) {
        self.quads = 0;
        let (Some((block, stage, boxes)), Some(layers)) = (cracked, self.layers) else {
            return;
        };
        self.origin = block.as_dvec3();
        let vertices = quads(block, boxes, layers[stage.min(CRACK_STAGES - 1)]);
        self.quads = (vertices.len() / 4) as u32;
        if vertices.len() > self.capacity {
            self.capacity = vertices.len().next_power_of_two();
            self.vertices = vertex_buffer(&gpu.device, self.capacity);
        }
        if !vertices.is_empty() {
            gpu.queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
        }
    }

    #[must_use]
    pub const fn quads(&self) -> u32 {
        self.quads
    }

    #[must_use]
    pub const fn origin(&self) -> DVec3 {
        self.origin
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

/// Six quads for each box, around `block`'s corner, counter-clockwise from outside.
fn quads(block: IVec3, boxes: &[Aabb], layer: u16) -> Vec<Vertex> {
    let origin = block.as_dvec3();
    let scale = f64::from(POSITION_SCALE) * 16.0;
    let mut vertices = Vec::with_capacity(boxes.len() * 24);
    for area in boxes {
        let min = (area.min - origin) * scale - DVec3::splat(STAND_OFF);
        let max = (area.max - origin) * scale + DVec3::splat(STAND_OFF);
        let at = |x: f64, y: f64, z: f64| [x as i16, y as i16, z as i16];
        for face in Face::ALL {
            let corners = match face {
                Face::Up => [at(min.x, max.y, min.z), at(min.x, max.y, max.z), at(max.x, max.y, max.z), at(max.x, max.y, min.z)],
                Face::Down => [at(min.x, min.y, max.z), at(min.x, min.y, min.z), at(max.x, min.y, min.z), at(max.x, min.y, max.z)],
                Face::North => [at(max.x, max.y, min.z), at(max.x, min.y, min.z), at(min.x, min.y, min.z), at(min.x, max.y, min.z)],
                Face::South => [at(min.x, max.y, max.z), at(min.x, min.y, max.z), at(max.x, min.y, max.z), at(max.x, max.y, max.z)],
                Face::West => [at(min.x, max.y, min.z), at(min.x, min.y, min.z), at(min.x, min.y, max.z), at(min.x, max.y, max.z)],
                Face::East => [at(max.x, max.y, max.z), at(max.x, min.y, max.z), at(max.x, min.y, min.z), at(max.x, max.y, min.z)],
            };
            for corner in corners {
                vertices.push(Vertex {
                    position: corner,
                    layer,
                    uv: uv(face, corner),
                    face_class: face as u8 | BLEND_CLASS,
                    unused: 0,
                    tint: [255; 4],
                });
            }
        }
    }
    vertices
}

/// A corner's texture coordinates, from where it is on its face, so that a smaller
/// box shows the matching part of the crack.
fn uv(face: Face, [x, y, z]: [i16; 3]) -> [u8; 2] {
    let block = i32::from(POSITION_SCALE) * 16;
    let whole = i32::from(UV_SCALE) * 16;
    let along = |value: i16| (i32::from(value) * whole / block).clamp(0, whole);
    let (u, v) = match face {
        Face::Up | Face::Down => (along(x), along(z)),
        Face::North | Face::South => (along(x), whole - along(y)),
        Face::West | Face::East => (along(z), whole - along(y)),
    };
    [u as u8, v as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cube_has_six_faces_just_outside_it() {
        let vertices = quads(IVec3::new(100, 64, -3), &[Aabb::cube(100, 64, -3)], 7);
        assert_eq!(vertices.len(), 24);
        let scale = POSITION_SCALE * 16;
        for vertex in &vertices {
            assert_eq!(vertex.layer, 7);
            assert_eq!(vertex.face_class >> 3, 3, "blended");
            assert_eq!(vertex.tint, [255; 4]);
            for value in vertex.position {
                assert!(value == -2 || value == scale + 2, "{value}");
            }
        }
    }

    #[test]
    fn each_quad_faces_outwards() {
        // Counter-clockwise from outside means the corners' cross product points the
        // way the face looks.
        let vertices = quads(IVec3::ZERO, &[Aabb::cube(0, 0, 0)], 0);
        let normals = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];
        for (quad, normal) in vertices.as_chunks::<4>().0.iter().zip(normals) {
            let at = |index: usize| glam::IVec3::from_array(quad[index].position.map(i32::from));
            let turned = (at(1) - at(0)).cross(at(2) - at(0)).signum();
            assert_eq!(turned.to_array(), normal, "{:?}", quad[0].face_class & 7);
        }
    }

    #[test]
    fn the_whole_texture_goes_over_a_whole_face() {
        let vertices = quads(IVec3::ZERO, &[Aabb::cube(0, 0, 0)], 0);
        let top = &vertices[4..8];
        let full = UV_SCALE * 16;
        assert_eq!(top[0].uv, [0, 0]);
        assert_eq!(top[2].uv, [full, full]);
    }

    #[test]
    fn a_box_smaller_than_its_block_shows_part_of_the_crack() {
        let area = Aabb::new(DVec3::ZERO, DVec3::new(1.0, 0.5, 1.0));
        let vertices = quads(IVec3::ZERO, &[area], 0);
        let north = &vertices[8..12];
        let full = UV_SCALE * 16;
        let lowest = north.iter().map(|vertex| vertex.uv[1]).min().unwrap();
        let highest = north.iter().map(|vertex| vertex.uv[1]).max().unwrap();
        assert_eq!(highest, full, "its bottom is the texture's");
        assert!((i32::from(lowest) - i32::from(full) / 2).abs() <= 1, "its top is the middle: {lowest}");
    }
}
