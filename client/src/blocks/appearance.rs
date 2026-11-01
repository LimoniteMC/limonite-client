//! How every Bedrock block state looks (m4-plan Decision 5): its shape, the
//! texture on each face, how each face is drawn, and its tint.
//!
//! - **Textures** come from the packs. `blocks.json` names a texture key for
//!   each face, and `terrain_texture.json` a path for each key, variant 0
//!   being the default.
//! - **Rules** (`blocks/rules.rs`) overrule that for the blocks whose slots
//!   aren't faces, or whose variants go by state: m5-plan Step 7b.
//! - **Models from the user's Java jar** overrule both for anything that isn't a
//!   full cube, if there's a jar (`blocks/java_models.rs`, m5-plan Step 7c). Their
//!   textures are Bedrock's where one fits, and Java's where none does.
//! - **Shapes, tints and the blend list** come from the tables below, keyed
//!   by Bedrock name, and from the rules' models (`blocks/models.rs`). The
//!   install has no geometry for vanilla blocks (Finding 8), so a client has
//!   to know the shapes itself. A block with neither is drawn as a cube, unless
//!   its Java state isn't one: then it's Pumpkin's outline boxes (m5-plan
//!   Decision 7), and counted in the log.
//! - **Render classes** come from the textures' alpha: a face whose texture
//!   has a see-through texel is alpha-tested.
//!
//! The table is built once at startup, for every id in the palette, and the
//! mesher looks ids up in it.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    rc::Rc,
};

use image::RgbaImage;
use limonite_packs::{
    PackStack,
    registry::{BlocksJson, Face, FaceTextures, Flipbooks, TerrainTextures},
};
use pumpkin_data::{BlockState as JavaState, BlockStateId};

use super::{
    jar::Jar,
    java_models::{self, JavaLook, Stats},
    models::ModelQuad,
    palette::{BedrockState, Palette},
    rules::{self, JavaBlock, Models, Pick},
    shapes,
};
use crate::{
    edit::CRACK_STAGES,
    render::{
        animation::Animator,
        textures::{self, AlphaUse, JAVA_TEXTURE, TextureArray},
        tint::Tint,
    },
    world::BlockTable,
};

/// Blocks renamed since `blocks.json` was keyed, which it still knows by the
/// old name (Finding 6).
const ALIASES: [(&str, &str); 9] = [
    // Bubbles are particles: the block itself is drawn as water.
    ("bubble_column", "water"),
    ("grass_block", "grass"),
    ("sea_lantern", "seaLantern"),
    ("iron_chain", "chain"),
    ("invisible_bedrock", "invisibleBedrock"),
    ("moving_block", "movingBlock"),
    ("piston_arm_collision", "pistonArmCollision"),
    ("sticky_piston_arm_collision", "stickyPistonArmCollision"),
    ("trip_wire", "tripWire"),
];

/// Java's textures whose Bedrock picture can't be found from its pixels: it goes by
/// another name and is drawn a little differently, or it's see-through in a few other
/// places. Each was compared by eye, and only has to be the same size in the install.
/// Every other texture goes by its own name, and has to be see-through in the same
/// places, or Java's is drawn: if Bedrock has the same pixels, that's the same thing.
const BEDROCK_ART: [(&str, &str); 16] = [
    ("cactus_side", "cactus_side"),
    ("cocoa_stage2", "cocoa_stage_2"),
    ("lilac_bottom", "double_plant_syringa_bottom"),
    ("lilac_top", "double_plant_syringa_top"),
    ("piston_top", "piston_top_normal"),
    ("pumpkin_stem", "pumpkin_stem_disconnected"),
    // Java's head is square, and Bedrock's a smaller cross with a yellow middle.
    ("redstone_torch", "redstone_torch_on"),
    ("scaffolding_side", "scaffolding_side"),
    ("soul_fire_1", "soul_fire_1"),
    ("spruce_planks", "planks_spruce"),
    // `stonecutter_` is Bedrock's older block.
    ("stonecutter_bottom", "stonecutter2_bottom"),
    ("stonecutter_saw", "stonecutter2_saw"),
    ("stonecutter_side", "stonecutter2_side"),
    ("stonecutter_top", "stonecutter2_top"),
    // Tinted by the biome, as Java's is. `water_still` already is blue.
    ("water_flow", "water_flow_grey"),
    ("water_still", "water_still_grey"),
];

/// Never drawn.
const INVISIBLE: &[&str] = &[
    "air",
    "barrier",
    "structure_void",
    "light_block_0",
    "light_block_1",
    "light_block_2",
    "light_block_3",
    "light_block_4",
    "light_block_5",
    "light_block_6",
    "light_block_7",
    "light_block_8",
    "light_block_9",
    "light_block_10",
    "light_block_11",
    "light_block_12",
    "light_block_13",
    "light_block_14",
    "light_block_15",
];

/// Two crossed planes with one texture: the plants. Saplings and corals
/// come in families, matched by [`CROSS_SUFFIXES`].
const CROSS: &[&str] = &[
    "short_grass",
    "fern",
    "deadbush",
    "bush",
    "firefly_bush",
    "short_dry_grass",
    "tall_dry_grass",
    "dandelion",
    "golden_dandelion",
    "poppy",
    "blue_orchid",
    "allium",
    "azure_bluet",
    "red_tulip",
    "orange_tulip",
    "white_tulip",
    "pink_tulip",
    "oxeye_daisy",
    "cornflower",
    "lily_of_the_valley",
    "wither_rose",
    "torchflower",
    "closed_eyeblossom",
    "open_eyeblossom",
    "cactus_flower",
    "wheat",
    "carrots",
    "potatoes",
    "beetroot",
    "sweet_berry_bush",
    "nether_wart",
    "torchflower_crop",
    "melon_stem",
    "pumpkin_stem",
    "brown_mushroom",
    "red_mushroom",
    "crimson_fungus",
    "warped_fungus",
    "crimson_roots",
    "warped_roots",
    "nether_sprouts",
    "weeping_vines",
    "twisting_vines",
    "cave_vines",
    "cave_vines_body_with_berries",
    "cave_vines_head_with_berries",
    "hanging_roots",
    "mangrove_propagule",
    // Sugar cane and cobweb keep their old Bedrock names.
    "reeds",
    "web",
    "kelp",
    "seagrass",
];

/// Families of crossed planes, by name suffix: every sapling, and the coral
/// plants and floor fans, dead or alive. (Wall fans hang on a side; they're
/// drawn as Pumpkin's boxes.)
const CROSS_SUFFIXES: &[&str] = &["_sapling", "_coral", "_coral_fan"];

/// Two-tall plants: crossed planes, with the `down` texture on the lower
/// half and `up` on the upper (`upper_block_bit`).
const TWO_TALL: &[&str] = &[
    "tall_grass",
    "large_fern",
    "sunflower",
    "lilac",
    "rose_bush",
    "peony",
    "pitcher_plant",
    "pitcher_crop",
];

/// Thin planes against the faces a state names: vines by
/// `vine_direction_bits`, the others by `multi_face_direction_bits`.
const VINE: &str = "vine";
const MULTI_FACE: &[&str] = &["glow_lichen", "resin_clump", "sculk_vein"];

/// The faces `vine_direction_bits` names, bit by bit. Checked against
/// Pumpkin's Java-to-Bedrock table in the tests.
const VINE_BITS: [(u8, Face); 4] = [
    (1, Face::South),
    (2, Face::West),
    (4, Face::North),
    (8, Face::East),
];

/// The faces `multi_face_direction_bits` names, bit by bit. Also checked
/// against Pumpkin's table.
const MULTI_FACE_BITS: [(u8, Face); 6] = [
    (1, Face::Down),
    (2, Face::Up),
    (4, Face::South),
    (8, Face::West),
    (16, Face::North),
    (32, Face::East),
];

/// Thin boxes on the floor, one sixteenth tall: carpets by suffix, and lily
/// pads.
const CARPET_SUFFIX: &str = "_carpet";
const LILY_PAD: &str = "waterlily";
/// Snow layers: `height` + 1 layers, each two sixteenths.
const SNOW_LAYER: &str = "snow_layer";

/// A thin stalk with crossed leaves (owner, at the M4 comparison).
/// `bamboo_stalk_thickness` picks the width and `bamboo_leaf_size` the
/// leaves.
const BAMBOO: &str = "bamboo";

/// Liquids, by name.
const LIQUIDS: [(&str, Liquid); 5] = [
    ("water", Liquid::Water),
    ("flowing_water", Liquid::Water),
    ("bubble_column", Liquid::Water),
    ("lava", Liquid::Lava),
    ("flowing_lava", Liquid::Lava),
];

/// Drawn with alpha blending, back to front, in the second pass. Stained
/// glass, and its panes, by suffix.
const BLEND: &[&str] = &["water", "flowing_water", "bubble_column", "ice", "slime", "honey_block"];
const BLEND_SUFFIXES: &[&str] = &["stained_glass", "stained_glass_pane"];

/// What each tinted block is tinted by.
const TINTS: &[(&str, Tint)] = &[
    ("grass_block", Tint::Grass),
    ("short_grass", Tint::Grass),
    ("bush", Tint::Grass),
    ("tall_grass", Tint::Grass),
    ("fern", Tint::Grass),
    ("large_fern", Tint::Grass),
    ("reeds", Tint::Grass),
    ("oak_leaves", Tint::Foliage),
    ("jungle_leaves", Tint::Foliage),
    ("acacia_leaves", Tint::Foliage),
    ("dark_oak_leaves", Tint::Foliage),
    ("mangrove_leaves", Tint::Foliage),
    ("vine", Tint::Foliage),
    ("birch_leaves", Tint::Birch),
    ("spruce_leaves", Tint::Evergreen),
    ("water", Tint::Water),
    ("flowing_water", Tint::Water),
    ("bubble_column", Tint::Water),
    // Java's colour, the same everywhere. Bedrock's texture is grey too.
    ("waterlily", Tint::Fixed([0x20, 0x80, 0x30])),
];

/// Blocks whose sides are drawn opaque, their texture's alpha marking the
/// texels to tint, and whose bottom isn't tinted (Finding 5).
const GRASS_SIDES: &str = "grass_block";

/// Blocks that show a snowy side under snow, and the key whose list holds it,
/// last. Java keeps this in the state; Bedrock works it out, as this does.
const SNOWY: [(&str, &str); 3] = [
    ("grass_block", "grass_side"),
    ("mycelium", "mycelium_side"),
    ("podzol", "grass_side"),
];
/// What counts as snow, for the block below.
const SNOW: [&str; 3] = ["snow_layer", "snow", "powder_snow"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liquid {
    Water,
    Lava,
}

/// The leaves on a bamboo block, from `bamboo_leaf_size`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BambooLeaves {
    None,
    Small,
    Large,
}

/// A block's geometry. Heights are in sixteenths of a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Invisible,
    Cube,
    /// Two planes crossed diagonally: plants.
    Cross,
    /// `amount` is how high it stands in ninths of a block, when the same liquid
    /// isn't above: 8 for a source or a fall, and a ninth less each block it's flowed.
    Liquid { liquid: Liquid, amount: u8 },
    /// A full-width box from `bottom` to `top`: slabs, snow, carpets.
    Slab { bottom: u8, top: u8 },
    /// Thin planes just inside the faces in this set of `1 << face` bits.
    Planes { faces: u8 },
    /// A bamboo stalk, 3 sixteenths wide if `thick` and 2 if not, with
    /// crossed leaves unless `leaves` is none. Bedrock's `blocks.json` uses
    /// the face slots as a bag of textures, and the table keeps three: the
    /// stem in north, the large leaves in up, and the small ones in south.
    Bamboo { thick: bool, leaves: BambooLeaves },
    /// The outline boxes of this Java state (`blocks/shapes.rs`): stairs,
    /// fences, doors, torches and the other blocks M4 drew as stand-in
    /// cubes. Each face shows the block's texture for that face, cut by its
    /// position, as a slab's does. A block whose model is its outline stays
    /// this; the others have models.
    Boxes { state: BlockStateId },
    /// A model, baked, hand-made (`blocks/models.rs`) or from the jar
    /// (`blocks/java_models.rs`): each quad shows one of the look's slots.
    Model { quads: &'static [ModelQuad] },
    /// Crossed planes that show their body in north, or their growing tip in
    /// up if no more of the plant lies the way it grows: kelp and vines.
    Stalk { grows_up: bool },
}

/// How a face's texels are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderClass {
    /// Every texel opaque.
    Opaque,
    /// Some texels see-through: alpha-tested, so they're either there or not.
    Cutout,
    /// Drawn opaque, with alpha marking the texels to tint.
    Overlay,
    /// Alpha-blended, in the second pass.
    Blend,
}

/// One face, ready for the mesher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceLook {
    /// The face's layer in the texture array; 0 is the missing texture.
    pub layer: u16,
    pub class: RenderClass,
    /// Whether the face takes its block's tint. An overlay face tints only
    /// the texels its alpha marks.
    pub tinted: bool,
    /// Quarter turns of the texture, always applied: a sideways log's bark.
    pub turns: u8,
    /// Whether the mesher may add a quarter turn picked by position.
    pub isotropic: bool,
}

/// The textures a look can hold: one for each face, and two more for a model
/// that needs them (`blocks/java_models.rs`). No state of 26.3 needs more than 7.
pub const SLOTS: usize = 8;

/// How one block state looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub shape: Shape,
    pub tint: Tint,
    /// Indexed by `Face as usize`: down, up, north, south, west, east. Crossed
    /// planes and attached planes use the same look in every slot. A model's quads
    /// say which slot they show.
    pub faces: [FaceLook; SLOTS],
    /// A full cube whose faces are all opaque or overlay: it hides the faces
    /// of its neighbours.
    pub occludes: bool,
    /// What its sides show under snow: grass, mycelium and podzol.
    pub under_snow: Option<FaceLook>,
    /// Whether it counts as snow, for the block below.
    pub snow: bool,
}

impl Appearance {
    /// A cube in the missing texture: for ids the palette doesn't have.
    const MISSING: Self = Self {
        shape: Shape::Cube,
        tint: Tint::None,
        faces: [FaceLook {
            layer: 0,
            class: RenderClass::Opaque,
            tinted: false,
            turns: 0,
            isotropic: false,
        }; SLOTS],
        occludes: true,
        under_snow: None,
        snow: false,
    };

    #[must_use]
    pub const fn face(&self, face: Face) -> &FaceLook {
        &self.faces[face as usize]
    }

    /// The liquid it is, if it's one.
    #[must_use]
    pub const fn liquid(&self) -> Option<Liquid> {
        match self.shape {
            Shape::Liquid { liquid, .. } => Some(liquid),
            _ => None,
        }
    }
}

/// Every palette id's appearance.
pub struct AppearanceTable {
    by_id: HashMap<u32, Appearance>,
}

impl AppearanceTable {
    /// The id's appearance. An id the palette doesn't know is a cube in the
    /// missing texture.
    #[must_use]
    pub fn get(&self, id: u32) -> &Appearance {
        self.by_id.get(&id).unwrap_or(&Appearance::MISSING)
    }

    /// A table of hand-made appearances, for tests that don't need packs.
    #[cfg(test)]
    pub fn from_looks(looks: impl IntoIterator<Item = (u32, Appearance)>) -> Self {
        Self {
            by_id: looks.into_iter().collect(),
        }
    }
}

/// What building the table found, for the startup log.
#[derive(Debug, Default)]
pub struct Report {
    /// Distinct block names in the palette.
    pub names: usize,
    /// Names with a `blocks.json` entry, after aliasing.
    pub with_entry: usize,
    /// Names drawn with every texture they need.
    pub textured: usize,
    /// Names never drawn.
    pub invisible: usize,
    /// Names without a `blocks.json` entry: Education Edition and
    /// engine-internal blocks.
    pub no_entry: BTreeSet<String>,
    /// Of those, the ones Pumpkin can send. Should be none.
    pub no_entry_sent: BTreeSet<String>,
    /// Names with an entry whose textures don't all resolve, and why.
    pub unresolved: BTreeMap<String, String>,
    /// Names drawn as Pumpkin's outline boxes. For stairs, walls and buttons
    /// that's their model. For the block entities it's a stand-in until M8:
    /// beds, chests and signs are boxes.
    pub approximate: BTreeSet<String>,
    /// Texture files that couldn't be loaded, and why.
    pub failed_textures: Vec<(String, String)>,
    /// What the jar's models were given: how many Bedrock textures, and which of
    /// Java's own, because Bedrock has none laid out the same.
    pub bedrock_art: usize,
    pub java_art: BTreeSet<String>,
}

/// The table, its texture array, and the report.
pub struct Built {
    pub table: AppearanceTable,
    pub textures: TextureArray,
    /// The layers that move.
    pub animator: Animator,
    pub texture_seconds: f64,
    pub report: Report,
    /// What the jar came to, if there was one.
    pub java: Option<Stats>,
}

/// The textures of the crack on a block being broken, each with its stage after it.
const CRACK_TEXTURES: &str = "textures/environment/destroy_stage_";

/// One face before textures are loaded: which requested texture, if any.
#[derive(Clone, Copy, Default)]
struct FacePlan {
    texture: Option<usize>,
    overlay: bool,
    tinted: bool,
    turns: u8,
    isotropic: bool,
}

struct Plan {
    shape: Shape,
    tint: Tint,
    blend: bool,
    faces: [FacePlan; SLOTS],
    /// The texture its sides show under snow.
    under_snow: Option<usize>,
}

/// Builds the table for every palette id, and loads the textures it uses.
#[must_use]
pub fn build(
    palette: &Palette,
    blocks: &BlocksJson,
    terrain: &TerrainTextures,
    flipbooks: &Flipbooks,
    stack: &PackStack,
    mut jar: Option<&mut Jar>,
) -> Built {
    // The same Java states physics collides with.
    let java = BlockTable::build();
    let states: Vec<(u32, &BedrockState, Option<JavaBlock>)> = palette
        .iter()
        .map(|(id, state)| (id, state, java.shape_state(id).map(JavaBlock::of)))
        .collect();
    let compiled = jar.as_deref_mut().map(|jar| {
        java_models::compile(
            |entry| jar.read(entry),
            states.iter().filter_map(|(id, _, block)| Some((*id, block.as_ref()?))),
        )
    });
    let mut resolver = Resolver {
        blocks,
        terrain,
        stack,
        jar,
        names: bedrock_textures_by_name(terrain),
        art: HashMap::new(),
        requests: Vec::new(),
        requested: HashMap::new(),
    };
    let mut report = Report::default();
    let mut plans = Vec::with_capacity(palette.len());
    let mut problems: HashMap<&str, String> = HashMap::new();
    let mut models = Models::default();
    for (id, state, java_block) in &states {
        let java_look = compiled.as_ref().and_then(|models| models.look(*id));
        let (plan, problem) = resolver.plan(state, java_block.as_ref(), java_look, &mut models);
        if let Some(problem) = problem {
            problems.entry(&*state.name).or_insert(problem);
        }
        plans.push((*id, &*state.name, plan));
    }
    for path in resolver.art.values() {
        match path.strip_prefix(JAVA_TEXTURE) {
            Some(name) => {
                report.java_art.insert(name.to_string());
            }
            None => report.bedrock_art += 1,
        }
    }

    // The crack on a block being broken.
    let crack: Vec<usize> = (0..CRACK_STAGES)
        .map(|stage| resolver.request(format!("{CRACK_TEXTURES}{stage}"), AlphaUse::Coverage))
        .collect();
    let mut loaded = textures::load(stack, resolver.jar.as_deref_mut(), flipbooks, &resolver.requests);
    let mut failed_names: HashMap<&str, String> = HashMap::new();
    let mut by_id = HashMap::with_capacity(plans.len());
    for (id, name, plan) in plans {
        let mut faces = Appearance::MISSING.faces;
        for (look, face) in faces.iter_mut().zip(&plan.faces) {
            let layer = face.texture.and_then(|index| {
                let layer = loaded.layers[index];
                if layer.is_none() {
                    failed_names
                        .entry(name)
                        .or_insert_with(|| format!("`{}` didn't load", resolver.requests[index].0));
                }
                layer
            });
            look.layer = layer.map_or(0, |layer| layer.index);
            look.class = if plan.blend {
                RenderClass::Blend
            } else if face.overlay {
                RenderClass::Overlay
            } else if layer.is_some_and(|layer| layer.transparent) {
                RenderClass::Cutout
            } else {
                RenderClass::Opaque
            };
            look.tinted = face.tinted;
            look.turns = face.turns;
            look.isotropic = face.isotropic;
        }
        // A cube in the tables whose Java state isn't one is drawn as its
        // boxes. An id Pumpkin never sends has no Java state, and stays a cube.
        let mut shape = plan.shape;
        if shape == Shape::Cube
            && let Some(state) = java.shape_state(id)
            && !shapes::is_full_cube(state)
        {
            shape = Shape::Boxes { state };
            report.approximate.insert(name.to_string());
        }
        let occludes = shape == Shape::Cube
            && faces
                .iter()
                .all(|face| matches!(face.class, RenderClass::Opaque | RenderClass::Overlay));
        let under_snow = plan.under_snow.and_then(|index| loaded.layers[index]).map(|layer| FaceLook {
            layer: layer.index,
            class: RenderClass::Opaque,
            tinted: false,
            turns: 0,
            isotropic: false,
        });
        by_id.insert(
            id,
            Appearance {
                shape,
                tint: plan.tint,
                faces,
                occludes,
                under_snow,
                snow: SNOW.contains(&name),
            },
        );
    }

    // Per name: the palette counts each state, the report each block.
    let names = palette.names();
    let sendable = sendable_names(palette);
    report.names = names.len();
    report.invisible = names.iter().filter(|name| INVISIBLE.contains(name)).count();
    for name in names {
        let key = blocks_json_key(name);
        if blocks.blocks.contains_key(key) {
            report.with_entry += 1;
        } else {
            report.no_entry.insert(name.to_string());
            if sendable.contains(name) {
                report.no_entry_sent.insert(name.to_string());
            }
            continue;
        }
        match problems.get(name).or_else(|| failed_names.get(name)) {
            Some(problem) => {
                report.unresolved.insert(name.to_string(), problem.clone());
            }
            None if !INVISIBLE.contains(&name) => report.textured += 1,
            None => {}
        }
    }
    // Breaking works without them, so they aren't among the blocks' failures.
    let (no_crack, failed): (Vec<_>, Vec<_>) =
        loaded.failed.into_iter().partition(|(path, _)| path.starts_with(CRACK_TEXTURES));
    if !no_crack.is_empty() {
        tracing::warn!("No crack on blocks being broken: {no_crack:?}");
    }
    report.failed_textures = failed;
    loaded.array.crack = crack
        .iter()
        .map(|&index| loaded.layers[index].map(|layer| layer.index))
        .collect::<Option<Vec<u16>>>()
        .and_then(|layers| layers.try_into().ok());
    Built {
        table: AppearanceTable { by_id },
        textures: loaded.array,
        animator: loaded.animator,
        texture_seconds: loaded.seconds,
        report,
        java: compiled.map(|models| models.stats),
    }
}

/// Every texture `terrain_texture.json` knows, by the last part of its path.
fn bedrock_textures_by_name(terrain: &TerrainTextures) -> HashMap<&str, &str> {
    let mut names = HashMap::new();
    for entry in terrain.textures.values() {
        for texture in entry.textures.as_slice() {
            let path = texture.path.as_str();
            names.entry(path.rsplit('/').next().unwrap_or(path)).or_insert(path);
        }
    }
    names
}

/// Whether two textures are laid out for the same shape: they're see-through in
/// the same places. Colours can differ, and often do between the two editions.
fn same_layout(a: &RgbaImage, b: &RgbaImage) -> bool {
    a.dimensions() == b.dimensions() && a.pixels().zip(b.pixels()).all(|(a, b)| (a[3] > 0) == (b[3] > 0))
}

/// Resolves block names to texture paths, and collects the textures to load.
struct Resolver<'a> {
    blocks: &'a BlocksJson,
    terrain: &'a TerrainTextures,
    stack: &'a PackStack,
    jar: Option<&'a mut Jar>,
    /// `bedrock_textures_by_name`.
    names: HashMap<&'a str, &'a str>,
    /// What to load for each texture the jar's models name.
    art: HashMap<Rc<str>, String>,
    requests: Vec<(String, AlphaUse)>,
    requested: HashMap<(String, AlphaUse), usize>,
}

impl Resolver<'_> {
    /// One state's plan, and the first problem resolving its textures.
    fn plan(
        &mut self,
        state: &BedrockState,
        java: Option<&JavaBlock>,
        java_look: Option<&JavaLook>,
        models: &mut Models,
    ) -> (Plan, Option<String>) {
        let name = &*state.name;
        let shape = shape_of(state);
        let tint = TINTS
            .iter()
            .find(|(tinted, _)| *tinted == name)
            .map_or(Tint::None, |&(_, tint)| tint);
        let blend = BLEND.contains(&name) || BLEND_SUFFIXES.iter().any(|suffix| name.ends_with(suffix));
        let mut plan = Plan {
            shape,
            tint,
            blend,
            faces: [FacePlan::default(); SLOTS],
            under_snow: None,
        };
        if shape == Shape::Invisible {
            return (plan, None);
        }

        // A clone, to outlive the borrows of `self` below.
        let blocks = self.blocks;
        let entry = blocks.blocks.get(blocks_json_key(name));
        let textures = entry.and_then(|entry| entry.textures.as_ref());
        // A model from the jar still takes its tint from the rules.
        models.skip = java_look.is_some();
        let rule = textures.and_then(|textures| rules::rule(state, java, textures, models));
        if let Some(tint) = rule.as_ref().and_then(|rule| rule.tint) {
            plan.tint = tint;
        }
        // Bamboo is the owner's own (M4), and a plant's tip depends on the block
        // beyond, which Java keeps in a state Bedrock hasn't got. Cave vines with
        // berries are what a stalk looks for beyond it.
        let keeps_its_shape = matches!(shape, Shape::Bamboo { .. })
            || matches!(rule.as_ref().map(|rule| rule.shape), Some(Shape::Stalk { .. }))
            || name.starts_with("cave_vines");
        if let Some(look) = java_look
            && !keeps_its_shape
        {
            plan.shape = Shape::Model { quads: look.quads };
            for (face, slot) in plan.faces.iter_mut().zip(&look.slots) {
                let path = self.art(&slot.texture);
                *face = FacePlan {
                    texture: Some(self.request(path, AlphaUse::Coverage)),
                    overlay: false,
                    tinted: slot.tinted,
                    turns: 0,
                    isotropic: false,
                };
            }
            return (plan, None);
        }

        let Some(textures) = textures else {
            let problem = if entry.is_some() {
                "its blocks.json entry has no textures"
            } else {
                "no blocks.json entry"
            };
            return (plan, Some(problem.to_string()));
        };
        let isotropic = entry.and_then(|entry| entry.isotropic.as_ref());

        let mut problem = None;
        // What each slot shows: a rule's say, or the tables'.
        let picks: [Pick; 6] = if let Some(rule) = rule {
            plan.shape = rule.shape;
            rule.slots
        } else {
            match shape {
                Shape::Cross | Shape::Planes { .. } => [Pick::key(cross_key(state, textures)); 6],
                Shape::Cube => pillar_keys(state, textures),
                Shape::Bamboo { .. } => {
                    // The stem everywhere but the two leaf slots. The entry's
                    // other slots (the sapling and a single leaf) go unused.
                    let mut picks = [Pick::key(textures.key(Face::North)); 6];
                    for leaves in [Face::Up, Face::South] {
                        picks[leaves as usize] = Pick::key(textures.key(leaves));
                    }
                    picks
                }
                _ => Face::ALL.map(|face| Pick::key(textures.key(face))),
            }
        };
        for (face, pick) in Face::ALL.into_iter().zip(picks) {
            let overlay = name == GRASS_SIDES && face.is_side();
            let tinted = pick
                .tinted
                .unwrap_or(plan.tint != Tint::None && !(name == GRASS_SIDES && face == Face::Down));
            let alpha = if overlay { AlphaUse::Mask } else { AlphaUse::Coverage };
            let texture = match pick.key.map(|key| (key, self.path(key, pick.variant))) {
                Some((_, Ok(path))) => Some(self.request(path, alpha)),
                Some((key, Err(err))) => {
                    problem.get_or_insert(format!("texture key `{key}`: {err}"));
                    None
                }
                None => {
                    problem.get_or_insert(format!("no texture for its {} face", face.key()));
                    None
                }
            };
            plan.faces[face as usize] = FacePlan {
                texture,
                overlay,
                tinted,
                turns: pick.turns,
                isotropic: isotropic.is_some_and(|iso| iso.applies(face)),
            };
        }
        // Bedrock keeps the snowy side at the end of the list, or for podzol
        // in grass's.
        if let Some(&(_, key)) = SNOWY.iter().find(|(snowy, _)| *snowy == name)
            && let Some(variant) = self.terrain.textures.get(key).map(|entry| entry.textures.as_slice().len() - 1)
            && let Ok(path) = self.path(key, variant)
        {
            plan.under_snow = Some(self.request(path, AlphaUse::Coverage));
        }
        (plan, problem)
    }

    /// What to load for a texture a Java model names: Bedrock's picture if it has
    /// one laid out the same, as the official client is the one to look like, and
    /// otherwise Java's own.
    fn art(&mut self, name: &Rc<str>) -> String {
        if let Some(path) = self.art.get(name) {
            return path.clone();
        }
        let short = name.rsplit('/').next().unwrap_or(name);
        let chosen = BEDROCK_ART.iter().find(|(java, _)| *java == short).map(|&(_, bedrock)| bedrock);
        let fits = self.names.get(chosen.unwrap_or(short)).copied().filter(|path| {
            let java = self.jar.as_deref_mut().map(|jar| textures::read_java_frame(jar, name));
            let bedrock = textures::read_pack_frame(self.stack, path);
            match (java, bedrock) {
                (Some(Ok(java)), Ok(bedrock)) if chosen.is_some() => java.dimensions() == bedrock.dimensions(),
                (Some(Ok(java)), Ok(bedrock)) => same_layout(&java, &bedrock),
                _ => false,
            }
        });
        let path = fits.map_or_else(|| format!("{JAVA_TEXTURE}{name}"), str::to_string);
        self.art.insert(name.clone(), path.clone());
        path
    }

    /// A texture key's path: this variant of its `terrain_texture.json` entry.
    /// A list shorter than that shows its first.
    fn path(&self, key: &str, variant: usize) -> Result<String, &'static str> {
        let entry = self.terrain.textures.get(key).ok_or("not in terrain_texture.json")?;
        entry
            .textures
            .as_slice()
            .get(variant)
            .or_else(|| entry.textures.first())
            .map(|texture| texture.path.clone())
            .ok_or("has no variants")
    }

    /// The index of a texture to load, asking for each one once.
    fn request(&mut self, path: String, alpha: AlphaUse) -> usize {
        let next = self.requests.len();
        *self.requested.entry((path.clone(), alpha)).or_insert_with(|| {
            self.requests.push((path, alpha));
            next
        })
    }
}

/// The key a block has in `blocks.json`.
pub(super) fn blocks_json_key(name: &str) -> &str {
    ALIASES
        .iter()
        .find(|(renamed, _)| *renamed == name)
        .map_or(name, |&(_, key)| key)
}

/// A state's shape, from the tables and its properties.
fn shape_of(state: &BedrockState) -> Shape {
    let name = &*state.name;
    if INVISIBLE.contains(&name) {
        return Shape::Invisible;
    }
    if let Some(&(_, liquid)) = LIQUIDS.iter().find(|(liquid, _)| *liquid == name) {
        return Shape::Liquid {
            liquid,
            amount: liquid_amount(state.int("liquid_depth").unwrap_or(0)),
        };
    }
    // Every block with this property is a slab. The double slabs are full
    // cubes, and their names all say "double".
    if let Some(half) = state.string("minecraft:vertical_half")
        && !name.contains("double")
    {
        return if half == "top" {
            Shape::Slab { bottom: 8, top: 16 }
        } else {
            Shape::Slab { bottom: 0, top: 8 }
        };
    }
    if name == SNOW_LAYER {
        let layers = state.int("height").unwrap_or(0).clamp(0, 7) as u8 + 1;
        return Shape::Slab { bottom: 0, top: layers * 2 };
    }
    if name.ends_with(CARPET_SUFFIX) || name == LILY_PAD {
        return Shape::Slab { bottom: 0, top: 1 };
    }
    if name == VINE {
        let bits = state.int("vine_direction_bits").unwrap_or(0);
        // With no side, a vine hangs from the block above.
        let faces = faces_of(bits, &VINE_BITS);
        return Shape::Planes {
            faces: if faces == 0 { 1 << Face::Up as u8 } else { faces },
        };
    }
    if MULTI_FACE.contains(&name) {
        let bits = state.int("multi_face_direction_bits").unwrap_or(0);
        return Shape::Planes {
            faces: faces_of(bits, &MULTI_FACE_BITS),
        };
    }
    if name == BAMBOO {
        return Shape::Bamboo {
            thick: state.string("bamboo_stalk_thickness") == Some("thick"),
            leaves: match state.string("bamboo_leaf_size") {
                Some("small_leaves") => BambooLeaves::Small,
                Some("large_leaves") => BambooLeaves::Large,
                _ => BambooLeaves::None,
            },
        };
    }
    if CROSS.contains(&name)
        || TWO_TALL.contains(&name)
        || CROSS_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
    {
        return Shape::Cross;
    }
    Shape::Cube
}

/// How high a liquid stands, in ninths of a block, when the same liquid isn't above:
/// Java's heights. A source (depth 0) is 8, and each step of flowing water a ninth
/// lower. Depth 8 and up is falling, which stands as high as a source.
fn liquid_amount(depth: i32) -> u8 {
    if depth >= 8 { 8 } else { (8 - depth.clamp(0, 7)) as u8 }
}

/// The faces a direction bit field names, as `1 << face` bits.
fn faces_of(bits: i32, table: &[(u8, Face)]) -> u8 {
    table
        .iter()
        .filter(|(bit, _)| bits & i32::from(*bit) != 0)
        .fold(0, |faces, &(_, face)| faces | 1 << face as u8)
}

/// The one texture key of crossed or attached planes.
///
/// Bedrock's own renderers for these blocks use the face slots as a bag of
/// per-state textures. Two-tall plants show `down` on the lower half and `up`
/// on the upper; seagrass picks by `sea_grass_type`. Everything else shows
/// its north face's key: its only key, or `side`.
fn cross_key<'a>(state: &BedrockState, textures: &'a FaceTextures) -> Option<&'a str> {
    let name = &*state.name;
    if TWO_TALL.contains(&name) {
        let upper = state.flag("upper_block_bit").unwrap_or(false);
        return textures.key(if upper { Face::Up } else { Face::Down });
    }
    if name == "seagrass" {
        return textures.key(match state.string("sea_grass_type") {
            Some("double_bot") => Face::Down,
            Some("double_top") => Face::North,
            _ => Face::Up,
        });
    }
    textures.key(Face::North)
}

/// A cube's face keys and turns, following `pillar_axis` for logs, pillars
/// and the like.
///
/// Standing up (axis y, or no axis), every face shows its own key. Lying
/// along x or z, the two faces at the ends show the `up` and `down` keys,
/// and the four around the length show the side, turned a quarter where
/// that runs the grain along the length. Which faces need the turn follows
/// from the mesher's texture orientation (`render::mesher`).
fn pillar_keys<'a>(state: &BedrockState, textures: &'a FaceTextures) -> [Pick<'a>; 6] {
    let side = textures.key(Face::North);
    let keys: [(Option<&'a str>, u8); 6] = match state.string("pillar_axis") {
        Some("x") => [
            (side, 1),                     // down
            (side, 1),                     // up
            (side, 1),                     // north
            (side, 1),                     // south
            (textures.key(Face::Down), 0), // west
            (textures.key(Face::Up), 0),   // east
        ],
        Some("z") => [
            (side, 0),                     // down
            (side, 0),                     // up
            (textures.key(Face::Down), 0), // north
            (textures.key(Face::Up), 0),   // south
            (side, 1),                     // west
            (side, 1),                     // east
        ],
        _ => Face::ALL.map(|face| (textures.key(face), 0)),
    };
    keys.map(|(key, turns)| Pick {
        turns,
        ..Pick::key(key)
    })
}

/// The names of the blocks Pumpkin can send.
fn sendable_names(palette: &Palette) -> HashSet<&str> {
    (0..BlockStateId::COUNT)
        .filter_map(BlockStateId::new)
        .filter_map(|state| palette.state(JavaState::to_be_network_id(state)))
        .map(|state| &*state.name)
        .collect()
}

#[cfg(test)]
mod tests;
