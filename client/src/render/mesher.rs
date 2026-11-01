//! CPU meshing of one sub-chunk (m4-plan Decision 6): the visible faces of
//! its blocks, as quads ready for the GPU.
//!
//! **A padded neighbourhood** (worth teaching). Whether a face is drawn
//! depends on the block beside it, and for blocks on the sub-chunk's edge
//! that block is in another sub-chunk. So [`Neighbourhood`] holds the 16³
//! blocks plus a one-block border copied from the six neighbours: 18³ cells.
//! The mesher then reads `x ± 1` for any block without a bounds check or a
//! lookup in another chunk. The border's edges and corners stay air: only
//! the six face neighbours decide anything.
//!
//! **Translate the palette, not each block** (worth teaching). A sub-chunk
//! layer stores a short palette of block ids and a small index per
//! position. Looking up each palette entry's appearance once, then copying
//! indices, does 4,096 cheap copies instead of 4,096 hash lookups.
//!
//! The mesh keeps culled faces and skips greedy merging (M6 does better on
//! the GPU). Its texture orientation is what `blocks::appearance` assumes:
//! side faces put `v` = 0 at the top, the up face runs `u` along +x and `v`
//! along +z, and the down face runs `v` along −z.

use std::collections::HashMap;

use limonite_packs::registry::Face;

use crate::{
    blocks::{
        appearance::{Appearance, AppearanceTable, BambooLeaves, FaceLook, Liquid, RenderClass, Shape},
        models::{POSITION_SCALE, UV_SCALE, box_face, face_uv, quarter_turn},
        shapes,
    },
    render::tint::{Tint, Tints},
    world::{AIR, World},
};

/// The neighbourhood's width: 16 blocks and a border block on each side.
pub const PAD: usize = 18;
/// A whole block, in the vertex's units.
const WHOLE: i16 = 16 * POSITION_SCALE;
/// A whole texture, in the vertex's units.
const UV_WHOLE: u8 = 16 * UV_SCALE;
const CELLS: usize = PAD * PAD * PAD;

/// A cell's index from padded coordinates, each 0 to 17. x is innermost, so
/// a row of blocks is contiguous.
const fn cell(x: usize, y: usize, z: usize) -> usize {
    (y * PAD + z) * PAD + x
}

/// The step to the neighbouring cell in each face's direction.
const STEP: [isize; 6] = [
    -((PAD * PAD) as isize), // down: y - 1
    (PAD * PAD) as isize,    // up: y + 1
    -(PAD as isize),         // north: z - 1
    PAD as isize,            // south: z + 1
    -1,                      // west: x - 1
    1,                       // east: x + 1
];

/// The face code of the diagonal planes of a cross. Faces are 0 to 5, in
/// [`Face`] order.
pub const DIAGONAL: u8 = 6;

/// One corner of a quad: 16 bytes (m4-plan Decision 6).
///
/// `#[repr(C)]` fixes the field order and padding, so the GPU reads it with
/// the layout `terrain.rs` declares. `Pod` (plain old data) promises
/// `bytemuck` that any bytes are a valid `Vertex` and the struct has no
/// padding, so a `Vec<Vertex>` can be handed to the GPU as bytes, uncopied.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// The position in the sub-chunk, in 1,024ths of a block
    /// ([`POSITION_SCALE`]): 0 to 16,384, and a little either side, for a model
    /// that reaches out of its block. The GPU reads it with layer, as four
    /// signed numbers; there are never 32,768 textures.
    pub position: [i16; 3],
    /// The texture's layer in the array.
    pub layer: u16,
    /// Texture coordinates in 128ths of the texture ([`UV_SCALE`]): 0 to 128.
    pub uv: [u8; 2],
    /// The face (bits 0 to 2: [`Face`] order, or [`DIAGONAL`]) and the render
    /// class (bits 3 and 4: opaque, cutout, overlay, blend).
    pub face_class: u8,
    pub unused: u8,
    /// The tint, RGBA. White leaves the texture as it is.
    pub tint: [u8; 4],
}

impl Vertex {
    #[cfg(test)]
    #[must_use]
    pub const fn face(&self) -> u8 {
        self.face_class & 0b111
    }

    #[cfg(test)]
    #[must_use]
    pub const fn class(&self) -> u8 {
        self.face_class >> 3
    }

    /// The position in sixteenths of a block, for a corner on that grid.
    #[cfg(test)]
    #[must_use]
    pub fn at(&self) -> [i16; 3] {
        self.position.map(|value| value / POSITION_SCALE)
    }

    /// The texture coordinates in sixteenths, for a corner on that grid.
    #[cfg(test)]
    #[must_use]
    pub fn tex(&self) -> [u8; 2] {
        self.uv.map(|value| value / UV_SCALE)
    }
}

/// One sub-chunk's quads, four vertices each, in two lists for the two
/// passes: opaque, cutout and overlay faces write depth; blended faces are
/// drawn after, back to front.
#[derive(Debug, Default)]
pub struct Mesh {
    pub opaque: Vec<Vertex>,
    pub blend: Vec<Vertex>,
}

impl Mesh {
    #[must_use]
    pub fn quads(&self) -> usize {
        (self.opaque.len() + self.blend.len()) / 4
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.opaque.is_empty() && self.blend.is_empty()
    }
}

/// A sub-chunk and a one-block border from its neighbours, as appearances.
pub struct Neighbourhood {
    /// The sub-chunk's lowest corner, in world blocks.
    pub origin: [i32; 3],
    /// Each distinct appearance here. Index 0 is air.
    looks: Vec<Appearance>,
    /// Which id each look is, for interning.
    ids: HashMap<u32, u16>,
    /// For each of the two layers, every cell's index into `looks`.
    cells: [Box<[u16]>; 2],
    /// The biome of each column, at `x + 16 z`.
    biomes: [u32; 256],
}

impl Neighbourhood {
    /// An all-air neighbourhood.
    fn empty(origin: [i32; 3], table: &AppearanceTable) -> Self {
        Self {
            origin,
            looks: vec![*table.get(AIR)],
            ids: HashMap::from([(AIR, 0)]),
            cells: [vec![0; CELLS].into_boxed_slice(), vec![0; CELLS].into_boxed_slice()],
            // Plains, until a chunk says otherwise.
            biomes: [1; 256],
        }
    }

    /// The index of an id's look, adding it the first time.
    fn intern(&mut self, table: &AppearanceTable, id: u32) -> u16 {
        if let Some(&index) = self.ids.get(&id) {
            return index;
        }
        let index = self.looks.len() as u16;
        self.looks.push(*table.get(id));
        self.ids.insert(id, index);
        index
    }

    fn set(&mut self, table: &AppearanceTable, layer: usize, at: usize, id: u32) {
        self.cells[layer][at] = self.intern(table, id);
    }

    /// Sub-chunk `section` (counted from the chunk's lowest) of chunk
    /// (`chunk_x`, `chunk_z`), with its border. `None` if the chunk or one of
    /// its four side neighbours isn't loaded: a border would be missing, and
    /// faces against it would be guesses.
    #[must_use]
    pub fn gather(
        world: &World,
        table: &AppearanceTable,
        chunk_x: i32,
        section: usize,
        chunk_z: i32,
    ) -> Option<Self> {
        let chunk = world.chunk(chunk_x, chunk_z)?;
        let north = world.chunk(chunk_x, chunk_z - 1)?;
        let south = world.chunk(chunk_x, chunk_z + 1)?;
        let west = world.chunk(chunk_x - 1, chunk_z)?;
        let east = world.chunk(chunk_x + 1, chunk_z)?;
        let sub_chunk = chunk.sub_chunks.get(section)?;
        let base_y = (chunk.min_section + section as i32) * 16;
        let mut n = Self::empty([chunk_x * 16, base_y, chunk_z * 16], table);

        for (layer, storage) in sub_chunk.layers.iter().enumerate().take(2) {
            let palette: Vec<u16> = storage
                .palette()
                .iter()
                .map(|&id| n.intern(table, id))
                .collect();
            let cells = &mut n.cells[layer];
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        cells[cell(x + 1, y + 1, z + 1)] = palette[storage.palette_index(x, y, z)];
                    }
                }
            }
        }

        for layer in 0..2 {
            for a in 0..16 {
                for b in 0..16 {
                    let y = base_y + b as i32;
                    // Below and above, from the same chunk (a = x, b = z).
                    n.set(table, layer, cell(a + 1, 0, b + 1), chunk.block(a, base_y - 1, b, layer));
                    n.set(table, layer, cell(a + 1, 17, b + 1), chunk.block(a, base_y + 16, b, layer));
                    // North and south (a = x, b = y).
                    n.set(table, layer, cell(a + 1, b + 1, 0), north.block(a, y, 15, layer));
                    n.set(table, layer, cell(a + 1, b + 1, 17), south.block(a, y, 0, layer));
                    // West and east (a = z, b = y).
                    n.set(table, layer, cell(0, b + 1, a + 1), west.block(15, y, a, layer));
                    n.set(table, layer, cell(17, b + 1, a + 1), east.block(0, y, a, layer));
                }
            }
        }

        // A liquid's corners take in the diagonals, so it's only there that the
        // border's edges are worth filling in.
        if n.looks.iter().any(|look| look.liquid().is_some()) {
            n.fill_edges(world, table, chunk_x, base_y, chunk_z);
        }

        // One biome per column: the sub-chunk's middle.
        for z in 0..16 {
            for x in 0..16 {
                if let Some(biome) = chunk.biome(x, base_y + 8, z) {
                    n.biomes[x + 16 * z] = biome;
                }
            }
        }
        Some(n)
    }

    /// The border's edges and corners. A chunk that isn't loaded leaves them air, and
    /// a corner of liquid beside it is a little off until the sub-chunk's next meshed.
    fn fill_edges(&mut self, world: &World, table: &AppearanceTable, chunk_x: i32, base_y: i32, chunk_z: i32) {
        let on_border = |value: usize| value == 0 || value == PAD - 1;
        for y in 0..PAD {
            for z in 0..PAD {
                for x in 0..PAD {
                    if usize::from(on_border(x)) + usize::from(on_border(y)) + usize::from(on_border(z)) < 2 {
                        continue;
                    }
                    let (world_x, world_z) = (chunk_x * 16 + x as i32 - 1, chunk_z * 16 + z as i32 - 1);
                    let Some(chunk) = world.chunk(world_x.div_euclid(16), world_z.div_euclid(16)) else {
                        continue;
                    };
                    let (column_x, column_z) = (world_x.rem_euclid(16) as usize, world_z.rem_euclid(16) as usize);
                    for layer in 0..2 {
                        let id = chunk.block(column_x, base_y + y as i32 - 1, column_z, layer);
                        self.set(table, layer, cell(x, y, z), id);
                    }
                }
            }
        }
    }

    /// One block alone, in the middle of a sub-chunk at the origin: an icon's.
    #[must_use]
    pub fn single(table: &AppearanceTable, biome: u32, id: u32) -> Self {
        Self::single_at(table, biome, id, [8, 8, 8])
    }

    /// One block alone, in the middle of a sub-chunk placed to put it at `at`, for what
    /// depends on where a block is: a crack's.
    #[must_use]
    pub fn single_at(table: &AppearanceTable, biome: u32, id: u32, at: [i32; 3]) -> Self {
        let mut n = Self::empty(at.map(|axis| axis - 8), table);
        n.biomes = [biome; 256];
        // The cells count from the border, so block 8 is cell 9.
        n.set(table, 0, cell(9, 9, 9), id);
        n
    }

    /// A neighbourhood from a function of block coordinates relative to the
    /// sub-chunk (-1 to 16 on each axis) and the layer.
    #[cfg(test)]
    pub fn from_fn(
        table: &AppearanceTable,
        origin: [i32; 3],
        biome: u32,
        id_at: impl Fn(i32, i32, i32, usize) -> u32,
    ) -> Self {
        let mut n = Self::empty(origin, table);
        n.biomes = [biome; 256];
        for layer in 0..2 {
            for y in 0..PAD {
                for z in 0..PAD {
                    for x in 0..PAD {
                        let id = id_at(x as i32 - 1, y as i32 - 1, z as i32 - 1, layer);
                        n.set(table, layer, cell(x, y, z), id);
                    }
                }
            }
        }
        n
    }
}

/// Meshes a neighbourhood's sub-chunk.
#[must_use]
pub fn mesh(n: &Neighbourhood, tints: &Tints) -> Mesh {
    let mut mesh = Mesh::default();
    for y in 1..=16 {
        for z in 1..=16 {
            for x in 1..=16 {
                let at = cell(x, y, z);
                for layer in 0..2 {
                    let index = n.cells[layer][at];
                    if index == 0 {
                        continue;
                    }
                    let look = &n.looks[usize::from(index)];
                    let block = Block {
                        n,
                        at,
                        local: [x as u16 - 1, y as u16 - 1, z as u16 - 1],
                        look,
                        biome: n.biomes[(x - 1) + 16 * (z - 1)],
                    };
                    block.emit(tints, &mut mesh);
                }
            }
        }
    }
    mesh
}

/// One block being meshed.
struct Block<'a> {
    n: &'a Neighbourhood,
    /// Its cell.
    at: usize,
    /// Its position in the sub-chunk, in blocks.
    local: [u16; 3],
    look: &'a Appearance,
    biome: u32,
}

impl Block<'_> {
    fn emit(&self, tints: &Tints, mesh: &mut Mesh) {
        match self.look.shape {
            Shape::Invisible => {}
            Shape::Cube => {
                // Sides that go snowy keep that off the common path.
                let snowy = self.look.under_snow.filter(|_| self.neighbour(Face::Up, 0).snow);
                for face in Face::ALL {
                    if !self.neighbour_occludes(face) {
                        match snowy {
                            Some(look) if face.is_side() => {
                                self.quad_in(&look, face, [0, 0, 0], [16, 16, 16], tints, mesh);
                            }
                            _ => self.quad(face, [0, 0, 0], [16, 16, 16], tints, mesh),
                        }
                    }
                }
            }
            Shape::Slab { bottom, top } => {
                for face in Face::ALL {
                    // Only faces on the block's boundary can be hidden.
                    let on_boundary = match face {
                        Face::Down => bottom == 0,
                        Face::Up => top == 16,
                        _ => true,
                    };
                    if !(on_boundary && self.neighbour_occludes(face)) {
                        self.quad(face, [0, bottom as i8, 0], [16, top as i8, 16], tints, mesh);
                    }
                }
            }
            Shape::Liquid { liquid, amount } => self.liquid(liquid, amount, tints, mesh),
            Shape::Cross => self.cross(self.look.face(Face::North), tints, mesh),
            Shape::Stalk { grows_up } => {
                let beyond = self.neighbour(if grows_up { Face::Up } else { Face::Down }, 0);
                // More of the plant, or another of its names, which are crosses.
                let goes_on = matches!(beyond.shape, Shape::Stalk { .. } | Shape::Cross);
                self.cross(self.look.face(if goes_on { Face::North } else { Face::Up }), tints, mesh);
            }
            Shape::Planes { faces } => {
                for face in Face::ALL.into_iter().filter(|&face| faces & (1 << face as u8) != 0) {
                    self.plane(face, tints, mesh);
                }
            }
            Shape::Bamboo { thick, leaves } => self.bamboo(thick, leaves, tints, mesh),
            Shape::Boxes { state } => {
                let [x, y, z] = self.world_position();
                for (min, max) in shapes::outline(state, x, y, z) {
                    for face in Face::ALL {
                        // A face on the block's edge is hidden by an opaque
                        // neighbour, as a cube's is; one inside the block
                        // never is.
                        let on_edge = match face {
                            Face::Down => min[1] == 0,
                            Face::Up => max[1] == 16,
                            Face::North => min[2] == 0,
                            Face::South => max[2] == 16,
                            Face::West => min[0] == 0,
                            Face::East => max[0] == 16,
                        };
                        if !(on_edge && self.neighbour_occludes(face)) {
                            self.quad(face, min.map(|v| v as i8), max.map(|v| v as i8), tints, mesh);
                        }
                    }
                }
            }
            Shape::Model { quads } => {
                for quad in quads {
                    if !quad.cull.is_some_and(|face| self.neighbour_occludes(face)) {
                        let look = &self.look.faces[usize::from(quad.slot)];
                        self.push_fine(quad.corners, quad.uvs, quad.face, look, tints, mesh);
                    }
                }
            }
        }
    }

    /// A liquid (worth teaching). A cube's faces don't do: a flow has to run downhill,
    /// so each corner of its top stands as high as the columns around it, and the
    /// texture has to run the way it does. These are Java's rules, on its numbers.
    ///
    /// - **Corners** average the four columns that share them. A source counts ten
    ///   times over, so water doesn't dip away from one, and a neighbour with the same
    ///   liquid above it pulls the corner up to meet that.
    /// - **Textures** come from the look's slots: `up` and `down` hold the still
    ///   texture, and the sides the flowing one, which covers two blocks (`quad`), so a
    ///   block only shows half of it.
    /// - **A level top** shows the still texture, and a sloping one the flowing texture,
    ///   turned so it runs downhill. `blocks.json` marks lava's top `isotropic`, but
    ///   that's for cubes: here it would turn each block's texture its own way.
    fn liquid(&self, liquid: Liquid, amount: u8, tints: &Tints, mesh: &mut Mesh) {
        let shows = |face: Face| !self.neighbour_is(face, liquid) && !self.neighbour_occludes(face);
        let covered = self.neighbour_is(Face::Up, liquid);
        let sides = [Face::North, Face::South, Face::West, Face::East].map(|face| (face, shows(face)));
        let bottom = shows(Face::Down);
        // Most of a sea, so it's the first thing to be sure of.
        if covered && !bottom && !sides.iter().any(|&(_, shows)| shows) {
            return;
        }
        let own = f32::from(amount) / 9.0;
        let height = |x: isize, z: isize| {
            if covered {
                WHOLE
            } else {
                (self.liquid_corner(liquid, own, x, z) * f32::from(WHOLE)).round() as i16
            }
        };
        // By which end of x and of z each corner's at.
        let heights = [[height(-1, -1), height(-1, 1)], [height(1, -1), height(1, 1)]];

        if !covered && !(self.neighbour_occludes(Face::Up) && heights.iter().flatten().all(|&h| h == WHOLE)) {
            let corners = box_face(Face::Up, [0; 3], [1; 3]).map(|[x, _, z]| {
                [i16::from(x) * WHOLE, heights[x as usize][z as usize], i16::from(z) * WHOLE]
            });
            let (look, uvs) = match self.liquid_flow(liquid, own) {
                Some(flow) => (self.look.face(Face::North), corners.map(|corner| flowing_uv(corner, flow))),
                None => (self.look.face(Face::Up), [[0, 0], [0, UV_WHOLE], [UV_WHOLE, UV_WHOLE], [UV_WHOLE, 0]]),
            };
            self.push_fine(corners, uvs, Face::Up as u8, look, tints, mesh);
        }
        for (face, shows) in sides {
            if shows {
                self.liquid_side(face, &heights, tints, mesh);
            }
        }
        if bottom {
            let corners = box_face(Face::Down, [0; 3], [1; 3]).map(|corner| corner.map(|value| i16::from(value) * WHOLE));
            let uvs = [[0, 0], [0, UV_WHOLE], [UV_WHOLE, UV_WHOLE], [UV_WHOLE, 0]];
            self.push_fine(corners, uvs, Face::Down as u8, self.look.face(Face::Down), tints, mesh);
        }
    }

    /// One side of a liquid, up to its two corners, in the left half of the flowing
    /// texture: its top edge is lower down the texture by as much as the liquid's low.
    fn liquid_side(&self, face: Face, heights: &[[i16; 2]; 2], tints: &Tints, mesh: &mut Mesh) {
        let half = UV_WHOLE / 2;
        let unit = box_face(face, [0; 3], [1; 3]);
        let corners = unit.map(|[x, y, z]| {
            let top = heights[x as usize][z as usize];
            [i16::from(x) * WHOLE, if y == 1 { top } else { 0 }, i16::from(z) * WHOLE]
        });
        // Box faces start at the texture's top left, and go down, across and up.
        let uvs = [0, 1, 2, 3].map(|index: usize| {
            let u = if index < 2 { 0 } else { half };
            let v = if corners[index][1] == 0 && unit[index][1] == 0 {
                half
            } else {
                ((i32::from(WHOLE) - i32::from(corners[index][1])) * i32::from(half) / i32::from(WHOLE)) as u8
            };
            [u, v]
        });
        self.push_fine(corners, uvs, face as u8, self.look.face(face), tints, mesh);
    }

    /// The cell this far from the block.
    fn cell_at(&self, x: isize, y: isize, z: isize) -> usize {
        self.at
            .wrapping_add_signed(x * STEP[Face::East as usize] + y * STEP[Face::Up as usize] + z * STEP[Face::South as usize])
    }

    /// How high this liquid stands in a cell, on either layer, in ninths.
    fn liquid_in(&self, cell: usize, liquid: Liquid) -> Option<u8> {
        (0..2).find_map(|layer| match self.n.looks[usize::from(self.n.cells[layer][cell])].shape {
            Shape::Liquid { liquid: other, amount } if other == liquid => Some(amount),
            _ => None,
        })
    }

    /// How high this liquid stands in the column this far away, on this block's level:
    /// the whole block with more of it above, or `None` if the block's solid.
    fn liquid_column(&self, liquid: Liquid, x: isize, z: isize) -> Option<f32> {
        if self.liquid_in(self.cell_at(x, 1, z), liquid).is_some() {
            return Some(1.0);
        }
        let cell = self.cell_at(x, 0, z);
        match self.liquid_in(cell, liquid) {
            Some(amount) => Some(f32::from(amount) / 9.0),
            None if self.n.looks[usize::from(self.n.cells[0][cell])].occludes => None,
            None => Some(0.0),
        }
    }

    /// The height of the corner towards `x` and `z`, each 1 or -1, as a fraction of
    /// the block, from the four columns that share it.
    fn liquid_corner(&self, liquid: Liquid, own: f32, x: isize, z: isize) -> f32 {
        let (along_x, along_z) = (self.liquid_column(liquid, x, 0), self.liquid_column(liquid, 0, z));
        if along_x == Some(1.0) || along_z == Some(1.0) {
            return 1.0;
        }
        let mut columns = vec![Some(own), along_x, along_z];
        // Around a solid block's corner, the diagonal isn't connected to this one.
        if along_x.is_some_and(|height| height > 0.0) || along_z.is_some_and(|height| height > 0.0) {
            let across = self.liquid_column(liquid, x, z);
            if across == Some(1.0) {
                return 1.0;
            }
            columns.push(across);
        }
        let (mut total, mut weight) = (0.0, 0.0);
        for height in columns.into_iter().flatten() {
            let count = if height >= 0.8 { 10.0 } else { 1.0 };
            total += height * count;
            weight += count;
        }
        total / weight
    }

    /// Which way this liquid runs, as a unit vector along x and z: downhill, towards
    /// lower neighbours, and towards an edge it can pour over. `None` if it's level.
    fn liquid_flow(&self, liquid: Liquid, own: f32) -> Option<[f32; 2]> {
        let mut flow = [0.0f32; 2];
        for (x, z) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let cell = self.cell_at(x, 0, z);
            let drop = match self.liquid_in(cell, liquid) {
                Some(amount) => own - f32::from(amount) / 9.0,
                None if self.n.looks[usize::from(self.n.cells[0][cell])].occludes => 0.0,
                None => match self.liquid_in(self.cell_at(x, -1, z), liquid) {
                    Some(below) => own - f32::from(below) / 9.0 + 8.0 / 9.0,
                    None => 0.0,
                },
            };
            flow[0] += x as f32 * drop;
            flow[1] += z as f32 * drop;
        }
        let length = flow[0].hypot(flow[1]);
        (length > 1e-4).then(|| [flow[0] / length, flow[1] / length])
    }

    /// The look of the block beside this one, on `layer`.
    fn neighbour(&self, face: Face, layer: usize) -> &Appearance {
        let at = self.at.wrapping_add_signed(STEP[face as usize]);
        &self.n.looks[usize::from(self.n.cells[layer][at])]
    }

    fn neighbour_occludes(&self, face: Face) -> bool {
        self.neighbour(face, 0).occludes
    }

    /// Whether the block beside holds `liquid` on either layer: water around
    /// a waterlogged block counts.
    fn neighbour_is(&self, face: Face, liquid: Liquid) -> bool {
        self.neighbour(face, 0).liquid() == Some(liquid) || self.neighbour(face, 1).liquid() == Some(liquid)
    }

    /// The colour a face of this block is multiplied by.
    fn tint(&self, look: &FaceLook, tints: &Tints) -> [u8; 4] {
        if look.tinted && self.look.tint != Tint::None {
            tints.color(self.biome, self.look.tint)
        } else {
            [255; 4]
        }
    }

    /// One face of a box from `min` to `max`, in sixteenths of the block.
    fn quad(&self, face: Face, min: [i8; 3], max: [i8; 3], tints: &Tints, mesh: &mut Mesh) {
        self.quad_in(self.look.face(face), face, min, max, tints, mesh);
    }

    /// One face of a box, showing `look`.
    fn quad_in(&self, look: &FaceLook, face: Face, min: [i8; 3], max: [i8; 3], tints: &Tints, mesh: &mut Mesh) {
        let corners = box_face(face, min, max);
        let mut uvs = corners.map(|corner| face_uv(face, corner));
        let mut turns = look.turns;
        if look.isotropic {
            let [x, y, z] = self.world_position();
            turns += isotropic_turns(x, y, z, face as u8);
        }
        for _ in 0..turns % 4 {
            uvs = uvs.map(quarter_turn);
        }
        self.push(corners, uvs, face as u8, look, tints, mesh);
    }

    /// Two planes crossed on the diagonals, each drawn from both sides, in
    /// `look`. They run from 1/16 to 15/16, as Java's rescaled `cross` model
    /// does on a sixteenth grid.
    fn cross(&self, look: &FaceLook, tints: &Tints, mesh: &mut Mesh) {
        let uvs = [[0, 0], [0, 16], [16, 16], [16, 0]];
        for (start, end) in [([1, 1], [15, 15]), ([1, 15], [15, 1])] {
            let front = [
                [start[0], 16, start[1]],
                [start[0], 0, start[1]],
                [end[0], 0, end[1]],
                [end[0], 16, end[1]],
            ];
            // The back: the same corners and texture, in the other order.
            let back = [front[3], front[2], front[1], front[0]];
            let back_uvs = [uvs[3], uvs[2], uvs[1], uvs[0]];
            self.push(front, uvs, DIAGONAL, look, tints, mesh);
            self.push(back, back_uvs, DIAGONAL, look, tints, mesh);
        }
    }

    /// A bamboo stalk, as Java's model draws it on a sixteenth grid.
    ///
    /// - **The stalk** is a box 2 sixteenths wide, or 3 when thick. Its sides
    ///   show one of the stem texture's four strips (at u = 0, 3, 6 and 9),
    ///   picked by position so the rings don't line up along a column; its
    ///   ends show the cut faces at u = 13. The ends are hidden by a full
    ///   cube, or by a stalk of the same width.
    /// - **The offset** is Java's: up to a quarter block in x and z, from a
    ///   seed without y, so every block of a column is nudged alike and the
    ///   stalk stays straight.
    /// - **The leaves** are crossed planes, centred like every M4 cross. Java
    ///   moves them with the stalk, but that would put corners outside the
    ///   block, which the vertex positions can't hold.
    fn bamboo(&self, thick: bool, leaves: BambooLeaves, tints: &Tints, mesh: &mut Mesh) {
        let stem = self.look.face(Face::North);
        let width: u8 = if thick { 3 } else { 2 };
        let [x, y, z] = self.world_position();
        let column = java_seed(x, 0, z);
        // At most 4 sixteenths either way, so the stalk stays in its block.
        let start = |offset: i8| (8 - width / 2).wrapping_add_signed(offset);
        let (x0, z0) = (start(xz_offset(column)), start(xz_offset(column >> 8)));
        let (min, max) = ([x0 as i8, 0, z0 as i8], [(x0 + width) as i8, 16, (z0 + width) as i8]);
        let strip = 3 * (java_seed(x, y, z) & 3) as u8;
        for face in Face::ALL {
            let hidden = matches!(face, Face::Up | Face::Down)
                && (self.neighbour_occludes(face)
                    || matches!(self.neighbour(face, 0).shape, Shape::Bamboo { thick: other, .. } if other == thick));
            if hidden {
                continue;
            }
            // The texel rectangle: a strip on the sides, a cut end above and
            // below.
            let ([u, v], height) = match face {
                Face::Up => ([13, 0], width),
                Face::Down => ([13, 4], width),
                _ => ([strip, 0], 16),
            };
            let uvs = [[u, v], [u, v + height], [u + width, v + height], [u + width, v]];
            self.push(box_face(face, min, max), uvs, face as u8, stem, tints, mesh);
        }
        match leaves {
            BambooLeaves::None => {}
            BambooLeaves::Small => self.cross(self.look.face(Face::South), tints, mesh),
            BambooLeaves::Large => self.cross(self.look.face(Face::Up), tints, mesh),
        }
    }

    /// A plane a sixteenth inside `face`, drawn from both sides: a vine or
    /// lichen on the block beside it.
    fn plane(&self, face: Face, tints: &Tints, mesh: &mut Mesh) {
        let look = self.look.face(face);
        // A box of no thickness, one sixteenth in from the face.
        let (axis, depth) = match face {
            Face::Down => (1, 1),
            Face::Up => (1, 15),
            Face::North => (2, 1),
            Face::South => (2, 15),
            Face::West => (0, 1),
            Face::East => (0, 15),
        };
        let (mut min, mut max) = ([0i8; 3], [16i8; 3]);
        min[axis] = depth;
        max[axis] = depth;
        let corners = box_face(face, min, max);
        let uvs = corners.map(|corner| face_uv(face, corner));
        let back = [corners[3], corners[2], corners[1], corners[0]];
        let back_uvs = [uvs[3], uvs[2], uvs[1], uvs[0]];
        self.push(corners, uvs, face as u8, look, tints, mesh);
        self.push(back, back_uvs, face as u8, look, tints, mesh);
    }

    /// Adds one quad: block-relative corners and texture coordinates in
    /// sixteenths.
    fn push(
        &self,
        corners: [[i8; 3]; 4],
        uvs: [[u8; 2]; 4],
        face: u8,
        look: &FaceLook,
        tints: &Tints,
        mesh: &mut Mesh,
    ) {
        let corners = corners.map(|corner| corner.map(|value| i16::from(value) * POSITION_SCALE));
        let uvs = uvs.map(|uv| uv.map(|value| value * UV_SCALE));
        self.push_fine(corners, uvs, face, look, tints, mesh);
    }

    /// Adds one quad, already in the vertex's units.
    fn push_fine(
        &self,
        corners: [[i16; 3]; 4],
        uvs: [[u8; 2]; 4],
        face: u8,
        look: &FaceLook,
        tints: &Tints,
        mesh: &mut Mesh,
    ) {
        let class = match look.class {
            RenderClass::Opaque => 0,
            RenderClass::Cutout => 1,
            RenderClass::Overlay => 2,
            RenderClass::Blend => 3,
        };
        let tint = self.tint(look, tints);
        let out = if look.class == RenderClass::Blend { &mut mesh.blend } else { &mut mesh.opaque };
        for (corner, uv) in corners.into_iter().zip(uvs) {
            out.push(Vertex {
                position: [0, 1, 2].map(|axis| self.local[axis] as i16 * 16 * POSITION_SCALE + corner[axis]),
                layer: look.layer,
                uv,
                face_class: face | class << 3,
                unused: 0,
                tint,
            });
        }
    }

    fn world_position(&self) -> [i32; 3] {
        [0, 1, 2].map(|axis| self.n.origin[axis] + i32::from(self.local[axis]))
    }
}

/// Where a corner of a flowing top is on the flowing texture. The texture's v runs the
/// way the liquid does, as its frames scroll towards higher v, and a block covers half
/// of it.
fn flowing_uv(corner: [i16; 3], [x, z]: [f32; 2]) -> [u8; 2] {
    let along = |value: i16| f32::from(value) / f32::from(WHOLE) - 0.5;
    let (from_x, from_z) = (along(corner[0]), along(corner[2]));
    let u = 0.5 + 0.5 * (from_x * z - from_z * x);
    let v = 0.5 + 0.5 * (from_x * x + from_z * z);
    [u, v].map(|value| (value * f32::from(UV_WHOLE)).round() as u8)
}

/// Java's `Mth.getSeed`: a position mixed into a number that's the same on
/// every run. Java picks texture turns, model variants and plant offsets
/// with it. Bedrock's own mixing isn't known, so what depends on it won't
/// match the official client block for block.
fn java_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut seed = i64::from(x.wrapping_mul(3_129_871)) ^ i64::from(z).wrapping_mul(116_129_781) ^ i64::from(y);
    seed = seed
        .wrapping_mul(seed)
        .wrapping_mul(42_317_861)
        .wrapping_add(seed.wrapping_mul(11));
    seed >> 16
}

/// A quarter-turn count picked by position, the same on every run.
fn isotropic_turns(x: i32, y: i32, z: i32, face: u8) -> u8 {
    (java_seed(x, y, z) as u8).wrapping_add(face) & 3
}

/// Java's horizontal offset along one axis, from the low 4 bits of `seed`:
/// −¼ to ¼ block in 16 steps, in sixteenths, rounded to the nearest one
/// (Java keeps the fraction; the vertex grid can't).
fn xz_offset(seed: i64) -> i8 {
    let fraction = (seed & 15) as f64 / 15.0;
    ((fraction - 0.5) * 8.0).round() as i8
}

#[cfg(test)]
mod tests;
