//! The outline around the block aimed at (m5-plan Decision 8): dark lines along
//! its shape's edges.
//!
//! **Edges, not boxes** (worth teaching). A block's outline can be several
//! boxes: a stair is two. Drawing every box's twelve edges would put lines
//! across the stair where one box meets the other. So [`edges`] looks at the
//! shape they make together, and keeps a line only where its surface folds.
//!
//! **Inside the terrain's pass** (worth teaching). The lines need the depth
//! buffer, so that a block in front of them hides them, and it isn't kept once
//! the pass ends. They go after the opaque faces and before the blended ones,
//! so water in front of them tints them as it does the block.
//!
//! **Wide lines are rectangles** (worth teaching). A GPU only promises lines one
//! pixel wide, which is thin on a big screen. So each line is one instance of two
//! triangles, and the vertex shader moves their corners apart on the screen: the
//! same number of pixels however far off the line is, on any GPU.

use glam::DVec3;

use super::Gpu;
use crate::blocks::shapes::Aabb;

/// How far the lines stand off the block, so that they don't fight its faces
/// for the same pixels.
const STAND_OFF: f64 = 0.002;
/// A line's two triangles.
const CORNERS: u32 = 6;
/// A line's two ends, relative to the camera.
type Line = [[f32; 3]; 2];

/// How wide the lines are, in pixels: 2 up to 1080p, and then in step with the window, so
/// that they look the same on any screen. A whole number of pixels keeps them crisp.
fn line_width(window_height: u32) -> f32 {
    (window_height as f32 / 540.0).round().max(2.0)
}

/// The lines' GPU state.
pub struct Outline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    /// The window's width and height and the lines' width, in pixels, and a last
    /// number that's only there because a uniform comes in 16 bytes.
    sizes: wgpu::Buffer,
    written: [f32; 4],
    lines: wgpu::Buffer,
    /// How many lines the buffer holds, and how many to draw.
    capacity: usize,
    count: u32,
}

impl Outline {
    #[must_use]
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
        frame: &wgpu::Buffer,
    ) -> Self {
        let sizes = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("outline sizes"),
            size: size_of::<[f32; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform = |binding, size| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(size),
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("outline"),
            entries: &[uniform(0, frame.size()), uniform(1, sizes.size())],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("outline"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: sizes.as_entire_binding(),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("outline"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("outline.wgsl"));
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("outline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Line>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(false),
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
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // A full cube's worth, which is most of what it ever draws.
        let capacity = 12;
        Self {
            pipeline,
            bind_group,
            sizes,
            written: [0.0; 4],
            lines: line_buffer(device, capacity),
            capacity,
            count: 0,
        }
    }

    /// What the next draw shows: the edges of `boxes`, in world blocks, seen
    /// from `eye`. No boxes draw nothing. It's written again each frame, since
    /// positions are camera-relative (`camera.rs`), and it's a few hundred bytes.
    pub fn show(&mut self, gpu: &Gpu, eye: DVec3, boxes: &[Aabb]) {
        let apart: Vec<Aabb> = boxes.iter().map(|area| area.shrunk(DVec3::splat(-STAND_OFF))).collect();
        let lines: Vec<Line> = edges(&apart)
            .into_iter()
            .map(|ends| ends.map(|end| (end - eye).as_vec3().to_array()))
            .collect();
        self.count = lines.len() as u32;
        if lines.len() > self.capacity {
            self.capacity = lines.len().next_power_of_two();
            self.lines = line_buffer(&gpu.device, self.capacity);
        }
        if !lines.is_empty() {
            gpu.queue.write_buffer(&self.lines, 0, bytemuck::cast_slice(&lines));
        }
        let (width, height) = (gpu.config.width, gpu.config.height);
        let sizes = [width as f32, height as f32, line_width(height), 0.0];
        if sizes != self.written {
            self.written = sizes;
            gpu.queue.write_buffer(&self.sizes, 0, bytemuck::cast_slice(&sizes));
        }
    }

    /// Draws the lines. This sets its own bind group 0, so whatever draws next
    /// has to set its own again.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count > 0 {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.lines.slice(..));
            pass.draw(0..CORNERS, 0..self.count);
        }
    }
}

fn line_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("outline"),
        size: (capacity * size_of::<Line>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The lines along the edges of the shape `boxes` make together (worth
/// teaching).
///
/// Every coordinate a box starts or ends at cuts space, and the cuts make a
/// grid of cells, each wholly in the shape or wholly out of it. Now take a piece
/// of a grid line, and the four cells around it:
/// - with none of them in the shape, or all four, it's not on the surface;
/// - with two side by side, it's on a flat stretch of surface, so no line;
/// - with one, it's an outside corner, with three an inside corner, and with two
///   across from each other, two corners meeting. Those are the edges.
///
/// Pieces in a row join up into one line.
#[must_use]
pub fn edges(boxes: &[Aabb]) -> Vec<[DVec3; 2]> {
    let grid: [Vec<f64>; 3] = [0, 1, 2].map(|axis| {
        let mut cuts: Vec<f64> = boxes.iter().flat_map(|area| [area.min[axis], area.max[axis]]).collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        cuts
    });
    // Whether the cell after each of these cuts is in the shape.
    let full = |cell: [usize; 3]| {
        let Some(middle) = (0..3)
            .map(|axis| Some((grid[axis].get(cell[axis])? + grid[axis].get(cell[axis] + 1)?) / 2.0))
            .collect::<Option<Vec<f64>>>()
        else {
            return false;
        };
        boxes
            .iter()
            .any(|area| (0..3).all(|axis| area.min[axis] < middle[axis] && middle[axis] < area.max[axis]))
    };

    let mut lines = Vec::new();
    for axis in 0..3 {
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        for i in 0..grid[a].len() {
            for j in 0..grid[b].len() {
                // The line along `axis` where cuts `i` and `j` of the other two cross.
                let end = |along: usize| {
                    let mut point = DVec3::ZERO;
                    point[axis] = grid[axis][along];
                    point[a] = grid[a][i];
                    point[b] = grid[b][j];
                    point
                };
                let mut start = None;
                for along in 0..grid[axis].len() {
                    let around = |before_i: bool, before_j: bool| {
                        let ci = if before_i { i.checked_sub(1) } else { Some(i) };
                        let cj = if before_j { j.checked_sub(1) } else { Some(j) };
                        let (Some(ci), Some(cj)) = (ci, cj) else {
                            return false;
                        };
                        let mut cell = [0; 3];
                        cell[axis] = along;
                        cell[a] = ci;
                        cell[b] = cj;
                        full(cell)
                    };
                    let (p, q, r, s) = (around(true, true), around(false, true), around(true, false), around(false, false));
                    let count = [p, q, r, s].into_iter().filter(|&in_shape| in_shape).count();
                    let folds = count == 1 || count == 3 || (count == 2 && p == s);
                    if folds {
                        start.get_or_insert(along);
                    } else if let Some(from) = start.take() {
                        lines.push([end(from), end(along)]);
                    }
                }
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(min: [f64; 3], max: [f64; 3]) -> Aabb {
        Aabb::new(DVec3::from_array(min), DVec3::from_array(max))
    }

    #[test]
    fn lines_are_two_pixels_wide_up_to_1080p_and_grow_with_the_window() {
        assert_eq!(line_width(620), 2.0);
        assert_eq!(line_width(1080), 2.0);
        assert_eq!(line_width(1440), 3.0);
        assert_eq!(line_width(2160), 4.0);
    }

    #[test]
    fn a_box_has_twelve_edges() {
        let lines = edges(&[Aabb::cube(3, 64, -2)]);
        assert_eq!(lines.len(), 12);
        for [from, to] in lines {
            assert!(((to - from).length() - 1.0).abs() < 1e-12, "{from} to {to}");
        }
    }

    #[test]
    fn nothing_has_no_edges() {
        assert!(edges(&[]).is_empty());
    }

    #[test]
    fn boxes_that_make_one_box_have_its_edges() {
        // One on top of the other, which even overlap a little.
        let lines = edges(&[area([0.0; 3], [1.0, 1.1, 1.0]), area([0.0, 1.0, 0.0], [1.0, 2.0, 1.0])]);
        assert_eq!(lines.len(), 12, "{lines:?}");
    }

    #[test]
    fn a_stair_has_no_line_across_it() {
        // The bottom half, and the top half of the back: an L, drawn out along x. The
        // L has six corners, so six lines along x and six at each end.
        let stair = [area([0.0; 3], [1.0, 0.5, 1.0]), area([0.0, 0.5, 0.5], [1.0, 1.0, 1.0])];
        let lines = edges(&stair);
        assert_eq!(lines.len(), 18, "{lines:?}");
        // Nothing runs across the back, where the two boxes meet.
        let across_the_back = |[from, to]: &[DVec3; 2]| from.y == 0.5 && to.y == 0.5 && from.z == 1.0 && to.z == 1.0;
        assert!(!lines.iter().any(across_the_back), "{lines:?}");
    }

    #[test]
    fn apart_boxes_keep_their_own_edges_and_a_shared_one_counts_once() {
        let two = [area([0.0; 3], [1.0; 3]), area([2.0, 0.0, 0.0], [3.0, 1.0, 1.0])];
        assert_eq!(edges(&two).len(), 24);
        // Diagonal neighbours, which meet along one line: 24 less that one, and less
        // the four that carry straight on from one box to the other.
        let diagonal = [area([0.0; 3], [1.0; 3]), area([1.0, 1.0, 0.0], [2.0, 2.0, 1.0])];
        assert_eq!(edges(&diagonal).len(), 19);
    }

    #[test]
    fn standing_off_keeps_the_same_edges() {
        let stair = [area([0.0; 3], [1.0, 0.5, 1.0]), area([0.0, 0.5, 0.5], [1.0, 1.0, 1.0])];
        let apart: Vec<Aabb> = stair.iter().map(|box_| box_.shrunk(DVec3::splat(-STAND_OFF))).collect();
        assert_eq!(edges(&apart).len(), 18);
        let cube = Aabb::cube(0, 0, 0).shrunk(DVec3::splat(-STAND_OFF));
        assert_eq!(edges(&[cube]).len(), 12);
    }

    /// As for the terrain's shader, without a GPU.
    #[test]
    fn the_shader_parses_and_validates() {
        let source = include_str!("outline.wgsl");
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|err| panic!("{}", err.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|err| panic!("{err:?}"));
    }
}
