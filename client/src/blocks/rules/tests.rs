//! The rules against Pumpkin's data, and against the real packs.

use std::collections::{BTreeMap, BTreeSet};

use limonite_packs::registry::FaceKeys;
use pumpkin_data::BlockState;

use super::*;
use crate::blocks::{appearance::blocks_json_key, palette::Palette, palette_source, shapes};

/// A model's bounding box, in sixteenths.
fn bounds(quads: &[ModelQuad]) -> ([i8; 3], [i8; 3]) {
    let mut min = [i8::MAX; 3];
    let mut max = [i8::MIN; 3];
    for corner in quads.iter().flat_map(|quad| quad.corners) {
        for axis in 0..3 {
            let value = (corner[axis] / models::POSITION_SCALE) as i8;
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    (min, max)
}

/// How far a model may stick out of its Java outline, in sixteenths, on each
/// axis, by Java block name. `None`: it isn't checked.
fn slack(name: &str, java: &JavaBlock) -> Option<[i8; 3]> {
    Some(match name {
        _ if name.ends_with("_door") => [0, 0, 0],
        _ if name.ends_with("_fence") => [0, 0, 0],
        // An open gate swings outside its outline.
        _ if name.ends_with("_fence_gate") => {
            if java.flag("open") {
                return None;
            }
            [0, 0, 0]
        }
        _ if name.ends_with("glass_pane") || name.ends_with("_bars") => [0, 0, 0],
        // A sloped rail climbs into the next block.
        _ if name.ends_with("rail") => {
            if java.get("shape").is_some_and(|shape| shape.starts_with("ascending")) {
                return None;
            }
            [0, 0, 0]
        }
        // A wall torch's foot is in the wall.
        _ if name.ends_with("torch") => [1, 0, 1],
        // The handle sticks out of a lantern's outline.
        _ if name.ends_with("lantern") && java.get("hanging").is_some() => [0, 7, 0],
        _ if name.ends_with("chain") => [0, 0, 0],
        // A wick.
        _ if name.ends_with("candle") => [0, 1, 0],
        _ if name.ends_with("candle_cake") => [0, 1, 0],
        _ if name.ends_with("lightning_rod") => [0, 0, 0],
        _ if name.ends_with("coral_wall_fan") => [0, 0, 0],
        _ if name.ends_with("anvil") => [0, 0, 0],
        // The flames rise out of its outline.
        _ if name.ends_with("campfire") => [0, 10, 0],
        "end_rod" | "ladder" | "cauldron" | "water_cauldron" | "lava_cauldron" | "powder_snow_cauldron"
        | "composter" | "hopper" | "sea_pickle" | "turtle_egg" | "flower_pot" | "grindstone" | "tripwire_hook" => {
            [0, 0, 0]
        }
        // A bracket's plate on the wall.
        "bell" => [0, 1, 0],
        // Its handle sticks out, whichever way it's fixed.
        "lever" => [5, 5, 5],
        // The torches stand on the base.
        "repeater" | "comparator" => [0, 5, 0],
        // The stalk above the pod, and into the log.
        "cocoa" => [1, 4, 1],
        // The saw.
        "stonecutter" => [0, 7, 0],
        // The slanted top.
        "lectern" => [0, 4, 0],
        _ => return None,
    })
}

/// Every Java state of the families above: its model must lie within what
/// Pumpkin says it occupies. Pumpkin's outlines follow the state, so a model
/// that's turned the wrong way, or on the wrong side, falls out of it.
#[test]
fn models_lie_where_pumpkin_says_the_block_is() {
    let palette = palette_source::load().unwrap();
    let textures = FaceTextures::All("any".to_string());
    let mut models = Models::default();
    let mut wrong: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut checked: BTreeSet<&str> = BTreeSet::new();
    for id in (0..BlockStateId::COUNT).filter_map(BlockStateId::new) {
        let java = JavaBlock::of(id);
        let Some(slack) = slack(java.name, &java) else {
            continue;
        };
        let Some(state) = palette.state(BlockState::to_be_network_id(id)) else {
            continue;
        };
        let Some(Rule {
            shape: Shape::Model { quads },
            ..
        }) = rule(state, Some(&java), &textures, &mut models)
        else {
            wrong.entry(java.name).or_default().push("has no model".to_string());
            continue;
        };
        checked.insert(java.name);
        let (mut low, mut high) = ([i8::MAX; 3], [i8::MIN; 3]);
        for (min, max) in shapes::outline(id, 0, 64, 0) {
            for axis in 0..3 {
                low[axis] = low[axis].min(min[axis] as i8);
                high[axis] = high[axis].max(max[axis] as i8);
            }
        }
        let (min, max) = bounds(quads);
        let fits = (0..3).all(|axis| min[axis] >= low[axis] - slack[axis] && max[axis] <= high[axis] + slack[axis]);
        if !fits {
            wrong.entry(java.name).or_default().push(format!(
                "{:?}: model {min:?} to {max:?}, outline {low:?} to {high:?}",
                java.props
            ));
        }
    }
    assert!(checked.len() > 100, "only {} blocks checked", checked.len());
    let report: Vec<String> = wrong
        .iter()
        .map(|(name, states)| format!("{name}, {} states, first: {}", states.len(), states[0]))
        .collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
}

/// The bounds of an oak door's model, with these Java properties.
fn door(palette: &Palette, models: &mut Models, wanted: &[(&str, &str)]) -> ([i8; 3], [i8; 3]) {
    let textures = FaceTextures::All("any".to_string());
    let id = Block::OAK_DOOR
        .states
        .iter()
        .map(|state| state.id)
        .find(|&id| {
            let java = JavaBlock::of(id);
            wanted.iter().all(|&(key, value)| java.is(key, value))
        })
        .expect("a door");
    let java = JavaBlock::of(id);
    let state = palette.state(BlockState::to_be_network_id(id)).expect("in the palette");
    match rule(state, Some(&java), &textures, models).expect("a rule").shape {
        Shape::Model { quads } => bounds(quads),
        other => panic!("{other:?}"),
    }
}

/// A real case by hand, in case the check above is ever too kind.
#[test]
fn a_door_opens_around_its_hinge() {
    let palette = palette_source::load().unwrap();
    let mut models = Models::default();
    let mut east = |hinge, open| {
        door(
            &palette,
            &mut models,
            &[("facing", "east"), ("half", "lower"), ("hinge", hinge), ("open", open)],
        )
    };
    // Facing east, it stands on the block's west side.
    assert_eq!(east("left", "false"), ([0, 0, 0], [3, 16, 16]));
    assert_eq!(east("right", "false"), ([0, 0, 0], [3, 16, 16]));
    // Opened, it swings to the side its hinge is on: left is north, as you
    // face east.
    assert_eq!(east("left", "true"), ([0, 0, 0], [16, 16, 3]));
    assert_eq!(east("right", "true"), ([0, 0, 13], [16, 16, 16]));
}

/// The slots of a block's Java state, on an entry with a key per face.
fn slots_of<'a>(
    palette: &'a Palette,
    textures: &'a FaceTextures,
    block: &Block,
    wanted: &[(&str, &str)],
) -> [Pick<'a>; 6] {
    let id = block
        .states
        .iter()
        .map(|state| state.id)
        .find(|&id| {
            let java = JavaBlock::of(id);
            wanted.iter().all(|&(key, value)| java.is(key, value))
        })
        .expect("a matching state");
    let state = palette.state(BlockState::to_be_network_id(id)).expect("in the palette");
    rule(state, Some(&JavaBlock::of(id)), textures, &mut Models::default())
        .expect("a rule")
        .slots
}

#[test]
fn blocks_with_a_front_turn_it_the_way_they_face() {
    let palette = palette_source::load().unwrap();
    // A chest's entry keeps its front in south, like a furnace's.
    let textures = FaceTextures::Faces(FaceKeys {
        up: Some("top".to_string()),
        down: Some("bottom".to_string()),
        north: Some("side".to_string()),
        south: Some("front".to_string()),
        west: Some("side".to_string()),
        east: Some("side".to_string()),
        side: None,
    });
    let faces = [("north", North), ("south", South), ("west", West), ("east", East)];
    // Lit furnaces are another Bedrock name, which keeps it in east.
    let lit_entry = FaceTextures::Faces(FaceKeys {
        up: Some("top".to_string()),
        down: Some("bottom".to_string()),
        side: Some("side".to_string()),
        east: Some("front".to_string()),
        ..FaceKeys::default()
    });
    type Case<'a> = (&'a Block, &'a FaceTextures, &'a [(&'a str, &'a str)]);
    let blocks: [Case; 5] = [
        (&Block::CHEST, &textures, &[]),
        (&Block::ENDER_CHEST, &textures, &[]),
        (&Block::CARVED_PUMPKIN, &textures, &[]),
        (&Block::FURNACE, &textures, &[("lit", "false")]),
        (&Block::FURNACE, &lit_entry, &[("lit", "true")]),
    ];
    for (block, entry, extra) in blocks {
        for (facing, face) in faces {
            let mut wanted = vec![("facing", facing)];
            wanted.extend_from_slice(extra);
            let slots = slots_of(&palette, entry, block, &wanted);
            for side in faces.map(|(_, side)| side) {
                let wanted = if side == face { "front" } else { "side" };
                assert_eq!(slots[side as usize].key, Some(wanted), "{}, {facing}: {side:?}", block.name);
            }
            assert_eq!(slots[Up as usize].key, Some("top"), "{}", block.name);
            assert_eq!(slots[Down as usize].key, Some("bottom"), "{}", block.name);
        }
    }

    // A barrel's entry keeps its top in north and its bottom in south.
    let barrel = FaceTextures::Faces(FaceKeys {
        side: Some("staves".to_string()),
        north: Some("top".to_string()),
        south: Some("bottom".to_string()),
        ..FaceKeys::default()
    });
    let six = [
        ("up", Up, Down),
        ("down", Down, Up),
        ("north", North, South),
        ("south", South, North),
        ("west", West, East),
        ("east", East, West),
    ];
    for (facing, top, bottom) in six {
        let slots = slots_of(&palette, &barrel, &Block::BARREL, &[("facing", facing), ("open", "false")]);
        for face in Face::ALL {
            let wanted = if face == top {
                "top"
            } else if face == bottom {
                "bottom"
            } else {
                "staves"
            };
            assert_eq!(slots[face as usize].key, Some(wanted), "facing {facing}: {face:?}");
        }
    }
    // Open, it shows the top's second picture.
    let open = slots_of(&palette, &barrel, &Block::BARREL, &[("facing", "up"), ("open", "true")]);
    assert_eq!(open[Up as usize].variant, 1);
}

#[test]
fn the_seven_older_doors_share_a_key_and_pick_a_variant() {
    let palette = palette_source::load().unwrap();
    let textures = FaceTextures::Faces(FaceKeys {
        up: Some("door_lower".to_string()),
        down: Some("door_lower".to_string()),
        side: Some("door_upper".to_string()),
        ..FaceKeys::default()
    });
    let doors = [
        (&Block::OAK_DOOR, 0),
        (&Block::SPRUCE_DOOR, 1),
        (&Block::BIRCH_DOOR, 2),
        (&Block::JUNGLE_DOOR, 3),
        (&Block::ACACIA_DOOR, 4),
        (&Block::DARK_OAK_DOOR, 5),
        (&Block::IRON_DOOR, 6),
    ];
    for (block, variant) in doors {
        let lower = slots_of(&palette, &textures, block, &[("half", "lower")]);
        assert_eq!((lower[North as usize].key, lower[North as usize].variant), (Some("door_lower"), variant));
        let upper = slots_of(&palette, &textures, block, &[("half", "upper")]);
        assert_eq!((upper[North as usize].key, upper[North as usize].variant), (Some("door_upper"), variant));
    }
}

/// The real packs: every block Pumpkin can send that has more than one texture
/// to choose from must have a rule, or a reason it doesn't need one. So a new
/// block can't quietly show the wrong one.
#[test]
#[ignore = "needs a Bedrock install; set LIMONITE_BEDROCK"]
fn every_block_with_a_choice_of_textures_has_a_rule() {
    let dir = std::env::var_os("LIMONITE_BEDROCK").expect("LIMONITE_BEDROCK is set");
    let install = crate::install::find(Some(std::path::Path::new(&dir))).unwrap();
    let vanilla = crate::install::load_vanilla(&install).unwrap();
    let palette: Palette = palette_source::load().unwrap();
    let mut models = Models::default();
    let textures_of = |name: &str| {
        vanilla
            .blocks
            .blocks
            .get(blocks_json_key(name))
            .and_then(|entry| entry.textures.as_ref())
    };

    // Each name Pumpkin sends, and whether any of its states has a rule.
    let mut by_name: BTreeMap<String, bool> = BTreeMap::new();
    for id in (0..BlockStateId::COUNT).filter_map(BlockStateId::new) {
        let Some(state) = palette.state(BlockState::to_be_network_id(id)) else {
            continue;
        };
        let Some(textures) = textures_of(&state.name) else {
            continue;
        };
        let ruled = rule(state, Some(&JavaBlock::of(id)), textures, &mut models).is_some();
        *by_name.entry(state.name.to_string()).or_insert(false) |= ruled;
    }

    let mut unruled = Vec::new();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (name, ruled) in &by_name {
        let textures = textures_of(name).expect("checked above");
        let keys: BTreeSet<&str> = Face::ALL.iter().filter_map(|&face| textures.key(face)).collect();
        let variants = keys
            .iter()
            .any(|key| vanilla.terrain.textures.get(*key).is_some_and(|entry| entry.textures.as_slice().len() > 1));
        if keys.len() <= 1 && !variants {
            continue;
        }
        if *ruled {
            *counts.entry("has a rule").or_default() += 1;
        } else if let Some(reason) = needs_no_rule(name) {
            *counts.entry(reason).or_default() += 1;
        } else {
            unruled.push(format!("{name}: {keys:?}"));
        }
    }
    for (why, count) in &counts {
        println!("{count:>4}  {why}");
    }
    assert!(unruled.is_empty(), "no rule, and no reason:\n{}", unruled.join("\n"));
}

/// Why a block with a choice of textures draws right with no rule.
fn needs_no_rule(name: &str) -> Option<&'static str> {
    const FACES: &str = "its slots really are its faces";
    const FIRST: &str = "its first variant is the right one";
    const PILLAR: &str = "turned by pillar_axis, in pillar_keys";
    Some(match name {
        _ if name.ends_with("_log") || name.ends_with("_stem") || name.ends_with("froglight") => PILLAR,
        "basalt" | "polished_basalt" | "deepslate" | "infested_deepslate" | "bone_block" | "hay_block"
        | "purpur_pillar" | "quartz_pillar" | "quartz_block" | "chiseled_quartz_block" | "bamboo_block"
        | "stripped_bamboo_block" | "muddy_mangrove_roots" => PILLAR,
        "bamboo" => "Shape::Bamboo picks its own slots",
        "water" | "flowing_water" | "bubble_column" | "lava" | "flowing_lava" => {
            "its slots really are its faces: still above and below, flowing around"
        }
        _ if name.ends_with("_leaves") || name.ends_with("_leaves_flowered") => {
            "its variants are fancy, then fast: the fancy one draws"
        }
        "tall_grass" | "large_fern" | "rose_bush" | "peony" | "lilac" | "pitcher_plant" | "sunflower" | "seagrass" => {
            "cross_key picks these by state"
        }
        _ if name.ends_with("_stairs") || name.ends_with("_slab") || name.ends_with("_double_slab") => FACES,
        "ancient_debris" | "blackstone" | "bookshelf" | "cartography_table" | "chiseled_red_sandstone"
        | "chiseled_sandstone" | "chiseled_tuff" | "chiseled_tuff_bricks" | "crafting_table" | "crimson_nylium"
        | "cut_red_sandstone" | "cut_sandstone" | "dried_kelp_block" | "enchanting_table" | "fletching_table"
        | "grass_block" | "grass_path" | "honey_block" | "jukebox" | "lodestone" | "mangrove_roots" | "melon_block"
        | "mycelium" | "podzol" | "red_sandstone" | "reinforced_deepslate" | "sandstone" | "scaffolding"
        | "sculk_sensor" | "calibrated_sculk_sensor" | "smithing_table" | "smooth_red_sandstone"
        | "smooth_sandstone" | "target" | "tnt" | "warped_nylium" => FACES,
        "firefly_bush" | "open_eyeblossom" | "crimson_roots" | "warped_roots" => FIRST,
        _ if name.ends_with("_button") || name.ends_with("_pressure_plate") => FIRST,
        _ if name.ends_with("banner") => "a stand-in until M8: block entities' textures aren't in the array",
        "pitcher_crop" => "not done: only ever grown from a sniffer's seed",
        "jigsaw" => "not done: only ever placed by a command",
        _ => return None,
    })
}
