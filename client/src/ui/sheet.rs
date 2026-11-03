//! The HUD on a sheet (m5-plan Step 8): drawn through the real atlas, tree and shader,
//! with no window, over made-up stripes, into PNGs.
//!
//! The owner's window can't be seen from here, so this is how the HUD gets looked at
//! while it's being made.
//!
//! ```text
//! LIMONITE_BEDROCK="C:/XboxGames/Minecraft for Windows/Content" \
//!     cargo test -p limonite-client --bin limonite-client hud_sheets -- --ignored --nocapture
//! ```
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
