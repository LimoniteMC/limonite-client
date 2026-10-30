//! The compiler, on model files made up for these tests: no test holds or needs
//! Mojang's (m5-plan Step 7c). The last one reads a real jar, if it's given one.

use std::collections::BTreeMap;

use pumpkin_data::{Block, BlockStateId};

use super::*;
use crate::blocks::{
    jar::{FoundBy, Jar},
    shapes,
};

const S: i16 = POSITION_SCALE;
const T: u8 = UV_SCALE;

/// A jar of files given as text, by a short name: `block/a` is a model, and any
/// other a block.
fn read_from(files: Vec<(&'static str, &'static str)>) -> impl FnMut(&str) -> Option<Vec<u8>> {
    let files: HashMap<String, &str> = files
        .into_iter()
        .map(|(name, text)| {
            let entry = if name.starts_with("block/") {
                format!("assets/minecraft/models/{name}.json")
            } else {
                format!("assets/minecraft/blockstates/{name}.json")
            };
            (entry, text)
        })
        .collect();
    move |entry| files.get(entry).map(|text| text.as_bytes().to_vec())
}

fn block(name: &'static str, props: &[(&'static str, &'static str)]) -> JavaBlock {
    JavaBlock::new(name, props.to_vec())
}

/// Compiles one state.
fn one(files: Vec<(&'static str, &'static str)>, state: &JavaBlock) -> JavaModels {
    compile(read_from(files), [(1, state)].into_iter())
}

fn look_of(files: Vec<(&'static str, &'static str)>, state: &JavaBlock) -> JavaLook {
    let mut models = one(files, state);
    models.looks.remove(&1).expect("it has a look")
}

const TILE: &str = r#"{"variants": {"": {"model": "minecraft:block/tile"}}}"#;
/// A box from the floor, a quarter of a block across and half a block tall.
const TILE_MODEL: &str = r##"{
    "textures": {"all": "minecraft:block/stone"},
    "elements": [{"from": [4, 0, 4], "to": [8, 8, 8], "faces": {
        "north": {"texture": "#all"},
        "up": {"texture": "#all", "cullface": "up"}
    }}]
}"##;

#[test]
fn a_box_bakes_to_quads_in_baked_units() {
    let look = look_of(vec![("tile", TILE), ("block/tile", TILE_MODEL)], &block("tile", &[]));
    assert_eq!(look.slots.len(), 1);
    assert_eq!(&*look.slots[0].texture, "block/stone");
    assert!(!look.slots[0].tinted);
    assert_eq!(look.quads.len(), 2);
    let north = look.quads.iter().find(|quad| quad.face == North as u8).expect("a north face");
    assert_eq!(north.corners[0], [8 * S, 8 * S, 4 * S], "the texture's top left, seen from outside");
    assert_eq!(north.corners[2], [4 * S, 0, 4 * S]);
    // By default it shows the part of the texture at its position.
    assert_eq!(north.uvs[0], [8 * T, 8 * T]);
    assert_eq!(north.uvs[2], [12 * T, 16 * T]);
    assert_eq!(north.cull, None);
    let up = look.quads.iter().find(|quad| quad.face == Up as u8).expect("an up face");
    assert_eq!(up.cull, Some(Up));
}

#[test]
fn a_parent_has_the_boxes_and_a_child_says_the_textures() {
    let child = r##"{"parent": "block/template", "textures": {"side": "block/oak"}}"##;
    let template = r##"{
        "parent": "block/bottom",
        "textures": {"all": "#side", "side": "block/never"},
        "elements": [{"from": [0, 0, 0], "to": [8, 8, 8], "faces": {"up": {"texture": "#all"}}}]
    }"##;
    let bottom = r##"{"elements": [{"from": [0, 0, 0], "to": [1, 1, 1], "faces": {"up": {"texture": "#never"}}}]}"##;
    let state = r#"{"variants": {"": {"model": "block/child"}}}"#;
    let look = look_of(
        vec![
            ("thing", state),
            ("block/child", child),
            ("block/template", template),
            ("block/bottom", bottom),
        ],
        &block("thing", &[]),
    );
    assert_eq!(&*look.slots[0].texture, "block/oak", "the child's variable wins");
    assert_eq!(look.quads.len(), 1, "and the nearest boxes are the model's");
    assert_eq!(look.quads[0].corners[1][2], 8 * S);
}

#[test]
fn blocks_that_differ_only_by_texture_share_their_quads() {
    let (a, b) = (block("a", &[]), block("b", &[]));
    let template = r##"{"elements": [{"from": [4, 0, 4], "to": [8, 8, 8], "faces": {"up": {"texture": "#t"}}}]}"##;
    let files = vec![
        ("a", r#"{"variants": {"": {"model": "block/a"}}}"#),
        ("b", r#"{"variants": {"": {"model": "block/b"}}}"#),
        ("block/a", r#"{"parent": "block/template", "textures": {"t": "block/oak"}}"#),
        ("block/b", r#"{"parent": "block/template", "textures": {"t": "block/spruce"}}"#),
        ("block/template", template),
    ];
    let models = compile(read_from(files), [(1, &a), (2, &b)].into_iter());
    let (one, two) = (models.look(1).unwrap(), models.look(2).unwrap());
    assert!(std::ptr::eq(one.quads, two.quads), "one list of quads");
    assert_eq!(&*one.slots[0].texture, "block/oak");
    assert_eq!(&*two.slots[0].texture, "block/spruce");
    assert_eq!(models.stats.shapes, 1);
    assert_eq!(models.stats.kept_quads, 1);
    assert_eq!(models.stats.baked_quads, 2, "baked apart, they'd be two");
}

#[test]
fn a_state_shows_the_variant_it_matches() {
    let state = r#"{"variants": {
        "facing=north,open=true": {"model": "block/a"},
        "facing=north,open=false": {"model": "block/b"},
        "facing=east": [{"model": "block/c", "weight": 3}, {"model": "block/a"}]
    }}"#;
    let model = |texture: &'static str| -> &'static str {
        match texture {
            "a" => r##"{"textures": {"t": "block/a"}, "elements": [{"from": [0,0,0], "to": [1,1,1], "faces": {"up": {"texture": "#t"}}}]}"##,
            "b" => r##"{"textures": {"t": "block/b"}, "elements": [{"from": [0,0,0], "to": [1,1,1], "faces": {"up": {"texture": "#t"}}}]}"##,
            _ => r##"{"textures": {"t": "block/c"}, "elements": [{"from": [0,0,0], "to": [1,1,1], "faces": {"up": {"texture": "#t"}}}]}"##,
        }
    };
    let files = || vec![("door", state), ("block/a", model("a")), ("block/b", model("b")), ("block/c", model("c"))];
    let shows = |props: &[(&'static str, &'static str)]| {
        let look = look_of(files(), &block("door", props));
        look.slots[0].texture.to_string()
    };
    assert_eq!(shows(&[("facing", "north"), ("open", "true")]), "block/a");
    assert_eq!(shows(&[("facing", "north"), ("open", "false")]), "block/b");
    assert_eq!(shows(&[("facing", "east"), ("open", "false")]), "block/c", "a list shows its first");
}

#[test]
fn multipart_draws_every_part_that_holds() {
    let state = r#"{"multipart": [
        {"apply": {"model": "block/post"}},
        {"when": {"north": "true"}, "apply": {"model": "block/arm"}},
        {"when": {"east": "true", "up": "false"}, "apply": {"model": "block/arm", "y": 90}},
        {"when": {"OR": [{"south": "true"}, {"AND": [{"west": "true"}, {"up": "true"}]}]}, "apply": {"model": "block/arm", "y": 180}},
        {"when": {"west": "low|tall"}, "apply": {"model": "block/arm", "y": 270}},
        {"when": {"up": "!true"}, "apply": {"model": "block/cap"}}
    ]}"#;
    let quad = |x: i32| -> String {
        format!(
            r##"{{"textures": {{"t": "block/x"}}, "elements": [{{"from": [{x},0,0], "to": [{},1,1], "faces": {{"up": {{"texture": "#t"}}}}}}]}}"##,
            x + 1
        )
    };
    let (post, arm, cap) = (quad(0).leak() as &str, quad(2).leak() as &str, quad(4).leak() as &str);
    let count = |props: &[(&'static str, &'static str)]| {
        let files = vec![("fence", state), ("block/post", post), ("block/arm", arm), ("block/cap", cap)];
        look_of(files, &block("fence", props)).quads.len()
    };
    assert_eq!(count(&[("up", "true")]), 1, "just the post");
    assert_eq!(count(&[("up", "false")]), 2, "the post and the cap");
    assert_eq!(count(&[("north", "true"), ("up", "true")]), 2);
    assert_eq!(count(&[("east", "true"), ("up", "false")]), 3, "both of an implicit AND");
    assert_eq!(count(&[("east", "true"), ("up", "true")]), 1, "only one of them");
    assert_eq!(count(&[("south", "true"), ("up", "true")]), 2, "one of an OR");
    assert_eq!(count(&[("west", "true"), ("up", "true")]), 2, "an AND inside an OR");
    assert_eq!(count(&[("west", "low"), ("up", "true")]), 2, "either value");
    assert_eq!(count(&[("west", "tall"), ("up", "true")]), 2);
    assert_eq!(count(&[("west", "none"), ("up", "true")]), 1, "neither");
}

#[test]
fn a_turn_moves_the_corners_and_the_face_that_hides_them() {
    let state = r#"{"variants": {"": {"model": "block/tile", "y": 90}}}"#;
    let model = r##"{"textures": {"t": "block/x"}, "elements": [
        {"from": [0, 0, 0], "to": [4, 8, 2], "faces": {"north": {"texture": "#t", "cullface": "north"}}}
    ]}"##;
    let look = look_of(vec![("tile", state), ("block/tile", model)], &block("tile", &[]));
    let quad = look.quads[0];
    // A quarter turn clockwise from above takes north to east.
    assert_eq!(quad.face, East as u8);
    assert_eq!(quad.cull, Some(East));
    assert!(quad.corners.iter().all(|corner| corner[0] == 16 * S));
    // The texture goes with it, unless it's locked.
    assert_eq!(quad.uvs[0], [12 * T, 8 * T]);
}

#[test]
fn uvlock_leaves_a_texture_where_it_was() {
    // The east half of the top of a block, turned a quarter to be its south half.
    let state = |y| -> &'static str {
        format!(r#"{{"variants": {{"": {{"model": "block/half", "y": {y}, "uvlock": true}}}}}}"#).leak()
    };
    let model = r##"{"textures": {"t": "block/x"}, "elements": [
        {"from": [8, 0, 0], "to": [16, 16, 16], "faces": {"up": {"texture": "#t"}, "north": {"texture": "#t"}}}
    ]}"##;
    for y in [0, 90, 180, 270] {
        let look = look_of(vec![("half", state(y)), ("block/half", model)], &block("half", &[]));
        for quad in look.quads {
            let landed = match quad.face {
                1 => Up,
                2 => North,
                3 => South,
                4 => West,
                _ => East,
            };
            // Wherever it landed, each corner shows what a whole face has there.
            for (corner, uv) in quad.corners.iter().zip(quad.uvs) {
                let at = corner.map(|value| (value / S) as i8);
                assert_eq!(uv, face_uv(landed, at).map(|value| value * T), "y {y}: {quad:?}");
            }
        }
    }
}

#[test]
fn a_faces_own_rectangle_and_rotation_are_kept() {
    let state = r#"{"variants": {"": {"model": "block/tile"}}}"#;
    let model = r##"{"textures": {"t": "block/x"}, "elements": [
        {"from": [0, 0, 0], "to": [16, 16, 16], "faces": {
            "north": {"texture": "#t", "uv": [1, 2, 3.5, 4.25]},
            "south": {"texture": "#t", "uv": [1, 2, 3, 4], "rotation": 90},
            "east": {"texture": "t", "uv": [3, 2, 1, 4]}
        }}
    ]}"##;
    let look = look_of(vec![("tile", state), ("block/tile", model)], &block("tile", &[]));
    let uvs = |face: Face| look.quads.iter().find(|quad| quad.face == face as u8).expect("the face").uvs;
    assert_eq!(uvs(North), [[8, 16], [8, 34], [28, 34], [28, 16]], "fractions survive, top left first");
    assert_eq!(uvs(South), [[8, 32], [24, 32], [24, 16], [8, 16]], "each corner takes the next one's");
    assert_eq!(uvs(East), [[24, 16], [24, 32], [8, 32], [8, 16]], "reversed mirrors, and # is optional");
}

#[test]
fn a_tilted_box_turns_about_its_origin_and_can_rescale() {
    let tilted = |rescale: bool| -> &'static str {
        format!(
            r##"{{"textures": {{"t": "block/x"}}, "elements": [
                {{"from": [0, 0, 8], "to": [16, 16, 8],
                  "rotation": {{"origin": [8, 8, 8], "axis": "y", "angle": 45, "rescale": {rescale}}},
                  "shade_direction_override": "up",
                  "faces": {{"north": {{"texture": "#t"}}}}}}
            ]}}"##
        )
        .leak()
    };
    let state = r#"{"variants": {"": {"model": "block/plane"}}}"#;
    let corners = |rescale| {
        let look = look_of(vec![("plane", state), ("block/plane", tilted(rescale))], &block("plane", &[]));
        assert_eq!(look.quads[0].face, Up as u8, "it's shaded as it says");
        look.quads[0].corners
    };
    let half_diagonal = 8.0 / std::f32::consts::SQRT_2;
    let fine = |value: f32| (value * f32::from(S)).round() as i16;
    // The north face's first corner is its top right from above: east, at x = 16.
    let plain = corners(false)[0];
    assert_eq!(plain[1], 16 * S);
    assert_eq!(plain[0], fine(8.0 + half_diagonal));
    // Positive angles turn by the right-hand rule: about y, x goes towards -z.
    assert_eq!(plain[2], fine(8.0 - half_diagonal));
    // Rescaled, it reaches the block's corners again.
    let stretched = corners(true)[0];
    assert_eq!(stretched[0], 16 * S);
    assert_eq!(stretched[2], 0);
    assert_eq!(stretched[1], 16 * S, "the axis it turned about doesn't stretch");
}

#[test]
fn euler_rotations_turn_about_each_axis_in_turn() {
    let rotation = Rotation::Euler {
        origin: [8.0; 3],
        degrees: [90.0, 90.0, 0.0],
    };
    let [x, y, z] = rotation.apply([16.0, 8.0, 8.0]);
    // Along +x, x leaves it alone, and y turns it towards -z.
    assert!((x - 8.0).abs() < 1e-4 && (y - 8.0).abs() < 1e-4 && z.abs() < 1e-4, "{x} {y} {z}");
}

#[test]
fn a_cube_stays_with_the_tables_and_nothing_to_draw_is_empty() {
    let cube = r##"{"textures": {"a": "block/a", "b": "block/b"}, "elements": [
        {"from": [0,0,0], "to": [16,16,16], "faces": {
            "down": {"texture": "#a"}, "up": {"texture": "#a"}, "north": {"texture": "#a"},
            "south": {"texture": "#a"}, "west": {"texture": "#a"}, "east": {"texture": "#a"}}},
        {"from": [0,0,0], "to": [16,16,16], "faces": {"north": {"texture": "#b", "tintindex": 0}}}
    ]}"##;
    let open_top = r##"{"textures": {"a": "block/a"}, "elements": [
        {"from": [0,0,0], "to": [16,16,16], "faces": {
            "down": {"texture": "#a"}, "north": {"texture": "#a"},
            "south": {"texture": "#a"}, "west": {"texture": "#a"}, "east": {"texture": "#a"}}}
    ]}"##;
    let thin = r##"{"textures": {"a": "block/a"}, "elements": [
        {"from": [0,0,0], "to": [16,15,16], "faces": {
            "down": {"texture": "#a"}, "up": {"texture": "#a"}, "north": {"texture": "#a"},
            "south": {"texture": "#a"}, "west": {"texture": "#a"}, "east": {"texture": "#a"}}}
    ]}"##;
    let state = r#"{"variants": {"": {"model": "block/m"}}}"#;
    let cube_stats = one(vec![("c", state), ("block/m", cube)], &block("c", &[])).stats;
    assert_eq!((cube_stats.cubes, cube_stats.looks), (1, 0), "an overlay is still a cube");
    assert_eq!(one(vec![("c", state), ("block/m", open_top)], &block("c", &[])).stats.looks, 1);
    assert_eq!(one(vec![("c", state), ("block/m", thin)], &block("c", &[])).stats.looks, 1);
    let empty = r#"{"textures": {"particle": "block/a"}}"#;
    assert_eq!(one(vec![("c", state), ("block/m", empty)], &block("c", &[])).stats.empty, 1);
    let stats = one(vec![], &block("c", &[])).stats;
    assert!(stats.no_file.contains("c"));
}

#[test]
fn flower_beds_tint_their_stems_and_others_what_they_mark() {
    let model = r##"{"textures": {"p": "block/petal", "s": "block/stem"}, "elements": [
        {"from": [0,0,0], "to": [8,1,8], "faces": {"up": {"texture": "#p", "tintindex": 0}}},
        {"from": [0,0,0], "to": [1,8,1], "faces": {"north": {"texture": "#s", "tintindex": 1}}},
        {"from": [8,0,0], "to": [9,8,1], "faces": {"north": {"texture": "#s"}}}
    ]}"##;
    let state = r#"{"variants": {"": {"model": "block/m"}}}"#;
    let tints = |name: &'static str| {
        let look = look_of(vec![(name, state), ("block/m", model)], &block(name, &[]));
        look.slots.iter().map(|slot| slot.tinted).collect::<Vec<_>>()
    };
    assert_eq!(tints("pink_petals"), [false, true, false]);
    assert_eq!(tints("wildflowers"), [false, true, false]);
    assert_eq!(tints("leaf_litter"), [true, true, false], "the same texture can be in two slots");
}

#[test]
fn a_look_has_a_limit_on_its_textures() {
    let mut faces = String::new();
    for i in 0..=SLOTS {
        let from = i * 2;
        faces += &format!(
            r##"{{"from": [{from},0,0], "to": [{},1,1], "faces": {{"up": {{"texture": "#t{i}"}}}}}},"##,
            from + 1
        );
    }
    let textures: Vec<String> = (0..=SLOTS).map(|i| format!(r#""t{i}": "block/{i}""#)).collect();
    let model: &'static str = format!(
        r#"{{"textures": {{{}}}, "elements": [{}]}}"#,
        textures.join(","),
        faces.trim_end_matches(',')
    )
    .leak();
    let state = r#"{"variants": {"": {"model": "block/m"}}}"#;
    let stats = one(vec![("many", state), ("block/m", model)], &block("many", &[])).stats;
    assert!(stats.too_many_textures.contains("many"));
}

#[test]
fn damaged_files_come_to_nothing_and_never_panic() {
    let state = r#"{"variants": {"": {"model": "block/m"}}}"#;
    let box_with = |rest: &'static str| -> &'static str {
        format!(r##"{{"textures": {{"t": "block/x"}}, "elements": [{{{rest}}}]}}"##).leak()
    };
    let bad_models = [
        "not json",
        "[]",
        r#"{"parent": "block/m"}"#,
        r#"{"parent": 3}"#,
        r#"{"elements": 3}"#,
        r#"{"elements": [3]}"#,
        r#"{"elements": [{}]}"#,
        box_with(r#""from": [0,0], "to": [1,1,1], "faces": {}"#),
        box_with(r##""from": [0,0,0], "to": [1,1,1], "faces": {"sideways": {"texture": "#t"}}"##),
        box_with(r#""from": [0,0,0], "to": [1,1,1], "faces": {"up": {}}"#),
        box_with(r##""from": [0,0,0], "to": [1,1,1], "faces": {"up": {"texture": "#unset"}}"##),
        box_with(r##""from": [0,0,0], "to": [0,1,1], "faces": {"up": {"texture": "#t"}}"##),
    ];
    for model in bad_models {
        let stats = one(vec![("m", state), ("block/m", model)], &block("m", &[])).stats;
        assert_eq!(stats.looks, 0, "{model}");
    }
    let looping = r##"{"textures": {"a": "#b", "b": "#a"}, "elements": [
        {"from": [0,0,0], "to": [1,1,1], "faces": {"up": {"texture": "#a"}}}
    ]}"##;
    assert_eq!(one(vec![("m", state), ("block/m", looping)], &block("m", &[])).stats.looks, 0);
    // Absurd numbers stay within what a vertex can hold.
    let far = box_with(r##""from": [-99999,0,0], "to": [99999,1e30,1], "faces": {"north": {"texture": "#t", "uv": [-5,0,999,1e30]}}"##);
    let look = look_of(vec![("m", state), ("block/m", far)], &block("m", &[]));
    assert!(look.quads[0].corners.iter().flatten().all(|&value| i32::from(value).abs() <= 4096));
    assert!(look.quads[0].uvs.iter().flatten().all(|&value| value <= 16 * T));
    for bad_state in ["not json", "{}", r##"{"variants": 3}"#, r##"{"multipart": [{}]}"#, r##"{"variants": {"": {}}}"##] {
        let stats = one(vec![("m", bad_state), ("block/m", TILE_MODEL)], &block("m", &[])).stats;
        assert_eq!(stats.looks, 0, "{bad_state}");
    }
}

/// Writes every model of a real jar, baked unturned, and the model files, for
/// `scripts/compare-models-with-blockbench.mjs` to compare with what Blockbench
/// builds from them (m5-plan Step 7c). It's all derived from Mojang's files, so
/// `LIMONITE_MODEL_DUMP` must name a gitignored folder.
#[test]
#[ignore = "needs a Java client jar; set LIMONITE_JAVA_JAR and LIMONITE_MODEL_DUMP"]
fn dump_every_model_for_comparing() {
    let path = std::env::var_os("LIMONITE_JAVA_JAR").expect("LIMONITE_JAVA_JAR is set");
    let out = std::env::var_os("LIMONITE_MODEL_DUMP").expect("LIMONITE_MODEL_DUMP is set");
    let mut jar = Jar::open(std::path::Path::new(&path), FoundBy::Flag).unwrap();
    let mut compiler = Compiler::new(|entry: &str| jar.read(entry));
    for state in (0..BlockStateId::COUNT).filter_map(BlockStateId::new) {
        let _ = compiler.state(&JavaBlock::of(state));
    }
    // A relative path means from the repo's root: tests run in the crate's folder.
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(out);
    std::fs::create_dir_all(&folder).unwrap();
    let mut models = serde_json::Map::new();
    for (name, resolved) in &compiler.resolved {
        let Some(resolved) = resolved else {
            continue;
        };
        let geometry = bake(&resolved.elements, Turn::NONE, false);
        let quads: Vec<serde_json::Value> = geometry
            .quads
            .iter()
            .map(|quad| {
                let (variable, tint) = &geometry.variables[usize::from(quad.slot)];
                serde_json::json!({
                    "corners": quad.corners.map(|corner| corner.map(|value| f32::from(value) / f32::from(S))),
                    "uvs": quad.uvs.map(|uv| uv.map(|value| f32::from(value) / f32::from(T))),
                    "face": quad.face,
                    "cull": quad.cull.map(Face::key),
                    "texture": resolved.texture(variable),
                    "tint": tint,
                })
            })
            .collect();
        models.insert(name.to_string(), quads.into());
    }
    println!("{} models", models.len());
    std::fs::write(folder.join("limonite-models.json"), serde_json::Value::Object(models).to_string()).unwrap();
    let names: Vec<Rc<str>> = compiler.model_files.keys().cloned().collect();
    drop(compiler);
    let mut files = serde_json::Map::new();
    for name in names {
        if let Some(bytes) = jar.read(&format!("assets/minecraft/models/{name}.json")) {
            files.insert(format!("models/{name}.json"), String::from_utf8_lossy(&bytes).into_owned().into());
        }
    }
    std::fs::write(folder.join("model-files.json"), serde_json::Value::Object(files).to_string()).unwrap();
}

/// Java states that share a Bedrock id and don't look the same, from a real jar. Each
/// such property is something Bedrock keeps elsewhere or works out itself, so the
/// model drawn is the one for its default. It prints which, by block, and fails on
/// any it can't explain, so a new one can't slip through.
#[test]
#[ignore = "needs a Java client jar; set LIMONITE_JAVA_JAR"]
fn states_sharing_a_bedrock_id_look_the_same_or_are_explained() {
    let path = std::env::var_os("LIMONITE_JAVA_JAR").expect("LIMONITE_JAVA_JAR is set");
    let mut jar = Jar::open(std::path::Path::new(&path), FoundBy::Flag).unwrap();
    let states: Vec<(BlockStateId, JavaBlock)> = (0..BlockStateId::COUNT)
        .filter_map(BlockStateId::new)
        .map(|state| (state, JavaBlock::of(state)))
        .collect();
    let models = compile(
        |entry| jar.read(entry),
        states.iter().map(|(state, block)| (u32::from(state.as_u16()), block)),
    );
    let table = crate::world::BlockTable::build();
    let look_of = |state: BlockStateId| {
        models
            .look(u32::from(state.as_u16()))
            .map(|look| (look.quads.as_ptr(), look.quads.len(), look.slots.clone()))
    };
    // By block, the properties that change how it looks, and how many states.
    let mut differing: BTreeMap<&str, (BTreeSet<&str>, usize)> = BTreeMap::new();
    for (state, block) in &states {
        let chosen = table
            .shape_state(pumpkin_data::BlockState::to_be_network_id(*state))
            .expect("in the table");
        if look_of(*state) != look_of(chosen) {
            let other = JavaBlock::of(chosen);
            let entry = differing.entry(block.name).or_default();
            entry.1 += 1;
            for (key, value) in block.props() {
                if other.get(key) != Some(*value) {
                    entry.0.insert(*key);
                }
            }
        }
    }
    println!("{} blocks have states that share a Bedrock id and look different", differing.len());
    for (name, (props, count)) in &differing {
        println!("{name:34} {count:5} states: {props:?}");
    }
    let explained = |name: &str| {
        // What Bedrock keeps in the block actor: a bed's colour, what's in a pot, and
        // whether a piston is out.
        name.ends_with("_bed")
            || name.starts_with("potted_")
            || matches!(name, "piston" | "sticky_piston" | "piston_head")
            // What it works out from the blocks around.
            || matches!(name, "chorus_plant" | "fire" | "redstone_wire" | "repeater" | "scaffolding" | "vine")
            // Java turns these on the floor and ceiling, and Bedrock doesn't.
            || name.ends_with("_button")
            || name == "lever"
            // Java's head and body are two blocks, and Bedrock's one.
            || matches!(name, "cave_vines_plant" | "kelp_plant" | "twisting_vines_plant" | "weeping_vines_plant")
    };
    let unexplained: Vec<&&str> = differing.keys().filter(|name| !explained(name)).collect();
    assert!(unexplained.is_empty(), "{unexplained:?}");
}

/// Every model of a real jar against Pumpkin's outline for its state, which came
/// from another part of the game. A model turned the wrong way sticks out of its
/// outline, so this is what checks `x`, `y` and which state picks which model. It
/// lists every block that differs, by how far, in sixteenths: many outlines aren't
/// their shapes, as a plant's or an open gate's isn't. The families whose outlines
/// are their shapes must agree.
#[test]
#[ignore = "needs a Java client jar; set LIMONITE_JAVA_JAR"]
fn models_lie_where_pumpkins_outlines_say() {
    let path = std::env::var_os("LIMONITE_JAVA_JAR").expect("LIMONITE_JAVA_JAR is set");
    let mut jar = Jar::open(std::path::Path::new(&path), FoundBy::Flag).unwrap();
    let states: Vec<(BlockStateId, JavaBlock)> = (0..BlockStateId::COUNT)
        .filter_map(BlockStateId::new)
        .map(|state| (state, JavaBlock::of(state)))
        .collect();
    let models = compile(
        |entry| jar.read(entry),
        states.iter().map(|(state, block)| (u32::from(state.as_u16()), block)),
    );
    // By block: states seen, and the worst it sticks out and falls short.
    let mut by_block: BTreeMap<&str, (usize, f32, f32, String)> = BTreeMap::new();
    for (state, block) in &states {
        let Some(look) = models.look(u32::from(state.as_u16())) else {
            continue;
        };
        let outline = shapes::outline(*state, 0, 0, 0);
        if outline.is_empty() {
            continue;
        }
        let mut model = ([f32::MAX; 3], [f32::MIN; 3]);
        for corner in look.quads.iter().flat_map(|quad| quad.corners) {
            for (axis, value) in corner.into_iter().enumerate() {
                let value = f32::from(value) / f32::from(S);
                model.0[axis] = model.0[axis].min(value);
                model.1[axis] = model.1[axis].max(value);
            }
        }
        let mut box_of_outline = ([f32::MAX; 3], [f32::MIN; 3]);
        for (min, max) in &outline {
            for axis in 0..3 {
                box_of_outline.0[axis] = box_of_outline.0[axis].min(f32::from(min[axis]));
                box_of_outline.1[axis] = box_of_outline.1[axis].max(f32::from(max[axis]));
            }
        }
        let (mut out, mut short) = (0.0f32, 0.0f32);
        for axis in 0..3 {
            out = out.max(box_of_outline.0[axis] - model.0[axis]).max(model.1[axis] - box_of_outline.1[axis]);
            short = short.max(model.0[axis] - box_of_outline.0[axis]).max(box_of_outline.1[axis] - model.1[axis]);
        }
        let entry = by_block.entry(block.name).or_insert((0, 0.0, 0.0, String::new()));
        entry.0 += 1;
        if out > entry.1 || short > entry.2 {
            entry.3 = format!("{:?}", block.props());
        }
        entry.1 = entry.1.max(out);
        entry.2 = entry.2.max(short);
    }
    let mut differing: Vec<_> = by_block
        .iter()
        .filter(|(_, (_, out, short, _))| *out > 0.5 || *short > 0.5)
        .collect();
    differing.sort_by(|a, b| (b.1.1 + b.1.2).total_cmp(&(a.1.1 + a.1.2)));
    println!("{} blocks checked, {} differ by over half a sixteenth", by_block.len(), differing.len());
    for (name, (count, out, short, props)) in &differing {
        println!("{name:38} {count:5} states, out {out:5.1}, short {short:5.1}  {props}");
    }
    let must_agree = ["_door", "_trapdoor", "_stairs", "_slab", "_button", "_pressure_plate", "_fence", "_pane", "_bars", "_shelf"];
    let checked = by_block.keys().filter(|name| must_agree.iter().any(|suffix| name.ends_with(suffix))).count();
    assert!(checked > 200, "only {checked} blocks in those families");
    let wrong: Vec<&&str> = differing
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| must_agree.iter().any(|suffix| name.ends_with(suffix)))
        .collect();
    assert!(wrong.is_empty(), "turned the wrong way, or the wrong model: {wrong:?}");
}

/// Every Java state Pumpkin has, against a real jar, listing what breaks an
/// assumption. Prints names and counts only.
#[test]
#[ignore = "needs a Java client jar; set LIMONITE_JAVA_JAR"]
fn every_java_state_compiles_from_a_real_jar() {
    let path = std::env::var_os("LIMONITE_JAVA_JAR").expect("LIMONITE_JAVA_JAR is set");
    let mut jar = Jar::open(std::path::Path::new(&path), FoundBy::Flag).unwrap();
    let states: Vec<(u32, JavaBlock)> = (0..BlockStateId::COUNT)
        .filter_map(BlockStateId::new)
        .map(|state| (u32::from(state.as_u16()), JavaBlock::of(state)))
        .collect();
    let models = compile(|entry| jar.read(entry), states.iter().map(|(id, block)| (*id, block)));
    let stats = &models.stats;
    println!("{stats:#?}");

    let mut failures = Vec::new();
    let mut slots_seen: BTreeMap<usize, usize> = BTreeMap::new();
    let mut names_with_looks = BTreeSet::new();
    let mut textures = BTreeSet::new();
    for (id, block) in &states {
        let Some(look) = models.look(*id) else {
            continue;
        };
        names_with_looks.insert(block.name);
        *slots_seen.entry(look.slots.len()).or_default() += 1;
        for quad in look.quads {
            if usize::from(quad.slot) >= look.slots.len() {
                failures.push(format!("{}: a slot out of range", block.name));
            }
            if quad.corners.iter().flatten().any(|&value| i32::from(value).abs() > i32::from(WHOLE) * 2) {
                failures.push(format!("{}: a corner more than a block out", block.name));
            }
            if quad.uvs.iter().flatten().any(|&value| value > 16 * T) {
                failures.push(format!("{}: texture coordinates out of range", block.name));
            }
            if facing_of(&quad.corners).is_none() {
                failures.push(format!("{}: a quad with no area", block.name));
            }
        }
        textures.extend(look.slots.iter().map(|slot| slot.texture.clone()));
    }
    for texture in &textures {
        if jar.read(&format!("assets/minecraft/textures/{texture}.png")).is_none() {
            failures.push(format!("{texture}: named by a model, and not in the jar"));
        }
    }
    println!("Names with looks: {}; textures: {}", names_with_looks.len(), textures.len());
    println!("Slots per state: {slots_seen:?}");
    failures.sort();
    failures.dedup();
    assert!(stats.no_file.is_empty(), "no block state file: {:?}", stats.no_file);
    assert!(stats.too_many_textures.is_empty(), "too many textures: {:?}", stats.too_many_textures);
    assert!(failures.is_empty(), "{failures:#?}");
    // Every block has a name Block::from_state_id gives, so this is every block.
    assert!(Block::from_state_id(BlockStateId::new(0).unwrap()).name == "air");
}
