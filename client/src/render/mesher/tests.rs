//! The mesher on hand-made appearances: no packs and no GPU.

use bytes::Bytes;
use limonite_packs::registry::Face;
use pumpkin_core::net::bedrock::level_chunk::CLevelChunk;
use pumpkin_data::{Block as JavaBlock, BlockState};
use pumpkin_protocol::{RawPacket, packet::Packet};
use pumpkin_world::chunk::ChunkData;

use super::*;
use crate::{
    blocks::appearance::{Appearance, AppearanceTable, BambooLeaves, FaceLook, Liquid, RenderClass, Shape},
    render::tint::{Tint, Tints},
    world::{AIR, World},
};

const STONE: u32 = 1;
const GLASS: u32 = 2;
const PLANT: u32 = 3;
const WATER: u32 = 4;
const SEAGRASS: u32 = 5;
const SLAB: u32 = 6;
const SNOW: u32 = 7;
const VINE: u32 = 8;
const SAND: u32 = 9;
const BAMBOO: u32 = 10;
const BAMBOO_THICK_LARGE: u32 = 11;
const BAMBOO_SMALL: u32 = 12;
const TORCH: u32 = 13;
const STAIRS: u32 = 14;

/// The texture layers of a bamboo look, as the table keeps them.
const STEM: u16 = 20;
const LARGE_LEAVES: u16 = 21;
const SMALL_LEAVES: u16 = 22;

fn look(shape: Shape, class: RenderClass, tint: Tint, layer: u16) -> Appearance {
    let face = FaceLook {
        layer,
        class,
        tinted: tint != Tint::None,
        turns: 0,
        isotropic: false,
    };
    Appearance {
        shape,
        tint,
        faces: [face; crate::blocks::appearance::SLOTS],
        occludes: shape == Shape::Cube && class == RenderClass::Opaque,
        under_snow: None,
        snow: false,
    }
}

/// Bamboo: the stem in north (and the other slots), the large leaves in up,
/// the small ones in south.
fn bamboo(thick: bool, leaves: BambooLeaves) -> Appearance {
    let mut bamboo = look(Shape::Bamboo { thick, leaves }, RenderClass::Opaque, Tint::None, STEM);
    for (face, layer) in [(Face::Up, LARGE_LEAVES), (Face::South, SMALL_LEAVES)] {
        bamboo.faces[face as usize].layer = layer;
        bamboo.faces[face as usize].class = RenderClass::Cutout;
    }
    bamboo
}

fn table() -> AppearanceTable {
    let mut sand = look(Shape::Cube, RenderClass::Opaque, Tint::None, 9);
    sand.faces[Face::Up as usize].isotropic = true;
    let vine_faces = (1 << Face::North as u8) | (1 << Face::South as u8);
    AppearanceTable::from_looks([
        (AIR, look(Shape::Invisible, RenderClass::Opaque, Tint::None, 0)),
        (STONE, look(Shape::Cube, RenderClass::Opaque, Tint::None, 1)),
        (GLASS, look(Shape::Cube, RenderClass::Cutout, Tint::None, 2)),
        (PLANT, look(Shape::Cross, RenderClass::Cutout, Tint::Grass, 3)),
        (
            WATER,
            look(
                Shape::Liquid {
                    liquid: Liquid::Water,
                    amount: 8,
                },
                RenderClass::Blend,
                Tint::Water,
                4,
            ),
        ),
        (SEAGRASS, look(Shape::Cross, RenderClass::Cutout, Tint::None, 5)),
        (SLAB, look(Shape::Slab { bottom: 0, top: 8 }, RenderClass::Opaque, Tint::None, 6)),
        (SNOW, look(Shape::Slab { bottom: 0, top: 2 }, RenderClass::Opaque, Tint::None, 7)),
        (VINE, look(Shape::Planes { faces: vine_faces }, RenderClass::Cutout, Tint::Foliage, 8)),
        (SAND, sand),
        (BAMBOO, bamboo(false, BambooLeaves::None)),
        (BAMBOO_THICK_LARGE, bamboo(true, BambooLeaves::Large)),
        (BAMBOO_SMALL, bamboo(false, BambooLeaves::Small)),
        (TORCH, boxes(JavaBlock::TORCH.default_state.id, 13)),
        (STAIRS, boxes(east_stairs(), 14)),
    ])
}

/// Pumpkin's outline boxes of a Java state, in texture layer `layer`.
fn boxes(state: pumpkin_data::BlockStateId, layer: u16) -> Appearance {
    look(Shape::Boxes { state }, RenderClass::Cutout, Tint::None, layer)
}

/// Oak stairs climbing to the east: a slab, and a step on its east half.
fn east_stairs() -> pumpkin_data::BlockStateId {
    JavaBlock::OAK_STAIRS
        .states
        .iter()
        .map(|state| state.id)
        .find(|&state| {
            JavaBlock::OAK_STAIRS.properties(state).is_some_and(|props| {
                let props: Vec<_> = props.to_props().iter().map(|p| (p.key().to_str(), p.value_str())).collect();
                [("facing", "east"), ("half", "bottom"), ("shape", "straight")]
                    .iter()
                    .all(|wanted| props.contains(wanted))
            })
        })
        .expect("east-facing stairs")
}

/// The quads in texture layer `layer`.
fn layer_quads(vertices: &[Vertex], layer: u16) -> Vec<&[Vertex]> {
    quads(vertices).into_iter().filter(|quad| quad[0].layer == layer).collect()
}

fn tints() -> Tints {
    Tints::uniform([10, 200, 10], [40, 100, 250, 166])
}

/// Meshes a sub-chunk at the origin holding `blocks` on layer 0 and water
/// on layer 1 at `waterlogged`.
fn mesh_of(blocks: &[(i32, i32, i32, u32)], waterlogged: &[(i32, i32, i32)]) -> Mesh {
    let table = table();
    let n = Neighbourhood::from_fn(&table, [0, 0, 0], 1, |x, y, z, layer| {
        if layer == 0 {
            blocks
                .iter()
                .find(|&&(bx, by, bz, _)| (bx, by, bz) == (x, y, z))
                .map_or(AIR, |&(.., id)| id)
        } else if waterlogged.contains(&(x, y, z)) {
            WATER
        } else {
            AIR
        }
    });
    mesh(&n, &tints())
}

/// The quads of a mesh's vertex list.
fn quads(vertices: &[Vertex]) -> Vec<&[Vertex]> {
    vertices.chunks(4).collect()
}

/// Whether some quad lies in the plane `axis` = `at` sixteenths.
fn has_quad_at(vertices: &[Vertex], axis: usize, at: i16) -> bool {
    quads(vertices)
        .iter()
        .any(|quad| quad.iter().all(|vertex| vertex.at()[axis] == at))
}

#[test]
fn one_stone_has_six_faces_and_two_touching_have_ten() {
    assert!(mesh_of(&[], &[]).is_empty());
    let one = mesh_of(&[(5, 5, 5, STONE)], &[]);
    assert_eq!(one.quads(), 6);
    assert!(!one.is_empty());
    assert!(one.blend.is_empty());
    let two = mesh_of(&[(5, 5, 5, STONE), (6, 5, 5, STONE)], &[]);
    assert_eq!(two.quads(), 10);
    assert!(!has_quad_at(&two.opaque, 0, 6 * 16), "the shared face is hidden");
}

#[test]
fn quads_face_outwards() {
    let mesh = mesh_of(&[(5, 5, 5, STONE)], &[]);
    for quad in quads(&mesh.opaque) {
        let p = |i: usize| quad[i].at().map(i32::from);
        let (a, b, c) = (p(0), p(1), p(2));
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let normal = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let outward = match quad[0].face() {
            0 => [0, -1, 0],
            1 => [0, 1, 0],
            2 => [0, 0, -1],
            3 => [0, 0, 1],
            4 => [-1, 0, 0],
            _ => [1, 0, 0],
        };
        let dot: i32 = normal.iter().zip(outward).map(|(n, o)| n * o).sum();
        assert!(dot > 0, "face {} winds inwards: {normal:?}", quad[0].face());
    }
}

#[test]
fn stone_beside_glass_keeps_the_face_between() {
    let mesh = mesh_of(&[(5, 5, 5, STONE), (6, 5, 5, GLASS)], &[]);
    // Glass doesn't hide the stone's east face; stone hides the glass's west.
    assert_eq!(mesh.quads(), 6 + 5);
    let stone_east = quads(&mesh.opaque).into_iter().filter(|quad| {
        quad[0].face() == Face::East as u8 && quad.iter().all(|v| v.at()[0] == 6 * 16)
    });
    assert_eq!(stone_east.count(), 1);
}

#[test]
fn a_cross_plant_is_four_quads_both_sides_of_two_planes() {
    let mesh = mesh_of(&[(5, 5, 5, PLANT)], &[]);
    assert_eq!(mesh.quads(), 4);
    for vertex in &mesh.opaque {
        assert_eq!(vertex.face(), DIAGONAL);
        assert_eq!(vertex.class(), 1, "cutout");
        assert_eq!(vertex.tint, [10, 200, 10, 255], "grass-tinted");
    }
}

#[test]
fn water_beside_water_or_waterlogged_seagrass_has_no_face_between() {
    // Two water blocks: 6 faces each, less the two between them.
    let mesh = mesh_of(&[(5, 5, 5, WATER), (6, 5, 5, WATER)], &[]);
    assert_eq!(mesh.blend.len() / 4, 10);
    assert!(!has_quad_at(&mesh.blend, 0, 6 * 16));
    assert!(mesh.blend.iter().all(|vertex| vertex.tint == [40, 100, 250, 166]));

    // Seagrass with water around it, on layer 1: the water faces between the
    // two water volumes go too, and the seagrass is drawn.
    let mesh = mesh_of(&[(5, 5, 5, WATER), (6, 5, 5, SEAGRASS)], &[(6, 5, 5)]);
    assert!(!has_quad_at(&mesh.blend, 0, 6 * 16));
    assert_eq!(mesh.blend.len() / 4, 10);
    assert_eq!(mesh.opaque.len() / 4, 4, "the seagrass cross");
}

#[test]
fn water_under_air_gets_a_lowered_top() {
    // On its own its corners dip towards the air around it, as they do in Java.
    let alone = mesh_of(&[(5, 5, 5, WATER)], &[]);
    let top = quads(&alone.blend)
        .into_iter()
        .find(|quad| quad[0].face() == Face::Up as u8)
        .expect("a top");
    assert!(top.iter().all(|vertex| vertex.at()[1] > 5 * 16 && vertex.at()[1] < 6 * 16));

    // With water above, the lower block has no top, and full-height sides.
    let stacked = mesh_of(&[(5, 5, 5, WATER), (5, 6, 5, WATER)], &[]);
    let lower_sides_reach_the_top = quads(&stacked.blend)
        .into_iter()
        .filter(|quad| quad[0].face() == Face::North as u8 && quad.iter().any(|v| v.at()[1] == 5 * 16))
        .any(|quad| quad.iter().any(|v| v.at()[1] == 6 * 16));
    assert!(lower_sides_reach_the_top);
    assert!(!has_quad_at(&stacked.blend, 1, 5 * 16 + 14));
}

/// A table with water of each depth at ids 100 to 108 (the amount over 100), with its
/// still texture on layer 40, its flowing one on 41, and a lava source at 110.
fn fluid_table() -> AppearanceTable {
    let liquid = |liquid, amount, class| {
        let mut look = look(Shape::Liquid { liquid, amount }, class, Tint::None, 41);
        look.faces[Face::Up as usize].layer = 40;
        look.faces[Face::Down as usize].layer = 40;
        look
    };
    let mut looks: Vec<(u32, Appearance)> = (1..=8)
        .map(|amount| (100 + u32::from(amount), liquid(Liquid::Water, amount, RenderClass::Blend)))
        .collect();
    let mut lava = liquid(Liquid::Lava, 8, RenderClass::Opaque);
    lava.faces[Face::Up as usize].isotropic = true;
    looks.push((110, lava));
    looks.push((AIR, look(Shape::Invisible, RenderClass::Opaque, Tint::None, 0)));
    looks.push((STONE, look(Shape::Cube, RenderClass::Opaque, Tint::None, 1)));
    AppearanceTable::from_looks(looks)
}

fn fluid_mesh(blocks: &[(i32, i32, i32, u32)]) -> Mesh {
    mesh(
        &Neighbourhood::from_fn(&fluid_table(), [0, 0, 0], 1, |x, y, z, layer| {
            if layer != 0 {
                return AIR;
            }
            blocks
                .iter()
                .find(|&&(bx, by, bz, _)| [bx, by, bz] == [x, y, z])
                .map_or(AIR, |&(.., id)| id)
        }),
        &tints(),
    )
}

/// The up face of the block at (x, z), by position.
fn top_at(mesh: &Mesh, x: i16, z: i16) -> [Vertex; 4] {
    let inside = |v: &Vertex, axis: usize, at: i16| (at * 1024..=(at + 1) * 1024).contains(&v.position[axis]);
    quads(&mesh.blend)
        .into_iter()
        .chain(quads(&mesh.opaque))
        .find(|quad| quad[0].face() == Face::Up as u8 && quad.iter().all(|v| inside(v, 0, x) && inside(v, 2, z)))
        .map(|quad| [quad[0], quad[1], quad[2], quad[3]])
        .expect("an up face there")
}

#[test]
fn a_still_pool_is_level_and_shows_the_whole_still_texture() {
    let mut blocks = Vec::new();
    for x in 4..=6 {
        for z in 4..=6 {
            blocks.push((x, 5, z, 108));
        }
    }
    let mesh = fluid_mesh(&blocks);
    let top = top_at(&mesh, 5, 5);
    assert_eq!(top[0].layer, 40, "the still texture");
    for vertex in top {
        let eight_ninths = 5 * 1024 + 8 * 1024 / 9;
        assert!((vertex.position[1] - eight_ninths).abs() <= 1, "{vertex:?}");
    }
    assert_eq!(top.map(|v| v.uv), [[0, 0], [0, 128], [128, 128], [128, 0]]);
}

#[test]
fn lava_doesnt_turn_its_texture_block_by_block() {
    // `blocks.json` marks lava's top isotropic, which a block's cube face would obey.
    // A pool of it would come out speckled.
    let mesh = fluid_mesh(&[(3, 5, 3, 110), (4, 5, 3, 110), (5, 5, 3, 110), (6, 5, 3, 110)]);
    for x in 3..=6 {
        assert_eq!(top_at(&mesh, x, 3).map(|v| v.uv), [[0, 0], [0, 128], [128, 128], [128, 0]], "at x {x}");
    }
}

#[test]
fn a_flow_slopes_between_its_blocks_with_no_step_at_their_edge() {
    // A source, then a block of each lower depth, going east.
    let blocks: Vec<_> = (0..4).map(|i| (4 + i, 5, 5, 108 - i as u32)).collect();
    let mesh = fluid_mesh(&blocks);
    let (first, second) = (top_at(&mesh, 5, 5), top_at(&mesh, 6, 5));
    // The two share an edge at x = 6, and it's at one height from both.
    let at_edge = |quad: &[Vertex; 4]| {
        let mut heights: Vec<i16> = quad
            .iter()
            .filter(|v| v.position[0] == 6 * 1024)
            .map(|v| v.position[1])
            .collect();
        heights.sort_unstable();
        heights
    };
    assert_eq!(at_edge(&first), at_edge(&second));
    // It runs downhill: each block's east side is lower than its west.
    let side = |quad: &[Vertex; 4], x: i16| quad.iter().find(|v| v.position[0] == x * 1024).expect("that side").position[1];
    assert!(side(&first, 6) < side(&first, 5));
    assert!(side(&second, 7) < side(&second, 6));
    assert!(side(&second, 7) > 5 * 1024, "and it's still above the block's floor");
}

#[test]
fn a_flowing_top_shows_the_flowing_texture_running_downhill() {
    // Depths falling to the east: it flows along +x.
    let mesh = fluid_mesh(&[(4, 5, 5, 108), (5, 5, 5, 107), (6, 5, 5, 106)]);
    let top = top_at(&mesh, 5, 5);
    assert_eq!(top[0].layer, 41, "the flowing texture");
    let v_at = |x: i16| top.iter().find(|v| v.position[0] == x * 1024).expect("that side").uv[1];
    assert!(v_at(6) > v_at(5), "its texture runs on down the texture's v as the water runs east");
    // A flowing texture covers two blocks, so one block shows half of it.
    let vs: Vec<u8> = top.iter().map(|v| v.uv[1]).collect();
    assert_eq!(vs.iter().max().unwrap() - vs.iter().min().unwrap(), 64);
}

#[test]
fn liquid_sides_show_half_the_flowing_texture_up_to_their_corners() {
    let mesh = fluid_mesh(&[(5, 5, 5, 108)]);
    let side = quads(&mesh.blend)
        .into_iter()
        .find(|quad| quad[0].face() == Face::North as u8)
        .expect("a north side");
    assert_eq!(side[0].layer, 41);
    for vertex in side {
        assert!(vertex.uv[0] <= 64, "half the width: {vertex:?}");
        if vertex.position[1] == 5 * 1024 {
            assert_eq!(vertex.uv[1], 64, "the bottom edge, half way down");
        } else {
            assert!(vertex.uv[1] < 64, "the top edge, higher by as much as the water is high");
        }
    }
}

#[test]
fn diagonal_neighbours_count_towards_a_corner() {
    let alone = fluid_mesh(&[(5, 5, 5, 104), (6, 5, 5, 104)]);
    let with_diagonal = fluid_mesh(&[(5, 5, 5, 104), (6, 5, 5, 104), (6, 5, 6, 108)]);
    let corner = |mesh: &Mesh| {
        top_at(mesh, 5, 5)
            .iter()
            .find(|v| v.position[0] == 6 * 1024 && v.position[2] == 6 * 1024)
            .expect("the corner")
            .position[1]
    };
    assert!(corner(&with_diagonal) > corner(&alone), "the source across the corner lifts it");
}

#[test]
fn water_above_a_neighbour_lifts_their_shared_corner_to_the_top() {
    let mesh = fluid_mesh(&[(5, 5, 5, 108), (6, 5, 5, 108), (6, 6, 5, 108)]);
    let top = top_at(&mesh, 5, 5);
    let at_east: Vec<i16> = top.iter().filter(|v| v.position[0] == 6 * 1024).map(|v| v.position[1]).collect();
    assert_eq!(at_east, [6 * 1024, 6 * 1024], "it meets the block above");
}

#[test]
fn a_liquid_with_nothing_showing_makes_no_quads() {
    let mut blocks = vec![(5, 5, 5, 108)];
    for (x, y, z) in [(4, 5, 5), (6, 5, 5), (5, 5, 4), (5, 5, 6), (5, 4, 5), (5, 6, 5)] {
        blocks.push((x, y, z, STONE));
    }
    assert_eq!(fluid_mesh(&blocks).blend.len(), 4, "only its top, as a block above doesn't reach it");
    let mut covered = blocks;
    covered[6] = (5, 6, 5, 108);
    let quads_at_five = fluid_mesh(&covered)
        .blend
        .iter()
        .filter(|v| v.position[1] <= 6 * 1024 && v.position[1] >= 5 * 1024)
        .count();
    // The block above still draws its own sides, from 6 up. None belong to the lower one.
    assert_eq!(quads_at_five % 4, 0);
}

#[test]
fn boxes_hide_only_the_faces_on_their_blocks_edge() {
    // A torch on stone: its box sits in the middle of the block, so only its
    // bottom, on the block's floor, is hidden by the stone.
    let torch = mesh_of(&[(5, 5, 5, TORCH), (5, 4, 5, STONE)], &[]);
    let torch_quads = layer_quads(&torch.opaque, 13);
    assert_eq!(torch_quads.len(), 5);
    assert!(has_quad_at(&torch.opaque, 1, 5 * 16 + 10), "its top, 10 sixteenths up");
    assert!(has_quad_at(&torch.opaque, 0, 5 * 16 + 6), "its west side, 6 in");

    // Stairs on stone, against stone to the east: the slab's bottom and both
    // east faces are hidden. The step's bottom, inside the block, isn't.
    let stairs = mesh_of(&[(5, 5, 5, STAIRS), (5, 4, 5, STONE), (6, 5, 5, STONE)], &[]);
    assert_eq!(layer_quads(&stairs.opaque, 14).len(), 2 * 6 - 3);
    assert!(has_quad_at(&stairs.opaque, 1, 5 * 16 + 8), "the slab's top and the step's bottom");
}

#[test]
fn slabs_snow_and_vines() {
    // A bottom slab under stone: its top is half way up, and isn't hidden.
    let slab = mesh_of(&[(5, 5, 5, SLAB), (5, 6, 5, STONE)], &[]);
    assert!(has_quad_at(&slab.opaque, 1, 5 * 16 + 8));
    // The slab's sides show the bottom half of the texture.
    let side = quads(&slab.opaque)
        .into_iter()
        .find(|quad| quad[0].face() == Face::South as u8 && quad[0].layer == 6)
        .unwrap();
    assert!(side.iter().all(|vertex| vertex.tex()[1] >= 8));

    let snow = mesh_of(&[(5, 5, 5, SNOW)], &[]);
    assert_eq!(snow.quads(), 6);
    assert!(has_quad_at(&snow.opaque, 1, 5 * 16 + 2));

    // A vine on the north and south: two planes, each from both sides, a
    // sixteenth in from the faces.
    let vine = mesh_of(&[(5, 5, 5, VINE)], &[]);
    assert_eq!(vine.quads(), 4);
    assert!(has_quad_at(&vine.opaque, 2, 5 * 16 + 1));
    assert!(has_quad_at(&vine.opaque, 2, 5 * 16 + 15));
}

#[test]
fn the_isotropic_rotation_is_the_same_every_run_and_varies_by_place() {
    let field: Vec<_> = (0..16).flat_map(|x| (0..16).map(move |z| (x, 0, z, SAND))).collect();
    let first = mesh_of(&field, &[]);
    let second = mesh_of(&field, &[]);
    assert_eq!(first.opaque, second.opaque);
    // The tops: one quad per block, and more than one orientation among them.
    let orientations: std::collections::HashSet<[u8; 2]> = quads(&first.opaque)
        .into_iter()
        .filter(|quad| quad[0].face() == Face::Up as u8)
        .map(|quad| quad[0].tex())
        .collect();
    assert!(orientations.len() > 1, "{orientations:?}");
    assert_eq!(isotropic_turns(10, 64, -3, 1), isotropic_turns(10, 64, -3, 1));
}

/// The smallest and largest of one coordinate over some vertices.
fn span(vertices: &[Vertex], axis: usize) -> (i16, i16) {
    let values = vertices.iter().map(|vertex| vertex.at()[axis]);
    (values.clone().min().unwrap(), values.max().unwrap())
}

#[test]
fn a_bamboo_stalk_is_a_thin_box_in_the_stem_strips() {
    let mesh = mesh_of(&[(5, 5, 5, BAMBOO)], &[]);
    assert_eq!(mesh.quads(), 6, "four sides and two ends, and no leaves");
    for axis in [0, 2] {
        let (min, max) = span(&mesh.opaque, axis);
        assert_eq!(max - min, 2, "two sixteenths wide");
        assert!(min >= 5 * 16 && max <= 6 * 16, "inside its block");
    }
    for quad in quads(&mesh.opaque) {
        assert_eq!(quad[0].layer, STEM);
        let (u_min, u_max) = (quad.iter().map(|v| v.tex()[0]).min().unwrap(), quad.iter().map(|v| v.tex()[0]).max().unwrap());
        let (v_min, v_max) = (quad.iter().map(|v| v.tex()[1]).min().unwrap(), quad.iter().map(|v| v.tex()[1]).max().unwrap());
        assert_eq!(u_max - u_min, 2);
        match quad[0].face() {
            1 => assert_eq!((u_min, v_min, v_max), (13, 0, 2), "the top's cut end"),
            0 => assert_eq!((u_min, v_min, v_max), (13, 4, 6), "the bottom's cut end"),
            _ => {
                assert!([0, 3, 6, 9].contains(&u_min), "a strip, not {u_min}");
                assert_eq!((v_min, v_max), (0, 16), "the whole strip's height");
            }
        }
    }
}

#[test]
fn bamboo_leaves_are_a_cross_in_their_own_texture() {
    let thick = mesh_of(&[(5, 5, 5, BAMBOO_THICK_LARGE)], &[]);
    assert_eq!(thick.quads(), 6 + 4);
    let stalk: Vec<Vertex> = thick.opaque.iter().copied().filter(|v| v.layer == STEM).collect();
    let (min, max) = span(&stalk, 0);
    assert_eq!(max - min, 3, "a thick stalk is three sixteenths wide");
    let leaves: Vec<&Vertex> = thick.opaque.iter().filter(|v| v.layer == LARGE_LEAVES).collect();
    assert_eq!(leaves.len(), 4 * 4);
    assert!(leaves.iter().all(|v| v.face() == DIAGONAL && v.class() == 1), "cut-out crossed planes");

    let small = mesh_of(&[(5, 5, 5, BAMBOO_SMALL)], &[]);
    assert_eq!(small.opaque.iter().filter(|v| v.layer == SMALL_LEAVES).count(), 4 * 4);
}

#[test]
fn a_bamboo_column_stands_straight_and_hides_its_inner_ends() {
    let mesh = mesh_of(
        &[(5, 4, 5, STONE), (5, 5, 5, BAMBOO), (5, 6, 5, BAMBOO), (5, 7, 5, BAMBOO_THICK_LARGE)],
        &[],
    );
    // The stone keeps all six faces: a stalk hides nothing. The lowest stalk
    // loses its bottom to the stone and its top to the stalk above; the next
    // keeps its top, under a wider stalk. The thick one keeps both ends, and
    // has leaves.
    assert_eq!(mesh.quads(), 6 + 4 + 5 + 6 + 4);
    // Both thin segments stand at the same x and z: the offset ignores y.
    let segment = |y: i16| -> Vec<Vertex> {
        quads(&mesh.opaque)
            .into_iter()
            .filter(|quad| {
                quad[0].layer == STEM
                    && quad[0].face() >= 2
                    && quad.iter().all(|v| (y * 16..=(y + 1) * 16).contains(&v.at()[1]))
            })
            .flatten()
            .copied()
            .collect()
    };
    for axis in [0, 2] {
        assert_eq!(span(&segment(5), axis), span(&segment(6), axis));
    }
}

#[test]
fn bamboo_offsets_stay_in_the_block_and_vary() {
    assert_eq!(xz_offset(0), -4);
    assert_eq!(xz_offset(15), 4);
    assert_eq!((xz_offset(7), xz_offset(8)), (0, 0));
    assert!((0..16).all(|bits| (-4..=4).contains(&xz_offset(bits))));
    assert_eq!(xz_offset(-1), xz_offset(15), "a negative seed's low bits, as Java's `&` reads them");

    // A field of stalks: each stays in its block, and they don't all stand
    // in the same place.
    let field: Vec<_> = (0..16).flat_map(|x| (0..16).map(move |z| (x, 0, z, BAMBOO))).collect();
    let mesh = mesh_of(&field, &[]);
    let mut places = std::collections::HashSet::new();
    for quad in quads(&mesh.opaque).into_iter().filter(|quad| quad[0].face() == Face::Up as u8) {
        let (x0, x1) = span(quad, 0);
        let (z0, z1) = span(quad, 2);
        assert_eq!((x0 / 16, z0 / 16), ((x1 - 1) / 16, (z1 - 1) / 16), "inside one block");
        places.insert((x0 % 16, z0 % 16));
    }
    assert!(places.len() > 4, "{places:?}");
}

/// Stone positions in a chunk: x and z within it, and y.
type Stones<'a> = &'a [(usize, i32, usize)];

/// A world with the given chunks, each with stone at the listed positions.
fn world_with(chunks: &[((i32, i32), Stones<'_>)]) -> World {
    let mut world = World::new();
    for &((cx, cz), stones) in chunks {
        let chunk = ChunkData::empty(cx, cz);
        for &(x, y, z) in stones {
            chunk.section.set_block_absolute_y(x, y, z, JavaBlock::STONE.default_state.id);
        }
        let (payload, _) = CLevelChunk::encode_chunk(&chunk, 0, false, &[]).unwrap();
        world
            .apply(&RawPacket {
                id: CLevelChunk::PACKET_ID,
                payload: Bytes::from(payload),
            })
            .unwrap();
    }
    world
}

#[test]
fn borders_read_the_neighbour_and_a_missing_one_stops_meshing() {
    let stone = BlockState::to_be_network_id(JavaBlock::STONE.default_state.id);
    let table = AppearanceTable::from_looks([
        (AIR, look(Shape::Invisible, RenderClass::Opaque, Tint::None, 0)),
        (stone, look(Shape::Cube, RenderClass::Opaque, Tint::None, 1)),
    ]);
    // Stone at the east edge of chunk (0, 0), and beside it in chunk (1, 0).
    let world = world_with(&[
        ((0, 0), &[(15, 70, 5)]),
        ((1, 0), &[(0, 70, 5)]),
        ((0, -1), &[]),
        ((0, 1), &[]),
        ((-1, 0), &[]),
    ]);
    // y 70 is in section 70 / 16 = 4, the 9th from the bottom (-4).
    let n = Neighbourhood::gather(&world, &table, 0, 8, 0).unwrap();
    assert_eq!(n.origin, [0, 64, 0]);
    let mesh = mesh(&n, &tints());
    assert_eq!(mesh.quads(), 5, "the east face is hidden by the neighbour's stone");

    let incomplete = world_with(&[((0, 0), &[]), ((1, 0), &[]), ((0, -1), &[]), ((-1, 0), &[])]);
    assert!(Neighbourhood::gather(&incomplete, &table, 0, 8, 0).is_none());
}

/// A table of one extra look, on the usual air and stone.
fn with_look(id: u32, extra: Appearance) -> AppearanceTable {
    AppearanceTable::from_looks([
        (AIR, look(Shape::Invisible, RenderClass::Opaque, Tint::None, 0)),
        (STONE, look(Shape::Cube, RenderClass::Opaque, Tint::None, 1)),
        (id, extra),
    ])
}

fn mesh_in(table: &AppearanceTable, blocks: &[(i32, i32, i32, u32)]) -> Mesh {
    let n = Neighbourhood::from_fn(table, [0, 0, 0], 1, |x, y, z, layer| {
        if layer == 0 {
            blocks
                .iter()
                .find(|&&(bx, by, bz, _)| (bx, by, bz) == (x, y, z))
                .map_or(AIR, |&(.., id)| id)
        } else {
            AIR
        }
    });
    mesh(&n, &tints())
}

#[test]
fn a_model_copies_its_quads_and_a_neighbour_hides_those_on_the_edge() {
    use crate::blocks::models::{Part, Turn, bake};
    // A plate on the floor, reaching a sixteenth below its block, as a crop
    // does, with a texture of its own in South.
    let mut quads = Vec::new();
    bake(
        &[
            Part::solid([0, 0, 0], [16, 2, 16], Face::North),
            Part::open([4, -1, 8], [12, 15, 8]).rect(Face::North, Face::South, [0, 0, 16, 16]),
        ],
        Turn::NONE,
        &mut quads,
    );
    let mut model = look(Shape::Model { quads: quads.leak() }, RenderClass::Cutout, Tint::None, 7);
    model.faces[Face::South as usize].layer = 8;
    let table = with_look(20, model);

    let alone = mesh_in(&table, &[(5, 0, 5, 20)]);
    assert_eq!(alone.quads(), 7);
    assert_eq!(layer_quads(&alone.opaque, 8).len(), 1, "the plane shows its own slot");
    let lowest = alone.opaque.iter().map(|vertex| vertex.at()[1]).min();
    assert_eq!(lowest, Some(-1), "a model can reach below its sub-chunk");
    assert_eq!(alone.opaque[0].at()[0], 5 * 16);

    // Stone below and beside: the bottom and the west side go, the rest stays.
    let boxed = mesh_in(&table, &[(5, 5, 5, 20), (5, 4, 5, STONE), (4, 5, 5, STONE)]);
    assert_eq!(layer_quads(&boxed.opaque, 7).len(), 4);
}

#[test]
fn a_stalk_shows_its_tip_where_the_plant_ends() {
    let (body, tip) = (30, 31);
    let mut kelp = look(Shape::Stalk { grows_up: true }, RenderClass::Cutout, Tint::None, body);
    kelp.faces[Face::Up as usize].layer = tip;
    let mut vine = look(Shape::Stalk { grows_up: false }, RenderClass::Cutout, Tint::None, body);
    vine.faces[Face::Up as usize].layer = tip;

    // Two kelp, one on the other: only the upper one has the tip.
    let table = with_look(20, kelp);
    let stack = mesh_in(&table, &[(5, 5, 5, 20), (5, 6, 5, 20)]);
    assert_eq!(layer_quads(&stack.opaque, body).len(), 4);
    let tips = layer_quads(&stack.opaque, tip);
    assert_eq!(tips.len(), 4);
    assert!(tips.iter().all(|quad| quad.iter().all(|vertex| vertex.at()[1] >= 6 * 16)));

    // A vine hanging: its tip is at the bottom.
    let table = with_look(20, vine);
    let stack = mesh_in(&table, &[(5, 5, 5, 20), (5, 6, 5, 20)]);
    let tips = layer_quads(&stack.opaque, tip);
    assert_eq!(tips.len(), 4);
    assert!(tips.iter().all(|quad| quad.iter().all(|vertex| vertex.at()[1] <= 6 * 16)));
}

#[test]
fn grass_under_snow_shows_a_snowy_side() {
    let snowy = FaceLook {
        layer: 40,
        class: RenderClass::Opaque,
        tinted: false,
        turns: 0,
        isotropic: false,
    };
    let mut grass = look(Shape::Cube, RenderClass::Opaque, Tint::None, 41);
    grass.under_snow = Some(snowy);
    let mut snow = look(Shape::Slab { bottom: 0, top: 2 }, RenderClass::Opaque, Tint::None, 42);
    snow.snow = true;
    let table = AppearanceTable::from_looks([
        (AIR, look(Shape::Invisible, RenderClass::Opaque, Tint::None, 0)),
        (20, grass),
        (21, snow),
    ]);

    let bare = mesh_in(&table, &[(5, 5, 5, 20)]);
    assert!(layer_quads(&bare.opaque, 40).is_empty());
    assert_eq!(layer_quads(&bare.opaque, 41).len(), 6);

    let covered = mesh_in(&table, &[(5, 5, 5, 20), (5, 6, 5, 21)]);
    assert_eq!(layer_quads(&covered.opaque, 40).len(), 4, "the four sides");
    assert_eq!(layer_quads(&covered.opaque, 41).len(), 2, "the top and bottom");
}

/// The quad of `face` on the block at (`x`, `y`, `z`).
fn face_of(mesh: &Mesh, face: Face, x: i16, y: i16, z: i16) -> Vec<Vertex> {
    let (low, high) = ([x, y, z].map(|v| v * 16), [x, y, z].map(|v| v * 16 + 16));
    let (axis, sign, _) = face_axes(face);
    quads(&mesh.opaque)
        .into_iter()
        .chain(quads(&mesh.blend))
        .find(|quad| {
            // Not the block beside's face on the far side of this one's boundary.
            let plane = quad[0].at()[axis];
            let own = if sign > 0 { plane > low[axis] } else { plane < high[axis] };
            quad[0].face() == face as u8
                && own
                && quad.iter().all(|vertex| (0..3).all(|axis| (low[axis]..=high[axis]).contains(&vertex.at()[axis])))
        })
        .expect("the face")
        .to_vec()
}

/// The corner of a quad at these sixteenths.
fn corner_at(quad: &[Vertex], at: [i16; 3]) -> Vertex {
    *quad.iter().find(|vertex| vertex.at() == at).expect("a corner there")
}

/// A stone floor at y 4, the whole sub-chunk wide.
fn floor_and(extra: &[(i32, i32, i32, u32)]) -> Vec<(i32, i32, i32, u32)> {
    let mut blocks: Vec<_> = (0..16).flat_map(|x| (0..16).map(move |z| (x, 4, z, STONE))).collect();
    blocks.extend_from_slice(extra);
    blocks
}

/// A neighbourhood of these blocks, lit as daylight until a test says otherwise.
fn lit(blocks: &[(i32, i32, i32, u32)], water: &[(i32, i32, i32)]) -> Neighbourhood {
    Neighbourhood::from_fn(&table(), [0, 0, 0], 1, |x, y, z, layer| match layer {
        0 => blocks.iter().find(|&&(bx, by, bz, _)| (bx, by, bz) == (x, y, z)).map_or(AIR, |&(.., id)| id),
        _ if water.contains(&(x, y, z)) => WATER,
        _ => AIR,
    })
}

#[test]
fn shade_in_the_open_along_an_edge_and_in_a_corner() {
    let up = |extra: &[(i32, i32, i32, u32)]| face_of(&mesh_of(&floor_and(extra), &[]), Face::Up, 5, 4, 5);
    // In the open, nothing shades the floor's top.
    assert!(up(&[]).iter().all(|vertex| vertex.occlusion() == 0));
    // A block beside the block the face looks into, to the east: one at the east corners.
    let edge = up(&[(6, 5, 5, STONE)]);
    for (corner, shade) in [([96, 80, 80], 1), ([96, 80, 96], 1), ([80, 80, 80], 0), ([80, 80, 96], 0)] {
        assert_eq!(corner_at(&edge, corner).occlusion(), shade, "{corner:?}");
    }
    // East and south: the corner between them is as dark as it gets.
    let corner = up(&[(6, 5, 5, STONE), (5, 5, 6, STONE)]);
    for (at, shade) in [([96, 80, 96], 3), ([96, 80, 80], 1), ([80, 80, 96], 1), ([80, 80, 80], 0)] {
        assert_eq!(corner_at(&corner, at).occlusion(), shade, "{at:?}");
    }
    // Only across a corner: just that corner.
    let across = up(&[(6, 5, 6, STONE)]);
    assert_eq!(corner_at(&across, [96, 80, 96]).occlusion(), 1);
    assert_eq!(corner_at(&across, [96, 80, 80]).occlusion(), 0);
}

#[test]
fn smooth_light_averages_the_four_blocks_around_a_corner() {
    let mut n = lit(&floor_and(&[]), &[]);
    // The block the face looks into, the ones beside it to the east and the south, and across.
    n.set_light(5, 5, 5, 15, 0);
    n.set_light(6, 5, 5, 11, 2);
    n.set_light(5, 5, 6, 7, 4);
    n.set_light(6, 5, 6, 3, 6);
    let quad = face_of(&mesh(&n, &tints()), Face::Up, 5, 4, 5);
    let east_south = corner_at(&quad, [96, 80, 96]);
    assert_eq!((east_south.sky(), east_south.block()), (9, 3), "(15 + 11 + 7 + 3) / 4, and (0 + 2 + 4 + 6) / 4");
    assert_eq!(corner_at(&quad, [80, 80, 80]).sky(), 15, "the west and north are open sky");

    // A block with no light at all, as stone has, counts as the middle one.
    n.set_light(6, 5, 5, 0, 0);
    let quad = face_of(&mesh(&n, &tints()), Face::Up, 5, 4, 5);
    assert_eq!(corner_at(&quad, [96, 80, 96]).sky(), 10, "(15 + 15 + 7 + 3) / 4");
}

#[test]
fn a_face_inside_its_block_looks_into_its_own() {
    let dimmer_around = |n: &mut Neighbourhood| {
        for x in 4..=6 {
            for z in 4..=6 {
                n.set_light(x, 5, z, 9, 0);
            }
        }
    };
    // A slab's top, halfway up its block, takes its own block's light.
    let mut slab = lit(&[(5, 5, 5, SLAB)], &[]);
    dimmer_around(&mut slab);
    assert!(face_of(&mesh(&slab, &tints()), Face::Up, 5, 5, 5).iter().all(|vertex| vertex.sky() == 9));
    // A cube's top looks into the block above, open to the sky.
    let mut cube = lit(&[(5, 5, 5, STONE)], &[]);
    dimmer_around(&mut cube);
    assert!(face_of(&mesh(&cube, &tints()), Face::Up, 5, 5, 5).iter().all(|vertex| vertex.sky() == 15));
}

#[test]
fn flat_lighting_gives_a_face_the_light_it_looks_into_and_no_shade() {
    let mut n = lit(&floor_and(&[(6, 5, 5, STONE), (5, 5, 6, STONE)]), &[]);
    n.set_light(5, 5, 5, 12, 3);
    n.set_light(6, 5, 6, 2, 0);
    n.smooth = false;
    let quad = face_of(&mesh(&n, &tints()), Face::Up, 5, 4, 5);
    assert!(quad.iter().all(|vertex| (vertex.sky(), vertex.block(), vertex.occlusion()) == (12, 3, 0)));
}

#[test]
fn a_liquid_takes_the_brighter_of_its_own_light_and_the_one_it_faces() {
    let mut n = lit(&[(5, 5, 5, WATER)], &[]);
    n.set_light(5, 5, 5, 14, 3);
    n.set_light(5, 6, 5, 12, 7);
    let top = face_of(&mesh(&n, &tints()), Face::Up, 5, 5, 5);
    assert!(top.iter().all(|vertex| (vertex.sky(), vertex.block(), vertex.occlusion()) == (14, 7, 0)));
}

#[test]
fn light_and_shade_fit_in_the_vertex_without_making_it_bigger() {
    // The vertex M4 measured.
    assert_eq!(size_of::<Vertex>(), 16);
    let mut n = lit(&floor_and(&[(6, 5, 5, STONE), (5, 5, 6, STONE), (5, 5, 5, GLASS)]), &[]);
    for (x, z) in [(5, 5), (6, 6), (6, 5), (5, 6)] {
        n.set_light(x, 6, z, 10, 5);
    }
    let quad = face_of(&mesh(&n, &tints()), Face::Up, 5, 5, 5);
    let vertex = corner_at(&quad, [96, 96, 96]);
    assert_eq!(
        (vertex.face(), vertex.class(), vertex.occlusion(), vertex.sky(), vertex.block()),
        (Face::Up as u8, 1, 0, 10, 5),
        "glass's top: up, cut out, open above, and lit"
    );
    let floor = corner_at(&face_of(&mesh(&n, &tints()), Face::Up, 4, 4, 5), [80, 80, 96]);
    assert_eq!((floor.class(), floor.occlusion()), (0, 1), "the floor beside it, opaque, with a stone at its corner");
}

#[test]
fn a_gathered_neighbourhood_has_the_worlds_light() {
    let stone = BlockState::to_be_network_id(JavaBlock::STONE.default_state.id);
    let table = AppearanceTable::from_looks([
        (AIR, look(Shape::Invisible, RenderClass::Opaque, Tint::None, 0)),
        (stone, look(Shape::Cube, RenderClass::Opaque, Tint::None, 1)),
    ]);
    let around: Vec<_> = (-1..=1).flat_map(|x| (-1..=1).map(move |z| (x, z))).collect();
    let chunks: Vec<_> = around.iter().map(|&at| (at, if at == (0, 0) { &[(5, 70, 5)][..] } else { &[][..] })).collect();
    let mut world = world_with(&chunks);
    let top = |world: &World| {
        let n = Neighbourhood::gather(world, &table, 0, 8, 0).unwrap();
        face_of(&mesh(&n, &tints()), Face::Up, 5, 6, 5)
    };
    assert!(top(&world).iter().all(|vertex| vertex.sky() == 0), "dark until the chunks are lit");
    world.update_light(std::time::Duration::MAX, (0, 0));
    assert!(top(&world).iter().all(|vertex| vertex.sky() == 15), "open sky above the stone");
}
