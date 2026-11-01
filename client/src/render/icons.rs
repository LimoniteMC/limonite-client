//! Block icons: a block as the inventory shows it, tilted to show three sides
//! (m5-plan Decision 10; worth teaching).
//!
//! **The world's own geometry.** An icon is the block's real mesh, from the same
//! mesher as the world, so stairs look like stairs and a fence like a fence, and a new
//! shape needs no icon code. It's meshed once, on its own, and then flattened: each
//! quad's corners are turned to the inventory's angle, and only x and y are kept.
//!
//! **No depth buffer.** With nothing to keep the nearest face in front, the quads are
//! drawn back to front instead, the painter's algorithm. It would fail on faces that
//! cross one another, which a block's faces don't. So an icon is a list of flat quads
//! that goes through the UI like everything else.
//!
//! **Plants and other flat blocks** show their texture flat, as they do on Bedrock:
//! seen from that angle, a cross of two planes is an X of nothing much.

use super::{
    mesher::{self, Neighbourhood, Vertex},
    tint::Tints,
};
use crate::blocks::{
    appearance::{AppearanceTable, Shape},
    models::{POSITION_SCALE, UV_SCALE},
};

/// Where an icon's biome tints come from: plains.
pub const PLAINS: u32 = 1;
/// How wide and tall a block ends up, turned to the inventory's angle and with sides of
/// 1: a cube's corners reach 0.787 up and down from its middle. Dividing by this makes
/// a cube fill the icon, top to bottom.
const TALLEST: f32 = 1.573;
/// How a sub-chunk's position relates to the block's middle: the mesher puts it in the
/// middle of the sub-chunk, at (8, 8, 8), and its own middle is half a block on.
const MIDDLE: f32 = 8.5;
/// How much light each way a quad can face gets, in [`Face`](crate::blocks::models)
/// order: down, up, north, south, west, east, and then the diagonal planes. A full cube
/// shows up, south and east.
const SHADE: [f32; 7] = [0.5, 1.0, 0.8, 0.8, 0.6, 0.6, 0.9];
/// A vertex's render class that tints only what its alpha marks.
const OVERLAY: u8 = 2;

/// One flat quad of an icon. Corners run from 0 to 1 across the icon's square, with y
/// downwards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconQuad {
    pub corners: [[f32; 2]; 4],
    /// Each corner's place on the texture, from 0 to 1.
    pub uvs: [[f32; 2]; 4],
    pub layer: u16,
    /// Whether only the texels its alpha marks take the tint, and it's drawn opaque.
    pub overlay: bool,
    /// The tint, sRGB-encoded.
    pub tint: [u8; 3],
    /// How much of the light it gets, from 0 to 1.
    pub shade: f32,
}

/// What an item's icon is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// A picture in the UI's atlas.
    Sprite(crate::ui::atlas::SpriteId),
    /// These quads of the icon store (`inventory/items.rs`).
    Block { first: u32, count: u32 },
}

/// Turns a position to the inventory's angle: 45° around y, then 30° down. Returns the
/// x and y on the screen, with y upwards, and how near it is to the viewer.
fn turned([x, y, z]: [f32; 3]) -> (f32, f32, f32) {
    const SIN_45: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let screen_x = (x - z) * SIN_45;
    let along = (x + z) * SIN_45;
    let up = y * 0.866_025_4 - along * 0.5;
    let near = along * 0.866_025_4 + y * 0.5;
    (screen_x, up, near)
}

/// A vertex's position, relative to the block's middle, in blocks.
fn from_middle(vertex: &Vertex) -> [f32; 3] {
    let scale = f32::from(POSITION_SCALE) * 16.0;
    vertex.position.map(|axis| f32::from(axis) / scale - MIDDLE)
}

/// The quads of the block with this id, back to front. Empty if it draws nothing.
#[must_use]
pub fn block_icon(table: &AppearanceTable, tints: &Tints, id: u32) -> Vec<IconQuad> {
    let look = table.get(id);
    match look.shape {
        Shape::Invisible => return Vec::new(),
        Shape::Cross | Shape::Stalk { .. } | Shape::Planes { .. } => {
            let face = &look.faces[0];
            let [r, g, b, _] = if face.tinted { tints.color(PLAINS, look.tint) } else { [255; 4] };
            return vec![IconQuad {
                corners: [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
                uvs: [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
                layer: face.layer,
                overlay: false,
                tint: [r, g, b],
                shade: 1.0,
            }];
        }
        _ => {}
    }
    let mesh = mesher::mesh(&Neighbourhood::single(table, PLAINS, id), tints);
    let (opaque, blend) = (mesh.opaque.as_chunks::<4>().0, mesh.blend.as_chunks::<4>().0);
    let mut quads: Vec<(f32, IconQuad)> = opaque.iter().chain(blend).filter_map(flattened).collect();
    quads.sort_by(|a, b| a.0.total_cmp(&b.0));
    quads.into_iter().map(|(_, quad)| quad).collect()
}

/// A quad of the mesh, flattened, with how near it is. `None` if it faces away.
fn flattened(vertices: &[Vertex; 4]) -> Option<(f32, IconQuad)> {
    let turned_corners = vertices.each_ref().map(|vertex| turned(from_middle(vertex)));
    // The terrain draws counter-clockwise quads, seen from the front. With y still
    // upwards, that's a positive area here.
    let [a, b, c, _] = turned_corners;
    let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    if area <= 0.0 {
        return None;
    }
    let near = turned_corners.iter().map(|corner| corner.2).sum::<f32>() / 4.0;
    let corners = turned_corners.map(|(x, up, _)| [0.5 + x / TALLEST, 0.5 - up / TALLEST]);
    let uv_scale = f32::from(UV_SCALE) * 16.0;
    let first = &vertices[0];
    let [r, g, b, _] = first.tint;
    Some((
        near,
        IconQuad {
            corners,
            uvs: vertices.each_ref().map(|vertex| vertex.uv.map(|axis| f32::from(axis) / uv_scale)),
            layer: first.layer,
            overlay: first.face_class >> 3 == OVERLAY,
            tint: [r, g, b],
            shade: SHADE[usize::from(first.face_class & 0b111).min(SHADE.len() - 1)],
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        blocks::appearance::{Appearance, FaceLook, RenderClass},
        render::tint::Tint,
    };

    const WHITE_WATER: [u8; 4] = [255; 4];

    fn tints() -> Tints {
        Tints::uniform([100, 200, 50], WHITE_WATER)
    }

    fn look(shape: Shape, layers: [u16; 6]) -> Appearance {
        let face = |layer| FaceLook {
            layer,
            class: RenderClass::Opaque,
            tinted: false,
            turns: 0,
            isotropic: false,
        };
        let mut faces = [face(0); crate::blocks::appearance::SLOTS];
        for (slot, layer) in faces.iter_mut().zip(layers) {
            *slot = face(layer);
        }
        Appearance {
            shape,
            tint: Tint::None,
            faces,
            occludes: false,
            under_snow: None,
            snow: false,
        }
    }

    fn table(appearance: Appearance) -> AppearanceTable {
        let air = look(Shape::Invisible, [0; 6]);
        AppearanceTable::from_looks([(crate::world::AIR, air), (5, appearance)])
    }

    /// Down, up, north, south, west, east: each a layer of its own.
    const LAYERS: [u16; 6] = [10, 11, 12, 13, 14, 15];

    #[test]
    fn a_cube_shows_three_sides_with_the_top_lit() {
        let quads = block_icon(&table(look(Shape::Cube, LAYERS)), &tints(), 5);
        let mut layers: Vec<u16> = quads.iter().map(|quad| quad.layer).collect();
        layers.sort_unstable();
        assert_eq!(layers, [11, 13, 15], "up, south and east");
        let shade = |layer| quads.iter().find(|quad| quad.layer == layer).unwrap().shade;
        assert_eq!((shade(11), shade(13), shade(15)), (1.0, 0.8, 0.6));
    }

    #[test]
    fn a_cube_fills_the_icon_from_top_to_bottom_and_stays_inside() {
        let quads = block_icon(&table(look(Shape::Cube, LAYERS)), &tints(), 5);
        let corners = quads.iter().flat_map(|quad| quad.corners);
        let (mut top, mut bottom, mut left, mut right) = (1.0_f32, 0.0_f32, 1.0_f32, 0.0_f32);
        for [x, y] in corners {
            top = top.min(y);
            bottom = bottom.max(y);
            left = left.min(x);
            right = right.max(x);
        }
        assert!(top.abs() < 0.01 && (bottom - 1.0).abs() < 0.01, "{top} to {bottom}");
        assert!(left > 0.0 && right < 1.0, "narrower than it is tall: {left} to {right}");
        assert!((left + right - 1.0).abs() < 0.001, "and in the middle");
    }

    #[test]
    fn the_top_is_on_top_and_the_sides_either_side() {
        let quads = block_icon(&table(look(Shape::Cube, LAYERS)), &tints(), 5);
        let middle = |layer| {
            let quad = quads.iter().find(|quad| quad.layer == layer).unwrap();
            let sum = quad.corners.iter().fold([0.0, 0.0], |sum, corner| [sum[0] + corner[0], sum[1] + corner[1]]);
            [sum[0] / 4.0, sum[1] / 4.0]
        };
        let (top, south, east) = (middle(11), middle(13), middle(15));
        assert!(top[1] < south[1] && top[1] < east[1], "y runs down the screen");
        assert!(south[0] < 0.5 && east[0] > 0.5, "south on the left, east on the right");
    }

    #[test]
    fn a_slab_is_lower_than_a_cube() {
        let slab = Shape::Slab { bottom: 0, top: 8 };
        let quads = block_icon(&table(look(slab, LAYERS)), &tints(), 5);
        assert_eq!(quads.len(), 3);
        let top = quads.iter().flat_map(|quad| quad.corners).map(|[_, y]| y).fold(1.0, f32::min);
        assert!(top > 0.1, "its top comes down: {top}");
    }

    #[test]
    fn a_plant_is_its_texture_flat() {
        let quads = block_icon(&table(look(Shape::Cross, [7; 6])), &tints(), 5);
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].layer, 7);
        assert_eq!(quads[0].corners[0], [0.0, 0.0]);
        assert_eq!(quads[0].corners[2], [1.0, 1.0]);
        assert_eq!(quads[0].shade, 1.0);
    }

    #[test]
    fn a_tinted_plant_takes_the_plains_tint() {
        let mut appearance = look(Shape::Cross, [7; 6]);
        appearance.tint = Tint::Grass;
        appearance.faces[0].tinted = true;
        let quads = block_icon(&table(appearance), &tints(), 5);
        assert_eq!(quads[0].tint, [100, 200, 50]);
    }

    #[test]
    fn nothing_to_draw_is_no_quads() {
        assert!(block_icon(&table(look(Shape::Invisible, [0; 6])), &tints(), 5).is_empty());
    }

    #[test]
    fn nearer_quads_come_later() {
        // A cube's three sides don't overlap, so check the order on what can: the sums.
        let (_, _, far) = turned([-0.5, -0.5, -0.5]);
        let (_, _, near) = turned([0.5, 0.5, 0.5]);
        assert!(near > far);
    }
}
