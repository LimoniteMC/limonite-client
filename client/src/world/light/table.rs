//! What each block does to light, from Pumpkin's data (m5-plan Finding 9).
//!
//! **Two numbers a block** ([`Props`]): how much light it gives out, and how much it dims light
//! coming into it. They're Pumpkin's own, so the client's light follows the same rules as the
//! server's (`pumpkin-world/src/lighting/engine.rs`):
//! - **gives out:** the Java state's luminance, such as 14 for a torch and 15 for lava;
//! - **dims:** the state's opacity, but at least 1 for a block that can occlude. So slabs, stairs,
//!   fences, carpets and snow layers dim by 1, though their opacity is 0, and leaves, water and ice
//!   by 1 too. Glass and air dim nothing; stone, dirt and glowstone everything.
//!
//! **From a Bedrock block to Java's numbers** (worth teaching). The server keeps Java states, and a
//! Bedrock client sees each as a block id on layer 0, and water on layer 1 when it's waterlogged. So
//! the table is keyed by both: the id, and whether layer 1 holds water. Of Pumpkin's 30,154 keys,
//! only 13 mean more than one light: whether a piston is pushed out, which Bedrock keeps in the
//! piston's block actor, and a test block. Those take the state nearest the block's default, as
//! shapes do (`World::blocks().shape_state`): a piston drawn pulled in, and dimming like one.
//!
//! A waterlogged block nearly always dims as much as the stronger of itself and water, but not quite
//! always: a waterlogged copper grate still dims nothing. Keying by layer 1 gets every one right
//! without knowing which.

use std::{
    collections::HashMap,
    sync::OnceLock,
};

use pumpkin_data::{Block, BlockState, BlockStateId};
use pumpkin_world::chunk::palette::bedrock_water_state;
use tracing::debug;

use crate::world::nearest_default;

/// What a block does to light: the light it gives out, and how much it dims light coming into it,
/// each 0 to 15, packed in one byte (dims low).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Props(u8);

impl Props {
    /// Air: gives out nothing, dims nothing.
    pub const AIR: Self = Self(0);

    #[must_use]
    pub const fn new(dims: u8, emits: u8) -> Self {
        Self((dims & 15) | (emits & 15) << 4)
    }

    /// How much it takes from light coming into it.
    #[must_use]
    pub const fn dims(self) -> u8 {
        self.0 & 15
    }

    /// The light it gives out.
    #[must_use]
    pub const fn emits(self) -> u8 {
        self.0 >> 4
    }

    /// A Java state's numbers, as Pumpkin's light engine reads them.
    #[must_use]
    pub const fn of_state(id: BlockStateId) -> Self {
        let state = id.to_state();
        let dims = if state.can_occlude() && state.opacity == 0 { 1 } else { state.opacity };
        Self::new(dims, state.luminance)
    }
}

/// [`Props`] for every block the server can send, by Bedrock id and whether layer 1 holds water.
pub struct LightTable {
    props: HashMap<(u32, bool), Props>,
    water: Props,
    /// Keys whose Java states disagree, and which one won.
    ambiguous: Vec<((u32, bool), BlockStateId)>,
}

/// The table, built the first time it's asked for. It's the same in every world, and every test.
pub fn table() -> &'static LightTable {
    static TABLE: OnceLock<LightTable> = OnceLock::new();
    TABLE.get_or_init(LightTable::build)
}

impl LightTable {
    fn build() -> Self {
        let mut groups: HashMap<(u32, bool), Vec<BlockStateId>> = HashMap::new();
        for raw in 0..BlockStateId::COUNT {
            let Some(id) = BlockStateId::new(raw) else {
                continue;
            };
            let wet = bedrock_water_state(id) != Block::AIR.default_state.id;
            groups.entry((BlockState::to_be_network_id(id), wet)).or_default().push(id);
        }
        let mut ambiguous = Vec::new();
        let props = groups
            .into_iter()
            .map(|(key, states)| {
                let chosen = nearest_default(&states);
                let props = Props::of_state(chosen);
                if states.iter().any(|&state| Props::of_state(state) != props) {
                    ambiguous.push((key, chosen));
                }
                (key, props)
            })
            .collect();
        ambiguous.sort_unstable_by_key(|(key, _)| *key);
        let table = Self {
            props,
            water: Props::of_state(Block::WATER.default_state.id),
            ambiguous,
        };
        debug!(
            "Light table: {} keys, {} with Java states that disagree",
            table.props.len(),
            table.ambiguous.len()
        );
        table
    }

    /// What the block with Bedrock id `id` does to light, with water on layer 1 or not.
    ///
    /// A key the server never sends, such as a block the player's own edit put in water, gets the
    /// block's numbers with water's dimming, and an id nobody knows dims nothing.
    #[must_use]
    pub fn get(&self, id: u32, wet: bool) -> Props {
        if let Some(&props) = self.props.get(&(id, wet)) {
            return props;
        }
        let block = self
            .props
            .get(&(id, false))
            .or_else(|| self.props.get(&(id, true)))
            .copied()
            .unwrap_or(Props::AIR);
        if wet {
            Props::new(block.dims().max(self.water.dims()), block.emits())
        } else {
            block
        }
    }

    /// The keys whose Java states disagree, each with the state whose numbers it takes.
    #[cfg(test)]
    #[must_use]
    pub fn ambiguous(&self) -> &[((u32, bool), BlockStateId)] {
        &self.ambiguous
    }
}
