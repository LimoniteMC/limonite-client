//! Block models compiled from the user's own Java jar, at startup and in memory
//! (m5-plan Step 7c). `blocks/jar.rs` says why a Java jar, and why it's not kept.
//!
//! **The two kinds of file** (worth teaching):
//! - `blockstates/<block>.json` says which models a state shows. Either
//!   `variants`, one entry per combination of properties, or `multipart`, a list
//!   of parts with conditions, every part that holds being drawn: a fence is a
//!   post, and an arm for each side it connects to. A reference can turn its model
//!   by quarter turns, and lock the texture so it stays where it was.
//! - `models/block/<name>.json` has boxes (`elements`) and texture variables. A
//!   model with a `parent` takes from it what it doesn't say, so an oak fence's
//!   post only says that `#texture` is oak planks, and the template has the boxes.
//!
//! **The encoding** (worth teaching). Baking each reference on its own would make
//! tens of thousands of quads, nearly all of them differing only by texture. So a
//! [`ModelQuad`] holds a slot, not a texture: all the wooden fences share one list
//! of quads, and which wood is in the look's slots. The lists lie one after another
//! in one allocation, each kept once, and a state just points at its own. Baked
//! once like this, the mesher only copies them, with no maths per block.
//!
//! **What isn't in the files:** which colour a tinted face takes is in Java's code
//! and in our tables, and which of the jar's textures are drawn is the caller's
//! choice (`appearance.rs`), as Bedrock's art comes first.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    rc::Rc,
    time::Instant,
};

use limonite_packs::registry::Face::{self, Down, East, North, South, Up, West};
use serde_json::Value;

use super::{
    appearance::SLOTS,
    models::{ModelQuad, POSITION_SCALE, Turn, UV_SCALE, box_face, face_uv},
    rules::JavaBlock,
};

/// How long a chain of parents can be. Mojang's are five long at most, so this
/// only stops a loop in a damaged file.
const MAX_PARENTS: usize = 32;
/// How far a corner can be from its block, in baked units: four blocks. Mojang's
/// are within one, so this only stops a damaged file overflowing a vertex.
const MAX_CORNER: f32 = 4096.0;
/// A whole block, in baked units.
const WHOLE: i16 = 16 * POSITION_SCALE;

/// One texture a look needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JavaSlot {
    /// As models name it, without its namespace: `block/lantern`.
    pub texture: Rc<str>,
    /// Whether the faces showing it take the block's tint.
    pub tinted: bool,
}

/// How one state looks, by the jar.
#[derive(Debug)]
pub struct JavaLook {
    pub quads: &'static [ModelQuad],
    /// What each quad's `slot` means.
    pub slots: Vec<JavaSlot>,
}

/// What compiling found, for the log and the docs.
#[derive(Debug, Default)]
pub struct Stats {
    /// States asked about.
    pub states: usize,
    /// States with a look here.
    pub looks: usize,
    /// States that are full cubes, which `blocks.json` and the rules know.
    pub cubes: usize,
    /// States the jar draws nothing for: liquids, and what's drawn as an entity.
    pub empty: usize,
    /// Blocks with no block state file, which a jar of another version can have.
    pub no_file: BTreeSet<&'static str>,
    /// Blocks needing more textures in one state than a look has slots.
    pub too_many_textures: BTreeSet<&'static str>,
    /// Files read, and their bytes.
    pub files: usize,
    pub json_bytes: usize,
    /// The quads of every model reference if each were baked on its own.
    pub baked_quads: usize,
    /// The distinct lists of quads kept, and the quads in them.
    pub shapes: usize,
    pub kept_quads: usize,
    pub seconds: f64,
}

/// Every state's look.
#[derive(Debug, Default)]
pub struct JavaModels {
    looks: HashMap<u32, JavaLook>,
    pub stats: Stats,
}

impl JavaModels {
    #[must_use]
    pub fn look(&self, id: u32) -> Option<&JavaLook> {
        self.looks.get(&id)
    }
}

/// Compiles these states, each with the id it goes by. `read` gets a file of the
/// jar by its path.
pub fn compile<'a>(
    read: impl FnMut(&str) -> Option<Vec<u8>>,
    states: impl Iterator<Item = (u32, &'a JavaBlock)>,
) -> JavaModels {
    let start = Instant::now();
    let mut compiler = Compiler::new(read);
    let mut stats = Stats::default();
    // Offsets for now, as the pool still moves as it grows.
    let mut pool = Vec::new();
    let mut seen: HashMap<Vec<ModelQuad>, (usize, usize)> = HashMap::new();
    let mut found = Vec::new();
    for (id, block) in states {
        stats.states += 1;
        match compiler.state(block) {
            Compiled::Look(quads, slots) => {
                let range = *seen.entry(quads).or_insert_with_key(|quads| {
                    let first = pool.len();
                    pool.extend_from_slice(quads);
                    (first, quads.len())
                });
                found.push((id, range, slots));
            }
            Compiled::Cube => stats.cubes += 1,
            Compiled::Empty => stats.empty += 1,
            Compiled::NoFile => {
                stats.no_file.insert(block.name);
            }
            Compiled::TooManyTextures => {
                stats.too_many_textures.insert(block.name);
            }
        }
    }
    stats.looks = found.len();
    stats.shapes = seen.len();
    stats.kept_quads = pool.len();
    stats.files = compiler.files;
    stats.json_bytes = compiler.json_bytes;
    stats.baked_quads = compiler.baked_quads;
    stats.seconds = start.elapsed().as_secs_f64();
    // It lives as long as the table that points at it, so it's leaked rather
    // than counted, like the hand-made models.
    let pool: &'static [ModelQuad] = pool.leak();
    let looks = found
        .into_iter()
        .map(|(id, (first, len), slots)| {
            let quads = &pool[first..first + len];
            (id, JavaLook { quads, slots })
        })
        .collect();
    JavaModels { looks, stats }
}

/// What a state came to.
enum Compiled {
    Look(Vec<ModelQuad>, Vec<JavaSlot>),
    Cube,
    Empty,
    NoFile,
    TooManyTextures,
}

/// A reference to a model.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ModelRef {
    model: Rc<str>,
    turn: Turn,
    uvlock: bool,
}

impl ModelRef {
    /// One reference, or the first of a weighted list: Java picks by position,
    /// and Bedrock's own way of picking isn't known.
    fn parse(value: &Value) -> Option<Self> {
        let value = value.as_array().map_or(Some(value), |options| options.first())?;
        let quarters = |key| value.get(key).and_then(Value::as_i64).unwrap_or(0).div_euclid(90).rem_euclid(4) as u8;
        Some(Self {
            model: without_namespace(value.get("model")?.as_str()?).into(),
            turn: Turn {
                x: quarters("x"),
                y: quarters("y"),
            },
            uvlock: value.get("uvlock").and_then(Value::as_bool).unwrap_or(false),
        })
    }
}

/// A block state file.
enum BlockFile {
    /// Each with the properties it needs. The first that holds is the state's.
    Variants(Vec<(Vec<(String, String)>, ModelRef)>),
    /// Every part that holds is drawn.
    Multipart(Vec<(When, ModelRef)>),
}

impl BlockFile {
    fn parse(value: &Value) -> Option<Self> {
        if let Some(variants) = value.get("variants").and_then(Value::as_object) {
            let variants = variants
                .iter()
                .filter_map(|(key, value)| Some((parse_properties(key), ModelRef::parse(value)?)))
                .collect();
            return Some(Self::Variants(variants));
        }
        let parts = value.get("multipart")?.as_array()?;
        let parts = parts
            .iter()
            .filter_map(|part| {
                let when = part.get("when").map_or(When::All(Vec::new()), When::parse);
                Some((when, ModelRef::parse(part.get("apply")?)?))
            })
            .collect();
        Some(Self::Multipart(parts))
    }

    fn select(&self, block: &JavaBlock) -> Vec<&ModelRef> {
        match self {
            Self::Variants(variants) => variants
                .iter()
                .find(|(props, _)| props.iter().all(|(key, value)| block.get(key) == Some(value.as_str())))
                .map(|(_, reference)| reference)
                .into_iter()
                .collect(),
            Self::Multipart(parts) => parts
                .iter()
                .filter(|(when, _)| when.holds(block))
                .map(|(_, reference)| reference)
                .collect(),
        }
    }
}

/// `facing=east,open=true`. A key with no `=`, as in the old `normal`, asks nothing.
fn parse_properties(key: &str) -> Vec<(String, String)> {
    key.split(',')
        .filter_map(|prop| prop.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// A part's condition.
enum When {
    All(Vec<When>),
    Any(Vec<When>),
    /// The property has one of these values, or with `negated`, none of them.
    Is {
        key: String,
        values: Vec<String>,
        negated: bool,
    },
}

impl When {
    /// An object's entries must all hold. `OR` and `AND` hold lists of objects, and
    /// a value can list alternatives: `north|south`.
    fn parse(value: &Value) -> Self {
        let Some(object) = value.as_object() else {
            return Self::All(Vec::new());
        };
        let parts = object.iter().map(|(key, value)| {
            let list = || value.as_array().into_iter().flatten().map(Self::parse).collect();
            match key.as_str() {
                "OR" => Self::Any(list()),
                "AND" => Self::All(list()),
                _ => {
                    let text = value.as_str().map_or_else(|| value.to_string(), str::to_string);
                    let (negated, text) = text.strip_prefix('!').map_or((false, text.as_str()), |rest| (true, rest));
                    Self::Is {
                        key: key.clone(),
                        values: text.split('|').map(str::to_string).collect(),
                        negated,
                    }
                }
            }
        });
        Self::All(parts.collect())
    }

    fn holds(&self, block: &JavaBlock) -> bool {
        match self {
            Self::All(parts) => parts.iter().all(|part| part.holds(block)),
            Self::Any(parts) => parts.iter().any(|part| part.holds(block)),
            Self::Is { key, values, negated } => {
                block.get(key).is_some_and(|value| values.iter().any(|wanted| wanted == value)) != *negated
            }
        }
    }
}

/// A box of a model.
#[derive(Debug)]
struct Element {
    from: [f32; 3],
    to: [f32; 3],
    rotation: Option<Rotation>,
    /// Which way it's shaded as if it faced, whichever way it does. Plants say up,
    /// which is the same as no shade at all.
    shade: Option<Face>,
    faces: Vec<ElementFace>,
}

/// How a box is tilted, in the model.
#[derive(Debug)]
enum Rotation {
    /// About one axis. `rescale` stretches it to fill the block again: a plane on
    /// the diagonal is a sixteenth-grid block's width across, and ought to reach
    /// its corners.
    Axis {
        origin: [f32; 3],
        axis: usize,
        degrees: f32,
        rescale: bool,
    },
    /// About each axis in turn: x, then y, then z.
    Euler { origin: [f32; 3], degrees: [f32; 3] },
}

/// A face of a box.
#[derive(Debug)]
struct ElementFace {
    face: Face,
    /// The rectangle of the texture, or by default what's there at its position.
    uv: Option<[f32; 4]>,
    /// Which way round the texture lies, in quarter turns.
    quarters: usize,
    /// Which variable names its texture.
    variable: Rc<str>,
    cull: Option<Face>,
    tint: Option<i64>,
}

impl Element {
    fn parse(value: &Value) -> Option<Self> {
        let faces = value.get("faces")?.as_object()?;
        let faces = faces
            .iter()
            .filter_map(|(key, face)| {
                Some(ElementFace {
                    face: parse_face(key)?,
                    uv: floats(face.get("uv")),
                    quarters: face.get("rotation").and_then(Value::as_i64).unwrap_or(0).div_euclid(90).rem_euclid(4) as usize,
                    variable: face.get("texture")?.as_str()?.trim_start_matches('#').into(),
                    cull: face.get("cullface").and_then(Value::as_str).and_then(parse_face),
                    tint: face.get("tintindex").and_then(Value::as_i64),
                })
            })
            .collect();
        let shade = value
            .get("shade_direction_override")
            .and_then(Value::as_str)
            .and_then(parse_face)
            // Before 26.x, `"shade": false` said the same as `up` does now.
            .or_else(|| (value.get("shade").and_then(Value::as_bool) == Some(false)).then_some(Up));
        Some(Self {
            from: floats(value.get("from"))?,
            to: floats(value.get("to"))?,
            rotation: value.get("rotation").and_then(Rotation::parse),
            shade,
            faces,
        })
    }
}

impl Rotation {
    fn parse(value: &Value) -> Option<Self> {
        let origin = floats(value.get("origin"))?;
        let angle = |key| value.get(key).and_then(Value::as_f64).map(|degrees| degrees as f32);
        if let Some(axis) = value.get("axis").and_then(Value::as_str) {
            return Some(Self::Axis {
                origin,
                axis: ["x", "y", "z"].iter().position(|name| *name == axis)?,
                degrees: angle("angle")?,
                rescale: value.get("rescale").and_then(Value::as_bool).unwrap_or(false),
            });
        }
        let degrees = ["x", "y", "z"].map(|key| angle(key).unwrap_or(0.0));
        Some(Self::Euler { origin, degrees })
    }

    /// Where it takes a point.
    fn apply(&self, point: [f32; 3]) -> [f32; 3] {
        match *self {
            Self::Axis {
                origin,
                axis,
                degrees,
                rescale,
            } => {
                let mut local = turned_about(axis, degrees, sub(point, origin));
                if rescale {
                    let cos = degrees.to_radians().cos().abs();
                    if cos > 0.01 {
                        for (other, value) in local.iter_mut().enumerate() {
                            if other != axis {
                                *value /= cos;
                            }
                        }
                    }
                }
                add(local, origin)
            }
            Self::Euler { origin, degrees } => {
                let mut local = sub(point, origin);
                for (axis, degrees) in degrees.into_iter().enumerate() {
                    local = turned_about(axis, degrees, local);
                }
                add(local, origin)
            }
        }
    }
}

/// Turns a vector about an axis through the origin, by the right-hand rule.
fn turned_about(axis: usize, degrees: f32, [x, y, z]: [f32; 3]) -> [f32; 3] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    match axis {
        0 => [x, cos * y - sin * z, sin * y + cos * z],
        1 => [cos * x + sin * z, y, cos * z - sin * x],
        _ => [cos * x - sin * y, sin * x + cos * y, z],
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn parse_face(name: &str) -> Option<Face> {
    Face::ALL.into_iter().find(|face| face.key() == name)
}

fn floats<const N: usize>(value: Option<&Value>) -> Option<[f32; N]> {
    let values = value?.as_array()?;
    let mut out = [0.0; N];
    if values.len() != N {
        return None;
    }
    for (out, value) in out.iter_mut().zip(values) {
        let value = value.as_f64()? as f32;
        if !value.is_finite() {
            return None;
        }
        *out = value;
    }
    Some(out)
}

fn without_namespace(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// One model file, with nothing taken from its parents yet.
struct ModelFile {
    parent: Option<Rc<str>>,
    /// A variable's value can be another variable: `#side`.
    textures: Vec<(String, String)>,
    elements: Option<Rc<[Element]>>,
}

impl ModelFile {
    fn parse(value: &Value) -> Self {
        let textures = value.get("textures").and_then(Value::as_object).into_iter().flatten();
        let textures = textures
            .filter_map(|(key, value)| {
                // 26.x can write one as an object, with its sprite inside.
                let value = value.as_str().or_else(|| value.get("sprite")?.as_str())?;
                Some((key.clone(), value.to_string()))
            })
            .collect();
        let elements = value.get("elements").and_then(Value::as_array);
        Self {
            parent: value
                .get("parent")
                .and_then(Value::as_str)
                .map(|parent| without_namespace(parent).into()),
            textures,
            elements: elements.map(|elements| elements.iter().filter_map(Element::parse).collect()),
        }
    }
}

/// A model with its parents followed.
struct Resolved {
    /// The model that has the boxes, which every model built on it shares.
    owner: Rc<str>,
    elements: Rc<[Element]>,
    textures: HashMap<String, String>,
}

impl Resolved {
    /// The texture a face's variable comes to, without its namespace. `None`: it's
    /// not set, and Java would draw its missing texture on the face.
    fn texture(&self, variable: &str) -> Option<&str> {
        let mut name = variable;
        for _ in 0..MAX_PARENTS {
            let value = self.textures.get(name)?;
            match value.strip_prefix('#') {
                Some(next) => name = next,
                None => return Some(without_namespace(value)),
            }
        }
        None
    }
}

/// A model's boxes, baked and turned, before knowing its textures.
struct Geometry {
    /// Each quad's slot is an index into `variables`.
    quads: Vec<ModelQuad>,
    variables: Vec<(Rc<str>, Option<i64>)>,
}

struct Compiler<R> {
    read: R,
    block_files: HashMap<&'static str, Option<Rc<BlockFile>>>,
    model_files: HashMap<Rc<str>, Option<Rc<ModelFile>>>,
    resolved: HashMap<Rc<str>, Option<Rc<Resolved>>>,
    geometry: HashMap<(Rc<str>, Turn, bool), Rc<Geometry>>,
    /// The references counted in `baked_quads`.
    counted: HashSet<ModelRef>,
    files: usize,
    json_bytes: usize,
    baked_quads: usize,
}

impl<R: FnMut(&str) -> Option<Vec<u8>>> Compiler<R> {
    fn new(read: R) -> Self {
        Self {
            read,
            block_files: HashMap::new(),
            model_files: HashMap::new(),
            resolved: HashMap::new(),
            geometry: HashMap::new(),
            counted: HashSet::new(),
            files: 0,
            json_bytes: 0,
            baked_quads: 0,
        }
    }

    fn read_json(&mut self, entry: &str) -> Option<Value> {
        let bytes = (self.read)(entry)?;
        self.files += 1;
        self.json_bytes += bytes.len();
        serde_json::from_slice(&bytes).ok()
    }

    fn block_file(&mut self, name: &'static str) -> Option<Rc<BlockFile>> {
        if let Some(file) = self.block_files.get(name) {
            return file.clone();
        }
        let file = self
            .read_json(&format!("assets/minecraft/blockstates/{name}.json"))
            .and_then(|value| BlockFile::parse(&value))
            .map(Rc::new);
        self.block_files.insert(name, file.clone());
        file
    }

    fn model_file(&mut self, name: &Rc<str>) -> Option<Rc<ModelFile>> {
        if let Some(file) = self.model_files.get(name) {
            return file.clone();
        }
        let file = self
            .read_json(&format!("assets/minecraft/models/{name}.json"))
            .map(|value| Rc::new(ModelFile::parse(&value)));
        self.model_files.insert(name.clone(), file.clone());
        file
    }

    /// Follows a model's parents. A child's variables win, and the nearest boxes
    /// are the model's. `None`: it has no boxes.
    fn resolve(&mut self, name: &Rc<str>) -> Option<Rc<Resolved>> {
        if let Some(resolved) = self.resolved.get(name) {
            return resolved.clone();
        }
        let mut textures = HashMap::new();
        let mut boxes = None;
        let mut at = Some(name.clone());
        for _ in 0..MAX_PARENTS {
            let Some(file) = at.as_ref().and_then(|name| self.model_file(name)) else {
                break;
            };
            for (key, value) in &file.textures {
                textures.entry(key.clone()).or_insert_with(|| value.clone());
            }
            if boxes.is_none()
                && let (Some(elements), Some(owner)) = (&file.elements, &at)
            {
                boxes = Some((owner.clone(), elements.clone()));
            }
            at = file.parent.clone();
        }
        let resolved = boxes.map(|(owner, elements)| {
            Rc::new(Resolved {
                owner,
                elements,
                textures,
            })
        });
        self.resolved.insert(name.clone(), resolved.clone());
        resolved
    }

    fn geometry_of(&mut self, resolved: &Resolved, turn: Turn, uvlock: bool) -> Rc<Geometry> {
        self.geometry
            .entry((resolved.owner.clone(), turn, uvlock))
            .or_insert_with(|| Rc::new(bake(&resolved.elements, turn, uvlock)))
            .clone()
    }

    fn state(&mut self, block: &JavaBlock) -> Compiled {
        let Some(file) = self.block_file(block.name) else {
            return Compiled::NoFile;
        };
        let mut quads = Vec::new();
        let mut slots: Vec<JavaSlot> = Vec::new();
        for reference in file.select(block) {
            let Some(resolved) = self.resolve(&reference.model) else {
                continue;
            };
            let geometry = self.geometry_of(&resolved, reference.turn, reference.uvlock);
            if self.counted.insert(reference.clone()) {
                self.baked_quads += geometry.quads.len();
            }
            // This model's variables, as slots of this state.
            let slot_of: Vec<Option<u8>> = geometry
                .variables
                .iter()
                .map(|(variable, tint)| {
                    let texture = resolved.texture(variable)?;
                    let tinted = takes_tint(block.name, *tint);
                    let slot = slots
                        .iter()
                        .position(|slot| &*slot.texture == texture && slot.tinted == tinted)
                        .unwrap_or_else(|| {
                            slots.push(JavaSlot {
                                texture: texture.into(),
                                tinted,
                            });
                            slots.len() - 1
                        });
                    Some(slot as u8)
                })
                .collect();
            quads.extend(geometry.quads.iter().filter_map(|quad| {
                Some(ModelQuad {
                    slot: slot_of[usize::from(quad.slot)]?,
                    ..*quad
                })
            }));
        }
        if quads.is_empty() {
            Compiled::Empty
        } else if is_full_cube(&quads) {
            Compiled::Cube
        } else if slots.len() > SLOTS {
            Compiled::TooManyTextures
        } else {
            Compiled::Look(quads, slots)
        }
    }
}

/// Whether a face with this `tintindex` takes its block's tint. A number is the
/// question Java's code answers for each block. For flower beds it's the stems,
/// 1, that are grass-coloured, and not the petals, 0. Lava and powder snow share
/// their model with a cauldron's water, which is the only one with a colour.
fn takes_tint(block: &str, index: Option<i64>) -> bool {
    match index {
        None => false,
        Some(0) => !matches!(block, "pink_petals" | "wildflowers" | "lava_cauldron" | "powder_snow_cauldron"),
        Some(_) => true,
    }
}

/// Bakes a model's boxes.
fn bake(elements: &[Element], turn: Turn, uvlock: bool) -> Geometry {
    let mut geometry = Geometry {
        quads: Vec::new(),
        variables: Vec::new(),
    };
    for element in elements {
        for face in &element.faces {
            let corners = box_corners(face.face, element.from, element.to).map(|corner| {
                let tilted = element.rotation.as_ref().map_or(corner, |rotation| rotation.apply(corner));
                turn.corner_fine(tilted.map(fine_position))
            });
            // No area, so nothing to see: a plane's edges, if a file gives them.
            let Some(facing) = facing_of(&corners) else {
                continue;
            };
            let rect = face.uv.unwrap_or_else(|| default_uv(face.face, element.from, element.to));
            let mut uvs = rect_corners(rect, face.quarters);
            if uvlock {
                uvs = locked(uvs, face.face, turn);
            }
            let key = (face.variable.clone(), face.tint);
            let slot = geometry
                .variables
                .iter()
                .position(|variable| *variable == key)
                .unwrap_or_else(|| {
                    geometry.variables.push(key);
                    geometry.variables.len() - 1
                });
            geometry.quads.push(ModelQuad {
                corners,
                uvs: uvs.map(|uv| uv.map(fine_uv)),
                face: element.shade.unwrap_or(facing) as u8,
                slot: slot as u8,
                cull: face.cull.map(|cull| turn.face(cull)),
            });
        }
    }
    geometry
}

/// A box face's corners, in the order of [`box_face`].
fn box_corners(face: Face, from: [f32; 3], to: [f32; 3]) -> [[f32; 3]; 4] {
    // Asked for a unit box, it says which end of the box each coordinate is at.
    box_face(face, [0; 3], [1; 3]).map(|corner| [0, 1, 2].map(|axis| if corner[axis] == 0 { from[axis] } else { to[axis] }))
}

/// The rectangle a face shows if it doesn't say: what's there at its position.
fn default_uv(face: Face, [x0, y0, z0]: [f32; 3], [x1, y1, z1]: [f32; 3]) -> [f32; 4] {
    match face {
        Down => [x0, 16.0 - z1, x1, 16.0 - z0],
        Up => [x0, z0, x1, z1],
        North => [16.0 - x1, 16.0 - y1, 16.0 - x0, 16.0 - y0],
        South => [x0, 16.0 - y1, x1, 16.0 - y0],
        West => [z0, 16.0 - y1, z1, 16.0 - y0],
        East => [16.0 - z1, 16.0 - y1, 16.0 - z0, 16.0 - y0],
    }
}

/// A rectangle's coordinates at a quad's corners, turned this many quarters.
fn rect_corners([u0, v0, u1, v1]: [f32; 4], quarters: usize) -> [[f32; 2]; 4] {
    let corners = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
    [0, 1, 2, 3].map(|i| corners[(i + quarters) % 4])
}

/// Texture coordinates after `uvlock`, which keeps a texture where it was as its
/// box turns (worth teaching).
///
/// A square texture has eight ways to lie on itself. Whichever turning the block
/// does to a face, one of them puts every texel back where a whole face would
/// show it. So this finds that one, from the corners of a whole face, and applies
/// it to whatever rectangle the face shows.
fn locked(uvs: [[f32; 2]; 4], face: Face, turn: Turn) -> [[f32; 2]; 4] {
    let landed = turn.face(face);
    let corners = box_face(face, [0; 3], [16; 3]);
    let before = corners.map(|corner| face_uv(face, corner).map(f32::from));
    let after = corners.map(|corner| face_uv(landed, turn.corner(corner)).map(f32::from));
    for swap in [false, true] {
        for flip_u in [false, true] {
            for flip_v in [false, true] {
                let map = |[u, v]: [f32; 2]| {
                    let (u, v) = if swap { (v, u) } else { (u, v) };
                    [if flip_u { 16.0 - u } else { u }, if flip_v { 16.0 - v } else { v }]
                };
                if before.iter().zip(&after).all(|(&before, after)| map(before) == *after) {
                    return uvs.map(map);
                }
            }
        }
    }
    uvs
}

/// Sixteenths of a block, in baked units.
fn fine_position(value: f32) -> i16 {
    (value * f32::from(POSITION_SCALE)).round().clamp(-MAX_CORNER, MAX_CORNER) as i16
}

/// Sixteenths of a texture, in baked units.
fn fine_uv(value: f32) -> u8 {
    (value * f32::from(UV_SCALE)).round().clamp(0.0, 16.0 * f32::from(UV_SCALE)) as u8
}

/// The way a quad faces, to the nearest face. `None`: it has no area.
fn facing_of(corners: &[[i16; 3]; 4]) -> Option<Face> {
    let [a, b, c, _] = corners.map(|corner| corner.map(i64::from));
    let (e, f) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let normal = [e[1] * f[2] - e[2] * f[1], e[2] * f[0] - e[0] * f[2], e[0] * f[1] - e[1] * f[0]];
    if normal == [0; 3] {
        return None;
    }
    let axis = (0..3).max_by_key(|&axis| normal[axis].abs())?;
    Some(match (axis, normal[axis] > 0) {
        (0, false) => West,
        (0, true) => East,
        (1, false) => Down,
        (1, true) => Up,
        (_, false) => North,
        (_, true) => South,
    })
}

/// Whether these quads are just a block's six whole faces, however many layers
/// of them: a grass block has an overlay.
fn is_full_cube(quads: &[ModelQuad]) -> bool {
    let mut covered = 0u8;
    for quad in quads {
        let mut plane = None;
        for axis in 0..3 {
            let values = quad.corners.map(|corner| corner[axis]);
            let (low, high) = (values.into_iter().min(), values.into_iter().max());
            match (low, high) {
                (Some(0), Some(WHOLE)) => {}
                (Some(low), Some(high)) if low == high && (low == 0 || low == WHOLE) && plane.is_none() => {
                    plane = Some(axis * 2 + usize::from(low == WHOLE));
                }
                _ => return false,
            }
        }
        match plane {
            Some(plane) => covered |= 1 << plane,
            None => return false,
        }
    }
    covered == 0b11_1111
}

#[cfg(test)]
mod tests;
