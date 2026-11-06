//! The flood fills, over the 3 × 3 chunks around one (worth teaching).
//!
//! **Why 3 × 3 is enough.** Light loses at least 1 for every block it goes sideways, so nothing a
//! change does reaches more than 15 blocks from it: less than a chunk. A chunk that arrives, goes, or
//! has a block change can only change light in itself and its eight neighbours. [`Area`] lends their
//! light out of the store for the length of one job, so the fills index arrays instead of looking
//! chunks up in a map at every block.
//!
//! **Raising** ([`Area::raise`]) is a breadth-first flood fill. Each block on the queue offers each
//! neighbour its own level less what the neighbour dims, and a neighbour that gets brighter joins
//! the queue. Light only ever goes up, so the fill ends, and in any order it ends at the same levels.
//!
//! **Lowering** ([`Area::lower`]) is the half worth teaching. When a torch goes out, nothing says
//! which blocks it lit. So the fill takes away every level that could have come from it: from the
//! torch outwards, a neighbour darker than the level taken away, or exactly what it would have
//! given, goes to 0, and its own neighbours are looked at in turn. A neighbour at least as bright,
//! otherwise, has light from somewhere else: it goes on a second queue, and once the darkness has
//! spread, raising from that queue fills the hole back in from its edges. A block that gives out
//! light, or sees the sky, keeps its own level as it's cleared, and fills back in too.

use std::collections::VecDeque;

use super::{
    ChunkLight, Kind, Levels,
    table::{LightTable, Props},
};
use crate::world::{AIR, chunk::Chunk, storage::Storage};

/// The area's width in blocks: three chunks.
const WIDE: i32 = 48;
/// The middle chunk's slot.
pub const MIDDLE: usize = 4;

/// The six neighbours, `(dx, dy, dz)`, with down first.
const DIRECTIONS: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];

/// The level light at `level` gives a neighbour that dims by `dims`.
///
/// Every step costs at least 1, except the sky's own: 15 going down into a block that dims nothing
/// stays 15.
pub const fn spread(kind: Kind, level: u8, dims: u8, down: bool) -> u8 {
    if matches!(kind, Kind::Sky) && level == 15 && down && dims == 0 {
        15
    } else {
        level.saturating_sub(if dims > 1 { dims } else { 1 })
    }
}

/// A block's place in the area, packed in 24 bits: x and z from 0 to 47, then y from the world's
/// bottom. The queues hold millions of these in a big fill, so smaller is faster.
const fn pack(x: i32, y: i32, z: i32) -> u32 {
    (x as u32) | (z as u32) << 6 | (y as u32) << 12
}

const fn unpack(at: u32) -> (i32, i32, i32) {
    ((at & 63) as i32, (at >> 12) as i32, (at >> 6 & 63) as i32)
}

const fn slot_of(x: i32, z: i32) -> usize {
    (x >> 4) as usize * 3 + (z >> 4) as usize
}

/// A block's index in its sub-chunk's [`Levels`].
const fn index(x: i32, y: i32, z: i32) -> usize {
    ((x & 15) << 8 | (z & 15) << 4 | (y & 15)) as usize
}

/// Whether layer 1 holds water, for a sub-chunk.
enum Water<'w> {
    None,
    All,
    Some(&'w Storage, Vec<bool>),
}

/// What a sub-chunk's blocks do to light: one [`Props`] for all of it, or its palette translated
/// once, as the mesher translates looks.
enum SectionProps<'w> {
    Same(Props),
    Mixed {
        blocks: &'w Storage,
        dry: Vec<Props>,
        wet: Vec<Props>,
        water: Water<'w>,
    },
}

impl<'w> SectionProps<'w> {
    fn build(table: &LightTable, chunk: &'w Chunk, section: usize) -> Self {
        let Some(blocks) = chunk.sub_chunks.get(section).and_then(|sub| sub.layers.first()) else {
            return Self::Same(table.get(AIR, false));
        };
        let water = match chunk.sub_chunks[section].layers.get(1) {
            None | Some(Storage::Single(AIR)) => Water::None,
            Some(Storage::Single(_)) => Water::All,
            Some(storage) => Water::Some(storage, storage.palette().iter().map(|&id| id != AIR).collect()),
        };
        let dry: Vec<Props> = blocks.palette().iter().map(|&id| table.get(id, false)).collect();
        let wet: Vec<Props> = blocks.palette().iter().map(|&id| table.get(id, true)).collect();
        let same = |list: &[Props]| list.iter().all(|&props| props == list[0]);
        match water {
            Water::None if same(&dry) => Self::Same(dry[0]),
            Water::All if same(&wet) => Self::Same(wet[0]),
            Water::Some(..) if same(&dry) && same(&wet) && dry[0] == wet[0] => Self::Same(dry[0]),
            water => Self::Mixed {
                blocks,
                dry,
                wet,
                water,
            },
        }
    }

    fn get(&self, x: usize, y: usize, z: usize) -> Props {
        match self {
            Self::Same(props) => *props,
            Self::Mixed {
                blocks,
                dry,
                wet,
                water,
            } => {
                let wet_here = match water {
                    Water::None => false,
                    Water::All => true,
                    Water::Some(storage, entries) => entries[storage.palette_index(x, y, z)],
                };
                let i = blocks.palette_index(x, y, z);
                if wet_here { wet[i] } else { dry[i] }
            }
        }
    }

    /// The most light any block here gives out.
    fn most_emitted(&self) -> u8 {
        match self {
            Self::Same(props) => props.emits(),
            Self::Mixed { dry, wet, .. } => dry.iter().chain(wet).map(|props| props.emits()).max().unwrap_or(0),
        }
    }
}

/// One chunk of an [`Area`].
pub struct Slot<'w> {
    /// Its blocks. `None` while its light is being forgotten: light neither enters it nor leaves it.
    pub blocks: Option<&'w Chunk>,
    pub light: ChunkLight,
    props: Vec<Option<SectionProps<'w>>>,
    /// The sub-chunks whose light changed, or that hold a block beside a change, one bit each. Their
    /// meshes take light from those blocks once Step 14 draws it.
    pub changed: u32,
}

impl<'w> Slot<'w> {
    pub fn new(blocks: Option<&'w Chunk>, light: ChunkLight) -> Self {
        let sections = light.sky.len();
        Self {
            blocks,
            light,
            props: (0..sections).map(|_| None).collect(),
            changed: 0,
        }
    }
}

/// The 3 × 3 chunks around one, lent out of the store for one job.
///
/// Positions in it are in blocks from its lowest corner: x and z from 0 to 47, so the middle chunk
/// is 16 to 31, and y from the world's bottom.
pub struct Area<'w> {
    table: &'static LightTable,
    height: i32,
    pub slots: [Option<Slot<'w>>; 9],
}

impl<'w> Area<'w> {
    /// An area of `sections` sub-chunks a column, with its slots still to fill in.
    pub fn new(table: &'static LightTable, sections: usize) -> Self {
        Self {
            table,
            height: sections as i32 * 16,
            slots: Default::default(),
        }
    }

    const fn sections(&self) -> usize {
        (self.height / 16) as usize
    }

    /// Whether light can go into the block: inside the area and the world, in a chunk with its
    /// blocks.
    fn open(&self, x: i32, y: i32, z: i32) -> bool {
        (0..WIDE).contains(&x)
            && (0..WIDE).contains(&z)
            && (0..self.height).contains(&y)
            && self.slots[slot_of(x, z)].as_ref().is_some_and(|slot| slot.blocks.is_some())
    }

    /// The level of light of `kind` at a block: 0 in a chunk the area doesn't have.
    pub fn level(&self, kind: Kind, x: i32, y: i32, z: i32) -> u8 {
        self.slots[slot_of(x, z)]
            .as_ref()
            .map_or(0, |slot| slot.light.levels(kind)[(y >> 4) as usize].get(index(x, y, z)))
    }

    /// Sets a level, and marks every sub-chunk with a block beside it as changed.
    fn set(&mut self, kind: Kind, x: i32, y: i32, z: i32, level: u8) {
        let Some(slot) = self.slots[slot_of(x, z)].as_mut() else {
            return;
        };
        if !slot.light.levels_mut(kind)[(y >> 4) as usize].set(index(x, y, z), level) {
            return;
        }
        let (top_x, top_y, top_z) = ((x + 1).min(WIDE - 1), (y + 1).min(self.height - 1), (z + 1).min(WIDE - 1));
        for sx in ((x - 1).max(0) >> 4)..=(top_x >> 4) {
            for sz in ((z - 1).max(0) >> 4)..=(top_z >> 4) {
                if let Some(slot) = self.slots[(sx * 3 + sz) as usize].as_mut() {
                    for sy in ((y - 1).max(0) >> 4)..=(top_y >> 4) {
                        slot.changed |= 1 << sy;
                    }
                }
            }
        }
    }

    /// What the block does to light. A chunk the area lacks counts as air, though no fill ever goes
    /// there.
    fn props(&mut self, x: i32, y: i32, z: i32) -> Props {
        let table = self.table;
        let section = (y >> 4) as usize;
        let Some(slot) = self.slots[slot_of(x, z)].as_mut() else {
            return Props::AIR;
        };
        let Some(blocks) = slot.blocks else {
            return Props::AIR;
        };
        slot.props[section]
            .get_or_insert_with(|| SectionProps::build(table, blocks, section))
            .get((x & 15) as usize, (y & 15) as usize, (z & 15) as usize)
    }

    /// The sub-chunk's [`Props`], if all its blocks share them.
    fn section_same(&mut self, slot: usize, section: usize) -> Option<Props> {
        let table = self.table;
        let slot = self.slots[slot].as_mut()?;
        let blocks = slot.blocks?;
        match slot.props[section].get_or_insert_with(|| SectionProps::build(table, blocks, section)) {
            SectionProps::Same(props) => Some(*props),
            SectionProps::Mixed { .. } => None,
        }
    }

    fn most_emitted(&mut self, slot: usize, section: usize) -> u8 {
        let table = self.table;
        let Some(slot) = self.slots[slot].as_mut() else {
            return 0;
        };
        let Some(blocks) = slot.blocks else {
            return 0;
        };
        slot.props[section]
            .get_or_insert_with(|| SectionProps::build(table, blocks, section))
            .most_emitted()
    }

    /// The sky's own light at a block: 15, less what it and every block above it dim.
    fn sky_source(&mut self, x: i32, y: i32, z: i32) -> u8 {
        let slot = slot_of(x, z);
        let mut run = 15i32;
        let mut cy = y;
        while cy < self.height {
            if let Some(props) = self.section_same(slot, (cy >> 4) as usize) {
                let cells = 16 - (cy & 15);
                run -= i32::from(props.dims()) * cells;
                cy += cells;
            } else {
                run -= i32::from(self.props(x, cy, z).dims());
                cy += 1;
            }
            if run <= 0 {
                return 0;
            }
        }
        run as u8
    }

    /// The light a block keeps whatever its neighbours do.
    fn source(&mut self, kind: Kind, x: i32, y: i32, z: i32, props: Props) -> u8 {
        match kind {
            Kind::Block => props.emits(),
            Kind::Sky => self.sky_source(x, y, z),
        }
    }

    /// The lowest y from which a column dims nothing, all the way up: the sky's light is 15 there.
    fn open_from(&mut self, x: i32, z: i32) -> i32 {
        let slot = slot_of(x, z);
        let mut y = self.height - 1;
        while y >= 0 {
            let section = (y >> 4) as usize;
            if self.section_same(slot, section).is_some_and(|props| props.dims() == 0) {
                y = section as i32 * 16 - 1;
                continue;
            }
            if self.props(x, y, z).dims() > 0 {
                return y + 1;
            }
            y -= 1;
        }
        0
    }

    /// Raises light from every block on the queue, until nothing gets brighter.
    pub fn raise(&mut self, kind: Kind, queue: &mut VecDeque<u32>) {
        while let Some(at) = queue.pop_front() {
            let (x, y, z) = unpack(at);
            let level = self.level(kind, x, y, z);
            if level <= 1 {
                continue;
            }
            for (dx, dy, dz) in DIRECTIONS {
                let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                if !self.open(nx, ny, nz) {
                    continue;
                }
                let down = dy < 0;
                // The most it could get, before asking what the block is.
                let most = if matches!(kind, Kind::Sky) && level == 15 && down { 15 } else { level - 1 };
                let there = self.level(kind, nx, ny, nz);
                if there >= most {
                    continue;
                }
                let new = spread(kind, level, self.props(nx, ny, nz).dims(), down);
                if new > there {
                    self.set(kind, nx, ny, nz, new);
                    if new > 1 {
                        queue.push_back(pack(nx, ny, nz));
                    }
                }
            }
        }
    }

    /// Takes away every level that could have come from the blocks on `removals`, each with the
    /// level it had, and puts the edges of the hole on `refill`, for [`Area::raise`].
    pub fn lower(&mut self, kind: Kind, removals: &mut VecDeque<(u32, u8)>, refill: &mut VecDeque<u32>) {
        while let Some((at, old)) = removals.pop_front() {
            let (x, y, z) = unpack(at);
            for (dx, dy, dz) in DIRECTIONS {
                let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                if !self.open(nx, ny, nz) {
                    continue;
                }
                let there = self.level(kind, nx, ny, nz);
                if there == 0 {
                    continue;
                }
                let next = pack(nx, ny, nz);
                let props = self.props(nx, ny, nz);
                if there >= old && there != spread(kind, old, props.dims(), dy < 0) {
                    // Brighter than this light could make it: lit from somewhere else.
                    refill.push_back(next);
                    continue;
                }
                let source = self.source(kind, nx, ny, nz, props);
                if source >= there {
                    // Its own light is all it has.
                    refill.push_back(next);
                    continue;
                }
                self.set(kind, nx, ny, nz, source);
                if source > 1 {
                    refill.push_back(next);
                }
                removals.push_back((next, there));
            }
        }
    }

    /// Lights the middle chunk, which has just arrived: the sky down every column, the blocks that
    /// give out light, and the light its lit neighbours have at its borders. It lights them in turn.
    pub fn light_middle(&mut self) {
        let sections = self.sections();
        // From the top down, the sub-chunks that dim nothing anywhere are open sky.
        let mut open = sections;
        while open > 0 && self.section_same(MIDDLE, open - 1).is_some_and(|props| props.dims() == 0) {
            open -= 1;
        }
        if let Some(slot) = self.slots[MIDDLE].as_mut() {
            for (section, levels) in slot.light.sky.iter_mut().enumerate() {
                *levels = Levels::Uniform(if section >= open { 15 } else { 0 });
            }
            slot.light.block.fill(Levels::Uniform(0));
        }

        // The sky: straight down each column, losing only what each block dims.
        let mut tops = [[0i32; 16]; 16];
        for lx in 0..16 {
            for lz in 0..16 {
                let (x, z) = (16 + lx, 16 + lz);
                let mut top = None;
                let mut run = 15u8;
                let mut y = open as i32 * 16 - 1;
                while y >= 0 {
                    let dims = self.props(x, y, z).dims();
                    if dims > 0 && top.is_none() {
                        top = Some(y + 1);
                    }
                    run = run.saturating_sub(dims);
                    if run == 0 {
                        break;
                    }
                    self.set(Kind::Sky, x, y, z, run);
                    y -= 1;
                }
                tops[lx as usize][lz as usize] = top.unwrap_or(0);
            }
        }

        // It spreads sideways only below a neighbouring column's top: above, that column has 15.
        let mut sky = VecDeque::new();
        for lx in 0..16i32 {
            for lz in 0..16i32 {
                let (x, z) = (16 + lx, 16 + lz);
                let mut beside = 0;
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let (bx, bz) = (lx + dx, lz + dz);
                    let top = if (0..16).contains(&bx) && (0..16).contains(&bz) {
                        tops[bx as usize][bz as usize]
                    } else if self.open(x + dx, 0, z + dz) {
                        self.open_from(x + dx, z + dz)
                    } else {
                        0
                    };
                    beside = beside.max(top);
                }
                let mut y = beside - 1;
                while y >= 0 {
                    let level = self.level(Kind::Sky, x, y, z);
                    if level == 0 {
                        break;
                    }
                    if level > 1 {
                        sky.push_back(pack(x, y, z));
                    }
                    y -= 1;
                }
            }
        }

        // The blocks that give out light.
        let mut block = VecDeque::new();
        for section in 0..sections {
            if self.most_emitted(MIDDLE, section) == 0 {
                continue;
            }
            let base = section as i32 * 16;
            for x in 16..32 {
                for z in 16..32 {
                    for y in base..base + 16 {
                        let emits = self.props(x, y, z).emits();
                        if emits > 0 {
                            self.set(Kind::Block, x, y, z, emits);
                            if emits > 1 {
                                block.push_back(pack(x, y, z));
                            }
                        }
                    }
                }
            }
        }

        // The light at the neighbours' borders comes in.
        self.borders_into_middle(Kind::Sky, &mut sky);
        self.borders_into_middle(Kind::Block, &mut block);
        self.raise(Kind::Sky, &mut sky);
        self.raise(Kind::Block, &mut block);
    }

    /// Each pair of blocks either side of the middle chunk's border with a neighbour it has:
    /// `(middle, neighbour)`, a column at a time.
    fn border_columns() -> impl Iterator<Item = ((i32, i32), (i32, i32))> {
        (16..32).flat_map(|a| {
            [
                ((16, a), (15, a)), // west
                ((31, a), (32, a)), // east
                ((a, 16), (a, 15)), // north
                ((a, 31), (a, 32)), // south
            ]
        })
    }

    /// Queues each neighbour's border block whose light would brighten the middle chunk's.
    fn borders_into_middle(&mut self, kind: Kind, queue: &mut VecDeque<u32>) {
        for ((mx, mz), (nx, nz)) in Self::border_columns() {
            if !self.open(nx, 0, nz) {
                continue;
            }
            let slot = slot_of(nx, nz);
            let mut y = 0;
            while y < self.height {
                let section = (y >> 4) as usize;
                if let Some(Levels::Uniform(level)) =
                    self.slots[slot].as_ref().map(|slot| &slot.light.levels(kind)[section])
                    && *level <= 1
                {
                    y = (section as i32 + 1) * 16;
                    continue;
                }
                let there = self.level(kind, nx, y, nz);
                if there > 1 && there - 1 > self.level(kind, mx, y, mz) {
                    let new = spread(kind, there, self.props(mx, y, mz).dims(), false);
                    if new > self.level(kind, mx, y, mz) {
                        queue.push_back(pack(nx, y, nz));
                    }
                }
                y += 1;
            }
        }
    }

    /// Takes the middle chunk's light out of its neighbours: it's gone, or about to be replaced.
    /// Its own light is dropped afterwards; its slot has no blocks, so no fill goes into it.
    pub fn forget_middle(&mut self) {
        for kind in [Kind::Sky, Kind::Block] {
            let mut removals = VecDeque::new();
            for ((mx, mz), (nx, nz)) in Self::border_columns() {
                if !self.open(nx, 0, nz) {
                    continue;
                }
                let slot = slot_of(nx, nz);
                let mut y = 0;
                while y < self.height {
                    let section = (y >> 4) as usize;
                    let theirs = self.slots[slot].as_ref().map(|slot| &slot.light.levels(kind)[section]);
                    let ours = self.slots[MIDDLE].as_ref().map(|slot| &slot.light.levels(kind)[section]);
                    // Nothing to take where they're dark, or where both are open sky, which is its own.
                    let skip = match (theirs, ours) {
                        (Some(Levels::Uniform(0)), _) => true,
                        (Some(Levels::Uniform(15)), Some(Levels::Uniform(15))) => matches!(kind, Kind::Sky),
                        _ => false,
                    };
                    if skip {
                        y = (section as i32 + 1) * 16;
                        continue;
                    }
                    let level = self.level(kind, mx, y, mz);
                    if level > 0 && self.level(kind, nx, y, nz) > 0 {
                        removals.push_back((pack(mx, y, mz), level));
                    }
                    y += 1;
                }
            }
            let mut refill = VecDeque::new();
            self.lower(kind, &mut removals, &mut refill);
            self.raise(kind, &mut refill);
        }
    }

    /// Lights the middle chunk again where its blocks changed: each block, and below it the column
    /// the sky comes down. `cells` are in area positions, in the middle chunk.
    pub fn relight(&mut self, cells: &[(i32, i32, i32)]) {
        let (mut removals, mut refill) = (VecDeque::new(), VecDeque::new());
        for &(x, y, z) in cells {
            let old = self.level(Kind::Block, x, y, z);
            let emits = self.props(x, y, z).emits();
            self.set(Kind::Block, x, y, z, emits);
            // Even from 0, so that the neighbours' light comes back into an opened block.
            removals.push_back((pack(x, y, z), old));
            if emits > 1 {
                refill.push_back(pack(x, y, z));
            }
        }
        self.lower(Kind::Block, &mut removals, &mut refill);
        self.raise(Kind::Block, &mut refill);

        for &(x, y, z) in cells {
            let mut source = self.sky_source(x, y, z);
            let mut cy = y;
            loop {
                let old = self.level(Kind::Sky, x, cy, z);
                // Below the first block that was dark and still is, the column's light is the same.
                if cy < y && old == 0 && source == 0 {
                    break;
                }
                self.set(Kind::Sky, x, cy, z, source);
                removals.push_back((pack(x, cy, z), old));
                if source > 1 {
                    refill.push_back(pack(x, cy, z));
                }
                if cy == 0 {
                    break;
                }
                cy -= 1;
                source = source.saturating_sub(self.props(x, cy, z).dims());
            }
        }
        self.lower(Kind::Sky, &mut removals, &mut refill);
        self.raise(Kind::Sky, &mut refill);
    }
}
