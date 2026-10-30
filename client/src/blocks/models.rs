//! Hand-made block models (m5-plan Step 7b): the boxes and planes of blocks
//! that aren't cubes.
//!
//! The install has no geometry for vanilla blocks (m4-plan Finding 8): Bedrock's
//! engine draws them from its own code. So a client has to know the shapes
//! itself, and these tables are ours, written to fit the install's textures.
//!
//! **The language is Java's** (worth teaching), because Bedrock's textures are
//! laid out for the same shapes. A model is a list of [`Part`]s: boxes with a
//! texture rectangle per face, and free quads for what's tilted. A block state
//! then turns its model: `x` quarter turns about east-west, then `y` about up
//! ([`Turn`]).
//!
//! **Baked once** (worth teaching). [`bake`] runs the turn at startup, and leaves
//! a flat list of [`ModelQuad`]s: four corners, four texture coordinates, and the
//! face that can hide it. The mesher only copies them, with no maths per block,
//! and the tests can look at a model without a GPU.
//!
//! **Slots:** a part names its texture by one of the look's six slots, as bamboo
//! does. `blocks/rules.rs` says what each block keeps in each slot.
//!
//! Parts are in sixteenths of a block and can leave the block: a crop reaches a
//! sixteenth below it, to meet the farmland. Rotated planes are rounded to the
//! same grid. Baked quads use the vertex's finer units ([`POSITION_SCALE`]).

use limonite_packs::registry::Face::{self, Down, East, North, South, Up, West};

/// The face code of a quad with no face of its own: planes on the diagonal, and
/// anything else that takes no shade. Faces are 0 to 5, in [`Face`] order.
pub const DIAGONAL: u8 = 6;

/// Baked positions per sixteenth of a block, so a block is 1,024 wide. Java's
/// models have halves and quarters of a sixteenth, and tilted planes that land
/// anywhere; a 1,024th of a block is far less than a pixel from any distance.
pub const POSITION_SCALE: i16 = 64;
/// Baked texture coordinates per sixteenth of a texture, so a texture is 128
/// wide, which still fits a byte. Java's rectangles have halves and quarters.
pub const UV_SCALE: u8 = 8;

/// A rectangle of a texture, in sixteenths: u0, v0, u1, v1. Reversed
/// coordinates mirror it, as they do in Java's model files.
pub type Rect = [u8; 4];

/// A box face's four corners, counter-clockwise seen from outside, starting
/// at the texture's top left. Counter-clockwise is the front face the GPU
/// keeps; it culls the back of every quad.
#[must_use]
pub const fn box_face(face: Face, min: [i8; 3], max: [i8; 3]) -> [[i8; 3]; 4] {
    let ([x0, y0, z0], [x1, y1, z1]) = (min, max);
    match face {
        Up => [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        Down => [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
        North => [[x1, y1, z0], [x1, y0, z0], [x0, y0, z0], [x0, y1, z0]],
        South => [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]],
        West => [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
        East => [[x1, y1, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0]],
    }
}

/// A corner's texture coordinates on `face`, from its position: a slab's
/// side shows the matching part of the texture.
#[must_use]
pub const fn face_uv(face: Face, [x, y, z]: [i8; 3]) -> [u8; 2] {
    /// Outside the block there's no texture left to show.
    const fn clamp(value: i8) -> u8 {
        if value < 0 {
            0
        } else if value > 16 {
            16
        } else {
            value as u8
        }
    }
    let (u, v) = match face {
        Up => (x, z),
        Down => (x, 16 - z),
        North => (16 - x, 16 - y),
        South => (x, 16 - y),
        West => (z, 16 - y),
        East => (16 - z, 16 - y),
    };
    [clamp(u), clamp(v)]
}

/// Turns texture coordinates a quarter around the texture's centre.
#[must_use]
pub const fn quarter_turn([u, v]: [u8; 2]) -> [u8; 2] {
    [16 - v, u]
}

/// A block state's rotation of its model, in quarter turns, as Java's block
/// states give it: `x` first, then `y`.
///
/// `y` turns clockwise seen from above: north to east. `x` tips up towards north.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Turn {
    pub x: u8,
    pub y: u8,
}

impl Turn {
    pub const NONE: Self = Self { x: 0, y: 0 };

    #[must_use]
    pub const fn y(y: u8) -> Self {
        Self { x: 0, y }
    }

    /// For a model that faces north: its turn to face `facing`.
    #[must_use]
    pub fn from_north(facing: &str) -> Self {
        Self::y(match facing {
            "east" => 1,
            "south" => 2,
            "west" => 3,
            _ => 0,
        })
    }

    /// For a model that faces east.
    #[must_use]
    pub fn from_east(facing: &str) -> Self {
        Self::y(match facing {
            "south" => 1,
            "west" => 2,
            "north" => 3,
            _ => 0,
        })
    }

    /// For a model that faces south.
    #[must_use]
    pub fn from_south(facing: &str) -> Self {
        Self::y(match facing {
            "west" => 1,
            "north" => 2,
            "east" => 3,
            _ => 0,
        })
    }

    /// For a model that faces west.
    #[must_use]
    pub fn from_west(facing: &str) -> Self {
        Self::y(match facing {
            "north" => 1,
            "east" => 2,
            "south" => 3,
            _ => 0,
        })
    }

    /// For a model that faces up, which can face all six ways.
    #[must_use]
    pub fn from_up(facing: &str) -> Self {
        match facing {
            "down" => Self { x: 2, y: 0 },
            "north" => Self { x: 1, y: 0 },
            "south" => Self { x: 1, y: 2 },
            "east" => Self { x: 1, y: 1 },
            "west" => Self { x: 1, y: 3 },
            _ => Self::NONE,
        }
    }

    /// Where the turn takes a point, in sixteenths.
    #[must_use]
    pub const fn corner(self, [mut x, mut y, mut z]: [i8; 3]) -> [i8; 3] {
        let mut i = 0;
        while i < self.x % 4 {
            (y, z) = (z, 16 - y);
            i += 1;
        }
        let mut i = 0;
        while i < self.y % 4 {
            (x, z) = (16 - z, x);
            i += 1;
        }
        [x, y, z]
    }

    /// Where the turn takes a point in baked units.
    #[must_use]
    pub const fn corner_fine(self, [mut x, mut y, mut z]: [i16; 3]) -> [i16; 3] {
        const WHOLE: i16 = 16 * POSITION_SCALE;
        let mut i = 0;
        while i < self.x % 4 {
            (y, z) = (z, WHOLE - y);
            i += 1;
        }
        let mut i = 0;
        while i < self.y % 4 {
            (x, z) = (WHOLE - z, x);
            i += 1;
        }
        [x, y, z]
    }

    /// Where the turn takes a face.
    #[must_use]
    pub const fn face(self, mut face: Face) -> Face {
        let mut i = 0;
        while i < self.x % 4 {
            face = match face {
                Up => North,
                North => Down,
                Down => South,
                South => Up,
                other => other,
            };
            i += 1;
        }
        let mut i = 0;
        while i < self.y % 4 {
            face = match face {
                North => East,
                East => South,
                South => West,
                West => North,
                other => other,
            };
            i += 1;
        }
        face
    }

    /// Which face turning lands on `face`, and how far its texture must turn
    /// so a whole block's faces move with it: for a cube with a front, which
    /// keeps its six faces rather than a model.
    #[must_use]
    pub fn cube_face(self, face: Face) -> (Face, u8) {
        let source = Face::ALL
            .into_iter()
            .find(|&source| self.face(source) == face)
            .expect("a turn is a permutation of the faces");
        let (min, max) = ([0; 3], [16; 3]);
        // Where each corner of the source face's texture lands.
        let wanted = box_face(source, min, max).map(|corner| (self.corner(corner), face_uv(source, corner)));
        let turns = (0..4u8).find(|&turns| {
            wanted.iter().all(|&(corner, uv)| {
                let mut at = face_uv(face, corner);
                for _ in 0..turns {
                    at = quarter_turn(at);
                }
                at == uv
            })
        });
        (source, turns.expect("every face's texture coordinates have the same handedness"))
    }
}

/// How a box face takes its texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Uv {
    /// By where it ends up, so a texture runs on from block to block: Java's
    /// `uvlock`.
    World,
    /// This rectangle, which turns with the part.
    Rect(Rect),
    /// The rectangle turned clockwise this many quarters: Java's face `rotation`.
    Turned(Rect, u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Skin {
    slot: Face,
    uv: Uv,
}

/// A piece of a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// A box. With no thickness it's a plane, drawn by the faces it has.
    Cuboid {
        from: [i8; 3],
        to: [i8; 3],
        faces: [Option<Skin>; 6],
        shaded: bool,
    },
    /// Four corners counter-clockwise from the texture's top left, for what's
    /// tilted. Takes no shade, as Java's plants don't.
    Quad {
        corners: [[i8; 3]; 4],
        uv: Rect,
        slot: Face,
        two_sided: bool,
    },
}

impl Part {
    /// A box with every face in `slot`, textured by where it ends up.
    #[must_use]
    pub const fn solid(from: [i8; 3], to: [i8; 3], slot: Face) -> Self {
        Self::Cuboid {
            from,
            to,
            faces: [Some(Skin { slot, uv: Uv::World }); 6],
            shaded: true,
        }
    }

    /// A box with no faces yet, for [`Self::rect`] and [`Self::world`] to add.
    #[must_use]
    pub const fn open(from: [i8; 3], to: [i8; 3]) -> Self {
        Self::Cuboid {
            from,
            to,
            faces: [None; 6],
            shaded: true,
        }
    }

    const fn with(mut self, face: Face, skin: Option<Skin>) -> Self {
        if let Self::Cuboid { faces, .. } = &mut self {
            faces[face as usize] = skin;
        }
        self
    }

    /// One face, showing a rectangle of `slot`.
    #[must_use]
    pub const fn rect(self, face: Face, slot: Face, rect: Rect) -> Self {
        self.with(face, Some(Skin { slot, uv: Uv::Rect(rect) }))
    }

    /// One face, showing a rectangle of `slot`, turned clockwise.
    #[must_use]
    pub const fn turned(self, face: Face, slot: Face, rect: Rect, quarters: u8) -> Self {
        self.with(face, Some(Skin { slot, uv: Uv::Turned(rect, quarters) }))
    }

    /// One face, showing `slot` by where it ends up.
    #[must_use]
    pub const fn world(self, face: Face, slot: Face) -> Self {
        self.with(face, Some(Skin { slot, uv: Uv::World }))
    }

    /// The four sides, showing the same rectangle of `slot`.
    #[must_use]
    pub const fn sides(self, slot: Face, rect: Rect) -> Self {
        self.rect(North, slot, rect)
            .rect(South, slot, rect)
            .rect(West, slot, rect)
            .rect(East, slot, rect)
    }

    /// Without one face.
    #[must_use]
    pub const fn without(self, face: Face) -> Self {
        self.with(face, None)
    }

    /// Takes no shade from its faces' directions, as Java's torches don't.
    #[must_use]
    pub const fn unshaded(mut self) -> Self {
        if let Self::Cuboid { shaded, .. } = &mut self {
            *shaded = false;
        }
        self
    }

    /// A plane standing from `a` to `b`, drawn from both sides.
    #[must_use]
    pub const fn plane(a: [i8; 2], b: [i8; 2], y0: i8, y1: i8, slot: Face, uv: Rect) -> Self {
        Self::Quad {
            corners: [[a[0], y1, a[1]], [a[0], y0, a[1]], [b[0], y0, b[1]], [b[0], y1, b[1]]],
            uv,
            slot,
            two_sided: true,
        }
    }

    /// A free quad, drawn from both sides.
    #[must_use]
    pub const fn quad(corners: [[i8; 3]; 4], slot: Face, uv: Rect) -> Self {
        Self::Quad {
            corners,
            uv,
            slot,
            two_sided: true,
        }
    }

    /// A free quad, drawn from the front: one face of something closed.
    #[must_use]
    pub const fn front(corners: [[i8; 3]; 4], slot: Face, uv: Rect) -> Self {
        Self::Quad {
            corners,
            uv,
            slot,
            two_sided: false,
        }
    }
}

/// One baked face of a model, ready for the mesher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModelQuad {
    /// Block-relative, in [`POSITION_SCALE`]ths of a sixteenth.
    pub corners: [[i16; 3]; 4],
    /// In [`UV_SCALE`]ths of a sixteenth of the texture.
    pub uvs: [[u8; 2]; 4],
    /// What shades it: a [`Face`], or [`DIAGONAL`].
    pub face: u8,
    /// Which of the look's textures it shows.
    pub slot: u8,
    /// The neighbour that hides it, if it lies on the block's edge.
    pub cull: Option<Face>,
}

/// Sixteenths of a block, in baked units.
fn fine_corners(corners: [[i8; 3]; 4]) -> [[i16; 3]; 4] {
    corners.map(|corner| corner.map(|value| i16::from(value) * POSITION_SCALE))
}

/// Sixteenths of a texture, in baked units.
fn fine_uvs(uvs: [[u8; 2]; 4]) -> [[u8; 2]; 4] {
    uvs.map(|uv| uv.map(|value| value * UV_SCALE))
}

/// A rectangle's coordinates at a quad's corners: top left, bottom left, bottom
/// right, top right.
const fn rect_uvs([u0, v0, u1, v1]: Rect, quarters: u8) -> [[u8; 2]; 4] {
    let corners = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
    let q = quarters as usize;
    [corners[q % 4], corners[(q + 1) % 4], corners[(q + 2) % 4], corners[(q + 3) % 4]]
}

/// Whether a point on a plane facing `face` lies on the block's edge that way.
const fn on_edge(face: Face, [x, y, z]: [i8; 3]) -> bool {
    match face {
        Down => y == 0,
        Up => y == 16,
        North => z == 0,
        South => z == 16,
        West => x == 0,
        East => x == 16,
    }
}

/// Adds `parts`, turned, to a baked model.
pub fn bake(parts: &[Part], turn: Turn, out: &mut Vec<ModelQuad>) {
    for part in parts {
        match *part {
            Part::Cuboid {
                from,
                to,
                faces,
                shaded,
            } => {
                for face in Face::ALL {
                    let Some(skin) = faces[face as usize] else {
                        continue;
                    };
                    let corners = box_face(face, from, to).map(|corner| turn.corner(corner));
                    let landed = turn.face(face);
                    let uvs = match skin.uv {
                        Uv::World => corners.map(|corner| face_uv(landed, corner)),
                        Uv::Rect(rect) => rect_uvs(rect, 0),
                        Uv::Turned(rect, quarters) => rect_uvs(rect, quarters),
                    };
                    out.push(ModelQuad {
                        corners: fine_corners(corners),
                        uvs: fine_uvs(uvs),
                        face: if shaded { landed as u8 } else { DIAGONAL },
                        slot: skin.slot as u8,
                        cull: on_edge(landed, corners[0]).then_some(landed),
                    });
                }
            }
            Part::Quad {
                corners,
                uv,
                slot,
                two_sided,
            } => {
                let corners = fine_corners(corners.map(|corner| turn.corner(corner)));
                let uvs = fine_uvs(rect_uvs(uv, 0));
                out.push(ModelQuad {
                    corners,
                    uvs,
                    face: DIAGONAL,
                    slot: slot as u8,
                    cull: None,
                });
                if two_sided {
                    // The back: the same corners and texture, in the other order.
                    out.push(ModelQuad {
                        corners: [corners[3], corners[2], corners[1], corners[0]],
                        uvs: [uvs[3], uvs[2], uvs[1], uvs[0]],
                        face: DIAGONAL,
                        slot: slot as u8,
                        cull: None,
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The models. Each comment says which slots it reads.
// ---------------------------------------------------------------------------

const FULL: Rect = [0, 0, 16, 16];

/// Two planes crossed on the diagonals, in North: like `Shape::Cross`, for a
/// model that has to turn. Sixteenth-grid corners, as M4's crossed plants have.
pub const CROSS: &[Part] = &[
    Part::plane([1, 1], [15, 15], 0, 16, North, FULL),
    Part::plane([1, 15], [15, 1], 0, 16, North, FULL),
];

/// Doors, in North: a plate 3 sixteenths thick on the west side, one model for
/// each way the handle can lie. A door's texture has its hinge on the left.
const fn door(west: Rect, east: Rect) -> [Part; 1] {
    [Part::open([0, 0, 0], [3, 16, 16])
        .rect(Down, North, [13, 0, 16, 16])
        .rect(Up, North, [13, 0, 16, 16])
        .rect(North, North, [3, 0, 0, 16])
        .rect(South, North, [0, 0, 3, 16])
        .rect(West, North, west)
        .rect(East, North, east)]
}
/// The hinge is at the plate's north end.
pub const DOOR_HINGE_NORTH: &[Part] = &door([0, 0, 16, 16], [16, 0, 0, 16]);
/// The hinge is at the plate's south end.
pub const DOOR_HINGE_SOUTH: &[Part] = &door([16, 0, 0, 16], [0, 0, 16, 16]);

/// A fence's post, in North, textured by where it stands.
pub const FENCE_POST: &[Part] = &[Part::solid([6, 0, 6], [10, 16, 10], North)];
/// A fence's two rails towards north, for every side it connects to.
pub const FENCE_SIDE: &[Part] = &[
    Part::solid([7, 12, 0], [9, 15, 9], North).without(South),
    Part::solid([7, 6, 0], [9, 9, 9], North).without(South),
];

/// A fence gate that faces south, in North, shut. It sits 3 lower in a wall.
const fn gate_parts(drop: i8) -> [Part; 8] {
    let (post_y, rail_y, low, high) = (5 - drop, 6 - drop, 9 - drop, 12 - drop);
    let (top, rail_top) = (16 - drop, 15 - drop);
    [
        Part::solid([0, post_y, 7], [2, top, 9], North),
        Part::solid([14, post_y, 7], [16, top, 9], North),
        Part::solid([6, rail_y, 7], [8, rail_top, 9], North),
        Part::solid([8, rail_y, 7], [10, rail_top, 9], North),
        Part::solid([2, rail_y, 7], [6, low, 9], North),
        Part::solid([2, high, 7], [6, rail_top, 9], North),
        Part::solid([10, rail_y, 7], [14, low, 9], North),
        Part::solid([10, high, 7], [14, rail_top, 9], North),
    ]
}
/// The same gate, swung open to the south.
const fn gate_open_parts(drop: i8) -> [Part; 8] {
    let (post_y, rail_y, low, high) = (5 - drop, 6 - drop, 9 - drop, 12 - drop);
    let (top, rail_top) = (16 - drop, 15 - drop);
    [
        Part::solid([0, post_y, 7], [2, top, 9], North),
        Part::solid([14, post_y, 7], [16, top, 9], North),
        Part::solid([0, rail_y, 13], [2, rail_top, 15], North),
        Part::solid([14, rail_y, 13], [16, rail_top, 15], North),
        Part::solid([0, rail_y, 9], [2, low, 13], North),
        Part::solid([0, high, 9], [2, rail_top, 13], North),
        Part::solid([14, rail_y, 9], [16, low, 13], North),
        Part::solid([14, high, 9], [16, rail_top, 13], North),
    ]
}
pub const GATE: &[Part] = &gate_parts(0);
pub const GATE_WALL: &[Part] = &gate_parts(3);
pub const GATE_OPEN: &[Part] = &gate_open_parts(0);
pub const GATE_OPEN_WALL: &[Part] = &gate_open_parts(3);

/// Glass panes: the pane in North, its edge in East. The post's ends.
pub const PANE_POST: &[Part] = &[Part::open([7, 0, 7], [9, 16, 9])
    .rect(Up, East, [7, 7, 9, 9])
    .rect(Down, East, [7, 7, 9, 9])];
/// An arm towards north.
pub const PANE_SIDE: &[Part] = &[Part::open([7, 0, 0], [9, 16, 7])
    .rect(Up, East, [7, 0, 9, 7])
    .rect(Down, East, [7, 0, 9, 7])
    .rect(North, East, [7, 0, 9, 16])
    .world(West, North)
    .world(East, North)];
/// The post's face where no arm joins it.
pub const PANE_NO_SIDE: &[Part] = &[Part::open([7, 0, 7], [9, 16, 9]).rect(North, North, [9, 0, 7, 16])];

/// Bars: thin planes through the middle, in North, with edges in East.
pub const BARS_POST: &[Part] = &[
    Part::open([8, 0, 7], [8, 16, 9]).rect(West, North, [7, 0, 9, 16]).rect(East, North, [7, 0, 9, 16]),
    Part::open([7, 0, 8], [9, 16, 8]).rect(North, North, [7, 0, 9, 16]).rect(South, North, [7, 0, 9, 16]),
    Part::open([7, 0, 7], [9, 0, 9]).rect(Down, East, [7, 7, 9, 9]),
    Part::open([7, 16, 7], [9, 16, 9]).rect(Up, East, [7, 7, 9, 9]),
];
/// An arm towards north: a plane, and the thin edges of its ends.
pub const BARS_SIDE: &[Part] = &[
    Part::open([8, 0, 0], [8, 16, 8]).world(West, North).world(East, North),
    Part::open([7, 0, 0], [9, 0, 7]).rect(Down, East, [9, 0, 7, 7]),
    Part::open([7, 16, 0], [9, 16, 7]).rect(Up, East, [9, 0, 7, 7]),
    Part::open([7, 0, 0], [9, 16, 0]).rect(North, East, [7, 0, 9, 16]),
];

/// A ladder on the south side of its block, in North.
pub const LADDER: &[Part] = &[Part::open([0, 0, 15], [16, 16, 15])
    .rect(North, North, FULL)
    .rect(South, North, FULL)
    .unshaded()];

/// A rail on the floor, in North: straight runs north to south, a curve joins
/// south and east.
pub const RAIL_FLAT: &[Part] = &[Part::open([0, 1, 0], [16, 1, 16])
    .rect(Up, North, FULL)
    .rect(Down, North, [0, 16, 16, 0])
    .unshaded()];
/// A rail climbing towards north, into the next block up.
pub const RAIL_RAISED: &[Part] = &[Part::quad(
    [[0, 17, 0], [0, 1, 16], [16, 1, 16], [16, 17, 0]],
    North,
    FULL,
)];

/// Redstone wire on the floor, in North. It's a plain cross: Bedrock works out
/// the connections from the blocks around, which isn't done yet.
pub const WIRE: &[Part] = &[Part::open([0, 1, 0], [16, 1, 16])
    .rect(Up, North, FULL)
    .rect(Down, North, [0, 16, 16, 0])
    .unshaded()];

/// A torch standing on the floor, in North.
pub const TORCH: &[Part] = &[Part::solid([7, 0, 7], [9, 10, 9], North)
    .rect(Up, North, [7, 6, 9, 8])
    .rect(Down, North, [7, 13, 9, 15])
    .unshaded()];
/// A torch leaning out of a wall on its west side, tipped 22.5 degrees and
/// rounded to the grid.
pub const TORCH_WALL: &[Part] = {
    const SIDE: Rect = [7, 6, 9, 16];
    // Across the stick: top and bottom corners, lower x first.
    const T1: [i8; 2] = [3, 13];
    const T2: [i8; 2] = [5, 12];
    const B1: [i8; 2] = [-1, 4];
    const B2: [i8; 2] = [1, 3];
    const fn at(point: [i8; 2], z: i8) -> [i8; 3] {
        [point[0], point[1], z]
    }
    &[
        Part::front([at(T1, 7), at(B1, 7), at(B1, 9), at(T1, 9)], North, SIDE),
        Part::front([at(T2, 9), at(B2, 9), at(B2, 7), at(T2, 7)], North, SIDE),
        Part::front([at(T2, 7), at(B2, 7), at(B1, 7), at(T1, 7)], North, SIDE),
        Part::front([at(T1, 9), at(B1, 9), at(B2, 9), at(T2, 9)], North, SIDE),
        Part::front([at(T1, 7), at(T1, 9), at(T2, 9), at(T2, 7)], North, [7, 6, 9, 8]),
    ]
};

const fn lantern_parts(drop: i8, handle_y: [i8; 2], handle_uvs: [Rect; 2]) -> [Part; 4] {
    let (bottom, middle, top) = (drop, 7 + drop, 9 + drop);
    [
        Part::open([5, bottom, 5], [11, middle, 11])
            .rect(Down, North, [0, 9, 6, 15])
            .rect(Up, North, [0, 9, 6, 15])
            .sides(North, [0, 2, 6, 9]),
        Part::open([6, middle, 6], [10, top, 10])
            .rect(Up, North, [0, 10, 4, 14])
            .sides(North, [0, 0, 4, 2]),
        Part::plane([7, 7], [9, 9], handle_y[0], handle_y[1], North, handle_uvs[0]),
        Part::plane([7, 9], [9, 7], handle_y[0], handle_y[1], North, handle_uvs[1]),
    ]
}
/// A lantern on the floor, in North.
pub const LANTERN: &[Part] = &lantern_parts(0, [9, 11], [[14, 1, 11, 3], [14, 10, 11, 12]]);
/// A lantern hanging, in North.
pub const LANTERN_HANGING: &[Part] = &lantern_parts(1, [10, 16], [[14, 1, 11, 7], [14, 6, 11, 12]]);

/// A chain, the two planes in Up and North. It stands along y, and turns.
pub const CHAIN: &[Part] = &[
    Part::plane([7, 7], [9, 9], 0, 16, Up, [0, 0, 3, 16]),
    Part::plane([7, 9], [9, 7], 0, 16, North, [0, 0, 3, 16]),
];

/// An end rod pointing up, in North.
pub const END_ROD: &[Part] = &[
    Part::open([6, 0, 6], [10, 1, 10])
        .rect(Down, North, [6, 6, 2, 2])
        .rect(Up, North, [2, 2, 6, 6])
        .sides(North, [2, 6, 6, 7]),
    Part::open([7, 1, 7], [9, 16, 9])
        .rect(Up, North, [2, 0, 0, 2])
        .sides(North, [0, 0, 2, 15]),
];

/// A lightning rod pointing up, in North.
pub const LIGHTNING_ROD: &[Part] = &[
    Part::open([6, 12, 6], [10, 16, 10])
        .rect(Down, North, [0, 0, 4, 4])
        .rect(Up, North, [4, 4, 0, 0])
        .sides(North, [0, 0, 4, 4]),
    Part::open([7, 0, 7], [9, 12, 9])
        .rect(Down, North, [0, 0, 2, 2])
        .sides(North, [0, 4, 2, 16]),
];

/// One candle, in `slot`, `height` tall, standing at (`x`, `y`, `z`), with its wick.
const fn candle(x: i8, y: i8, z: i8, height: i8, slot: Face) -> [Part; 3] {
    let top = y + height;
    [
        Part::open([x, y, z], [x + 2, top, z + 2])
            .rect(Down, slot, [0, 14, 2, 16])
            .rect(Up, slot, [0, 6, 2, 8])
            .sides(slot, [0, 8, 2, 8 + height as u8]),
        Part::plane([x, z + 1], [x + 1, z + 1], top, top + 1, slot, [0, 5, 1, 6]),
        Part::plane([x + 1, z], [x + 1, z + 1], top, top + 1, slot, [0, 5, 1, 6]),
    ]
}
const fn join3<const N: usize, const M: usize>(parts: [[Part; 3]; N]) -> [Part; M] {
    let mut out = [Part::open([0; 3], [0; 3]); M];
    let mut i = 0;
    while i < N {
        let mut j = 0;
        while j < 3 {
            out[i * 3 + j] = parts[i][j];
            j += 1;
        }
        i += 1;
    }
    out
}
/// One to four candles, in North.
pub const CANDLES: [&[Part]; 4] = [
    &candle(7, 0, 7, 6, North),
    &join3::<2, 6>([candle(5, 0, 7, 6, North), candle(9, 0, 6, 5, North)]),
    &join3::<3, 9>([candle(7, 0, 9, 6, North), candle(5, 0, 7, 5, North), candle(8, 0, 6, 3, North)]),
    &join3::<4, 12>([
        candle(6, 0, 8, 6, North),
        candle(9, 0, 8, 5, North),
        candle(5, 0, 5, 5, North),
        candle(8, 0, 5, 3, North),
    ]),
];
/// A cake with a candle: the cake's faces in Down, Up and North, the candle in
/// South.
pub const CANDLE_CAKE: &[Part] = &{
    let [candle, wick_a, wick_b] = candle(7, 8, 7, 6, South);
    [
        Part::open([1, 0, 1], [15, 8, 15])
            .world(Down, Down)
            .world(Up, Up)
            .world(North, North)
            .world(South, North)
            .world(West, North)
            .world(East, North),
        candle,
        wick_a,
        wick_b,
    ]
};

/// The base all repeaters and comparators stand on: the floor in Down, the top
/// in Up.
const DIODE_BASE: Part = Part::open([0, 0, 0], [16, 2, 16])
    .rect(Down, Down, FULL)
    .rect(Up, Up, FULL)
    .sides(Down, [0, 14, 16, 16]);
/// A torch on a diode, `height` tall.
const fn diode_torch(x: i8, z: i8, height: i8, slot: Face) -> Part {
    Part::open([x, 2, z], [x + 2, 2 + height, z + 2])
        .rect(Up, slot, [7, 6, 9, 8])
        .sides(slot, [7, 6, 9, 6 + height as u8])
}
const fn repeater(delay_z: i8) -> [Part; 3] {
    [DIODE_BASE, diode_torch(7, 2, 5, North), diode_torch(7, delay_z, 5, North)]
}
/// A repeater with its output north, a delay of 1 to 4 ticks. Torches in North.
pub const REPEATER: [&[Part]; 4] = [&repeater(6), &repeater(8), &repeater(10), &repeater(12)];
const fn comparator(front: i8) -> [Part; 4] {
    [
        DIODE_BASE,
        diode_torch(7, 2, front, South),
        diode_torch(4, 11, 5, North),
        diode_torch(10, 11, 5, North),
    ]
}
/// A comparator with its output north, comparing or subtracting: the front
/// torch in South, the back two in North.
pub const COMPARATOR: &[Part] = &comparator(2);
pub const COMPARATOR_SUBTRACT: &[Part] = &comparator(3);

const COCOA_STEM: Part = Part::open([8, 12, 12], [8, 16, 16])
    .rect(West, North, [16, 0, 12, 4])
    .rect(East, North, [12, 0, 16, 4]);
const fn cocoa(from: [i8; 3], to: [i8; 3], top: Rect, side: Rect) -> [Part; 2] {
    [
        Part::open(from, to).rect(Down, North, top).rect(Up, North, top).sides(North, side),
        COCOA_STEM,
    ]
}
/// A cocoa pod hanging from a log to its south, by age, in North.
pub const COCOA: [&[Part]; 3] = [
    &cocoa([6, 7, 11], [10, 12, 15], [0, 0, 4, 4], [11, 4, 15, 9]),
    &cocoa([5, 5, 9], [11, 12, 15], [0, 0, 6, 6], [9, 4, 15, 11]),
    &cocoa([4, 3, 7], [12, 12, 15], [0, 0, 7, 7], [7, 4, 15, 13]),
];

/// Crops: four planes in a # , a sixteenth down into the farmland, in North.
pub const CROP: &[Part] = &[
    Part::open([4, -1, 0], [4, 15, 16]).rect(West, North, FULL).rect(East, North, FULL).unshaded(),
    Part::open([12, -1, 0], [12, 15, 16]).rect(West, North, FULL).rect(East, North, FULL).unshaded(),
    Part::open([0, -1, 4], [16, 15, 4]).rect(North, North, FULL).rect(South, North, FULL).unshaded(),
    Part::open([0, -1, 12], [16, 15, 12]).rect(North, North, FULL).rect(South, North, FULL).unshaded(),
];

/// A melon or pumpkin stem by age, crossed, in North.
#[must_use]
pub fn stem(age: u8) -> Vec<Part> {
    let height = (i16::from(age.min(7)) * 2 + 2) as i8;
    let uv = [0, 16 - height as u8, 16, 16];
    vec![
        Part::plane([1, 1], [15, 15], -1, height - 1, North, uv),
        Part::plane([1, 15], [15, 1], -1, height - 1, North, uv),
    ]
}
/// A stem bent towards the fruit to its west, in North.
pub const STEM_ATTACHED: &[Part] = &[Part::plane([0, 8], [16, 8], 0, 16, North, FULL)];

/// Leaf litter's quarters: each one more of the block, up to four, by how much
/// there is. North is what's kept in the first quarter. Texture in North.
const fn quarter(x: i8, z: i8, y: i8, slot: Face) -> Part {
    let (u, v) = (x as u8, z as u8);
    Part::open([x, y, z], [x + 8, y, z + 8])
        .rect(Up, slot, [u, v, u + 8, v + 8])
        .rect(Down, slot, [u, v + 8, u + 8, v])
        .unshaded()
}
/// The order the quarters fill in, at the first facing, north: north-west,
/// south-west, south-east, north-east.
const QUARTERS: [[i8; 2]; 4] = [[0, 0], [0, 8], [8, 8], [8, 0]];
/// A flat layer of 1 to 4 quarters, at height `y`, in North.
#[must_use]
pub fn flat_layer(amount: usize, y: i8) -> Vec<Part> {
    QUARTERS
        .iter()
        .take(amount.clamp(1, 4))
        .map(|&[x, z]| quarter(x, z, y, North))
        .collect()
}
/// Flowers on short stems: `flat_layer` at height 3, and stems in East.
#[must_use]
pub fn flower_bed(amount: usize) -> Vec<Part> {
    let mut parts = flat_layer(amount, 3);
    const STEM_UV: Rect = [0, 5, 1, 8];
    for &[x, z] in QUARTERS.iter().take(amount.clamp(1, 4)) {
        // Two short stems under each quarter's flowers, in a little cross.
        let (cx, cz) = (x + 4, z + 4);
        parts.push(Part::plane([cx - 1, cz - 1], [cx, cz], 0, 3, East, STEM_UV));
        parts.push(Part::plane([cx, cz - 1], [cx - 1, cz], 0, 3, East, STEM_UV));
    }
    parts
}

/// An azalea: a leafy top, sides, and a plant inside: Up, North and East.
pub const AZALEA: &[Part] = &[
    Part::open([0, 16, 0], [16, 16, 16]).rect(Up, Up, FULL),
    Part::open([0, 15, 0], [16, 15, 16]).rect(Down, Up, [0, 16, 16, 0]),
    Part::open([0, 0, 0], [16, 16, 0]).rect(North, North, FULL),
    Part::open([0, 0, 1], [16, 16, 1]).rect(South, North, FULL),
    Part::open([0, 0, 16], [16, 16, 16]).rect(South, North, FULL),
    Part::open([0, 0, 15], [16, 16, 15]).rect(North, North, FULL),
    Part::open([0, 0, 0], [0, 16, 16]).rect(West, North, FULL),
    Part::open([1, 0, 0], [1, 16, 16]).rect(East, North, FULL),
    Part::open([16, 0, 0], [16, 16, 16]).rect(East, North, FULL),
    Part::open([15, 0, 0], [15, 16, 16]).rect(West, North, FULL),
    Part::plane([1, 1], [15, 15], 0, 16, East, FULL),
    Part::plane([1, 15], [15, 1], 0, 16, East, FULL),
];

/// The stem under and through a big dripleaf, in North.
pub const DRIPLEAF_STEM: &[Part] = &[
    Part::plane([1, 1], [15, 15], 0, 16, North, FULL),
    Part::plane([1, 15], [15, 1], 0, 16, North, FULL),
];
/// A big dripleaf's leaf, its tip north: South holds the top, Up the tip's rim,
/// Down the other rims. `drop` is how far its tip hangs, and `reach` how far
/// north it still gets, for a leaf that's tilting.
#[must_use]
pub fn dripleaf(drop: i8, reach: i8) -> Vec<Part> {
    const TOP: i8 = 15;
    const RIM: i8 = 4;
    let (near, far, tip_z) = (TOP, TOP - drop, 16 - reach);
    let rim = [0, 0, 16, 4];
    vec![
        Part::Quad {
            corners: [[0, far, tip_z], [0, near, 16], [16, near, 16], [16, far, tip_z]],
            uv: FULL,
            slot: South,
            two_sided: true,
        },
        Part::plane([0, tip_z], [16, tip_z], far - RIM, far, Up, rim),
        Part::quad(
            [[0, far, tip_z], [0, far - RIM, tip_z], [0, near - RIM, 16], [0, near, 16]],
            Down,
            rim,
        ),
        Part::quad(
            [[16, far, tip_z], [16, far - RIM, tip_z], [16, near - RIM, 16], [16, near, 16]],
            Down,
            rim,
        ),
    ]
}

/// A small dripleaf's lower half: its stems in South.
pub const SMALL_DRIPLEAF_LOWER: &[Part] = &[
    Part::plane([1, 1], [15, 15], 0, 16, South, FULL),
    Part::plane([1, 15], [15, 1], 0, 16, South, FULL),
];
/// A small dripleaf's upper half: stems in North, and three leaves in Up.
pub const SMALL_DRIPLEAF_UPPER: &[Part] = &[
    Part::plane([1, 1], [15, 15], 0, 16, North, FULL),
    Part::plane([1, 15], [15, 1], 0, 16, North, FULL),
    Part::open([1, 14, 1], [9, 14, 9]).rect(Up, Up, [0, 0, 8, 8]).rect(Down, Up, [0, 8, 8, 0]),
    Part::open([8, 11, 4], [16, 11, 12]).rect(Up, Up, [0, 0, 8, 8]).rect(Down, Up, [0, 8, 8, 0]),
    Part::open([2, 8, 8], [10, 8, 16]).rect(Up, Up, [0, 0, 8, 8]).rect(Down, Up, [0, 8, 8, 0]),
];

/// A coral fan on a wall to its south, in North: two planes spreading from the
/// wall, 22.5 degrees either side. The fan is the texture's lower half, its
/// point at the wall.
pub const CORAL_WALL_FAN: &[Part] = &[
    Part::quad([[0, 12, 5], [0, 7, 16], [16, 7, 16], [16, 12, 5]], North, [0, 16, 16, 5]),
    Part::quad([[0, 4, 5], [0, 9, 16], [16, 9, 16], [16, 4, 5]], North, [0, 16, 16, 5]),
];

/// A sunflower's upper half: crossed planes in North, and its head looking east:
/// the front in South, the back in West.
pub const SUNFLOWER_UPPER: &[Part] = &[
    Part::plane([1, 1], [15, 15], 0, 16, North, FULL),
    Part::plane([1, 15], [15, 1], 0, 16, North, FULL),
    Part::front([[12, 14, 15], [6, -1, 15], [6, -1, 1], [12, 14, 1]], South, FULL),
    Part::front([[12, 14, 1], [6, -1, 1], [6, -1, 15], [12, 14, 15]], West, FULL),
];

/// A flame: four planes leaning across the block, taller than it, in North.
pub const FIRE: &[Part] = &[
    Part::quad([[0, 22, 3], [0, 0, 13], [16, 0, 13], [16, 22, 3]], North, FULL),
    Part::quad([[0, 22, 13], [0, 0, 3], [16, 0, 3], [16, 22, 13]], North, FULL),
    Part::quad([[3, 22, 0], [13, 0, 0], [13, 0, 16], [3, 22, 16]], North, FULL),
    Part::quad([[13, 22, 0], [3, 0, 0], [3, 0, 16], [13, 22, 16]], North, FULL),
];

/// A campfire's logs, facing south: the plain log in North, the lit one in South.
pub const CAMPFIRE: &[Part] = &[
    Part::open([1, 0, 0], [5, 4, 16])
        .rect(North, North, [0, 4, 4, 8])
        .rect(East, South, [0, 1, 16, 5])
        .rect(South, North, [0, 4, 4, 8])
        .rect(West, North, [16, 0, 0, 4])
        .turned(Up, North, [0, 0, 16, 4], 1)
        .turned(Down, North, [0, 0, 16, 4], 1),
    Part::open([0, 3, 11], [16, 7, 15])
        .rect(North, North, [16, 0, 0, 4])
        .rect(East, North, [0, 4, 4, 8])
        .rect(South, South, [0, 0, 16, 4])
        .rect(West, North, [0, 4, 4, 8])
        .turned(Up, North, [0, 0, 16, 4], 2)
        .rect(Down, North, [0, 4, 16, 8]),
    Part::open([11, 0, 0], [15, 4, 16])
        .rect(North, North, [0, 4, 4, 8])
        .rect(East, North, [16, 0, 0, 4])
        .rect(South, North, [0, 4, 4, 8])
        .rect(West, South, [0, 1, 16, 5])
        .turned(Up, North, [0, 0, 16, 4], 1)
        .turned(Down, North, [0, 0, 16, 4], 1),
    Part::open([0, 3, 1], [16, 7, 5])
        .rect(North, South, [0, 0, 16, 4])
        .rect(East, North, [0, 4, 4, 8])
        .rect(South, North, [16, 0, 0, 4])
        .rect(West, North, [0, 4, 4, 8])
        .turned(Up, North, [0, 0, 16, 4], 2)
        .rect(Down, North, [0, 4, 16, 8]),
    Part::open([5, 0, 0], [11, 1, 16])
        .rect(North, South, [0, 15, 6, 16])
        .rect(South, South, [10, 15, 16, 16])
        .turned(Up, South, [0, 8, 16, 14], 1)
        .turned(Down, North, [0, 8, 16, 14], 1),
];
/// The campfire's flames, in Up.
pub const CAMPFIRE_FLAMES: &[Part] = &[
    Part::plane([1, 1], [15, 15], 1, 17, Up, FULL),
    Part::plane([1, 15], [15, 1], 1, 17, Up, FULL),
];

/// A cauldron: outside in North, inside in South, the rim in Up, the bottom in
/// Down.
pub const CAULDRON: &[Part] = &[
    Part::solid([0, 3, 0], [2, 16, 16], North).world(Up, Up).world(Down, Down).world(East, South),
    Part::solid([14, 3, 0], [16, 16, 16], North).world(Up, Up).world(Down, Down).world(West, South),
    Part::solid([2, 3, 0], [14, 16, 2], North).world(Up, Up).world(Down, Down).world(South, South),
    Part::solid([2, 3, 14], [14, 16, 16], North).world(Up, Up).world(Down, Down).world(North, South),
    Part::open([2, 3, 2], [14, 4, 14]).world(Up, South).world(Down, Down),
    Part::solid([0, 0, 0], [4, 3, 2], North).world(Down, Down),
    Part::solid([0, 0, 2], [2, 3, 4], North).world(Down, Down),
    Part::solid([12, 0, 0], [16, 3, 2], North).world(Down, Down),
    Part::solid([14, 0, 2], [16, 3, 4], North).world(Down, Down),
    Part::solid([0, 0, 14], [4, 3, 16], North).world(Down, Down),
    Part::solid([0, 0, 12], [2, 3, 14], North).world(Down, Down),
    Part::solid([12, 0, 14], [16, 3, 16], North).world(Down, Down),
    Part::solid([14, 0, 12], [16, 3, 14], North).world(Down, Down),
];
/// The liquid in a cauldron, at height `y`, in West.
#[must_use]
pub const fn cauldron_liquid(y: i8) -> Part {
    Part::open([2, y, 2], [14, y, 14]).world(Up, West)
}

/// A hopper: outside in North, the rim in Up, inside in Down. Its spout points
/// down.
pub const HOPPER: &[Part] = &[
    Part::solid([0, 10, 0], [16, 11, 16], North).world(Up, Down),
    Part::solid([0, 11, 0], [2, 16, 16], North).world(Up, Up).world(East, Down).without(Down),
    Part::solid([14, 11, 0], [16, 16, 16], North).world(Up, Up).world(West, Down).without(Down),
    Part::solid([2, 11, 0], [14, 16, 2], North).world(Up, Up).world(South, Down).without(Down),
    Part::solid([2, 11, 14], [14, 16, 16], North).world(Up, Up).world(North, Down).without(Down),
    Part::solid([4, 4, 4], [12, 10, 12], North).without(Up),
];
pub const HOPPER_SPOUT_DOWN: &[Part] = &[Part::solid([6, 0, 6], [10, 4, 10], North).without(Up)];
/// A spout out of the side, north.
pub const HOPPER_SPOUT_SIDE: &[Part] = &[Part::solid([6, 4, 0], [10, 8, 4], North).without(South)];

/// A brewing stand's rod in North, and its base stones in Down.
pub const BREWING_STAND: &[Part] = &[
    Part::open([7, 0, 7], [9, 14, 9])
        .rect(Up, North, [7, 0, 9, 2])
        .sides(North, [7, 2, 9, 16]),
    Part::solid([9, 0, 5], [15, 2, 11], Down),
    Part::solid([2, 0, 1], [8, 2, 7], Down),
    Part::solid([2, 0, 9], [8, 2, 15], Down),
];
/// A bottle's arm, from the rod out to `far`: with a bottle on it or empty.
#[must_use]
pub const fn brewing_arm(far: [i8; 2], bottle: bool) -> Part {
    let uv = if bottle { [0, 0, 8, 16] } else { [16, 0, 8, 16] };
    Part::plane(far, [8, 8], 0, 16, North, uv)
}

/// An anvil's body and working top, its long side north to south: the top in
/// Up, the rest in North.
pub const ANVIL: &[Part] = &[
    Part::solid([2, 0, 2], [14, 4, 14], North),
    Part::solid([4, 4, 3], [12, 5, 13], North),
    Part::solid([6, 5, 4], [10, 10, 12], North),
    Part::solid([3, 10, 0], [13, 16, 16], North).world(Up, Up),
];

/// A bell's body: its sides in North, top in Up, and rim's underside in Down.
const BELL_BODY: [Part; 2] = [
    Part::open([5, 6, 5], [11, 13, 11]).rect(Up, Up, [1, 1, 7, 7]).sides(North, [1, 0, 7, 7]),
    Part::open([4, 4, 4], [12, 6, 12])
        .rect(Up, Up, [0, 0, 8, 8])
        .rect(Down, Down, [0, 0, 8, 8])
        .sides(North, [0, 7, 8, 9]),
];
/// A bell on the floor between two posts: the stone in West, the bar in East.
pub const BELL_FLOOR: &[Part] = &[
    BELL_BODY[0],
    BELL_BODY[1],
    Part::solid([2, 0, 6], [4, 13, 10], West),
    Part::solid([12, 0, 6], [14, 13, 10], West),
    Part::solid([4, 10, 7], [12, 12, 9], East),
];
/// A bell hanging from above.
pub const BELL_CEILING: &[Part] = &[
    BELL_BODY[0],
    BELL_BODY[1],
    Part::solid([7, 13, 7], [9, 16, 9], East),
];
/// A bell on a bracket from one wall, to the north.
pub const BELL_WALL: &[Part] = &[
    BELL_BODY[0],
    BELL_BODY[1],
    Part::solid([7, 13, 0], [9, 15, 13], East),
    Part::solid([6, 12, 0], [10, 16, 3], West),
];
/// A bell on a bar between two walls, north and south.
pub const BELL_BETWEEN: &[Part] = &[
    BELL_BODY[0],
    BELL_BODY[1],
    Part::solid([7, 13, 0], [9, 15, 16], East),
];

/// A flower pot, in North, with the soil in Up.
pub const FLOWER_POT: &[Part] = &[
    Part::solid([5, 0, 5], [6, 6, 11], North),
    Part::solid([10, 0, 5], [11, 6, 11], North),
    Part::solid([6, 0, 5], [10, 6, 6], North),
    Part::solid([6, 0, 10], [10, 6, 11], North),
    Part::open([6, 0, 6], [10, 4, 10]).world(Up, Up).world(Down, North),
];

/// A pickle's body, in North: the `height` of it shown.
const fn pickle(x: i8, z: i8, height: i8) -> Part {
    Part::open([x, 0, z], [x + 4, height, z + 4])
        .rect(Down, North, [8, 1, 12, 5])
        .rect(Up, North, [4, 1, 8, 5])
        .sides(North, [0, 5, 4, 5 + height as u8])
}
/// One to four sea pickles, in North.
pub const SEA_PICKLES: [&[Part]; 4] = [
    &[pickle(6, 6, 6)],
    &[pickle(3, 3, 6), pickle(8, 8, 4)],
    &[pickle(5, 9, 6), pickle(2, 2, 4), pickle(10, 4, 6)],
    &[pickle(2, 2, 6), pickle(9, 10, 4), pickle(9, 2, 6), pickle(2, 8, 7)],
];

/// A turtle egg, in North.
const fn egg(from: [i8; 3], size: [i8; 2], top: Rect, side: Rect) -> Part {
    Part::open(from, [from[0] + size[0], from[1] + size[1], from[2] + size[0]])
        .rect(Down, North, top)
        .rect(Up, North, top)
        .sides(North, side)
}
const EGG_1: Part = egg([5, 0, 4], [4, 7], [0, 0, 4, 4], [1, 4, 5, 11]);
const EGG_2: Part = egg([1, 0, 7], [3, 5], [6, 7, 9, 10], [10, 0, 13, 5]);
const EGG_3: Part = egg([11, 0, 7], [3, 4], [5, 0, 8, 3], [8, 0, 11, 4]);
const EGG_4: Part = egg([6, 0, 9], [4, 4], [0, 0, 4, 4], [1, 4, 5, 8]);
/// One to four turtle eggs, in North.
pub const TURTLE_EGGS: [&[Part]; 4] = [
    &[EGG_1],
    &[EGG_1, EGG_2],
    &[EGG_1, EGG_2, EGG_3],
    &[EGG_1, EGG_2, EGG_3, EGG_4],
];

/// A composter: outside in North, the rim in Up, the floor in Down.
pub const COMPOSTER: &[Part] = &[
    Part::solid([0, 0, 0], [2, 16, 16], North).world(Up, Up).world(Down, Down),
    Part::solid([14, 0, 0], [16, 16, 16], North).world(Up, Up).world(Down, Down),
    Part::solid([2, 0, 0], [14, 16, 2], North).world(Up, Up).world(Down, Down),
    Part::solid([2, 0, 14], [14, 16, 16], North).world(Up, Up).world(Down, Down),
    Part::open([2, 0, 2], [14, 2, 14]).world(Up, Down).world(Down, Down),
];
/// A composter's contents at height `y`, in South.
#[must_use]
pub const fn compost(y: i8) -> Part {
    Part::open([2, y, 2], [14, y, 14]).world(Up, South)
}

/// A stonecutter: the block's faces in Down, Up and North, the saw in West,
/// running east to west.
pub const STONECUTTER: &[Part] = &[
    Part::open([0, 0, 0], [16, 9, 16])
        .world(Down, Down)
        .world(Up, Up)
        .world(North, North)
        .world(South, North)
        .world(West, North)
        .world(East, North),
    Part::plane([1, 8], [15, 8], 9, 16, West, [1, 9, 15, 16]),
];

/// A lectern, facing north: the foot in East, the post's front in North and
/// its other sides in South, its top in Up and that top's underside in Down.
/// The top is a plate 4 thick, tipped 22.5 degrees, rounded to the grid.
pub const LECTERN: &[Part] = &[
    Part::solid([0, 0, 0], [16, 2, 16], East),
    Part::open([4, 2, 4], [12, 15, 12])
        .rect(North, North, [0, 0, 8, 13])
        .rect(South, South, [0, 0, 8, 13])
        .rect(West, South, [0, 0, 8, 13])
        .rect(East, South, [0, 0, 8, 13]),
    Part::front([[0, 14, 3], [0, 19, 15], [16, 19, 15], [16, 14, 3]], Up, [0, 1, 16, 14]),
    Part::front([[0, 15, 16], [0, 10, 4], [16, 10, 4], [16, 15, 16]], Down, [0, 0, 16, 13]),
    Part::front([[16, 14, 3], [16, 10, 4], [0, 10, 4], [0, 14, 3]], South, [0, 0, 16, 4]),
    Part::front([[0, 19, 15], [0, 15, 16], [16, 15, 16], [16, 19, 15]], South, [0, 0, 16, 4]),
    Part::front([[0, 14, 3], [0, 10, 4], [0, 15, 16], [0, 19, 15]], South, [0, 0, 13, 4]),
    Part::front([[16, 19, 15], [16, 15, 16], [16, 10, 4], [16, 14, 3]], South, [0, 0, 13, 4]),
];

/// A grindstone on the floor, its wheel running north to south: the wheel's
/// sides in South, its rim in Up, the arms in North, the legs in Down.
pub const GRINDSTONE: &[Part] = &[
    Part::open([4, 4, 2], [12, 16, 14])
        .rect(West, South, [0, 0, 12, 12])
        .rect(East, South, [0, 0, 12, 12])
        .rect(Up, Up, [0, 0, 8, 12])
        .rect(Down, Up, [0, 0, 8, 12])
        .rect(North, Up, [0, 0, 8, 12])
        .rect(South, Up, [0, 0, 8, 12]),
    Part::solid([2, 7, 5], [4, 13, 11], North),
    Part::solid([12, 7, 5], [14, 13, 11], North),
    Part::solid([2, 0, 6], [4, 7, 10], Down),
    Part::solid([12, 0, 6], [14, 7, 10], Down),
];

/// A lever's cobblestone base, in East, on the floor.
const LEVER_BASE: Part = Part::solid([5, 0, 4], [11, 3, 12], East);
/// A lever tipped north, off, in North: its handle's four sides and its end.
pub const LEVER_NORTH: &[Part] = &[
    LEVER_BASE,
    Part::front([[9, 11, 1], [9, 1, 7], [9, 1, 9], [9, 11, 3]], North, [7, 6, 9, 16]),
    Part::front([[7, 11, 3], [7, 1, 9], [7, 1, 7], [7, 11, 1]], North, [7, 6, 9, 16]),
    Part::front([[9, 11, 3], [9, 1, 9], [7, 1, 9], [7, 11, 3]], North, [7, 6, 9, 16]),
    Part::front([[7, 11, 1], [7, 1, 7], [9, 1, 7], [9, 11, 1]], North, [7, 6, 9, 16]),
    Part::front([[7, 11, 1], [7, 11, 3], [9, 11, 3], [9, 11, 1]], North, [7, 6, 9, 8]),
];
/// The same lever, tipped south: on.
pub const LEVER_SOUTH: &[Part] = &[
    LEVER_BASE,
    Part::front([[7, 11, 13], [7, 1, 7], [7, 1, 9], [7, 11, 15]], North, [7, 6, 9, 16]),
    Part::front([[9, 11, 15], [9, 1, 9], [9, 1, 7], [9, 11, 13]], North, [7, 6, 9, 16]),
    Part::front([[7, 11, 15], [7, 1, 9], [9, 1, 9], [9, 11, 15]], North, [7, 6, 9, 16]),
    Part::front([[9, 11, 13], [9, 1, 7], [7, 1, 7], [7, 11, 13]], North, [7, 6, 9, 16]),
    Part::front([[7, 11, 13], [7, 11, 15], [9, 11, 15], [9, 11, 13]], North, [7, 6, 9, 8]),
];

/// A tripwire hook on a wall to its south: the plate in Down, the hook in Up.
pub const TRIPWIRE_HOOK: &[Part] = &[
    Part::solid([6, 1, 14], [10, 9, 16], Down),
    Part::plane([8, 10], [8, 16], 1, 10, Up, [5, 2, 11, 11]),
];

/// One run of tripwire towards north, in North.
pub const TRIPWIRE_SIDE: &[Part] = &[Part::open([7, 1, 0], [9, 1, 8])
    .rect(Up, North, [0, 2, 8, 4])
    .rect(Down, North, [0, 2, 8, 4])
    .unshaded()];

/// An end portal frame facing south: the block's faces in Down, Up and North.
pub const PORTAL_FRAME: &[Part] = &[Part::open([0, 0, 0], [16, 13, 16])
    .world(Down, Down)
    .world(Up, Up)
    .world(North, North)
    .world(South, North)
    .world(West, North)
    .world(East, North)];
/// The eye in a portal frame, in South.
pub const PORTAL_EYE: &[Part] = &[Part::open([4, 13, 4], [12, 16, 12])
    .rect(Up, South, [4, 0, 12, 8])
    .sides(South, [0, 8, 8, 11])];

/// A beacon: the glass in North, the obsidian base in Down, the core in Up.
pub const BEACON: &[Part] = &[
    Part::solid([2, 0, 2], [14, 3, 14], Down),
    Part::solid([3, 3, 3], [13, 14, 13], Up),
    Part::solid([0, 0, 0], [16, 16, 16], North),
];

/// A sculk shrieker: its base's faces in Down, Up and North, and in South the top
/// of the part that rises out of it. The side's texture has both.
pub const SCULK_SHRIEKER: &[Part] = &[
    Part::solid([0, 0, 0], [16, 8, 16], North).world(Up, Up).world(Down, Down),
    Part::open([1, 8, 1], [15, 15, 15])
        .rect(Up, South, [1, 1, 15, 15])
        .sides(North, [1, 1, 15, 8]),
];

/// A cactus: its top and bottom whole, its sides a sixteenth in: Up, Down and
/// North. Its textures are drawn for this.
pub const CACTUS: &[Part] = &[
    Part::open([0, 0, 0], [16, 16, 16]).rect(Up, Up, FULL).rect(Down, Down, FULL),
    Part::open([0, 0, 1], [16, 16, 15]).rect(North, North, FULL).rect(South, North, FULL),
    Part::open([1, 0, 0], [15, 16, 16]).rect(West, North, FULL).rect(East, North, FULL),
];

/// A spore blossom under a ceiling: the leaves in Up, the flower in Down.
pub const SPORE_BLOSSOM: &[Part] = &[
    Part::open([1, 15, 1], [15, 15, 15]).rect(Down, Up, [1, 15, 15, 1]).rect(Up, Up, [1, 1, 15, 15]),
    Part::quad([[0, 15, 8], [0, 7, 0], [16, 7, 0], [16, 15, 8]], Down, FULL),
    Part::quad([[0, 15, 8], [0, 7, 16], [16, 7, 16], [16, 15, 8]], Down, FULL),
    Part::quad([[8, 15, 0], [0, 7, 0], [0, 7, 16], [8, 15, 16]], Down, FULL),
    Part::quad([[8, 15, 0], [16, 7, 0], [16, 7, 16], [8, 15, 16]], Down, FULL),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_moves_points_and_faces_together() {
        let turns = [
            Turn::NONE,
            Turn::y(1),
            Turn::y(2),
            Turn::y(3),
            Turn { x: 1, y: 0 },
            Turn { x: 2, y: 0 },
            Turn { x: 1, y: 1 },
            Turn { x: 1, y: 3 },
        ];
        for turn in turns {
            for face in Face::ALL {
                // A face's corners land on the plane of the face it turns into.
                let landed = turn.face(face);
                let corners = box_face(face, [0; 3], [16; 3]).map(|corner| turn.corner(corner));
                assert!(
                    corners.iter().all(|&corner| on_edge(landed, corner)),
                    "{turn:?}: {face:?} landed on {landed:?} at {corners:?}"
                );
            }
        }
        // Java's own examples: y turns north to east, x tips up to north.
        assert_eq!(Turn::y(1).face(North), East);
        assert_eq!(Turn { x: 1, y: 0 }.face(Up), North);
        assert_eq!(Turn::from_up("south").face(Up), South);
        assert_eq!(Turn::from_up("east").face(Up), East);
        assert_eq!(Turn::from_up("west").face(Up), West);
        assert_eq!(Turn::from_up("down").face(Up), Down);
    }

    /// The direction a quad faces, from its winding, as unit steps. `None` for
    /// a tilted one.
    fn wound(corners: [[i16; 3]; 4]) -> [i32; 3] {
        let p = corners.map(|corner| corner.map(i32::from));
        let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
        let e2 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ]
    }

    #[test]
    fn baked_faces_wind_outwards_however_they_turn() {
        let box_part = [Part::solid([1, 2, 3], [9, 10, 11], North)];
        for turn in [Turn::NONE, Turn::y(1), Turn::y(3), Turn { x: 1, y: 2 }, Turn { x: 2, y: 0 }] {
            let mut quads = Vec::new();
            bake(&box_part, turn, &mut quads);
            assert_eq!(quads.len(), 6);
            for quad in quads {
                let outward: [i32; 3] = match quad.face {
                    0 => [0, -1, 0],
                    1 => [0, 1, 0],
                    2 => [0, 0, -1],
                    3 => [0, 0, 1],
                    4 => [-1, 0, 0],
                    _ => [1, 0, 0],
                };
                let normal = wound(quad.corners);
                let dot: i32 = normal.iter().zip(outward).map(|(n, o)| n * o).sum();
                assert!(dot > 0, "{turn:?}: {quad:?}");
            }
        }
    }

    #[test]
    fn a_turned_cube_keeps_its_texture_on_its_face() {
        // A furnace-like cube, its front turned from north to east: east shows
        // what north did, unturned, as a side face has no reason to spin.
        let turn = Turn::y(1);
        assert_eq!(turn.cube_face(East), (North, 0));
        // The top stays on top, a quarter turned.
        let (source, turns) = turn.cube_face(Up);
        assert_eq!(source, Up);
        assert_eq!(turns, 3);
        // A barrel tipped north: its top is on the north face, unspun.
        let tipped = Turn::from_up("north");
        assert_eq!(tipped.cube_face(North).0, Up);
        assert_eq!(tipped.cube_face(Down).0, North);
    }

    #[test]
    fn faces_on_the_blocks_edge_can_be_hidden() {
        let mut quads = Vec::new();
        bake(&[Part::solid([0, 0, 0], [16, 2, 8], North)], Turn::NONE, &mut quads);
        let cull = |face: Face| quads.iter().find(|quad| quad.face == face as u8).and_then(|quad| quad.cull);
        assert_eq!(cull(Down), Some(Down));
        assert_eq!(cull(Up), None, "its top is inside the block");
        assert_eq!(cull(South), None, "and so is its south side");
        assert_eq!(cull(North), Some(North));
    }
}
