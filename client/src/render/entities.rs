//! Drawing the entities until milestone 8 gives them models (m5-plan Decision 13; worth
//! teaching).
//!
//! **Boxes, turned the way they face.** Each mob, player and minecart is a box of its own
//! size, in one colour per group (`entities::kinds::Group`), and turned to its yaw, as a
//! model would be. Its front is a little darker, so which way it faces shows. The light is
//! the light where it stands, through the terrain's light map, so a zombie in a dark cave
//! is dark and one under a torch is lit.
//!
//! **Items are their icons, on a card.** A dropped item is the picture the hotbar shows for
//! it, on a small card that turns and bobs, as Java's turn and bob: a radian a second, and
//! a tenth of a block up and down. A block's icon is many quads in one plane, which the UI
//! draws back to front; here each is set a hair in front of the last, so the depth test
//! keeps that order. The card has a picture on each side, each facing out, and the back
//! face of each is culled, so whichever side the eye sees reads the right way round.
//!
//! **Camera-relative, every frame.** Entities move every frame, and there are a few
//! thousand vertices at most, so the CPU works their positions out from the eye each
//! frame, in `f32`, and the GPU only projects them. Far from the origin, `f32` world
//! positions would wobble; relative to the eye, the near ones are precise.

use std::f32::consts::TAU;

use bytemuck::{Pod, Zeroable};
use glam::{DVec3, IVec3, Vec3};

use super::{Gpu, icons::Icon};
use crate::{
    entities::Drawn,
    inventory::items::ItemIcons,
    ui::atlas::Atlas,
};

/// One corner of an entity's triangle: 32 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct EntityVertex {
    /// In blocks, from the eye.
    pub position: [f32; 3],
    pub uv: [f32; 2],
    /// The terrain texture array's layer.
    pub layer: u32,
    /// sRGB-encoded.
    pub tint: [u8; 4],
    /// Where the colour comes from ([`FLAT`], ...), the face for its shade, and the light:
    /// the sky's level high and the blocks' low.
    pub info: [u8; 4],
}

/// Where a vertex's colour comes from, as `entities.wgsl` reads it.
const FLAT: u8 = 0;
const BLOCKS: u8 = 1;
const ATLAS: u8 = 2;
const OVERLAY: u8 = 3;

/// The faces, as the terrain's shader shades them: 0.5 below, 1 above, 0.8 north and
/// south, 0.6 west and east.
const DOWN: u8 = 0;
const UP: u8 = 1;
const NORTH: u8 = 2;
const SOUTH: u8 = 3;
const WEST: u8 = 4;
const EAST: u8 = 5;

/// How much darker a box's front is, so which way it faces shows.
const FRONT: f32 = 0.7;
/// An item card's side, in blocks: Java draws a dropped item at half size.
const CARD: f32 = 0.5;
/// How far apart the quads of a block's icon stand, front to back, in blocks.
const LAYERED: f32 = 0.002;
/// How fast a dropped item turns, in radians a second, and bobs: Java's.
const SPIN: f32 = 1.0;
const BOB_SPEED: f32 = 2.0;
const BOB: f32 = 0.1;

/// What the cards need: each item's icon, and where the pictures are in the atlas.
#[derive(Clone, Copy)]
pub struct Looks<'a> {
    pub icons: &'a ItemIcons,
    pub atlas: &'a Atlas,
}

/// Every entity's triangles, three vertices each, from the eye at `eye`. `light` gives the
/// light byte at a block. Without `looks`, items are drawn as boxes too.
#[must_use]
pub fn vertices(drawn: &[Drawn], eye: DVec3, light: &dyn Fn(IVec3) -> u8, looks: Option<Looks<'_>>) -> Vec<EntityVertex> {
    let mut out = Vec::with_capacity(drawn.len() * 36);
    for entity in drawn {
        let middle = entity.pose.feet + DVec3::Y * (entity.size[1] / 2.0);
        let lit = light(middle.floor().as_ivec3());
        let from_eye = (entity.pose.feet - eye).as_vec3();
        match (entity.dropped, looks) {
            (Some(dropped), Some(looks)) => {
                if let Some(icon) = looks.icons.get(dropped.item, dropped.aux) {
                    card(&mut out, entity, from_eye, lit, icon, looks);
                }
            }
            _ => cuboid(&mut out, entity, from_eye, lit),
        }
    }
    out
}

/// Turns a point round y to a Bedrock yaw: yaw 0 faces +z, and 90 faces -x.
fn turned(point: Vec3, yaw_degrees: f32) -> Vec3 {
    let (sin, cos) = yaw_degrees.to_radians().sin_cos();
    Vec3::new(point.x * cos - point.z * sin, point.y, point.x * sin + point.z * cos)
}

/// Pushes a quad's two triangles, corners counter-clockwise as seen from the front.
fn quad(out: &mut Vec<EntityVertex>, corners: [EntityVertex; 4]) {
    let [a, b, c, d] = corners;
    out.extend_from_slice(&[a, b, c, a, c, d]);
}

fn shaded([r, g, b, a]: [u8; 4], by: f32) -> [u8; 4] {
    let shade = |channel: u8| (f32::from(channel) * by).round() as u8;
    [shade(r), shade(g), shade(b), a]
}

/// A box `size` wide and tall, standing on `from_eye`, turned to the entity's yaw.
fn cuboid(out: &mut Vec<EntityVertex>, entity: &Drawn, from_eye: Vec3, light: u8) {
    let half = entity.size[0] as f32 / 2.0;
    let height = entity.size[1] as f32;
    let colour = entity.group.colour();
    let yaw = entity.pose.yaw;
    let corner = |x: f32, y: f32, z: f32| from_eye + turned(Vec3::new(x * half, y * height, z * half), yaw);
    // Each face's corners, counter-clockwise seen from outside, in the box's own frame:
    // x across, y up from 0 to 1, z from back to front.
    let faces: [(u8, f32, [[f32; 3]; 4]); 6] = [
        (DOWN, 1.0, [[-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0]]),
        (UP, 1.0, [[-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, -1.0]]),
        (NORTH, 1.0, [[-1.0, 0.0, -1.0], [-1.0, 1.0, -1.0], [1.0, 1.0, -1.0], [1.0, 0.0, -1.0]]),
        (SOUTH, FRONT, [[-1.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]]),
        (WEST, 1.0, [[-1.0, 0.0, -1.0], [-1.0, 0.0, 1.0], [-1.0, 1.0, 1.0], [-1.0, 1.0, -1.0]]),
        (EAST, 1.0, [[1.0, 0.0, -1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]]),
    ];
    for (face, front, points) in faces {
        let tint = shaded(colour, front);
        quad(
            out,
            points.map(|[x, y, z]| EntityVertex {
                position: corner(x, y, z).to_array(),
                uv: [0.0, 0.0],
                layer: 0,
                tint,
                info: [FLAT, face, light, 0],
            }),
        );
    }
}

/// A dropped item: its icon on a card that turns and bobs.
fn card(out: &mut Vec<EntityVertex>, entity: &Drawn, from_eye: Vec3, light: u8, icon: Icon, looks: Looks<'_>) {
    let seconds = entity.age.as_secs_f32();
    // Each item starts its turn and bob somewhere else, so a pile doesn't turn as one.
    let offset = (entity.runtime_id as f32 * 0.618_034).fract() * TAU;
    let spin = (seconds * SPIN + offset).to_degrees();
    let lift = ((seconds * BOB_SPEED + offset).sin() + 1.0) * BOB;
    let base = from_eye + Vec3::Y * lift;
    // Each side faces out: the front along +z, turned, and the back along -z. The back is
    // the front seen from behind, so its x runs the other way and it reads the right way
    // round. Each of a block's quads stands a hair in front of the one before.
    let place = |[across, down]: [f32; 2], layer: usize, back: bool| {
        let x = (across - 0.5) * CARD;
        let y = (1.0 - down) * CARD;
        let z = LAYERED * layer as f32;
        let local = if back { Vec3::new(-x, y, -z) } else { Vec3::new(x, y, z) };
        base + turned(local, spin)
    };
    let mut side = |back: bool| match icon {
        Icon::Sprite(sprite) => {
            let [left, top, right, bottom] = looks.atlas.get(sprite).source.map(|edge| edge as f32);
            let [width, height] = looks.atlas.size().map(|side| side as f32);
            let corners: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
            quad(
                out,
                corners.map(|[across, down]| EntityVertex {
                    position: place([across, down], 0, back).to_array(),
                    uv: [(left + across * (right - left)) / width, (top + down * (bottom - top)) / height],
                    layer: 0,
                    tint: [255; 4],
                    info: [ATLAS, UP, light, 0],
                }),
            );
        }
        Icon::Block { first, count } => {
            for (layer, piece) in looks.icons.quads(first, count).iter().enumerate() {
                let [r, g, b] = piece.tint;
                let tint = shaded([r, g, b, 255], piece.shade);
                // Counter-clockwise with y up, as the terrain's quads; `place` turns the
                // icon's y-down corners back up, so they keep their order.
                quad(
                    out,
                    [0, 1, 2, 3].map(|i| EntityVertex {
                        position: place(piece.corners[i], layer, back).to_array(),
                        uv: piece.uvs[i],
                        layer: u32::from(piece.layer),
                        tint,
                        info: [if piece.overlay { OVERLAY } else { BLOCKS }, UP, light, 0],
                    }),
                );
            }
        }
    };
    side(false);
    side(true);
}

/// The entities' GPU state: one pipeline, and a buffer of vertices filled each frame.
pub struct EntityRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    /// How many vertices the buffer holds, and how many to draw.
    capacity: usize,
    count: u32,
}

impl EntityRenderer {
    /// Builds the pipeline, reading the terrain's frame uniform, texture array and light
    /// map, and the UI's atlas.
    #[must_use]
    pub fn new(
        gpu: &Gpu,
        depth: wgpu::TextureFormat,
        frame: &wgpu::Buffer,
        blocks: &wgpu::TextureView,
        light_map: &wgpu::TextureView,
        atlas: &wgpu::TextureView,
    ) -> Self {
        let device = &gpu.device;
        // As the terrain samples them: nearest up close, linear between mips, clamped.
        let block_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("entities, blocks"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let light_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("entities, light"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        // The atlas has no mips, and its sprites sit a texel apart: nearest, as the UI.
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("entities, atlas"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let texture = |binding, dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: dimension,
                multisampled: false,
            },
            count: None,
        };
        let sampler = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("entities"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(frame.size()),
                    },
                    count: None,
                },
                texture(1, wgpu::TextureViewDimension::D2Array),
                sampler(2),
                texture(3, wgpu::TextureViewDimension::D2),
                sampler(4),
                texture(5, wgpu::TextureViewDimension::D2),
                sampler(6),
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("entities"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(blocks),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&block_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(light_map),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&light_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(atlas),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("entities"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("entities.wgsl"));
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x3,
            1 => Float32x2,
            2 => Uint32,
            3 => Unorm8x4,
            4 => Uint8x4,
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("entities"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<EntityVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: gpu.config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let capacity = 4096;
        Self {
            pipeline,
            bind_group,
            vertices: vertex_buffer(device, capacity),
            capacity,
            count: 0,
        }
    }

    /// Takes this frame's vertices.
    pub fn upload(&mut self, gpu: &Gpu, vertices: &[EntityVertex]) {
        if vertices.len() > self.capacity {
            self.capacity = vertices.len().next_power_of_two();
            self.vertices = vertex_buffer(&gpu.device, self.capacity);
        }
        if !vertices.is_empty() {
            gpu.queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(vertices));
        }
        self.count = vertices.len() as u32;
    }

    /// Draws them, in the terrain's pass: its bind group is replaced, so the caller sets
    /// its own again after.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..self.count, 0..1);
    }

}

fn vertex_buffer(device: &wgpu::Device, vertices: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("entity vertices"),
        size: (vertices * size_of::<EntityVertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::entities::{kinds::Group, track::Pose};

    fn drawn(group: Group, size: [f64; 2], yaw: f32) -> Drawn {
        Drawn {
            runtime_id: 1,
            group,
            pose: Pose {
                feet: DVec3::new(10.0, 64.0, 5.0),
                yaw,
                pitch: 0.0,
                head_yaw: yaw,
            },
            size,
            dropped: None,
            age: Duration::ZERO,
        }
    }

    #[test]
    fn a_box_is_its_size_from_the_eye_and_lit_where_it_stands() {
        let cow = drawn(Group::Animal, [0.9, 1.4], 0.0);
        let eye = DVec3::new(10.0, 65.0, 0.0);
        let out = vertices(&[cow], eye, &|_| 0xF3, None);
        assert_eq!(out.len(), 36);
        let (min, max) = bounds(&out);
        assert!((min - Vec3::new(-0.45, -1.0, 4.55)).length() < 1e-5, "{min}");
        assert!((max - Vec3::new(0.45, 0.4, 5.45)).length() < 1e-5, "{max}");
        assert!(out.iter().all(|v| v.info[0] == FLAT && v.info[2] == 0xF3));
    }

    fn bounds(out: &[EntityVertex]) -> (Vec3, Vec3) {
        let points = out.iter().map(|v| Vec3::from_array(v.position));
        let min = points.clone().fold(Vec3::splat(f32::MAX), Vec3::min);
        let max = points.fold(Vec3::splat(f32::MIN), Vec3::max);
        (min, max)
    }

    #[test]
    fn the_light_is_the_blocks_at_the_middle_of_the_box() {
        let zombie = drawn(Group::Monster, [0.6, 1.95], 0.0);
        let out = vertices(&[zombie], DVec3::ZERO, &|at: IVec3| if at == IVec3::new(10, 64, 5) { 0x7A } else { 0 }, None);
        assert!(out.iter().all(|v| v.info[2] == 0x7A));
    }

    #[test]
    fn a_box_turns_to_its_yaw_with_its_front_darker() {
        // Facing east (yaw -90, +x): the darker face is the one furthest along +x.
        let pig = drawn(Group::Animal, [0.9, 0.9], -90.0);
        let out = vertices(&[pig], DVec3::new(10.0, 64.0, 5.0), &|_| 0xF0, None);
        let front: Vec<&EntityVertex> = out.iter().filter(|v| v.tint[0] < Group::Animal.colour()[0]).collect();
        assert_eq!(front.len(), 6);
        assert!(front.iter().all(|v| (v.position[0] - 0.45).abs() < 1e-5), "{front:?}");
    }

    /// The GPU driver's wgpu compiles the shader at startup; this catches what naga,
    /// wgpu's own compiler, would reject, without a GPU.
    #[test]
    fn the_shader_parses_and_validates() {
        let source = include_str!("entities.wgsl");
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|err| panic!("{}", err.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|err| panic!("{err:?}"));
    }

    #[test]
    fn a_vertex_is_32_bytes() {
        assert_eq!(size_of::<EntityVertex>(), 32);
    }

    #[test]
    fn turning_matches_bedrocks_yaw() {
        let ahead = Vec3::Z;
        assert!((turned(ahead, 0.0) - Vec3::Z).length() < 1e-6);
        assert!((turned(ahead, 90.0) - Vec3::NEG_X).length() < 1e-6);
        assert!((turned(ahead, -90.0) - Vec3::X).length() < 1e-6);
        assert!((turned(ahead, 180.0) - Vec3::NEG_Z).length() < 1e-6);
    }
}
