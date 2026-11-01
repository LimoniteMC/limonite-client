//! What each item is, and what it looks like (m5-plan Decision 10, Findings 8 and 16).
//!
//! **Item to block** (worth teaching). A network item stack doesn't say which block an
//! item places: Pumpkin always sends 0. But `pumpkin-data` has the whole path: the
//! Bedrock item to its Java item, to the Java block, to that block's default state, to
//! its Bedrock id, which is what the appearance table is keyed by.
//!
//! **Which picture.** Only a few items say what their icon is. The rest are mapped inside
//! the game's engine, so the client looks in this order:
//! 1. `items/*.json`'s icon, a key of `item_texture.json`;
//! 2. a key of `item_texture.json` with the item's name;
//! 3. `textures/items/<name>`;
//! 4. the same under the names the files really go by, from [`aliases`];
//! 5. if it places a block, that block as an icon (`render/icons.rs`).
//!
//! Anything else draws the missing texture, and the census at startup lists it, so a new
//! version's renames get noticed.

use std::collections::{BTreeMap, HashMap};

use image::RgbaImage;
use limonite_packs::{PackStack, json};
use pumpkin_data::{
    Block, BlockState,
    item::{BedrockItem, JavaToBedrockItemMapping},
};
use rayon::prelude::*;
use tracing::{info, warn};

use crate::{
    blocks::appearance::AppearanceTable,
    render::{
        icons::{Icon, IconQuad, block_icon},
        textures::{MISSING_TEXTURE, read_image},
        tint::Tints,
    },
    ui::{
        atlas::{Atlas, Input},
        lang::Lang,
    },
};

const ITEM_TEXTURES: &str = "textures/item_texture.json";
const ITEMS_FOLDER: &str = "textures/items";
const DEFINITIONS: &str = "items";
/// The most an old-style item's data value goes up to, for its variants.
const MOST_VARIANTS: u32 = 16;

/// The Bedrock id of the block an item places, if it places one.
#[must_use]
pub fn block_of(id: i16, aux: u32) -> Option<u32> {
    let mapping = JavaToBedrockItemMapping::from_bedrock(id, aux)?;
    let block = Block::from_item_id(mapping.java_item.id)?;
    Some(BlockState::to_be_network_id(block.default_state.id))
}

/// The item's name without its namespace.
fn short_name(key: &str) -> &str {
    key.strip_prefix("minecraft:").unwrap_or(key)
}

/// What to call an item: the game's own string if it goes by the plain pattern, and
/// otherwise its name in words, which nearly always comes to the same thing.
#[must_use]
pub fn display_name(lang: &Lang, key: &str) -> String {
    let name = short_name(key);
    let string = lang
        .get(&format!("item.{name}.name"))
        .or_else(|| lang.get(&format!("tile.{name}.name")));
    string.map_or_else(|| in_words(name), str::to_string)
}

/// `oak_planks` as `Oak Planks`.
fn in_words(name: &str) -> String {
    let words = name.split('_').map(|word| {
        let mut letters = word.chars();
        letters.next().map_or_else(String::new, |first| first.to_uppercase().chain(letters).collect())
    });
    words.collect::<Vec<_>>().join(" ")
}

/// The names an item's picture might go by, when it isn't the item's own: Bedrock's
/// files kept their old names when the items were renamed.
#[must_use]
pub fn aliases(name: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut swap = |from: &str, to: &str| {
        if let Some(rest) = name.strip_prefix(from) {
            names.push(format!("{to}{rest}"));
        }
    };
    swap("wooden_", "wood_");
    swap("golden_", "gold_");
    swap("cooked_", "");
    swap("music_disc_", "record_");
    swap("light_gray_", "silver_");
    if let Some(wood) = name.strip_suffix("_door") {
        names.push(format!("door_{wood}"));
    }
    if let Some(wood) = name.strip_suffix("_boat") {
        names.push(format!("boat_{wood}"));
    }
    if let Some(wood) = name.strip_suffix("_chest_boat") {
        names.push(format!("chest_boat_{wood}"));
        names.push(format!("{wood}_chest_boat"));
    }
    if let Some(wood) = name.strip_suffix("_sign") {
        names.push(format!("sign_{wood}"));
    }
    if let Some(colour) = name.strip_suffix("_dye") {
        names.push(format!("dye_powder_{colour}"));
    }
    if let Some(mob) = name.strip_suffix("_spawn_egg") {
        names.push(format!("egg_{mob}"));
        names.push(format!("egg_{}", mob.replace('_', "")));
    }
    if let Some(colour) = name.strip_suffix("_bundle") {
        names.push(format!("bundle_{colour}"));
    }
    if let Some(what) = name.strip_prefix("cooked_") {
        names.push(format!("{what}_cooked"));
    }
    if let Some(what) = name.strip_suffix("_bucket") {
        names.push(format!("bucket_{what}"));
    }
    names.push(format!("{name}_raw"));
    names.push(format!("{name}_item"));
    names.extend(ODD_NAMES.iter().filter(|(item, _)| *item == name).map(|(_, file)| (*file).to_string()));
    names
}

/// Items whose file doesn't follow any pattern.
const ODD_NAMES: [(&str, &str); 48] = [
    ("bow", "bow_standby"),
    ("crossbow", "crossbow_standby"),
    ("book", "book_normal"),
    ("enchanted_book", "book_enchanted"),
    ("writable_book", "book_writable"),
    ("written_book", "book_written"),
    ("bone_meal", "dye_powder_white"),
    ("ink_sac", "dye_powder_black"),
    ("glow_ink_sac", "dye_powder_glow"),
    ("lapis_lazuli", "dye_powder_blue"),
    ("light_gray_dye", "dye_powder_silver"),
    ("minecart", "minecart_normal"),
    ("chest_minecart", "minecart_chest"),
    ("command_block_minecart", "minecart_command_block"),
    ("hopper_minecart", "minecart_hopper"),
    ("tnt_minecart", "minecart_tnt"),
    ("empty_map", "map_empty"),
    ("filled_map", "map_filled"),
    ("enchanted_golden_apple", "apple_golden"),
    ("fermented_spider_eye", "spider_eye_fermented"),
    ("fire_charge", "fireball"),
    ("firework_rocket", "fireworks"),
    ("firework_star", "fireworks_charge"),
    ("frame", "item_frame"),
    ("glow_frame", "glow_item_frame"),
    ("glass_bottle", "potion_bottle_empty"),
    ("potion", "potion_bottle_drinkable"),
    ("splash_potion", "potion_bottle_splash"),
    ("lingering_potion", "potion_bottle_lingering"),
    ("popped_chorus_fruit", "chorus_fruit_popped"),
    ("slime_ball", "slimeball"),
    ("totem_of_undying", "totem"),
    ("tropical_fish_bucket", "bucket_tropical"),
    ("turtle_scute", "turtle_shell_piece"),
    ("light_block", "light_block_15"),
    ("cod", "fish_raw"),
    ("salmon", "fish_salmon_raw"),
    ("tropical_fish", "fish_clownfish_raw"),
    ("pufferfish", "fish_pufferfish_raw"),
    ("cooked_cod", "fish_cooked"),
    ("cooked_salmon", "fish_salmon_cooked"),
    ("wooden_door", "door_wood"),
    ("oak_sign", "sign"),
    ("oak_boat", "boat_oak"),
    ("sugar_cane", "reeds"),
    ("melon_slice", "melon"),
    ("glistering_melon_slice", "melon_speckled"),
    ("golden_carrot", "carrot_golden"),
];

/// Every item's icon, and the quads of those drawn as blocks.
#[derive(Debug, Default)]
pub struct ItemIcons {
    by_item: HashMap<i16, Icon>,
    /// Old-style items whose data value picks another block.
    by_variant: HashMap<(i16, u32), Icon>,
    names: HashMap<i16, &'static str>,
    quads: Vec<IconQuad>,
    missing: Option<Icon>,
}

impl ItemIcons {
    /// The icon for this item. Anything unknown gets the missing texture.
    #[must_use]
    pub fn get(&self, id: i16, aux: u32) -> Option<Icon> {
        let exact = self.by_variant.get(&(id, aux)).or_else(|| self.by_item.get(&id));
        exact.copied().or(self.missing)
    }

    /// The quads of a block icon.
    #[must_use]
    pub fn quads(&self, first: u32, count: u32) -> &[IconQuad] {
        &self.quads[first as usize..(first + count) as usize]
    }

    /// The item's name with its namespace, as the game knows it.
    #[must_use]
    pub fn name(&self, id: i16) -> Option<&'static str> {
        self.names.get(&id).copied()
    }

    /// Stores a block's quads, if it draws anything.
    fn add_quads(&mut self, quads: &[IconQuad]) -> Option<Icon> {
        if quads.is_empty() {
            return None;
        }
        let first = self.quads.len() as u32;
        self.quads.extend(quads);
        Some(Icon::Block {
            first,
            count: quads.len() as u32,
        })
    }
}

/// Where each item's picture is, before the atlas exists.
#[derive(Debug, Default)]
pub struct Pictures {
    /// The path of each item's picture, by id, for those that have one.
    paths: HashMap<i16, String>,
}

/// The text of `item_texture.json`'s entries: each key's first path.
fn texture_keys(stack: &PackStack) -> HashMap<String, String> {
    let mut keys = HashMap::new();
    for (_, read) in stack.layers(ITEM_TEXTURES) {
        let Ok(Some(bytes)) = read else { continue };
        let Ok(value) = json::parse::<serde_json::Value>(&bytes) else { continue };
        let Some(data) = value.get("texture_data").and_then(serde_json::Value::as_object) else {
            continue;
        };
        for (key, entry) in data {
            let Some(textures) = entry.get("textures") else { continue };
            let path = match textures {
                serde_json::Value::String(path) => Some(path.as_str()),
                serde_json::Value::Array(paths) => paths.first().and_then(|path| {
                    path.as_str().or_else(|| path.get("path").and_then(serde_json::Value::as_str))
                }),
                other => other.get("path").and_then(serde_json::Value::as_str),
            };
            if let Some(path) = path {
                keys.insert(key.clone(), path.to_string());
            }
        }
    }
    keys
}

/// What `items/*.json` says each item's icon is: the item's full name, and a key of
/// `item_texture.json`.
fn defined_icons(stack: &PackStack) -> HashMap<String, String> {
    let mut icons = HashMap::new();
    for file in stack.list(DEFINITIONS).into_keys().filter(|file| file.ends_with(".json")) {
        let Ok(Some(found)) = stack.read(&format!("{DEFINITIONS}/{file}")) else { continue };
        let Ok(value) = json::parse::<serde_json::Value>(&found.bytes) else { continue };
        let Some(item) = value.get("minecraft:item") else { continue };
        let name = item.pointer("/description/identifier").and_then(serde_json::Value::as_str);
        let icon = item.pointer("/components/minecraft:icon");
        let key = icon.and_then(|icon| {
            icon.as_str()
                .or_else(|| icon.get("texture").and_then(serde_json::Value::as_str))
                .or_else(|| icon.pointer("/textures/default").and_then(serde_json::Value::as_str))
        });
        if let (Some(name), Some(key)) = (name, key) {
            icons.insert(name.to_string(), key.to_string());
        }
    }
    icons
}

/// The path of the picture for an item called `key`, from what's on file.
fn picture_path(
    key: &str,
    defined: &HashMap<String, String>,
    keys: &HashMap<String, String>,
    files: &BTreeMap<String, usize>,
) -> Option<String> {
    let name = short_name(key);
    if let Some(path) = defined.get(key).and_then(|icon| keys.get(icon)) {
        return Some(path.clone());
    }
    if let Some(path) = keys.get(name) {
        return Some(path.clone());
    }
    let on_file = |name: &str| {
        (files.contains_key(&format!("{name}.png")) || files.contains_key(&format!("{name}.tga")))
            .then(|| format!("{ITEMS_FOLDER}/{name}"))
    };
    on_file(name).or_else(|| aliases(name).iter().find_map(|alias| on_file(alias).or_else(|| keys.get(alias).cloned())))
}

/// Finds each item's picture, reads them, and returns them to go in the atlas.
///
/// They're read from the pack that has them, in parallel: there are about a thousand, and
/// looking for each through all 55 packs in turn would take up most of a second.
pub fn read_pictures(stack: &PackStack) -> (Pictures, Vec<Input>) {
    let keys = texture_keys(stack);
    let defined = defined_icons(stack);
    let files = stack.list(ITEMS_FOLDER);
    let mut pictures = Pictures::default();
    for item in BedrockItem::ALL_BEDROCK_ITEMS {
        if let Some(path) = picture_path(item.registry_key, &defined, &keys, &files) {
            pictures.paths.insert(item.id, path);
        }
    }
    let missing = MISSING_TEXTURE.to_string();
    let mut paths: Vec<&String> = pictures.paths.values().collect();
    paths.sort_unstable();
    paths.dedup();
    paths.push(&missing);
    let read: Vec<Result<Input, String>> = paths
        .par_iter()
        .map(|path| {
            let image = read_on_file(stack, &files, path).or_else(|_| read_image(stack, path)).map_err(|err| format!("{path}: {err}"))?;
            Ok(Input {
                name: (*path).clone(),
                image: first_frame(image),
                nine_slice: None,
            })
        })
        .collect();
    let mut inputs = Vec::with_capacity(read.len());
    for input in read {
        match input {
            Ok(input) => inputs.push(input),
            Err(reason) => warn!("An item's picture didn't load: {reason}"),
        }
    }
    (pictures, inputs)
}

/// Reads a picture straight from the pack that has it.
fn read_on_file(stack: &PackStack, files: &BTreeMap<String, usize>, path: &str) -> Result<RgbaImage, String> {
    let name = path.strip_prefix(ITEMS_FOLDER).and_then(|rest| rest.strip_prefix('/')).ok_or("not in the items folder")?;
    let (file, format) = [("png", image::ImageFormat::Png), ("tga", image::ImageFormat::Tga)]
        .iter()
        .find_map(|(extension, format)| {
            let file = format!("{name}.{extension}");
            files.contains_key(&file).then_some((file, *format))
        })
        .ok_or("not on file")?;
    let pack = &stack.packs()[files[&file]];
    let bytes = pack
        .read(&format!("{ITEMS_FOLDER}/{file}"))
        .map_err(|err| err.to_string())?
        .ok_or("gone from its pack")?;
    image::load_from_memory_with_format(&bytes, format)
        .map(|image| image.to_rgba8())
        .map_err(|err| err.to_string())
}

/// Some items' pictures are strips of frames: a clock, a compass. The icon is the first.
fn first_frame(image: RgbaImage) -> RgbaImage {
    let (width, height) = image.dimensions();
    if height > width {
        image::imageops::crop_imm(&image, 0, 0, width, width).to_image()
    } else {
        image
    }
}

/// Works out every item's icon, now that the pictures are in the atlas, and logs the
/// census.
#[must_use]
pub fn build_icons(pictures: &Pictures, atlas: &Atlas, table: &AppearanceTable, tints: &Tints) -> ItemIcons {
    let mut icons = ItemIcons {
        missing: atlas.sprite(MISSING_TEXTURE).map(Icon::Sprite),
        ..ItemIcons::default()
    };
    // Every block an item can place, and its quads, worked out all at once.
    let wanted: Vec<u32> = BedrockItem::ALL_BEDROCK_ITEMS
        .iter()
        .flat_map(|item| (0..MOST_VARIANTS).filter_map(|aux| block_of(item.id, aux)))
        .collect::<std::collections::BTreeSet<u32>>()
        .into_iter()
        .collect();
    let drawn: Vec<(u32, Vec<IconQuad>)> =
        wanted.par_iter().map(|&block| (block, block_icon(table, tints, block))).collect();
    let mut by_block: HashMap<u32, Icon> = HashMap::with_capacity(drawn.len());
    for (block, quads) in drawn {
        if let Some(icon) = icons.add_quads(&quads) {
            by_block.insert(block, icon);
        }
    }

    let (mut sendable, mut with_picture, mut as_block) = (0_u32, 0_u32, 0_u32);
    let mut missing = Vec::new();
    for item in BedrockItem::ALL_BEDROCK_ITEMS {
        icons.names.insert(item.id, item.registry_key);
        let can_be_sent = (0..MOST_VARIANTS).any(|aux| JavaToBedrockItemMapping::from_bedrock(item.id, aux).is_some());
        sendable += u32::from(can_be_sent);
        if let Some(sprite) = pictures.paths.get(&item.id).and_then(|path| atlas.sprite(path)) {
            icons.by_item.insert(item.id, Icon::Sprite(sprite));
            with_picture += u32::from(can_be_sent);
            continue;
        }
        let base = block_of(item.id, 0);
        if let Some(icon) = base.and_then(|block| by_block.get(&block)) {
            icons.by_item.insert(item.id, *icon);
            as_block += u32::from(can_be_sent);
        } else if can_be_sent {
            missing.push(short_name(item.registry_key));
        }
        // An old-style item's other data values can be other blocks.
        for aux in 1..MOST_VARIANTS {
            let block = block_of(item.id, aux).filter(|block| Some(*block) != base);
            if let Some(icon) = block.and_then(|block| by_block.get(&block)) {
                icons.by_variant.insert((item.id, aux), *icon);
            }
        }
    }
    missing.sort_unstable();
    info!(
        "Items: {} known, of which the server can send {sendable}: {with_picture} with a picture, {as_block} as blocks \
         ({} block quads), {} draw the missing texture",
        BedrockItem::ALL_BEDROCK_ITEMS.len(),
        icons.quads.len(),
        missing.len(),
    );
    if !missing.is_empty() {
        info!("Items that draw the missing texture: {}", missing.join(", "));
    }
    icons
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str) -> &'static BedrockItem {
        BedrockItem::ALL_BEDROCK_ITEMS
            .iter()
            .copied()
            .find(|item| short_name(item.registry_key) == name)
            .unwrap_or_else(|| panic!("no item {name}"))
    }

    /// The Bedrock id `name`'s block has in its default state.
    fn block(block: &Block) -> u32 {
        BlockState::to_be_network_id(block.default_state.id)
    }

    #[test]
    fn items_lead_to_the_blocks_they_place() {
        assert_eq!(block_of(item("dirt").id, 0), Some(block(&Block::DIRT)));
        assert_eq!(block_of(item("oak_log").id, 0), Some(block(&Block::OAK_LOG)));
        assert_eq!(block_of(item("poppy").id, 0), Some(block(&Block::POPPY)));
        assert_eq!(block_of(item("torch").id, 0), Some(block(&Block::TORCH)));
    }

    #[test]
    fn items_that_place_nothing_lead_nowhere() {
        assert_eq!(block_of(item("apple").id, 0), None);
        assert_eq!(block_of(item("diamond_sword").id, 0), None);
    }

    #[test]
    fn a_name_is_the_games_string_or_the_words() {
        let mut lang = Lang::default();
        lang.add("item.apple.name=Tasty Apple\ntile.dirt.name=Earth");
        assert_eq!(display_name(&lang, "minecraft:apple"), "Tasty Apple");
        assert_eq!(display_name(&lang, "minecraft:dirt"), "Earth");
        assert_eq!(display_name(&lang, "minecraft:oak_planks"), "Oak Planks");
        assert_eq!(display_name(&lang, "tnt"), "Tnt");
    }

    fn files(names: &[&str]) -> BTreeMap<String, usize> {
        names.iter().map(|name| ((*name).to_string(), 0)).collect()
    }

    fn path(key: &str, defined: &[(&str, &str)], keys: &[(&str, &str)], on_file: &[&str]) -> Option<String> {
        let owned = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs.iter().map(|(a, b)| ((*a).to_string(), (*b).to_string())).collect()
        };
        picture_path(key, &owned(defined), &owned(keys), &files(on_file))
    }

    #[test]
    fn the_icon_lookup_goes_in_order() {
        let key = "minecraft:thing";
        let all_defined = [("minecraft:thing", "the_key")];
        let all_keys = [("the_key", "textures/a"), ("thing", "textures/b")];
        let on_file = ["thing.png"];
        assert_eq!(path(key, &all_defined, &all_keys, &on_file).as_deref(), Some("textures/a"), "its definition first");
        assert_eq!(path(key, &[], &all_keys, &on_file).as_deref(), Some("textures/b"), "then a key of its name");
        assert_eq!(path(key, &[], &[], &on_file).as_deref(), Some("textures/items/thing"), "then a file of its name");
        assert_eq!(path(key, &[], &[], &[]), None);
    }

    #[test]
    fn aliases_find_the_files_old_names() {
        let found = |name: &str, on_file: &str| path(&format!("minecraft:{name}"), &[], &[], &[on_file]);
        assert_eq!(found("wooden_sword", "wood_sword.png").as_deref(), Some("textures/items/wood_sword"));
        assert_eq!(found("golden_apple", "gold_apple.png").as_deref(), Some("textures/items/gold_apple"));
        assert_eq!(found("red_dye", "dye_powder_red.png").as_deref(), Some("textures/items/dye_powder_red"));
        assert_eq!(found("cooked_beef", "beef_cooked.png").as_deref(), Some("textures/items/beef_cooked"));
        assert_eq!(found("beef", "beef_raw.png").as_deref(), Some("textures/items/beef_raw"));
        assert_eq!(found("spruce_door", "door_spruce.png").as_deref(), Some("textures/items/door_spruce"));
        assert_eq!(found("bow", "bow_standby.png").as_deref(), Some("textures/items/bow_standby"));
        assert_eq!(found("bow", "nothing.png"), None);
    }

    #[test]
    fn the_first_frame_of_a_strip_is_square() {
        let frame = first_frame(RgbaImage::new(16, 64));
        assert_eq!(frame.dimensions(), (16, 16));
        assert_eq!(first_frame(RgbaImage::new(32, 16)).dimensions(), (32, 16));
    }
}
