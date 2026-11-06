//! The HUD and the inventory on sheets (m5-plan Steps 8 and 12): drawn through the real
//! atlas, tree and shader, with no window, over made-up stripes, into PNGs.
//!
//! The owner's window can't be seen from here, so this is how the HUD gets looked at
//! while it's being made.
//!
//! ```text
//! LIMONITE_BEDROCK="C:/XboxGames/Minecraft for Windows/Content" \
//!     cargo test -p limonite-client --bin limonite-client _sheets -- --ignored --nocapture
//! ```
//!
//! `hud_sheets` draws the HUD, and `inventory_sheets` the inventory screen.
//!
//! **The sheets go in `run/sheets`, which is gitignored:** they show Mojang's sprites,
//! so they're never committed.

use std::path::PathBuf;

use image::{Rgba, RgbaImage};
use pumpkin_util::GameMode;

use super::{Ui, load_with, render::UiRenderer, screens::hud::HudState, tree::Node};
use crate::{
    blocks, install,
    inventory::{HOTBAR, Item},
    render::{self, Gpu, Needs, terrain::Terrain},
    settings::UiScale,
    ui::layout::{Anchor, Layout},
};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
/// The wide part of the zoomed crops: the hotbar and what's above it, at scale 3.
const CROP: [u32; 4] = [340, 555, 600, 165];
/// A sprite with a nine-slice, to see one drawn.
const NINE_SLICED: &str = "textures/ui/button_borderless_light";

/// Stripes of what a HUD gets drawn over, and of black and white to show its blending.
fn stripes() -> RgbaImage {
    const COLOURS: [[u8; 3]; 6] = [
        [120, 167, 255],
        [255, 255, 255],
        [90, 140, 60],
        [0, 0, 0],
        [125, 125, 125],
        [150, 110, 70],
    ];
    RgbaImage::from_fn(WIDTH, HEIGHT, |x, _| {
        let [r, g, b] = COLOURS[(x / 106) as usize % COLOURS.len()];
        Rgba([r, g, b, 255])
    })
}

struct Sheet {
    gpu: Gpu,
    target: wgpu::Texture,
    background: RgbaImage,
}

impl Sheet {
    fn new(gpu: Gpu) -> Self {
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hud sheet"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gpu.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[gpu.ui_format],
        });
        Self {
            gpu,
            target,
            background: stripes(),
        }
    }

    /// Draws the UI as it is now over the stripes, through a view in the format
    /// `renderer` was made for, and reads the picture back.
    fn draw(&self, ui: &mut Ui, renderer: &mut UiRenderer, format: wgpu::TextureFormat) -> RgbaImage {
        let gpu = &self.gpu;
        gpu.queue.write_texture(
            self.target.as_image_copy(),
            self.background.as_raw(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * WIDTH),
                rows_per_image: Some(HEIGHT),
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        if let Some(drawing) = ui.drawing() {
            renderer.upload(gpu, drawing);
        }
        let view = self.target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format),
            ..Default::default()
        });
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        renderer.draw(&view, &mut encoder);
        read_back(gpu, &self.target, encoder)
    }
}

/// Finishes `encoder`, with a copy of `target`, and returns the picture. The target is
/// in `Rgba8UnormSrgb`, as `Gpu::offscreen` says, so its bytes are a PNG's.
pub fn read_back(gpu: &Gpu, target: &wgpu::Texture, mut encoder: wgpu::CommandEncoder) -> RgbaImage {
    let (width, height) = (target.width(), target.height());
    let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("picture readback"),
        size: u64::from(row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |result| result.expect("the map"));
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU");
    let bytes = buffer.slice(..).get_mapped_range().expect("the mapped range");
    RgbaImage::from_fn(width, height, |x, y| {
        let at = (y * row + x * 4) as usize;
        Rgba([bytes[at], bytes[at + 1], bytes[at + 2], 255])
    })
}

/// A hotbar of these items, with a few to a stack here and there.
fn hotbar_of(names: &[&str; HOTBAR]) -> [Option<Item>; HOTBAR] {
    std::array::from_fn(|slot| {
        let wanted = format!("minecraft:{}", names[slot]);
        let items = pumpkin_data::item::BedrockItem::ALL_BEDROCK_ITEMS;
        let item = items.iter().find(|item| item.registry_key == wanted);
        item.map(|item| Item {
            id: item.id,
            aux: 0,
            count: if slot % 3 == 0 { 64 } else { 1 },
        })
    })
}

/// The crops, one above another, each twice the size so its pixels can be counted.
fn zoomed(pictures: &[RgbaImage]) -> RgbaImage {
    let [x, y, width, height] = CROP;
    let mut sheet = RgbaImage::new(width * 2, height * 2 * pictures.len() as u32);
    for (row, picture) in (0_u32..).zip(pictures) {
        let crop = image::imageops::crop_imm(picture, x, y, width, height).to_image();
        let big = image::imageops::resize(&crop, width * 2, height * 2, image::imageops::FilterType::Nearest);
        image::imageops::replace(&mut sheet, &big, 0, i64::from(row * height * 2));
    }
    sheet
}

#[test]
#[ignore = "needs a GPU and a Bedrock install; set LIMONITE_BEDROCK"]
fn hud_sheets() {
    let _ = tracing_subscriber::fmt().try_init();
    let dir = std::env::var_os("LIMONITE_BEDROCK").expect("LIMONITE_BEDROCK is set");
    let install = install::find(Some(std::path::Path::new(&dir))).unwrap();
    let vanilla = install::load_vanilla(&install).unwrap();
    let palette = blocks::load_palette().unwrap();
    let mut jar = blocks::find_jar(None).unwrap();
    let looks = blocks::build_appearance(&palette, &vanilla, jar.as_mut());
    let tints = render::load_tints(&vanilla.stack);
    let assets = load_with(&vanilla.stack, &looks.table, &tints, &[NINE_SLICED]);
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../run/sheets");
    std::fs::create_dir_all(&out).unwrap();
    assets.picture.save(out.join("hud-atlas.png")).unwrap();

    let needs = Needs {
        texture_layers: looks.textures.layers,
        uncapped: false,
    };
    let mut gpu = Gpu::offscreen(WIDTH, HEIGHT, &needs).expect("a GPU");
    println!("The frame is {:?}, and the UI draws in {:?}", gpu.config.format, gpu.ui_format);
    let terrain = Terrain::new(&gpu, &looks.textures);
    let mut gamma = UiRenderer::new(&gpu, &assets.picture, &terrain.texture_view);
    let frame_format = gpu.config.format;
    let gamma_format = std::mem::replace(&mut gpu.ui_format, frame_format);
    let mut linear = UiRenderer::new(&gpu, &assets.picture, &terrain.texture_view);
    gpu.ui_format = gamma_format;
    let sheet = Sheet::new(gpu);

    let mut ui = Ui::new(assets.atlas, assets.font, assets.lang, assets.icons, UiScale::Auto);
    ui.resize(WIDTH, HEIGHT);
    let base = HudState {
        gamemode: GameMode::Survival,
        health: 20.0,
        max_health: 20.0,
        hunger: 20.0,
        armour: 0,
        air: 300.0,
        experience: 0.0,
        level: 0,
        selected: 0,
        hotbar: hotbar_of(&["dirt", "oak_log", "poppy", "torch", "oak_stairs", "diamond_sword", "apple", "oak_fence", "grass_block"]),
        tick: 0,
    };
    let cases = [
        ("survival-full", base, None),
        (
            "survival-mixed",
            HudState {
                health: 13.0,
                hunger: 7.0,
                armour: 15,
                air: 151.0,
                experience: 0.4,
                level: 7,
                selected: 4,
                ..base
            },
            None,
        ),
        (
            "creative-text",
            HudState {
                gamemode: GameMode::Creative,
                selected: 8,
                ..base
            },
            Some("The quick brown fox jumps over 13 lazy dogs! caf\u{e9} \u{2019}quoted\u{2019} \u{a7}eGold"),
        ),
    ];
    let mut pictures = Vec::new();
    for (name, state, note) in &cases {
        ui.hud.update(&mut ui.tree, &ui.icons, &ui.lang, Some(state));
        ui.set_note(*note);
        let picture = sheet.draw(&mut ui, &mut gamma, gamma_format);
        picture.save(out.join(format!("hud-{name}.png"))).unwrap();
        pictures.push(picture);
    }

    // The same, blended in linear light, as on an adapter without the second view.
    ui.hud.update(&mut ui.tree, &ui.icons, &ui.lang, Some(&cases[1].1));
    ui.set_note(None);
    ui.tree.invalidate();
    pictures.push(sheet.draw(&mut ui, &mut linear, frame_format));

    // A nine-sliced panel and scale 2, to see them drawn.
    let sprite = ui.atlas.sprite(NINE_SLICED).expect("the nine-sliced sprite");
    assert!(ui.atlas.get(sprite).nine_slice.is_some(), "it has its JSON");
    let panel = Layout::px(120.0, 30.0).at(Anchor::TopLeft).moved(10.0, 10.0);
    ui.tree.add(super::tree::Tree::ROOT, Node::image(panel, Some(sprite)));
    ui.scale = 2;
    ui.tree.invalidate();
    let picture = sheet.draw(&mut ui, &mut gamma, gamma_format);
    picture.save(out.join("hud-scale-2.png")).unwrap();

    zoomed(&pictures).save(out.join("hud-zoom.png")).unwrap();
    println!("Wrote the HUD's sheets to {}", out.display());
}

/// The creative list Pumpkin sends, built the way it builds it.
fn pumpkins_creative_list() -> crate::inventory::creative::CreativeList {
    use pumpkin_data::bedrock_creative::{CREATIVE_ENTRIES, CREATIVE_GROUPS};
    use pumpkin_protocol::{
        bedrock::{
            client::creative_content::{CreativeCategory, CreativeGroupInfoPayload, CreativeItemEntryPayload},
            client_codecs::CreativeContent,
            network_item::NetworkItemDescriptor,
        },
        codec::{var_int::VarInt, var_uint::VarUInt},
    };
    let item = |id: i16, aux: u32| NetworkItemDescriptor {
        id: VarInt(i32::from(id)),
        stack_size: 1,
        aux_value: VarUInt(aux),
        ..NetworkItemDescriptor::default()
    };
    let groups = CREATIVE_GROUPS
        .iter()
        .map(|group| CreativeGroupInfoPayload {
            creative_category: match group.category {
                1 => CreativeCategory::Construction,
                2 => CreativeCategory::Nature,
                3 => CreativeCategory::Equipment,
                4 => CreativeCategory::Items,
                _ => CreativeCategory::ItemCommandOnly,
            },
            name: group.name.to_string(),
            group_icon_item: if group.icon_item_id == 0 {
                NetworkItemDescriptor::default()
            } else {
                item(group.icon_item_id, group.icon_item_aux_value)
            },
        })
        .collect();
    let entries = CREATIVE_ENTRIES
        .iter()
        .enumerate()
        .map(|(index, entry)| CreativeItemEntryPayload {
            id: VarUInt(index as u32 + 1),
            item: item(entry.item_id, entry.item_aux_value),
            group_index: VarUInt(entry.group_index),
        })
        .collect();
    crate::inventory::creative::CreativeList::new(&CreativeContent { groups, entries })
}

/// A stack of the item called `name`.
fn stack_of(name: &str, count: u16) -> crate::inventory::slots::Stack {
    let wanted = format!("minecraft:{name}");
    let items = pumpkin_data::item::BedrockItem::ALL_BEDROCK_ITEMS;
    let item = items.iter().find(|item| item.registry_key == wanted).expect("a Bedrock item");
    crate::inventory::slots::Stack {
        id: item.id,
        stack_size: count,
        ..crate::inventory::slots::Stack::default()
    }
}

#[test]
#[ignore = "needs a GPU and a Bedrock install; set LIMONITE_BEDROCK"]
fn inventory_sheets() {
    use crate::{
        inventory::slots::Slots,
        ui::screens::inventory::{Pressed, SPRITES, View},
    };

    let _ = tracing_subscriber::fmt().try_init();
    let dir = std::env::var_os("LIMONITE_BEDROCK").expect("LIMONITE_BEDROCK is set");
    let install = install::find(Some(std::path::Path::new(&dir))).unwrap();
    let vanilla = install::load_vanilla(&install).unwrap();
    let palette = blocks::load_palette().unwrap();
    let mut jar = blocks::find_jar(None).unwrap();
    let looks = blocks::build_appearance(&palette, &vanilla, jar.as_mut());
    let tints = render::load_tints(&vanilla.stack);
    let assets = load_with(&vanilla.stack, &looks.table, &tints, &[]);
    let missing: Vec<&str> = SPRITES.iter().copied().filter(|path| assets.atlas.sprite(path).is_none()).collect();
    assert!(missing.is_empty(), "the install lacks {missing:?}");
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../run/sheets");
    std::fs::create_dir_all(&out).unwrap();

    let needs = Needs {
        texture_layers: looks.textures.layers,
        uncapped: false,
    };
    let gpu = Gpu::offscreen(WIDTH, HEIGHT, &needs).expect("a GPU");
    let terrain = Terrain::new(&gpu, &looks.textures);
    let mut renderer = UiRenderer::new(&gpu, &assets.picture, &terrain.texture_view);
    let format = gpu.ui_format;
    let sheet = Sheet::new(gpu);
    let mut ui = Ui::new(assets.atlas, assets.font, assets.lang, assets.icons, UiScale::Auto);
    ui.resize(WIDTH, HEIGHT);
    let scale = f64::from(ui.scale);
    let [width, height] = ui.screen();

    let list = pumpkins_creative_list();
    let mut slots = Slots::default();
    for (slot, (name, count)) in [
        ("dirt", 64),
        ("oak_log", 12),
        ("torch", 64),
        ("diamond_sword", 1),
        ("apple", 5),
        ("oak_stairs", 16),
        ("poppy", 1),
        ("oak_fence", 3),
        ("grass_block", 64),
    ]
    .iter()
    .enumerate()
    {
        slots.main[slot] = stack_of(name, *count);
    }
    slots.main[9] = stack_of("cobblestone", 40);
    slots.main[20] = stack_of("bread", 7);
    slots.main[35] = stack_of("iron_pickaxe", 1);
    slots.armour[0] = stack_of("iron_helmet", 1);
    slots.armour[2] = stack_of("iron_leggings", 1);

    // The survival panel's top left, and a point on a slot of it.
    let survival = [(width - 176.0) / 2.0, (height - 166.0) / 2.0];
    let at = |corner: [f32; 2], x: f32, y: f32| ((f64::from(corner[0] + x)) * scale, (f64::from(corner[1] + y)) * scale);
    let mut draw = |ui: &mut Ui, name: &str, view: View<'_>, pointer: (f64, f64)| {
        ui.hud.update(&mut ui.tree, &ui.icons, &ui.lang, None);
        ui.inventory.update(&mut ui.tree, &ui.icons, &ui.lang, Some(&view));
        // Laid out once, so that the pointer has something to land on, as in a window.
        sheet.draw(ui, &mut renderer, format);
        ui.pointer_moved(pointer.0, pointer.1);
        let picture = sheet.draw(ui, &mut renderer, format);
        picture.save(out.join(format!("inventory-{name}.png"))).unwrap();
    };

    // Survival, the pointer over the third of the 27, and then carrying a stack.
    let pointer = at(survival, 7.0 + 2.5 * 18.0, 86.0 + 9.0);
    draw(&mut ui, "survival", View { creative: false, slots: &slots, list: &list }, pointer);
    let mut carrying = slots.clone();
    carrying.cursor = stack_of("cobblestone", 40);
    carrying.main[9] = crate::inventory::slots::Stack::default();
    let pointer = at(survival, 7.0 + 4.3 * 18.0, 86.0 + 1.6 * 18.0);
    draw(&mut ui, "survival-carrying", View { creative: false, slots: &carrying, list: &list }, pointer);

    // Creative: the construction tab, then its first group opened, then scrolled, then
    // the nature tab.
    let creative = [(width - 326.0) / 2.0, (height - 166.0) / 2.0];
    let first_cell = at(creative, 7.0 + 9.0, 26.0 + 9.0);
    let view = || View { creative: true, slots: &slots, list: &list };
    draw(&mut ui, "creative", view(), first_cell);
    ui.pointer_moved(first_cell.0, first_cell.1);
    assert_eq!(ui.inventory_pressed(), Pressed::Nothing, "the first cell is a group, which opens");
    draw(&mut ui, "creative-group-open", view(), first_cell);
    ui.scroll(-4.0);
    draw(&mut ui, "creative-scrolled", view(), at(creative, 7.0 + 5.5 * 18.0, 26.0 + 3.5 * 18.0));
    let nature_tab = at(creative, 3.0 * 25.0 + 12.0, -11.0);
    ui.pointer_moved(nature_tab.0, nature_tab.1);
    assert_eq!(ui.inventory_pressed(), Pressed::Tab(crate::inventory::creative::Tab::Nature));
    draw(&mut ui, "creative-nature", view(), at(creative, 7.0 + 2.5 * 18.0, 26.0 + 1.5 * 18.0));
    // A stack on the cursor in creative, over the hotbar.
    let mut held = slots.clone();
    held.cursor = stack_of("stone", 64);
    draw(&mut ui, "creative-carrying", View { creative: true, slots: &held, list: &list }, at(creative, 163.0, 175.0));
    println!("Wrote the inventory's sheets to {}", out.display());
}
