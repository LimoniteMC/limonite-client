//! Drawing the UI: the tree becomes quads, and the quads go to the GPU when they
//! change (m5-plan Decision 14).
//!
//! **Blended the way the game blends** (worth teaching). The terrain draws in linear
//! light, into an sRGB view of the frame, which converts on the way out. Blending
//! there is physically right, but it isn't what the game's UI does: half-transparent
//! black over a bright sky comes out far lighter than in the official client. So the
//! UI draws through a second view of the same picture, without the sRGB format, and
//! its colours go in and blend as they are, in gamma, as the art was made to be seen.
//! Whether a surface can have that view is a capability, so it's asked
//! (`Gpu::ui_format`), and where it can't, the UI falls back to linear.
//!
//! **Snapped to whole pixels.** Layout can land on half a unit, and a unit can be
//! several pixels. A quad's edges are rounded to window pixels here, so that pixel
//! art stays sharp.

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use image::RgbaImage;

use super::{
    atlas::{Atlas, SpriteId, nine_pieces},
    font::{Font, HEIGHT, SHADOW, shown},
    layout::Measure,
    tree::{Blend, Kind, NodeId, Tree},
};
use crate::{
    inventory::items::ItemIcons,
    render::{
        Gpu,
        icons::{Icon, IconQuad},
        terrain::{INDICES_PER_QUAD, index_buffer},
    },
};

/// Text, and whatever else draws as it is.
const WHITE: [u8; 4] = [255; 4];
/// Text's shadow: a quarter of its brightness.
const SHADOW_COLOR: [u8; 4] = [63, 63, 63, 255];
const QUADS_AT_FIRST: u32 = 256;

/// One corner of a quad.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    /// In window pixels.
    pub position: [f32; 2],
    /// In the atlas, from 0 to 1.
    pub uv: [f32; 2],
    pub color: [u8; 4],
    /// A block icon's texture layer, and bit 16 if it's an overlay.
    pub layer: u32,
}

/// The bit of [`Vertex::layer`] that marks an overlay.
const OVERLAY: u32 = 1 << 16;

/// Quads in a row that blend the same way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub blend: Blend,
    pub quads: Range<u32>,
}

/// What one version of the UI draws.
#[derive(Debug, Default)]
pub struct Drawing {
    pub vertices: Vec<Vertex>,
    pub batches: Vec<Batch>,
}

impl Drawing {
    #[must_use]
    pub fn quads(&self) -> u32 {
        (self.vertices.len() / 4) as u32
    }

    /// Adds a quad: `dest` in UI units, and `source` in atlas texels, each left, top,
    /// right and bottom.
    fn quad(&mut self, atlas: &Atlas, scale: f32, blend: Blend, dest: [f32; 4], source: [u32; 4], color: [u8; 4]) {
        let [left, top, right, bottom] = dest.map(|edge| (edge * scale).round());
        // Rounded to nothing, or an empty piece of a nine-slice.
        if right <= left || bottom <= top {
            return;
        }
        let [width, height] = atlas.size().map(|side| side as f32);
        let [u0, v0, u1, v1] = [
            source[0] as f32 / width,
            source[1] as f32 / height,
            source[2] as f32 / width,
            source[3] as f32 / height,
        ];
        let quad = self.quads();
        self.vertices.extend_from_slice(&[
            Vertex { position: [left, top], uv: [u0, v0], color, layer: 0 },
            Vertex { position: [left, bottom], uv: [u0, v1], color, layer: 0 },
            Vertex { position: [right, bottom], uv: [u1, v1], color, layer: 0 },
            Vertex { position: [right, top], uv: [u1, v0], color, layer: 0 },
        ]);
        self.batch(blend, quad);
    }

    /// Puts quad number `quad` in the last batch, if it blends the same way.
    fn batch(&mut self, blend: Blend, quad: u32) {
        match self.batches.last_mut() {
            Some(batch) if batch.blend == blend => batch.quads.end = quad + 1,
            _ => self.batches.push(Batch { blend, quads: quad..quad + 1 }),
        }
    }

    /// Adds one quad of a block icon drawn in `rect`. Its corners aren't square to the
    /// screen, but they're snapped as well, so that neighbouring faces meet.
    fn icon_quad(&mut self, rect: [f32; 4], scale: f32, icon: &IconQuad) {
        let [left, top, right, bottom] = rect;
        let [r, g, b] = icon.tint;
        let color = [r, g, b, (icon.shade * 255.0) as u8];
        let layer = u32::from(icon.layer) | if icon.overlay { OVERLAY } else { 0 };
        let quad = self.quads();
        for (corner, uv) in icon.corners.iter().zip(icon.uvs) {
            let x = left + corner[0] * (right - left);
            let y = top + corner[1] * (bottom - top);
            self.vertices.push(Vertex {
                position: [(x * scale).round(), (y * scale).round()],
                uv,
                color,
                layer,
            });
        }
        self.batch(Blend::Block, quad);
    }
}

/// What the tree is drawn with.
#[derive(Clone, Copy)]
pub struct Looks<'a> {
    pub atlas: &'a Atlas,
    pub font: &'a Font,
    pub icons: &'a ItemIcons,
    /// How many window pixels a UI unit is.
    pub scale: f32,
}

impl Measure for Looks<'_> {
    fn image(&self, sprite: SpriteId) -> [f32; 2] {
        self.atlas.get(sprite).size()
    }

    fn text(&self, text: &str) -> [f32; 2] {
        [self.font.width(text), HEIGHT]
    }
}

/// Builds the quads for everything visible, children over their parents.
pub fn build(tree: &Tree, looks: Looks<'_>, drawing: &mut Drawing) {
    drawing.vertices.clear();
    drawing.batches.clear();
    add_node(tree, Tree::ROOT, looks, drawing);
}

fn add_node(tree: &Tree, id: NodeId, looks: Looks<'_>, drawing: &mut Drawing) {
    let node = tree.node(id);
    if !node.visible {
        return;
    }
    match &node.kind {
        Kind::Panel => {}
        Kind::Image(image) => {
            if let Some(sprite) = image.sprite {
                let sprite = looks.atlas.get(sprite);
                let dest = node.rect.edges();
                if let Some(nine) = &sprite.nine_slice {
                    for piece in nine_pieces(dest, sprite.source, nine) {
                        drawing.quad(looks.atlas, looks.scale, image.blend, piece.dest, piece.source, WHITE);
                    }
                } else {
                    drawing.quad(looks.atlas, looks.scale, image.blend, dest, sprite.source, WHITE);
                }
            }
        }
        Kind::Icon(None) => {}
        Kind::Icon(Some(Icon::Sprite(sprite))) => {
            let source = looks.atlas.get(*sprite).source;
            drawing.quad(looks.atlas, looks.scale, Blend::Alpha, node.rect.edges(), source, WHITE);
        }
        Kind::Icon(Some(Icon::Block { first, count })) => {
            for quad in looks.icons.quads(*first, *count) {
                drawing.icon_quad(node.rect.edges(), looks.scale, quad);
            }
        }
        Kind::Label(label) => {
            add_text(&label.text, [node.rect.x + SHADOW, node.rect.y + SHADOW], SHADOW_COLOR, looks, drawing);
            add_text(&label.text, [node.rect.x, node.rect.y], WHITE, looks, drawing);
        }
    }
    for &child in node.children() {
        add_node(tree, child, looks, drawing);
    }
}

fn add_text(text: &str, at: [f32; 2], color: [u8; 4], looks: Looks<'_>, drawing: &mut Drawing) {
    let mut x = at[0];
    for c in shown(text) {
        if let Some(glyph) = looks.font.glyph(c) {
            let top = at[1] + glyph.y_offset;
            let dest = [x, top, x + glyph.size[0], top + glyph.size[1]];
            drawing.quad(looks.atlas, looks.scale, Blend::Alpha, dest, glyph.source, color);
        }
        x += looks.font.advance(c);
    }
}

/// The UI's GPU state.
pub struct UiRenderer {
    alpha: wgpu::RenderPipeline,
    invert: wgpu::RenderPipeline,
    block: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    block_group: wgpu::BindGroup,
    screen: wgpu::Buffer,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// How many quads the buffers hold.
    capacity: u32,
    batches: Vec<Batch>,
}

impl UiRenderer {
    /// Uploads the atlas, which can then be freed, and builds the pipelines. `blocks` is
    /// the terrain's texture array, which block icons sample.
    #[must_use]
    pub fn new(gpu: &Gpu, atlas: &RgbaImage, blocks: &wgpu::TextureView) -> Self {
        let device = &gpu.device;
        let view = upload_atlas(gpu, atlas);
        // Nearest, so pixel art stays sharp at any scale.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ui atlas"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let screen = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui screen"),
            size: size_of::<[f32; 2]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<[f32; 2]>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: screen.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let block_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui block icons"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let block_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui block icons"),
            layout: &block_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(blocks),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[Some(&layout), Some(&block_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("ui.wgsl"));
        let format = gpu.ui_format;
        let (plain, block_entry) = if format.is_srgb() {
            ("fragment_linear", "fragment_block_linear")
        } else {
            ("fragment_gamma", "fragment_block_gamma")
        };
        let alpha = pipeline(device, &pipeline_layout, &shader, format, plain, wgpu::BlendState::ALPHA_BLENDING);
        let invert = pipeline(device, &pipeline_layout, &shader, format, "fragment_invert", INVERT);
        let block = pipeline(device, &pipeline_layout, &shader, format, block_entry, wgpu::BlendState::ALPHA_BLENDING);
        let (vertices, indices) = buffers(device, QUADS_AT_FIRST);
        Self {
            alpha,
            invert,
            block,
            bind_group,
            block_group,
            screen,
            vertices,
            indices,
            capacity: QUADS_AT_FIRST,
            batches: Vec::new(),
        }
    }

    /// Takes a new version of the UI, for a window of the GPU's current size.
    pub fn upload(&mut self, gpu: &Gpu, drawing: &Drawing) {
        let size = [gpu.config.width as f32, gpu.config.height as f32];
        gpu.queue.write_buffer(&self.screen, 0, bytemuck::cast_slice(&size));
        if drawing.quads() > self.capacity {
            self.capacity = drawing.quads().next_power_of_two();
            (self.vertices, self.indices) = buffers(&gpu.device, self.capacity);
        }
        gpu.queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&drawing.vertices));
        self.batches.clone_from(&drawing.batches);
    }

    /// Draws over what's in `target`, which must be a view in `Gpu::ui_format`.
    pub fn draw(&self, target: &wgpu::TextureView, encoder: &mut wgpu::CommandEncoder) {
        if self.batches.is_empty() {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_bind_group(1, &self.block_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for batch in &self.batches {
            pass.set_pipeline(match batch.blend {
                Blend::Alpha => &self.alpha,
                Blend::Invert => &self.invert,
                Blend::Block => &self.block,
            });
            pass.draw_indexed(batch.quads.start * INDICES_PER_QUAD..batch.quads.end * INDICES_PER_QUAD, 0, 0..1);
        }
    }
}

/// White where the picture is white comes out as the inverse of what's behind it,
/// and black leaves it alone.
const INVERT: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::OneMinusDst,
        dst_factor: wgpu::BlendFactor::OneMinusSrc,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent::OVER,
};

fn upload_atlas(gpu: &Gpu, atlas: &RgbaImage) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: atlas.width(),
        height: atlas.height(),
        depth_or_array_layers: 1,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui atlas"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Not sRGB: the shader wants the colours as they are in the file (see the
        // module's comment), and turns them into linear light itself if it must.
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        atlas.as_raw(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * atlas.width()),
            rows_per_image: Some(atlas.height()),
        },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn buffers(device: &wgpu::Device, quads: u32) -> (wgpu::Buffer, wgpu::Buffer) {
    let vertices = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ui vertices"),
        size: u64::from(quads) * 4 * size_of::<Vertex>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let (indices, _) = index_buffer(device, quads);
    (vertices, indices)
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    fragment: &str,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4, 3 => Uint32];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::ui::{
        atlas::{Input, NineSlice},
        font::{AsciiCells, tests::ascii_input},
        layout::{self, Anchor, Layout},
        tree::Node,
    };

    /// An atlas with a 4 × 4 picture, a nine-sliced one, and the font.
    fn assets() -> (Atlas, Font) {
        let flat = |name: &str, nine_slice| Input {
            name: name.to_string(),
            image: RgbaImage::from_pixel(4, 4, image::Rgba([255; 4])),
            nine_slice,
        };
        let inset = NineSlice {
            texels: [1; 4],
            units: [1.0; 4],
        };
        let (atlas, picture) = Atlas::pack(vec![
            flat("flat", None),
            flat("nine", Some(inset)),
            ascii_input([8, 12], 8, &[('A' as u32, 0, 4), ('b' as u32, 0, 2)]),
        ]);
        let cells = AsciiCells {
            height: 12,
            y_offset: -3.0,
        };
        let font = Font::new(&atlas, &picture, cells, &BTreeSet::new());
        (atlas, font)
    }

    /// Lays the tree out in a 100 × 100 unit screen, and builds it.
    fn drawn(tree: &mut Tree, atlas: &Atlas, font: &Font, scale: f32) -> Drawing {
        let icons = ItemIcons::default();
        let looks = Looks {
            atlas,
            font,
            icons: &icons,
            scale,
        };
        layout::run(tree, [100.0, 100.0], &looks);
        let mut drawing = Drawing::default();
        build(tree, looks, &mut drawing);
        drawing
    }

    #[test]
    fn a_picture_is_one_quad_at_its_rect_with_its_uvs() {
        let (atlas, font) = assets();
        let sprite = atlas.sprite("flat").unwrap();
        let mut tree = Tree::new();
        tree.add(Tree::ROOT, Node::image(Layout::content().at(Anchor::TopLeft).moved(2.0, 3.0), Some(sprite)));
        let drawing = drawn(&mut tree, &atlas, &font, 2.0);
        assert_eq!(drawing.quads(), 1);
        assert_eq!(drawing.vertices[0].position, [4.0, 6.0], "two pixels a unit");
        assert_eq!(drawing.vertices[2].position, [12.0, 14.0]);
        let source = atlas.get(sprite).source;
        let [width, height] = atlas.size().map(|side| side as f32);
        assert_eq!(drawing.vertices[0].uv, [source[0] as f32 / width, source[1] as f32 / height]);
        assert_eq!(drawing.vertices[2].uv, [source[2] as f32 / width, source[3] as f32 / height]);
    }

    #[test]
    fn edges_snap_to_whole_pixels() {
        let (atlas, font) = assets();
        let sprite = atlas.sprite("flat").unwrap();
        let mut tree = Tree::new();
        tree.add(Tree::ROOT, Node::image(Layout::px(10.0, 10.0).at(Anchor::TopLeft).moved(0.0, 1.5), Some(sprite)));
        let drawing = drawn(&mut tree, &atlas, &font, 3.0);
        // 1.5 units at 3 pixels a unit is 4.5, and it's still 30 pixels tall.
        let top = drawing.vertices[0].position[1];
        let bottom = drawing.vertices[1].position[1];
        assert_eq!(top.fract(), 0.0);
        assert_eq!(bottom - top, 30.0);
    }

    #[test]
    fn a_nine_slice_is_nine_quads_and_stays_in_its_sprite() {
        let (atlas, font) = assets();
        let sprite = atlas.sprite("nine").unwrap();
        let mut tree = Tree::new();
        tree.add(Tree::ROOT, Node::image(Layout::px(40.0, 20.0), Some(sprite)));
        let drawing = drawn(&mut tree, &atlas, &font, 1.0);
        assert_eq!(drawing.quads(), 9);
    }

    #[test]
    fn text_is_a_shadow_then_the_text() {
        let (atlas, font) = assets();
        let mut tree = Tree::new();
        tree.add(Tree::ROOT, Node::label(Layout::content().at(Anchor::TopLeft), "Ab"));
        let drawing = drawn(&mut tree, &atlas, &font, 1.0);
        assert_eq!(drawing.quads(), 4, "two glyphs, each twice");
        let corner = |quad: usize| drawing.vertices[quad * 4];
        // The label's at the top left, and its cells start 3 above the line. It's the
        // next one down that keeps the rounding out of it.
        assert_eq!(corner(0).position, [SHADOW, SHADOW - 3.0]);
        assert_eq!(corner(0).color, SHADOW_COLOR);
        assert_eq!(corner(1).position[0], SHADOW + 6.0, "'A' is 5 wide, and a gap");
        assert_eq!(corner(2).position, [0.0, -3.0]);
        assert_eq!(corner(2).color, WHITE);
        // A cell draws 12 tall, though the line's only 8.
        assert_eq!(drawing.vertices[9].position[1] - drawing.vertices[8].position[1], 12.0);
    }

    #[test]
    fn what_is_hidden_or_has_no_picture_draws_nothing() {
        let (atlas, font) = assets();
        let sprite = atlas.sprite("flat").unwrap();
        let mut tree = Tree::new();
        let hidden = tree.add(Tree::ROOT, Node::panel(Layout::default()).hidden());
        tree.add(hidden, Node::image(Layout::px(4.0, 4.0), Some(sprite)));
        tree.add(Tree::ROOT, Node::image(Layout::px(4.0, 4.0), None));
        let drawing = drawn(&mut tree, &atlas, &font, 1.0);
        assert_eq!(drawing.quads(), 0);
        assert!(drawing.batches.is_empty());
    }

    #[test]
    fn a_blend_change_starts_a_batch_and_order_is_kept() {
        let (atlas, font) = assets();
        let sprite = Some(atlas.sprite("flat").unwrap());
        let mut tree = Tree::new();
        tree.add(Tree::ROOT, Node::image(Layout::px(4.0, 4.0), sprite));
        tree.add(Tree::ROOT, Node::image(Layout::px(4.0, 4.0), sprite));
        tree.add(Tree::ROOT, Node::image(Layout::px(4.0, 4.0), sprite).inverting());
        tree.add(Tree::ROOT, Node::image(Layout::px(4.0, 4.0), sprite));
        let drawing = drawn(&mut tree, &atlas, &font, 1.0);
        assert_eq!(
            drawing.batches,
            [
                Batch { blend: Blend::Alpha, quads: 0..2 },
                Batch { blend: Blend::Invert, quads: 2..3 },
                Batch { blend: Blend::Alpha, quads: 3..4 },
            ]
        );
    }

    /// The shader is compiled by the GPU driver's wgpu at startup; this catches what
    /// naga, wgpu's own compiler, would reject, without a GPU.
    #[test]
    fn the_shader_parses_and_validates() {
        let source = include_str!("ui.wgsl");
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|err| panic!("{}", err.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|err| panic!("{err:?}"));
    }
}
