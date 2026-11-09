//! The contact sheets (m5-plan Step 7b): block states drawn through the real
//! mesher, texture array and shader, with no window, into labelled PNGs.
//!
//! The owner's window can't be seen from here, so this is how a block's model
//! gets looked at while it's being made, and compared with the official client
//! afterwards. Each tile is one state on a stone floor, from the south-east and
//! from the north-west.
//!
//! ```text
//! LIMONITE_BEDROCK="C:/XboxGames/Minecraft for Windows/Content" \
//!     cargo test -p limonite-client --bin limonite-client contact_sheets -- --ignored --nocapture
//! ```
//!
//! **The sheets go in `run/sheets`, which is gitignored:** they show Mojang's
//! textures, so they're never committed. `LIMONITE_SHEETS` picks sheets by name,
//! comma-separated, and `LIMONITE_TEXTURES` draws these terrain keys' textures,
//! large and with a grid, into `textures.png`, for reading off a model's UVs.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use glam::DVec3;
use image::{Rgba, RgbaImage};
use limonite_packs::PackStack;

use super::{
    Gpu, Needs,
    camera::Camera,
    animation::Animator,
    mesher::{self, Neighbourhood},
    meshes::{Meshes, SubChunkKey},
    sky,
    terrain::Terrain,
    textures::{self, TextureArray},
    tint::Tints,
};
use crate::{
    blocks::{
        appearance::{self, AppearanceTable},
        jar::{FoundBy, Jar},
        palette::{BedrockState, Palette, StateValue},
        palette_source,
        rules::JavaBlock,
    },
    debug_world::fluid_scenes,
    install::{self, Vanilla},
    world::{AIR, BlockTable},
};

/// One view's width and height, in pixels.
const VIEW: u32 = 150;
/// The label's height over each tile.
const LABEL: u32 = 14;
const COLUMNS: u32 = 6;
const ROWS: u32 = 8;
const BACKGROUND: Rgba<u8> = Rgba([32, 32, 36, 255]);
const INK: Rgba<u8> = Rgba([235, 235, 235, 255]);

/// Where a tile's block stands in its sub-chunk.
const AT: [i32; 3] = [8, 8, 8];

/// The sheets: a file name, and the block names on it, each with the states to
/// show (`key=value` pairs, comma-separated; empty for all of them).
const SHEETS: &[(&str, &[(&str, &str)])] = &[
    (
        "doors",
        &[
            ("wooden_door", "upper_block_bit=false"),
            ("iron_door", "upper_block_bit=false,open_bit=false,door_hinge_bit=false"),
            ("spruce_door", "upper_block_bit=false,open_bit=false,door_hinge_bit=false,minecraft:cardinal_direction=south"),
            ("birch_door", "upper_block_bit=false,open_bit=false,door_hinge_bit=false,minecraft:cardinal_direction=south"),
            ("jungle_door", "upper_block_bit=false,open_bit=false,door_hinge_bit=false,minecraft:cardinal_direction=south"),
            ("acacia_door", "upper_block_bit=false,open_bit=false,door_hinge_bit=false,minecraft:cardinal_direction=south"),
            ("dark_oak_door", "upper_block_bit=false,open_bit=false,door_hinge_bit=false,minecraft:cardinal_direction=south"),
            ("cherry_door", "upper_block_bit=false,open_bit=false,door_hinge_bit=false,minecraft:cardinal_direction=south"),
            ("copper_door", "upper_block_bit=false,open_bit=false,door_hinge_bit=false,minecraft:cardinal_direction=south"),
        ],
    ),
    ("dripstone", &[("pointed_dripstone", ""), ("sulfur_spike", "hanging=false")]),
    (
        "flat",
        &[
            ("leaf_litter", "minecraft:cardinal_direction=north"),
            ("leaf_litter", "growth=3"),
            ("pink_petals", "minecraft:cardinal_direction=north"),
            ("pink_petals", "growth=3"),
            ("wildflowers", "minecraft:cardinal_direction=north"),
            ("frog_spawn", ""),
            ("resin_clump", "multi_face_direction_bits=1"),
            ("resin_clump", "multi_face_direction_bits=4"),
            ("glow_lichen", "multi_face_direction_bits=1"),
            ("sculk_vein", "multi_face_direction_bits=4"),
            ("waterlily", ""),
            ("white_carpet", ""),
        ],
    ),
    (
        "chests",
        &[
            ("chest", ""),
            ("trapped_chest", "minecraft:cardinal_direction=south"),
            ("ender_chest", "minecraft:cardinal_direction=south"),
            ("copper_chest", "minecraft:cardinal_direction=south"),
            ("oxidized_copper_chest", "minecraft:cardinal_direction=east"),
            ("furnace", ""),
            ("lit_furnace", "minecraft:cardinal_direction=south"),
            ("blast_furnace", "minecraft:cardinal_direction=west"),
            ("smoker", "minecraft:cardinal_direction=north"),
            ("carved_pumpkin", ""),
            ("lit_pumpkin", "minecraft:cardinal_direction=south"),
            ("pumpkin", ""),
            ("melon_block", ""),
            ("barrel", "open_bit=false"),
            ("barrel", "open_bit=true,facing_direction=1"),
            ("beehive", "honey_level=0"),
            ("bee_nest", "honey_level=5"),
            ("dispenser", "triggered_bit=false"),
            ("dropper", "triggered_bit=false,facing_direction=1"),
            ("observer", "powered_bit=false"),
            ("piston", ""),
            ("sticky_piston", "facing_direction=1"),
            ("loom", ""),
            ("vault", "ominous=false,vault_state=inactive,minecraft:cardinal_direction=south"),
            ("crafter", "crafting=false,triggered_bit=false,orientation=north_up"),
        ],
    ),
    (
        "mushrooms",
        &[
            ("brown_mushroom_block", ""),
            ("red_mushroom_block", "huge_mushroom_bits=14"),
            ("red_mushroom_block", "huge_mushroom_bits=0"),
            ("mushroom_stem", "huge_mushroom_bits=15"),
            ("mushroom_stem", "huge_mushroom_bits=10"),
        ],
    ),
    (
        "rails",
        &[
            ("rail", ""),
            ("golden_rail", "rail_data_bit=true"),
            ("golden_rail", "rail_data_bit=false,rail_direction=0"),
            ("detector_rail", "rail_data_bit=false,rail_direction=2"),
            ("activator_rail", "rail_data_bit=false,rail_direction=1"),
            ("redstone_wire", "redstone_signal=0"),
            ("redstone_wire", "redstone_signal=15"),
            ("powered_repeater", "repeater_delay=0"),
            ("unpowered_repeater", "repeater_delay=3,minecraft:cardinal_direction=north"),
            ("powered_comparator", "output_lit_bit=true,output_subtract_bit=false,minecraft:cardinal_direction=south"),
            ("unpowered_comparator", "output_lit_bit=false,output_subtract_bit=true,minecraft:cardinal_direction=north"),
            ("daylight_detector", "redstone_signal=0"),
            ("daylight_detector_inverted", "redstone_signal=0"),
            ("lever", "open_bit=false"),
            ("tripwire_hook", "attached_bit=false,powered_bit=false"),
            ("trip_wire", "attached_bit=false,disarmed_bit=false,powered_bit=false,suspended_bit=true,minecraft:connection_east=true,minecraft:connection_west=true,minecraft:connection_north=false,minecraft:connection_south=false"),
        ],
    ),
    (
        "lights",
        &[
            ("torch", ""),
            ("soul_torch", "torch_facing_direction=top"),
            ("redstone_torch", "torch_facing_direction=west"),
            ("unlit_redstone_torch", "torch_facing_direction=top"),
            ("copper_torch", "torch_facing_direction=north"),
            ("lantern", ""),
            ("soul_lantern", ""),
            ("copper_lantern", "hanging=false"),
            ("end_rod", ""),
            ("lightning_rod", "powered_bit=false"),
            ("iron_chain", ""),
            ("copper_chain", "pillar_axis=y"),
            ("candle", "lit=false"),
            ("candle", "lit=true,candles=3"),
            ("red_candle", "lit=false,candles=1"),
            ("candle_cake", ""),
            ("red_candle_cake", "lit=true"),
            ("cake", ""),
            ("campfire", "minecraft:cardinal_direction=north"),
            ("soul_campfire", "minecraft:cardinal_direction=east,extinguished=false"),
            ("fire", "age=0"),
            ("soul_fire", "age=0"),
        ],
    ),
    (
        "fences",
        &[
            (
                "oak_fence",
                "minecraft:connection_north=false,minecraft:connection_south=false",
            ),
            ("oak_fence", "minecraft:connection_east=true,minecraft:connection_west=true,minecraft:connection_north=true"),
            ("nether_brick_fence", "minecraft:connection_east=true,minecraft:connection_north=false,minecraft:connection_south=true,minecraft:connection_west=false"),
            ("fence_gate", "in_wall_bit=false,minecraft:cardinal_direction=south"),
            ("fence_gate", "in_wall_bit=true,minecraft:cardinal_direction=east"),
            ("cobblestone_wall", "wall_post_bit=true,wall_connection_type_north=none,wall_connection_type_south=none,wall_connection_type_east=short,wall_connection_type_west=tall"),
            ("cobblestone_wall", "wall_post_bit=false,wall_connection_type_north=none,wall_connection_type_south=none,wall_connection_type_east=tall,wall_connection_type_west=tall"),
            ("glass_pane", "minecraft:connection_north=false,minecraft:connection_south=false"),
            ("glass_pane", "minecraft:connection_north=true,minecraft:connection_east=true,minecraft:connection_south=false,minecraft:connection_west=false"),
            ("red_stained_glass_pane", "minecraft:connection_north=false,minecraft:connection_south=false,minecraft:connection_east=true,minecraft:connection_west=true"),
            ("iron_bars", "minecraft:connection_north=false,minecraft:connection_south=false"),
            ("copper_bars", "minecraft:connection_north=true,minecraft:connection_east=false,minecraft:connection_south=true,minecraft:connection_west=false"),
            ("trapdoor", "direction=0"),
            ("iron_trapdoor", "direction=1,upside_down_bit=false"),
            ("oak_stairs", "minecraft:corner=none,upside_down_bit=false,weirdo_direction=0"),
            ("oak_stairs", "minecraft:corner=inner_left,upside_down_bit=true,weirdo_direction=2"),
            ("wooden_button", "button_pressed_bit=false,facing_direction=1"),
            ("wooden_button", "button_pressed_bit=false,facing_direction=2"),
            ("stone_pressure_plate", "redstone_signal=0"),
            ("ladder", ""),
        ],
    ),
    (
        "plants",
        &[
            ("azalea", ""),
            ("flowering_azalea", ""),
            ("big_dripleaf", "big_dripleaf_head=true,big_dripleaf_tilt=none,minecraft:cardinal_direction=north"),
            ("big_dripleaf", "big_dripleaf_head=true,big_dripleaf_tilt=partial_tilt,minecraft:cardinal_direction=east"),
            ("big_dripleaf", "big_dripleaf_head=true,big_dripleaf_tilt=full_tilt,minecraft:cardinal_direction=south"),
            ("big_dripleaf", "big_dripleaf_head=false,big_dripleaf_tilt=none,minecraft:cardinal_direction=north"),
            ("small_dripleaf_block", "upper_block_bit=false"),
            ("cocoa", "direction=0"),
            ("cocoa", "age=2"),
            ("small_amethyst_bud", "minecraft:block_face=up"),
            ("medium_amethyst_bud", "minecraft:block_face=north"),
            ("large_amethyst_bud", "minecraft:block_face=east"),
            ("amethyst_cluster", ""),
            ("spore_blossom", ""),
            ("golden_dandelion", ""),
            ("pale_hanging_moss", ""),
            ("tube_coral_wall_fan", ""),
            ("dead_fire_coral_wall_fan", "coral_direction=1"),
            ("sea_pickle", "dead_bit=false"),
            ("sea_pickle", "dead_bit=true,cluster_count=3"),
            ("turtle_egg", "cracked_state=no_cracks"),
            ("sniffer_egg", ""),
            ("dried_ghast", "rehydration_level=0"),
            ("wheat", "growth=0"),
            ("wheat", "growth=7"),
            ("carrots", "growth=4"),
            ("potatoes", "growth=7"),
            ("beetroot", "growth=7"),
            ("nether_wart", "age=3"),
            ("melon_stem", "growth=4"),
            ("pumpkin_stem", "growth=7"),
            ("sweet_berry_bush", "growth=3"),
            ("cave_vines", "growing_plant_age=0"),
            ("cave_vines_body_with_berries", "growing_plant_age=0"),
            ("cave_vines_head_with_berries", "growing_plant_age=0"),
            ("kelp", "kelp_age=0"),
            ("weeping_vines", "weeping_vines_age=0"),
            ("twisting_vines", "twisting_vines_age=0"),
            ("sunflower", "upper_block_bit=false"),
            ("open_eyeblossom", ""),
            ("firefly_bush", ""),
            ("mangrove_propagule", "hanging=true,propagule_stage=4"),
            ("pitcher_crop", "upper_block_bit=false,growth=4"),
            ("torchflower_crop", "growth=1"),
            ("cactus", "age=0"),
        ],
    ),
    // Models the jar doesn't turn in these states, to lay beside what Blockbench's
    // Asset Browser shows for the same file (m5-plan Step 7c).
    (
        "browser",
        &[
            ("lantern", "java.hanging=false"),
            ("soul_lantern", "java.hanging=true"),
            ("torch", "torch_facing_direction=top"),
            ("campfire", "java.facing=south,java.lit=true"),
            ("cauldron", "cauldron_liquid=water,fill_level=0"),
            ("hopper", "java.facing=down,toggle_bit=false"),
            ("anvil", "java.facing=south"),
            ("stonecutter_block", "java.facing=north"),
            ("lectern", "java.facing=north,powered_bit=false"),
            ("grindstone", "java.face=floor,java.facing=north"),
            ("end_portal_frame", "java.eye=true,java.facing=south"),
            ("big_dripleaf", "java.facing=north,java.tilt=partial"),
            ("pointed_dripstone", "java.thickness=tip,java.vertical_direction=up"),
            ("iron_chain", "java.axis=y"),
            ("candle", "java.candles=4,java.lit=true"),
            ("turtle_egg", "java.eggs=4,java.hatch=0"),
            ("cake", "java.bites=2"),
            ("flower_pot", "update_bit=false"),
            ("oak_stairs", "java.facing=east,java.half=bottom,java.shape=straight"),
            ("oak_stairs", "java.facing=east,java.half=bottom,java.shape=inner_right"),
            ("oak_stairs", "java.facing=east,java.half=bottom,java.shape=outer_right"),
            ("fence_gate", "java.facing=south,java.in_wall=false,java.open=false"),
            ("bell", "java.attachment=floor,java.facing=north,toggle_bit=false"),
            ("unpowered_repeater", "java.delay=1,java.facing=south"),
            ("unpowered_comparator", "java.facing=south,java.mode=compare"),
            ("standing_sign", "java.rotation=0"),
            ("bed", "java.part=head,java.facing=north,occupied_bit=false"),
            ("sculk_sensor", "java.sculk_sensor_phase=inactive"),
            ("beacon", ""),
            ("enchanting_table", ""),
            ("dragon_egg", ""),
            ("daylight_detector", "redstone_signal=0"),
            ("lever", "java.face=floor,java.facing=north,java.powered=false"),
            ("cocoa", "java.age=2,java.facing=south"),
            ("amethyst_cluster", "java.facing=up"),
            ("oak_hanging_sign", "java.attached=false,java.rotation=3"),
            ("sea_pickle", "java.pickles=4,java.waterlogged=true"),
        ],
    ),
    (
        "signs",
        &[
            ("bed", "occupied_bit=false"),
            ("straw_bed", ""),
            ("standing_sign", "ground_sign_direction=0"),
            ("standing_sign", "ground_sign_direction=2"),
            ("standing_sign", "ground_sign_direction=3"),
            ("standing_sign", "ground_sign_direction=4"),
            ("standing_sign", "ground_sign_direction=6"),
            ("spruce_standing_sign", "ground_sign_direction=8"),
            ("wall_sign", ""),
            ("oak_hanging_sign", "hanging=true,attached_bit=false,ground_sign_direction=0"),
            ("oak_hanging_sign", "hanging=true,attached_bit=true,ground_sign_direction=0"),
            ("oak_hanging_sign", "hanging=true,attached_bit=true,ground_sign_direction=2"),
            ("oak_hanging_sign", "hanging=true,attached_bit=true,ground_sign_direction=3"),
            ("oak_hanging_sign", "hanging=false,attached_bit=false,ground_sign_direction=0"),
        ],
    ),
    (
        "furniture",
        &[
            ("anvil", "minecraft:cardinal_direction=north"),
            ("damaged_anvil", "minecraft:cardinal_direction=east"),
            ("bell", "attachment=standing,toggle_bit=false,direction=0"),
            ("bell", "attachment=hanging,toggle_bit=false,direction=1"),
            ("bell", "attachment=side,toggle_bit=false,direction=0"),
            ("bell", "attachment=multiple,toggle_bit=false,direction=1"),
            ("brewing_stand", "brewing_stand_slot_a_bit=false,brewing_stand_slot_b_bit=false,brewing_stand_slot_c_bit=false"),
            ("brewing_stand", "brewing_stand_slot_a_bit=true,brewing_stand_slot_b_bit=true,brewing_stand_slot_c_bit=true"),
            ("cauldron", "cauldron_liquid=water,fill_level=0"),
            ("cauldron", "cauldron_liquid=water,fill_level=6"),
            ("cauldron", "cauldron_liquid=lava,fill_level=6"),
            ("hopper", "toggle_bit=false,facing_direction=0"),
            ("hopper", "toggle_bit=false,facing_direction=4"),
            ("composter", "composter_fill_level=0"),
            ("composter", "composter_fill_level=4"),
            ("composter", "composter_fill_level=8"),
            ("flower_pot", "update_bit=false"),
            ("decorated_pot", "direction=0"),
            ("grindstone", "attachment=standing,direction=0"),
            ("lectern", "powered_bit=false"),
            ("stonecutter_block", "minecraft:cardinal_direction=north"),
            ("enchanting_table", ""),
            ("end_portal_frame", "end_portal_eye_bit=true,minecraft:cardinal_direction=north"),
            ("beacon", ""),
            ("conduit", ""),
            ("scaffolding", "stability=0,stability_check=false"),
            ("sculk_sensor", "sculk_sensor_phase=0"),
            ("sculk_shrieker", "active=false,can_summon=false"),
            ("calibrated_sculk_sensor", "sculk_sensor_phase=0,minecraft:cardinal_direction=north"),
            ("oak_shelf", "powered_bit=false,powered_shelf_type=0,minecraft:cardinal_direction=north"),
            ("copper_golem_statue", "minecraft:cardinal_direction=north"),
            ("heavy_core", ""),
            ("dragon_egg", ""),
            ("chorus_plant", ""),
            ("chorus_flower", "age=0"),
            ("bed", "direction=0,head_piece_bit=false,occupied_bit=false"),
            ("standing_sign", "ground_sign_direction=0"),
            ("wall_sign", "facing_direction=2"),
            ("oak_hanging_sign", "hanging=true,attached_bit=false,facing_direction=2,ground_sign_direction=0"),
            ("standing_banner", "ground_sign_direction=0"),
            ("wall_banner", "facing_direction=2"),
            ("skeleton_skull", "facing_direction=1"),
            ("creeper_head", "facing_direction=2"),
            ("grass_path", ""),
            ("farmland", "moisturized_amount=0"),
            ("farmland", "moisturized_amount=7"),
            ("respawn_anchor", "respawn_anchor_charge=2"),
            ("trial_spawner", "ominous=false,trial_spawner_state=1"),
            ("creaking_heart", "creaking_heart_state=awake,natural=false,pillar_axis=y"),
            ("copper_bulb", "lit=true,powered_bit=false"),
        ],
    ),
];

fn value_text(value: &StateValue) -> String {
    match value {
        StateValue::Bool(value) => u8::from(*value).to_string(),
        StateValue::Int(value) => value.to_string(),
        StateValue::String(value) => value.to_string(),
    }
}

/// Whether a state has every `key=value` of `filter`, or for a `java.key=value`, its
/// Java state does.
fn matches(state: &BedrockState, java: Option<&JavaBlock>, filter: &str) -> bool {
    filter.split(',').filter(|pair| !pair.is_empty()).all(|pair| {
        let (key, wanted) = pair.split_once('=').expect("key=value");
        if let Some(key) = key.strip_prefix("java.") {
            return java.is_some_and(|java| java.get(key) == Some(wanted));
        }
        state.get(key).is_some_and(|value| match value {
            StateValue::Bool(value) => value.to_string() == wanted,
            value => value_text(value) == wanted,
        })
    })
}

/// A property's name, short enough for a label.
fn short_key(key: &str) -> &str {
    let key = key.strip_prefix("minecraft:").unwrap_or(key);
    key.rsplit('_').next().unwrap_or(key)
}

/// One tile: a label, and each block's offset from [`AT`] and id.
type Tile = (String, Vec<([i32; 3], u32)>);

/// The tiles of a sheet.
fn tiles(palette: &Palette, wanted: &[(&str, &str)]) -> Vec<Tile> {
    let java = BlockTable::build();
    let mut tiles = Vec::new();
    for (name, filter) in wanted {
        // A state Pumpkin can't send never gets drawn.
        let mut states: Vec<(u32, &BedrockState)> = palette
            .iter()
            .filter(|&(id, state)| {
                let java = java.shape_state(id).map(JavaBlock::of);
                &*state.name == *name && java.is_some() && matches(state, java.as_ref(), filter)
            })
            .collect();
        states.sort_by_cached_key(|(_, state)| {
            state.states.iter().map(|(key, value)| format!("{key}={}", value_text(value))).collect::<Vec<_>>()
        });
        if states.is_empty() {
            eprintln!("Nothing matches {name} {filter}");
        }
        // Only what differs along this row goes in the labels.
        let varies = |key: &str| {
            states
                .iter()
                .filter_map(|(_, state)| state.get(key).map(value_text))
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        };
        for (id, state) in &states {
            let props: Vec<String> = state
                .states
                .iter()
                .filter(|(key, _)| varies(key))
                .map(|(key, value)| format!("{}={}", short_key(key), value_text(value)))
                .collect();
            let mut blocks = vec![([0, 0, 0], *id)];
            // The upper half stands on the lower.
            if state.flag("upper_block_bit") == Some(false) {
                let upper = palette.iter().find(|(_, other)| {
                    other.name == state.name
                        && other.flag("upper_block_bit") == Some(true)
                        && other.states.iter().zip(&state.states).all(|(a, b)| a.0 == b.0 && (&*a.0 == "upper_block_bit" || a.1 == b.1))
                });
                if let Some((upper, _)) = upper {
                    blocks.push(([0, 1, 0], upper));
                }
            }
            tiles.push((format!("{name} {}", props.join(" ")), blocks));
        }
    }
    tiles
}

/// A very small font, drawn here: 3 × 5 pixels a glyph, row by row.
fn glyph(c: char) -> &'static str {
    match c.to_ascii_lowercase() {
        'a' => "010101111101101",
        'b' => "110101110101110",
        'c' => "011100100100011",
        'd' => "110101101101110",
        'e' => "111100110100111",
        'f' => "111100110100100",
        'g' => "011100101101011",
        'h' => "101101111101101",
        'i' => "111010010010111",
        'j' => "001001001101010",
        'k' => "101101110101101",
        'l' => "100100100100111",
        'm' => "101111111101101",
        'n' => "110101101101101",
        'o' => "010101101101010",
        'p' => "110101110100100",
        'q' => "010101101110011",
        'r' => "110101110101101",
        's' => "011100010001110",
        't' => "111010010010010",
        'u' => "101101101101111",
        'v' => "101101101101010",
        'w' => "101101111111101",
        'x' => "101101010101101",
        'y' => "101101010010010",
        'z' => "111001010100111",
        '0' => "111101101101111",
        '1' => "010110010010111",
        '2' => "110001010100111",
        '3' => "110001010001110",
        '4' => "101101111001001",
        '5' => "111100110001110",
        '6' => "011100111101111",
        '7' => "111001010010010",
        '8' => "111101111101111",
        '9' => "111101111001110",
        '_' => "000000000000111",
        '=' => "000111000111000",
        '-' => "000000111000000",
        '.' => "000000000000010",
        ':' => "000010000010000",
        _ => "000000000000000",
    }
}

fn write_text(sheet: &mut RgbaImage, x: u32, y: u32, text: &str, scale: u32, width: u32) {
    let fits = (width / (4 * scale)) as usize;
    for (i, c) in text.chars().take(fits).enumerate() {
        for (bit, set) in glyph(c).bytes().enumerate() {
            if set == b'1' {
                let (gx, gy) = ((bit % 3) as u32, (bit / 3) as u32);
                for dy in 0..scale {
                    for dx in 0..scale {
                        sheet.put_pixel(x + i as u32 * 4 * scale + gx * scale + dx, y + gy * scale + dy, INK);
                    }
                }
            }
        }
    }
}

/// Everything a frame needs, with no window.
struct Offscreen {
    gpu: Gpu,
    terrain: Terrain,
    target: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Offscreen {
    fn new(textures: &TextureArray) -> Self {
        let needs = Needs {
            texture_layers: textures.layers,
            uncapped: false,
        };
        let gpu = Gpu::offscreen(VIEW, VIEW, &needs).expect("a GPU");
        let terrain = Terrain::new(&gpu, textures);
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("contact sheet"),
            size: wgpu::Extent3d {
                width: VIEW,
                height: VIEW,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gpu.config.format,
            usage: gpu.config.usage,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            gpu,
            terrain,
            target,
            view,
        }
    }

    /// Draws `meshes` from `camera`, and reads the picture back.
    fn draw(&mut self, meshes: &Meshes, camera: &Camera) -> RgbaImage {
        let device = &self.gpu.device;
        let row = (VIEW * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("contact sheet readback"),
            size: u64::from(row * VIEW),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        // Noon, and no fog to speak of.
        let sky = sky::frame(&sky::FogSettings::FALLBACK, sky::NOON_TICKS, sky::Medium::Air, 100_000.0, glam::Vec3::Z);
        self.terrain.draw(&self.gpu, &self.view, &mut encoder, Some(camera), meshes, &sky);
        encoder.copy_texture_to_buffer(
            self.target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(VIEW),
                },
            },
            wgpu::Extent3d {
                width: VIEW,
                height: VIEW,
                depth_or_array_layers: 1,
            },
        );
        self.gpu.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |result| result.expect("the map"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU");
        let bytes = buffer.slice(..).get_mapped_range().expect("the mapped range");
        RgbaImage::from_fn(VIEW, VIEW, |x, y| {
            let at = (y * row + x * 4) as usize;
            Rgba([bytes[at], bytes[at + 1], bytes[at + 2], 255])
        })
    }
}

/// Lays `tile`s out over as many pages as they need, and writes them.
fn write_sheet(
    out: &Path,
    name: &str,
    tiles: &[Tile],
    offscreen: &mut Offscreen,
    table: &AppearanceTable,
    tints: &Tints,
    stone: u32,
) {
    let tile_width = VIEW * 2 + 4;
    let tile_height = VIEW + LABEL;
    let key = SubChunkKey { x: 0, y: 0, z: 0 };
    for (page, tiles) in tiles.chunks((COLUMNS * ROWS) as usize).enumerate() {
        let rows = (tiles.len() as u32).div_ceil(COLUMNS);
        let mut sheet = RgbaImage::from_pixel(COLUMNS * tile_width, rows * tile_height, BACKGROUND);
        for (index, (label, blocks)) in tiles.iter().enumerate() {
            let (column, row) = (index as u32 % COLUMNS, index as u32 / COLUMNS);
            let (x, y) = (column * tile_width, row * tile_height);
            write_text(&mut sheet, x + 2, y + 2, label, 2, tile_width - 4);

            let neighbourhood = Neighbourhood::from_fn(table, [0, 0, 0], 1, |bx, by, bz, layer| {
                let at = [bx - AT[0], by - AT[1], bz - AT[2]];
                // A floor under the tile's blocks, a block wider all round.
                let floor = at[1] == -1
                    && blocks.iter().any(|(offset, _)| (offset[0] - at[0]).abs() <= 1 && (offset[2] - at[2]).abs() <= 1);
                if layer != 0 {
                    AIR
                } else if floor {
                    stone
                } else {
                    blocks.iter().find(|(offset, _)| *offset == at).map_or(AIR, |&(_, id)| id)
                }
            });
            let mut meshes = Meshes::default();
            meshes.upload(&offscreen.gpu.device, key, mesher::mesh(&neighbourhood, tints));

            // A block, a little above its middle; two-tall ones further back.
            let tall = blocks.len() > 1;
            let center = DVec3::new(
                f64::from(AT[0]) + 0.5,
                f64::from(AT[1]) + if tall { 1.0 } else { 0.5 },
                f64::from(AT[2]) + 0.5,
            );
            let reach = blocks.iter().map(|(offset, _)| offset[0].abs().max(offset[2].abs())).max().unwrap_or(0);
            let distance = match reach {
                0 if tall => 7.5,
                0 => 5.0,
                reach => 6.0 + f64::from(reach) * 2.0,
            };
            for (side, (pitch, yaw)) in [(35.0f32, 135.0f32), (35.0, -45.0)].into_iter().enumerate() {
                let forward = super::camera::look_direction(pitch, yaw).as_dvec3();
                let mut camera = Camera::new(center - forward * distance, pitch, yaw);
                camera.aspect = 1.0;
                camera.fov_y = 30.0;
                let view = offscreen.draw(&meshes, &camera);
                image::imageops::replace(
                    &mut sheet,
                    &view,
                    i64::from(x + side as u32 * (VIEW + 1)),
                    i64::from(y + LABEL),
                );
            }
        }
        let file = out.join(if page == 0 { format!("{name}.png") } else { format!("{name}-{}.png", page + 1) });
        sheet.save(&file).unwrap_or_else(|err| panic!("{}: {err}", file.display()));
        println!("{}: {} tiles", file.display(), tiles.len());
    }
}

/// The debug world's fluid scenes, at a few moments, to see them move.
fn write_fluids(
    out: &Path,
    only: Option<&str>,
    offscreen: &mut Offscreen,
    mut animator: Animator,
    table: &AppearanceTable,
    tints: &Tints,
    stone: u32,
) {
    if only.is_some_and(|only| !only.split(',').any(|sheet| sheet == "fluids")) {
        return;
    }
    let tiles: Vec<Tile> = fluid_scenes()
        .into_iter()
        .map(|scene| {
            let blocks = scene.blocks.iter().map(|&(at, state)| (at, pumpkin_data::BlockState::to_be_network_id(state)));
            (scene.name.to_string(), blocks.collect())
        })
        .collect();
    for tick in [0, 8, 16] {
        offscreen.terrain.animate(&offscreen.gpu, &mut animator, tick);
        write_sheet(out, &format!("fluids-{tick}"), &tiles, offscreen, table, tints, stone);
    }
}

/// Terrain keys' textures, 10 pixels a texel, with a line every 4 texels.
fn write_textures(out: &Path, keys: &str, vanilla: &Vanilla, stack: &PackStack) {
    const TEXEL: u32 = 10;
    const WIDTH: u32 = 16 * TEXEL + 4;
    const HEIGHT: u32 = 16 * TEXEL + LABEL;
    const ACROSS: u32 = 8;
    let mut cells = Vec::new();
    for key in keys.split(',').filter(|key| !key.is_empty()) {
        let Some(entry) = vanilla.terrain.textures.get(key) else {
            eprintln!("No terrain key {key}");
            continue;
        };
        let mut seen = BTreeSet::new();
        for (variant, texture) in entry.textures.as_slice().iter().enumerate() {
            if seen.insert(texture.path.clone()) {
                match textures::read_image(stack, &texture.path) {
                    Ok(image) => cells.push((format!("{key} {variant}"), image)),
                    Err(err) => eprintln!("{key} {variant}: {err}"),
                }
            }
        }
    }
    let rows = (cells.len() as u32).div_ceil(ACROSS);
    let mut sheet = RgbaImage::from_pixel(ACROSS * WIDTH, rows.max(1) * HEIGHT, BACKGROUND);
    for (index, (label, image)) in cells.iter().enumerate() {
        let (x, y) = ((index as u32 % ACROSS) * WIDTH, (index as u32 / ACROSS) * HEIGHT);
        write_text(&mut sheet, x + 2, y + 2, label, 2, WIDTH - 4);
        // A flipbook shows its first frame.
        let side = image.width();
        let scale = 16.0 / f64::from(side);
        for ty in 0..16u32 * TEXEL {
            for tx in 0..16u32 * TEXEL {
                let (ix, iy) = (
                    (f64::from(tx) / f64::from(TEXEL) / scale) as u32,
                    (f64::from(ty) / f64::from(TEXEL) / scale) as u32,
                );
                let texel = image.get_pixel(ix.min(side - 1), iy.min(image.height() - 1));
                let checker = if (tx / 5 + ty / 5) % 2 == 0 { 70u8 } else { 90 };
                let a = u32::from(texel[3]);
                let mix = |c: u8| ((u32::from(c) * a + u32::from(checker) * (255 - a)) / 255) as u8;
                let mut pixel = Rgba([mix(texel[0]), mix(texel[1]), mix(texel[2]), 255]);
                if tx % (4 * TEXEL) == 0 || ty % (4 * TEXEL) == 0 {
                    pixel = Rgba([255, 0, 255, 255]);
                }
                sheet.put_pixel(x + tx, y + LABEL + ty, pixel);
            }
        }
    }
    let file = out.join("textures.png");
    sheet.save(&file).unwrap_or_else(|err| panic!("{}: {err}", file.display()));
    println!("{}: {} textures", file.display(), cells.len());
}

#[test]
#[ignore = "needs a GPU and a Bedrock install; set LIMONITE_BEDROCK"]
fn contact_sheets() {
    let dir = std::env::var_os("LIMONITE_BEDROCK").expect("LIMONITE_BEDROCK is set");
    let install = install::find(Some(std::path::Path::new(&dir))).unwrap();
    let vanilla = install::load_vanilla(&install).unwrap();
    let palette = palette_source::load().unwrap();
    // With a jar, the sheets show its models; without, the hand-made ones.
    let mut jar = std::env::var_os("LIMONITE_JAVA_JAR")
        .map(|path| Jar::open(std::path::Path::new(&path), FoundBy::Flag).expect("LIMONITE_JAVA_JAR is a jar"));
    let built = appearance::build(&palette, &vanilla.blocks, &vanilla.terrain, &vanilla.flipbooks, &vanilla.stack, jar.as_mut());
    let (tints, _) = Tints::load(&vanilla.stack);
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../run/sheets");
    std::fs::create_dir_all(&out).unwrap();

    if let Ok(keys) = std::env::var("LIMONITE_TEXTURES") {
        write_textures(&out, &keys, &vanilla, &vanilla.stack);
    }
    let only = std::env::var("LIMONITE_SHEETS").ok();
    if only.as_deref() == Some("") {
        return;
    }
    let stone = palette
        .iter()
        .find(|(_, state)| &*state.name == "stone")
        .map(|(id, _)| id)
        .expect("stone");
    let mut offscreen = Offscreen::new(&built.textures);
    write_fluids(&out, only.as_deref(), &mut offscreen, built.animator, &built.table, &tints, stone);
    for (name, wanted) in SHEETS {
        if only.as_deref().is_some_and(|only| !only.split(',').any(|sheet| sheet == *name)) {
            continue;
        }
        let tiles = tiles(&palette, wanted);
        write_sheet(&out, name, &tiles, &mut offscreen, &built.table, &tints, stone);
    }
}
