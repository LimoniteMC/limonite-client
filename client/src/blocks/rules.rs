//! Which textures and which shape a block state shows (m5-plan Step 7b), for
//! the blocks whose `blocks.json` entry doesn't simply give a texture per face.
//!
//! **Bedrock's engine picks by state** (worth teaching). Its entry for pointed
//! dripstone lists the five stages in five of the six face slots, and each stage
//! has a hanging and a standing variant in `terrain_texture.json`. The engine
//! knows which to draw, and the packs don't say. So a client needs a rule for
//! each such block, and the test at the end of `appearance/tests.rs` walks every
//! block Pumpkin can send, so a new one can't slip through.
//!
//! **Which way a block turns comes from its Java state**, the one Pumpkin's table
//! gives for its Bedrock id, and the one physics collides with. Java's states say
//! `facing=east` the same way for every block, where Bedrock's use several
//! numberings. Texture variants, on the other hand, follow Bedrock's own
//! properties, since it's Bedrock's lists they index.

use std::collections::HashMap;

use limonite_packs::registry::{
    Face::{self, Down, East, North, South, Up, West},
    FaceTextures,
};
use pumpkin_data::{Block, BlockStateId};

use super::{
    appearance::Shape,
    models::{self, ModelQuad, Part, Turn, bake},
    palette::BedrockState,
};
use crate::render::tint::Tint;

/// A Java block state: its block's name, and its properties.
pub struct JavaBlock {
    pub name: &'static str,
    props: Vec<(&'static str, &'static str)>,
}

impl JavaBlock {
    #[must_use]
    pub fn of(state: BlockStateId) -> Self {
        let block = Block::from_state_id(state);
        Self {
            name: block.name,
            props: block
                .properties(state)
                .map(|props| props.to_props().into_iter().map(|p| (p.key().to_str(), p.value_str())).collect())
                .unwrap_or_default(),
        }
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&'static str> {
        self.props.iter().find(|(name, _)| *name == key).map(|&(_, value)| value)
    }

    fn is(&self, key: &str, value: &str) -> bool {
        self.get(key) == Some(value)
    }

    fn flag(&self, key: &str) -> bool {
        self.is(key, "true")
    }

    fn int(&self, key: &str) -> Option<usize> {
        self.get(key)?.parse().ok()
    }

    fn facing(&self) -> &'static str {
        self.get("facing").unwrap_or("north")
    }
}

/// What one of a look's six slots shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pick<'a> {
    /// A key of `terrain_texture.json`.
    pub key: Option<&'a str>,
    /// Which of its variants.
    pub variant: usize,
    /// Quarter turns of the texture, on a cube's face.
    pub turns: u8,
    /// Whether it takes the block's tint. `None`: whatever the tables say.
    pub tinted: Option<bool>,
}

impl<'a> Pick<'a> {
    /// Variant 0 of a key.
    #[must_use]
    pub const fn key(key: Option<&'a str>) -> Self {
        Self {
            key,
            variant: 0,
            turns: 0,
            tinted: None,
        }
    }

    const fn variant(mut self, variant: usize) -> Self {
        self.variant = variant;
        self
    }

    const fn tinted(mut self, tinted: bool) -> Self {
        self.tinted = Some(tinted);
        self
    }
}

/// What a rule says about a state.
pub struct Rule<'a> {
    pub shape: Shape,
    pub slots: [Pick<'a>; 6],
    /// The tint, if the tables don't know it.
    pub tint: Option<Tint>,
}

/// The models baked so far, each once. They live as long as the program, like
/// the table that points at them, so they're leaked rather than counted.
#[derive(Default)]
pub struct Models {
    seen: HashMap<Vec<ModelQuad>, &'static [ModelQuad]>,
}

impl Models {
    fn shape(&mut self, parts: &[(&[Part], Turn)]) -> Shape {
        let mut quads = Vec::new();
        for (parts, turn) in parts {
            bake(parts, *turn, &mut quads);
        }
        let quads = match self.seen.get(&quads) {
            Some(&baked) => baked,
            None => {
                let baked: &'static [ModelQuad] = quads.clone().leak();
                self.seen.insert(quads, baked);
                baked
            }
        };
        Shape::Model { quads }
    }

    /// One list of parts, turned.
    fn one(&mut self, parts: &[Part], turn: Turn) -> Shape {
        self.shape(&[(parts, turn)])
    }

    /// A post and one arm per connected side: north, east, south, west. A side
    /// with no arm gets `closed`.
    fn connected(&mut self, post: &[Part], arm: &[Part], closed: &[Part], sides: [bool; 4]) -> Shape {
        let mut parts = vec![(post, Turn::NONE)];
        for (y, connected) in (0u8..).zip(sides) {
            parts.push((if connected { arm } else { closed }, Turn::y(y)));
        }
        self.shape(&parts)
    }
}

/// A block's own key for each slot.
fn own(textures: &FaceTextures) -> [Pick<'_>; 6] {
    Face::ALL.map(|face| Pick::key(textures.key(face)))
}

/// Every slot on the same variant of its own key.
fn all_variant(textures: &FaceTextures, variant: usize) -> [Pick<'_>; 6] {
    own(textures).map(|pick| pick.variant(variant))
}

/// A cube's six faces, turned with the block: each shows the face it came from.
fn turned(layout: [Pick<'_>; 6], turn: Turn) -> [Pick<'_>; 6] {
    Face::ALL.map(|face| {
        let (source, turns) = turn.cube_face(face);
        let mut pick = layout[source as usize];
        pick.turns = (pick.turns + turns) % 4;
        pick
    })
}

/// The rule for a state, if it has one.
#[must_use]
pub fn rule<'a>(
    state: &'a BedrockState,
    java: Option<&JavaBlock>,
    textures: &'a FaceTextures,
    models: &mut Models,
) -> Option<Rule<'a>> {
    by_bedrock_state(state, textures).or_else(|| by_java_state(state, java?, textures, models))
}

fn cube(slots: [Pick<'_>; 6]) -> Option<Rule<'_>> {
    Some(Rule {
        shape: Shape::Cube,
        slots,
        tint: None,
    })
}

/// Rules that need only Bedrock's own properties: what indexes its lists.
fn by_bedrock_state<'a>(state: &'a BedrockState, textures: &'a FaceTextures) -> Option<Rule<'a>> {
    let name = &*state.name;
    let int = |key: &str| usize::try_from(state.int(key).unwrap_or(0)).unwrap_or(0);
    let flag = |key: &str| state.flag(key).unwrap_or(false);
    match name {
        // Each face's list is indexed by these bits: pores, cap or stem.
        "brown_mushroom_block" | "red_mushroom_block" | "mushroom_stem" => {
            cube(all_variant(textures, int("huge_mushroom_bits")))
        }
        "respawn_anchor" => cube(all_variant(textures, int("respawn_anchor_charge"))),
        "frosted_ice" => cube(all_variant(textures, int("age"))),
        "suspicious_sand" | "suspicious_gravel" => cube(all_variant(textures, int("brushed_progress"))),
        "chorus_flower" => cube(all_variant(textures, usize::from(int("age") >= 5))),
        "sculk_catalyst" => cube(all_variant(textures, usize::from(flag("bloom")))),
        "structure_block" => {
            let variant = match state.string("structure_block_type") {
                Some("data") => 1,
                Some("save") => 2,
                Some("load") => 3,
                Some("corner") => 4,
                Some("export") => 5,
                _ => 0,
            };
            cube(all_variant(textures, variant))
        }
        "trial_spawner" => {
            let ominous = if flag("ominous") { 6 } else { 0 };
            cube(all_variant(textures, int("trial_spawner_state") + ominous))
        }
        "farmland" => {
            // Listed wet first.
            let mut slots = own(textures);
            slots[Up as usize] = slots[Up as usize].variant(usize::from(int("moisturized_amount") < 7));
            cube(slots)
        }
        "cake" => {
            // The cut side shows on the west.
            let mut slots = own(textures);
            slots[West as usize] = slots[West as usize].variant(usize::from(int("bite_counter") > 0));
            cube(slots)
        }
        "daylight_detector" => cube(all_variant(textures, 0)),
        "daylight_detector_inverted" => cube(all_variant(textures, 1)),
        // The side, and the base above and below.
        "decorated_pot" => {
            let mut slots = own(textures);
            for face in [Up, Down] {
                slots[face as usize] = slots[face as usize].variant(1);
            }
            cube(slots)
        }
        // An uncarved pumpkin: its face's list ends in a plain side.
        "pumpkin" => {
            let mut slots = own(textures);
            slots[South as usize] = slots[South as usize].variant(2);
            cube(slots)
        }
        // The stages, in five of the slots: Bedrock's list for each is
        // hanging, then standing. Crossed planes, as Java's model is.
        "pointed_dripstone" | "sulfur_spike" => {
            let slot = match state.string("dripstone_thickness") {
                Some("tip") => North,
                Some("frustum") => Down,
                Some("middle") => South,
                Some("merge") => West,
                _ => Up,
            };
            let hanging = flag("hanging");
            let pick = Pick::key(textures.key(slot)).variant(usize::from(!hanging));
            Some(Rule {
                shape: Shape::Cross,
                slots: [pick; 6],
                tint: None,
            })
        }
        "pale_hanging_moss" => Some(Rule {
            shape: Shape::Cross,
            slots: all_variant(textures, usize::from(flag("tip"))),
            tint: None,
        }),
        "cave_vines_body_with_berries" | "cave_vines_head_with_berries" => Some(Rule {
            shape: Shape::Cross,
            slots: all_variant(textures, 1),
            tint: None,
        }),
        "mangrove_propagule" => Some(Rule {
            shape: Shape::Cross,
            slots: all_variant(textures, usize::from(flag("hanging"))),
            tint: None,
        }),
        _ => None,
    }
}

/// Which way a chest or furnace faces: its front in North of `layout`, then
/// turned to its Java facing.
fn fronted<'a>(layout: [Pick<'a>; 6], java: &JavaBlock) -> Option<Rule<'a>> {
    cube(turned(layout, Turn::from_north(java.facing())))
}

/// A slot's layout with `front` on the north, and the plain `side` on the rest.
fn with_front<'a>(textures: &'a FaceTextures, front: Pick<'a>, side: Pick<'a>) -> [Pick<'a>; 6] {
    let mut layout = own(textures);
    for face in [South, West, East] {
        layout[face as usize] = side;
    }
    layout[North as usize] = front;
    layout
}

/// A model's rule: its shape, and its slots as the block's entry has them.
fn modelled<'a>(shape: Shape, slots: [Pick<'a>; 6]) -> Option<Rule<'a>> {
    Some(Rule {
        shape,
        slots,
        tint: None,
    })
}

/// Java's colour for redstone wire at a power level.
fn wire_color(power: usize) -> [u8; 3] {
    let f = power.min(15) as f32 / 15.0;
    let r = f * 0.6 + if f > 0.0 { 0.4 } else { 0.3 };
    let g = (f * f * 0.7 - 0.5).clamp(0.0, 1.0);
    let b = (f * f * 0.6 - 0.7).clamp(0.0, 1.0);
    [r, g, b].map(|c| (c * 255.0) as u8)
}

/// Rules that follow a Java state: what turns, and what's shaped by its state.
#[allow(clippy::too_many_lines)]
fn by_java_state<'a>(
    state: &'a BedrockState,
    java: &JavaBlock,
    textures: &'a FaceTextures,
    models: &mut Models,
) -> Option<Rule<'a>> {
    let bedrock = &*state.name;
    let name = java.name;
    let facing = java.facing();
    let key = |face: Face| Pick::key(textures.key(face));
    let slots = own(textures);

    // Cubes with a front.
    match name {
        "furnace" | "blast_furnace" | "smoker" => {
            // Lit ones are other Bedrock names, which keep it in another slot.
            let front = if bedrock.starts_with("lit_") { East } else { South };
            return fronted(with_front(textures, key(front), key(North)), java);
        }
        "carved_pumpkin" => return fronted(with_front(textures, key(South), key(North)), java),
        "jack_o_lantern" => return fronted(with_front(textures, key(South).variant(1), key(North)), java),
        "beehive" | "bee_nest" => {
            let full = java.is("honey_level", "5");
            return fronted(with_front(textures, key(South).variant(usize::from(full)), key(North)), java);
        }
        "loom" | "vault" | "chiseled_bookshelf" => {
            let variant = match name {
                "vault" => {
                    let state = match java.get("vault_state") {
                        Some("active") => 1,
                        Some("unlocking") => 2,
                        Some("ejecting") => 3,
                        _ => 0,
                    };
                    state + if java.flag("ominous") { 4 } else { 0 }
                }
                "chiseled_bookshelf" => usize::from(
                    (0..6).any(|slot| java.flag(&format!("slot_{slot}_occupied"))),
                ),
                _ => 0,
            };
            return fronted(slots.map(|pick| pick.variant(variant)), java);
        }
        "barrel" => {
            // Standing up, then tipped as it faces: its entry keeps the top in
            // north, the bottom in south, and the staves everywhere else.
            let mut layout = own(textures);
            layout[Up as usize] = key(North).variant(usize::from(java.flag("open")));
            layout[Down as usize] = key(South);
            for face in [North, South, West, East] {
                layout[face as usize] = key(West);
            }
            return cube(turned(layout, Turn::from_up(facing)));
        }
        "piston" | "sticky_piston" => {
            let mut layout = slots;
            if name == "sticky_piston" {
                layout[Up as usize] = Pick::key(Some("piston_top_sticky"));
            }
            return cube(turned(layout, Turn::from_up(facing)));
        }
        "observer" => {
            let mut layout = slots;
            layout[South as usize] = layout[South as usize].variant(usize::from(java.flag("powered")));
            let turn = match facing {
                "up" => Turn { x: 3, y: 0 },
                "down" => Turn { x: 1, y: 0 },
                other => Turn::from_north(other),
            };
            return cube(turned(layout, turn));
        }
        "dispenser" | "dropper" => {
            // Its entry keeps the sideways front in south, the upward one in
            // east. Facing up or down, its top's texture wraps the other faces.
            if matches!(facing, "up" | "down") {
                let end = if facing == "up" { Up } else { Down };
                let mut layout = [key(Up); 6];
                layout[end as usize] = key(East);
                return cube(layout);
            }
            return fronted(with_front(textures, key(South), key(North)), java);
        }
        "command_block" | "chain_command_block" | "repeating_command_block" => {
            let side = if java.flag("conditional") { key(Up) } else { key(West) };
            let mut layout = [side; 6];
            layout[Up as usize] = key(North);
            layout[Down as usize] = key(South);
            return cube(turned(layout, Turn::from_up(facing)));
        }
        "crafter" => {
            let variant = if java.flag("crafting") {
                2
            } else {
                usize::from(java.flag("triggered"))
            };
            let orientation = java.get("orientation").unwrap_or("north_up");
            let (front, top) = orientation.split_once('_').unwrap_or(("north", "up"));
            let turn = match front {
                "up" => Turn {
                    x: 3,
                    y: Turn::from_north(top).y,
                },
                "down" => Turn {
                    x: 1,
                    y: Turn::from_north(top).y,
                },
                other => Turn::from_north(other),
            };
            return cube(turned(slots.map(|pick| pick.variant(variant)), turn));
        }
        "creaking_heart" => {
            let variant = match java.get("creaking_heart_state") {
                Some("dormant") => 1,
                Some("awake") => 2,
                _ => 0,
            };
            let turn = match java.get("axis") {
                Some("x") => Turn { x: 1, y: 1 },
                Some("z") => Turn { x: 1, y: 0 },
                _ => Turn::NONE,
            };
            return cube(turned(slots.map(|pick| pick.variant(variant)), turn));
        }
        "sniffer_egg" => {
            return cube(slots.map(|pick| pick.variant(java.int("hatch").unwrap_or(0))));
        }
        "dried_ghast" => {
            let layout = slots.map(|pick| pick.variant(java.int("hydration").unwrap_or(0)));
            return cube(turned(layout, Turn::from_south(facing)));
        }
        _ => {}
    }
    if name.ends_with("copper_bulb") {
        let variant = usize::from(java.flag("lit")) + 2 * usize::from(java.flag("powered"));
        return cube(slots.map(|pick| pick.variant(variant)));
    }
    if name.ends_with("glazed_terracotta") {
        return cube(turned(slots, Turn::from_south(facing)));
    }
    // A chest's front is in its south slot. These stay boxes until M8's models.
    if name.ends_with("chest") {
        return fronted(with_front(textures, key(South), key(North)), java);
    }

    // Models.
    if name.ends_with("_door") {
        let upper = java.is("half", "upper");
        // The seven older doors share keys, with a variant each.
        let variant = match bedrock {
            "spruce_door" => 1,
            "birch_door" => 2,
            "jungle_door" => 3,
            "acacia_door" => 4,
            "dark_oak_door" => 5,
            "iron_door" => 6,
            _ => 0,
        };
        let pick = key(if upper { North } else { Down }).variant(variant);
        let (open, right) = (java.flag("open"), java.is("hinge", "right"));
        let closed = Turn::from_east(facing);
        let turn = match (open, right) {
            (false, _) => closed,
            (true, false) => Turn::y(closed.y + 1),
            (true, true) => Turn::y(closed.y + 3),
        };
        let parts = if open == right { models::DOOR_HINGE_NORTH } else { models::DOOR_HINGE_SOUTH };
        return modelled(models.one(parts, turn), [pick; 6]);
    }
    if name.ends_with("_fence") {
        let sides = ["north", "east", "south", "west"].map(|side| java.flag(side));
        return modelled(models.connected(models::FENCE_POST, models::FENCE_SIDE, &[], sides), slots);
    }
    if name.ends_with("_fence_gate") {
        let parts = match (java.flag("open"), java.flag("in_wall")) {
            (false, false) => models::GATE,
            (false, true) => models::GATE_WALL,
            (true, false) => models::GATE_OPEN,
            (true, true) => models::GATE_OPEN_WALL,
        };
        return modelled(models.one(parts, Turn::from_south(facing)), slots);
    }
    if name.ends_with("glass_pane") {
        let sides = ["north", "east", "south", "west"].map(|side| java.flag(side));
        return modelled(
            models.connected(models::PANE_POST, models::PANE_SIDE, models::PANE_NO_SIDE, sides),
            slots,
        );
    }
    if name.ends_with("_bars") {
        let sides = ["north", "east", "south", "west"].map(|side| java.flag(side));
        return modelled(models.connected(models::BARS_POST, models::BARS_SIDE, &[], sides), slots);
    }
    if name.ends_with("rail") {
        let shape = java.get("shape").unwrap_or("north_south");
        let (parts, turn) = match shape {
            "east_west" => (models::RAIL_FLAT, Turn::y(1)),
            "south_east" => (models::RAIL_FLAT, Turn::NONE),
            "south_west" => (models::RAIL_FLAT, Turn::y(1)),
            "north_west" => (models::RAIL_FLAT, Turn::y(2)),
            "north_east" => (models::RAIL_FLAT, Turn::y(3)),
            "ascending_north" => (models::RAIL_RAISED, Turn::NONE),
            "ascending_east" => (models::RAIL_RAISED, Turn::y(1)),
            "ascending_south" => (models::RAIL_RAISED, Turn::y(2)),
            "ascending_west" => (models::RAIL_RAISED, Turn::y(3)),
            _ => (models::RAIL_FLAT, Turn::NONE),
        };
        // Its entry's up slot holds the texture for a curve, or for power.
        let curved = matches!(shape, "south_east" | "south_west" | "north_west" | "north_east");
        let pick = key(if curved || java.flag("powered") { Up } else { Down });
        return modelled(models.one(parts, turn), [pick; 6]);
    }
    if name.ends_with("torch") {
        let shape = if name.contains("wall") {
            models.one(models::TORCH_WALL, Turn::from_east(facing))
        } else {
            models.one(models::TORCH, Turn::NONE)
        };
        return modelled(shape, slots);
    }
    if name.ends_with("lantern") && java.get("hanging").is_some() {
        let parts = if java.flag("hanging") { models::LANTERN_HANGING } else { models::LANTERN };
        return modelled(models.one(parts, Turn::NONE), slots);
    }
    if name.ends_with("chain") {
        let turn = match java.get("axis") {
            Some("x") => Turn { x: 1, y: 1 },
            Some("z") => Turn { x: 1, y: 0 },
            _ => Turn::NONE,
        };
        return modelled(models.one(models::CHAIN, turn), slots);
    }
    if name.ends_with("candle_cake") {
        let candle = bedrock.strip_suffix("_cake").unwrap_or("candle");
        let mut slots = slots;
        slots[South as usize] = Pick::key(Some(candle)).variant(usize::from(java.flag("lit")));
        return modelled(models.one(models::CANDLE_CAKE, Turn::NONE), slots);
    }
    if name.ends_with("candle") {
        let count = java.int("candles").unwrap_or(1).clamp(1, 4);
        let variant = usize::from(java.flag("lit"));
        return modelled(
            models.one(models::CANDLES[count - 1], Turn::NONE),
            slots.map(|pick| pick.variant(variant)),
        );
    }
    if name.ends_with("lightning_rod") {
        let variant = usize::from(java.flag("powered"));
        return modelled(
            models.one(models::LIGHTNING_ROD, Turn::from_up(facing)),
            slots.map(|pick| pick.variant(variant)),
        );
    }
    if name.ends_with("amethyst_bud") || name == "amethyst_cluster" {
        return modelled(models.one(models::CROSS, Turn::from_up(facing)), slots);
    }
    if name.ends_with("coral_wall_fan") {
        return modelled(models.one(models::CORAL_WALL_FAN, Turn::from_north(facing)), slots);
    }
    if name.ends_with("anvil") {
        return modelled(models.one(models::ANVIL, Turn::from_south(facing)), slots);
    }
    if name.ends_with("campfire") {
        let lit = java.flag("lit");
        let mut slots = slots;
        slots[North as usize] = key(Down);
        slots[South as usize] = if lit { key(North) } else { key(Down) };
        let turn = Turn::from_south(facing);
        let shape = if lit {
            models.shape(&[(models::CAMPFIRE, turn), (models::CAMPFIRE_FLAMES, Turn::NONE)])
        } else {
            models.one(models::CAMPFIRE, turn)
        };
        return modelled(shape, slots);
    }

    match name {
        "ladder" => modelled(models.one(models::LADDER, Turn::from_north(facing)), slots),
        "end_rod" => modelled(models.one(models::END_ROD, Turn::from_up(facing)), slots),
        "redstone_wire" => {
            let power = java.int("power").unwrap_or(0);
            Some(Rule {
                shape: models.one(models::WIRE, Turn::NONE),
                slots: [key(Up).tinted(true); 6],
                tint: Some(Tint::Fixed(wire_color(power))),
            })
        }
        "repeater" => {
            let powered = usize::from(java.flag("powered"));
            let delay = java.int("delay").unwrap_or(1).clamp(1, 4);
            let mut slots = slots;
            slots[Up as usize] = slots[Up as usize].variant(powered);
            slots[North as usize] = Pick::key(Some("repeater_torch")).variant(powered);
            modelled(models.one(models::REPEATER[delay - 1], Turn::from_south(facing)), slots)
        }
        "comparator" => {
            let powered = usize::from(java.flag("powered"));
            let subtract = java.is("mode", "subtract");
            let mut slots = slots;
            slots[Up as usize] = slots[Up as usize].variant(powered);
            slots[North as usize] = Pick::key(Some("comparator_torch")).variant(powered);
            slots[South as usize] = Pick::key(Some("comparator_torch")).variant(usize::from(subtract));
            let parts = if subtract { models::COMPARATOR_SUBTRACT } else { models::COMPARATOR };
            modelled(models.one(parts, Turn::from_south(facing)), slots)
        }
        "lever" => {
            let powered = java.flag("powered");
            let (tipped_north, turn) = match java.get("face") {
                Some("ceiling") => (
                    powered,
                    Turn {
                        x: 2,
                        y: Turn::from_south(facing).y,
                    },
                ),
                Some("wall") => (
                    powered,
                    Turn {
                        x: 1,
                        y: Turn::from_north(facing).y,
                    },
                ),
                _ => (!powered, Turn::from_north(facing)),
            };
            let parts = if tipped_north { models::LEVER_NORTH } else { models::LEVER_SOUTH };
            modelled(models.one(parts, turn), slots)
        }
        "tripwire_hook" => modelled(models.one(models::TRIPWIRE_HOOK, Turn::from_north(facing)), slots),
        "tripwire" => {
            let sides = ["north", "east", "south", "west"].map(|side| java.flag(side));
            modelled(models.connected(&[], models::TRIPWIRE_SIDE, &[], sides), slots)
        }
        "cauldron" | "water_cauldron" | "lava_cauldron" | "powder_snow_cauldron" => {
            let mut slots = slots.map(|pick| pick.tinted(false));
            let level = java.int("level").unwrap_or(3);
            let height = [9, 12, 15][level.clamp(1, 3) - 1];
            let liquid = match name {
                "water_cauldron" => Some(key(West).tinted(true)),
                "lava_cauldron" => Some(Pick::key(Some("still_lava")).tinted(false)),
                "powder_snow_cauldron" => Some(Pick::key(Some("powder_snow")).tinted(false)),
                _ => None,
            };
            let shape = if let Some(pick) = liquid {
                slots[West as usize] = pick;
                models.shape(&[(models::CAULDRON, Turn::NONE), (&[models::cauldron_liquid(height)], Turn::NONE)])
            } else {
                models.one(models::CAULDRON, Turn::NONE)
            };
            Some(Rule {
                shape,
                slots,
                tint: Some(Tint::Water),
            })
        }
        "hopper" => {
            let spout = if facing == "down" {
                (models::HOPPER_SPOUT_DOWN, Turn::NONE)
            } else {
                (models::HOPPER_SPOUT_SIDE, Turn::from_north(facing))
            };
            modelled(models.shape(&[(models::HOPPER, Turn::NONE), spout]), slots)
        }
        "brewing_stand" => {
            let arms = [
                models::brewing_arm([16, 8], java.flag("has_bottle_0")),
                models::brewing_arm([2, 2], java.flag("has_bottle_1")),
                models::brewing_arm([2, 14], java.flag("has_bottle_2")),
            ];
            modelled(models.shape(&[(models::BREWING_STAND, Turn::NONE), (&arms, Turn::NONE)]), slots)
        }
        "bell" => {
            let parts = match java.get("attachment") {
                Some("ceiling") => models::BELL_CEILING,
                Some("single_wall") => models::BELL_WALL,
                Some("double_wall") => models::BELL_BETWEEN,
                _ => models::BELL_FLOOR,
            };
            modelled(models.one(parts, Turn::from_north(facing)), slots)
        }
        "sea_pickle" => {
            let count = java.int("pickles").unwrap_or(1).clamp(1, 4);
            modelled(models.one(models::SEA_PICKLES[count - 1], Turn::NONE), slots)
        }
        "turtle_egg" => {
            let count = java.int("eggs").unwrap_or(1).clamp(1, 4);
            let cracks = java.int("hatch").unwrap_or(0);
            modelled(
                models.one(models::TURTLE_EGGS[count - 1], Turn::NONE),
                slots.map(|pick| pick.variant(cracks)),
            )
        }
        "composter" => {
            let level = java.int("level").unwrap_or(0);
            let mut slots = slots;
            slots[South as usize] = key(Up).variant(if level >= 8 { 2 } else { 1 });
            let height = (1 + 2 * level.min(7)) as i8;
            let shape = if level == 0 {
                models.one(models::COMPOSTER, Turn::NONE)
            } else {
                models.shape(&[(models::COMPOSTER, Turn::NONE), (&[models::compost(height)], Turn::NONE)])
            };
            modelled(shape, slots)
        }
        "stonecutter" => modelled(models.one(models::STONECUTTER, Turn::from_north(facing)), slots),
        "lectern" => modelled(models.one(models::LECTERN, Turn::from_north(facing)), slots),
        "grindstone" => {
            let y = Turn::from_north(facing).y;
            let x = match java.get("face") {
                Some("wall") => 1,
                Some("ceiling") => 2,
                _ => 0,
            };
            modelled(models.one(models::GRINDSTONE, Turn { x, y }), slots)
        }
        "flower_pot" => {
            let mut slots = slots;
            slots[Up as usize] = Pick::key(Some("dirt"));
            modelled(models.one(models::FLOWER_POT, Turn::NONE), slots)
        }
        _ if bedrock == "flower_pot" => {
            let mut slots = slots;
            slots[Up as usize] = Pick::key(Some("dirt"));
            modelled(models.one(models::FLOWER_POT, Turn::NONE), slots)
        }
        "azalea" | "flowering_azalea" => modelled(models.one(models::AZALEA, Turn::NONE), slots),
        "big_dripleaf_stem" => modelled(models.one(models::DRIPLEAF_STEM, Turn::NONE), slots),
        "big_dripleaf" => {
            let (drop, reach) = match java.get("tilt") {
                Some("partial") => (6, 15),
                Some("full") => (11, 11),
                _ => (0, 16),
            };
            let leaf = models::dripleaf(drop, reach);
            modelled(
                models.shape(&[(models::DRIPLEAF_STEM, Turn::NONE), (&leaf, Turn::from_north(facing))]),
                slots,
            )
        }
        "small_dripleaf" => {
            let parts = if java.is("half", "upper") {
                models::SMALL_DRIPLEAF_UPPER
            } else {
                models::SMALL_DRIPLEAF_LOWER
            };
            modelled(models.one(parts, Turn::from_north(facing)), slots)
        }
        "cocoa" => {
            let age = java.int("age").unwrap_or(0).min(2);
            modelled(
                models.one(models::COCOA[age], Turn::from_south(facing)),
                slots.map(|pick| pick.variant(age)),
            )
        }
        "wheat" | "beetroots" | "nether_wart" => {
            let age = java.int("age").unwrap_or(0);
            modelled(models.one(models::CROP, Turn::NONE), slots.map(|pick| pick.variant(age)))
        }
        // Four pictures over eight ages.
        "carrots" | "potatoes" => {
            let stage = [0, 0, 1, 1, 2, 2, 2, 3][java.int("age").unwrap_or(0).min(7)];
            modelled(models.one(models::CROP, Turn::NONE), slots.map(|pick| pick.variant(stage)))
        }
        "torchflower_crop" => Some(Rule {
            shape: Shape::Cross,
            slots: slots.map(|pick| pick.variant(java.int("age").unwrap_or(0))),
            tint: None,
        }),
        "sweet_berry_bush" => {
            let slot = [Down, Up, North, South][java.int("age").unwrap_or(0).min(3)];
            Some(Rule {
                shape: Shape::Cross,
                slots: [key(slot); 6],
                tint: None,
            })
        }
        "melon_stem" | "pumpkin_stem" => {
            let age = java.int("age").unwrap_or(0).min(7);
            let shade = age as u8;
            Some(Rule {
                shape: models.one(&models::stem(shade), Turn::NONE),
                slots: slots.map(|pick| pick.tinted(true)),
                tint: Some(Tint::Fixed([shade * 32, 255 - shade * 8, shade * 4])),
            })
        }
        "attached_melon_stem" | "attached_pumpkin_stem" => Some(Rule {
            shape: models.one(models::STEM_ATTACHED, Turn::from_west(facing)),
            slots: slots.map(|pick| pick.variant(1).tinted(true)),
            tint: Some(Tint::Fixed([224, 199, 28])),
        }),
        "leaf_litter" => {
            let amount = java.int("segment_amount").unwrap_or(1);
            Some(Rule {
                shape: models.one(&models::flat_layer(amount, 1), Turn::from_north(facing)),
                slots: slots.map(|pick| pick.tinted(true)),
                tint: Some(Tint::DryFoliage),
            })
        }
        "pink_petals" | "wildflowers" => {
            let amount = java.int("flower_amount").unwrap_or(1);
            let mut slots = slots.map(|pick| pick.tinted(false));
            slots[East as usize] = slots[East as usize].variant(1).tinted(true);
            Some(Rule {
                shape: models.one(&models::flower_bed(amount), Turn::from_north(facing)),
                slots,
                tint: Some(Tint::Grass),
            })
        }
        "frogspawn" => {
            let plane = [Part::open([0, 1, 0], [16, 1, 16])
                .rect(Up, North, [0, 0, 16, 16])
                .rect(Down, North, [0, 16, 16, 0])
                .unshaded()];
            modelled(models.one(&plane, Turn::NONE), slots)
        }
        "sunflower" if java.is("half", "upper") => {
            let mut slots = slots;
            slots[North as usize] = key(Up);
            slots[South as usize] = Pick::key(Some("sunflower_additional"));
            slots[West as usize] = Pick::key(Some("sunflower_additional")).variant(1);
            modelled(models.one(models::SUNFLOWER_UPPER, Turn::NONE), slots)
        }
        "fire" | "soul_fire" => {
            let mut slots = slots;
            slots[North as usize] = key(Up);
            modelled(models.one(models::FIRE, Turn::NONE), slots)
        }
        "end_portal_frame" => {
            let turn = Turn::from_south(facing);
            let mut slots = slots;
            slots[South as usize] = Pick::key(Some("endframe_eye"));
            let shape = if java.flag("eye") {
                models.shape(&[(models::PORTAL_FRAME, turn), (models::PORTAL_EYE, turn)])
            } else {
                models.one(models::PORTAL_FRAME, turn)
            };
            modelled(shape, slots)
        }
        "beacon" => modelled(models.one(models::BEACON, Turn::NONE), slots),
        "spore_blossom" => modelled(models.one(models::SPORE_BLOSSOM, Turn::NONE), slots),
        "cactus" => modelled(models.one(models::CACTUS, Turn::NONE), slots),
        "sculk_shrieker" => {
            let mut slots = slots;
            slots[South as usize] =
                Pick::key(Some("sculk_shrieker_inner_top")).variant(usize::from(java.flag("can_summon")));
            modelled(models.one(models::SCULK_SHRIEKER, Turn::NONE), slots)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
